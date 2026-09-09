use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::atomic::{AtomicU64, Ordering},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

#[path = "codex_paginated_history_migration_recovery.rs"]
mod migration_recovery;

#[cfg(test)]
static REPAIR_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RolloutOrdinalScan {
    pub duplicate_count: usize,
    pub byte_len: u64,
    pub first_ordinal: Option<u64>,
    pub last_original_ordinal: Option<u64>,
    pub last_normalized_ordinal: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedHistoryRepairPreflight {
    pub affected_rollout_count: usize,
    pub duplicate_ordinal_count: usize,
    pub provider_migration_cursor_count: usize,
    pub provider_migration_history_base_count: usize,
    pub rotated_thread_count: usize,
    pub rotated_segment_count: usize,
    pub affected_bytes: u64,
    pub blocked_rollout_count: usize,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PaginatedHistoryRepairOutcome {
    pub repaired_rollout_count: usize,
    pub repaired_duplicate_count: usize,
    pub repaired_provider_migration_cursor_count: usize,
    pub repaired_provider_migration_history_base_count: usize,
    pub repaired_rotated_thread_count: usize,
    pub repaired_rotated_segment_count: usize,
    pub(super) targets: Vec<ProjectionCatchUpTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProjectionCatchUpTarget {
    pub(super) source_id: String,
    pub(super) rollout_path: PathBuf,
    pub(super) minimum_next_ordinal: u64,
    pub(super) minimum_next_byte_offset: u64,
}

#[derive(Clone, Debug)]
struct RolloutRepairCandidate {
    path: PathBuf,
    source_id: String,
    projection_db: PathBuf,
    #[cfg(any(target_os = "windows", test))]
    repair: ProjectionCursorRepair,
}

#[derive(Default)]
struct RolloutRepairPlan {
    projection_db: Option<PathBuf>,
    candidates: Vec<RolloutRepairCandidate>,
    provider_migration: migration_recovery::ProviderMigrationRecoveryPlan,
    blocked: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectionCursorRepair {
    skipped_duplicate_count: usize,
    stalled_byte_offset: u64,
    stalled_expected_ordinal: u64,
    minimum_next_byte_offset: u64,
    minimum_next_ordinal: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderMigrationBoundaryMapping {
    current_offset: u64,
    changed_provider_records: usize,
    backup_end_ordinal: u64,
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct RotatedRolloutRepairCandidate {
    thread_id: String,
    canonical_path: PathBuf,
    segments: Vec<PathBuf>,
    projection_db: PathBuf,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct CoalescedRollout {
    segment_count: usize,
    first_ordinal: u64,
    last_ordinal: u64,
    byte_len: u64,
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct RolloutSegment {
    path: PathBuf,
    first_ordinal: u64,
    last_ordinal: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct RolloutSessionMetadata {
    id: String,
    #[serde(default)]
    history_mode: Option<String>,
    #[serde(default)]
    history_base: Option<RolloutHistoryBase>,
}

#[derive(Clone, Debug, Deserialize)]
struct RolloutHistoryBase {
    thread_id: String,
    end_ordinal_exclusive: u64,
    end_byte_offset: u64,
}

fn rollout_session_metadata(path: &Path) -> Result<RolloutSessionMetadata, String> {
    let file = File::open(path)
        .map_err(|error| format!("open_rollout_session_metadata_failed: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    if reader
        .read_until(b'\n', &mut line)
        .map_err(|error| format!("read_rollout_session_metadata_failed: {error}"))?
        == 0
    {
        return Err("rollout_contains_no_records".to_string());
    }
    let value: serde_json::Value = serde_json::from_slice(&line)
        .map_err(|error| format!("parse_rollout_session_metadata_failed: {error}"))?;
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
        return Err("rollout_does_not_start_with_session_metadata".to_string());
    }
    serde_json::from_value(
        value
            .get("payload")
            .cloned()
            .ok_or_else(|| "rollout_session_metadata_missing_payload".to_string())?,
    )
    .map_err(|error| format!("parse_rollout_session_payload_failed: {error}"))
}

fn provider_state_pointer(value: &serde_json::Value) -> Option<&'static str> {
    match (
        value.get("type").and_then(serde_json::Value::as_str),
        value
            .pointer("/payload/type")
            .and_then(serde_json::Value::as_str),
    ) {
        (Some("session_meta"), _) => Some("/payload/model_provider"),
        (Some("event_msg"), Some("thread_settings_applied")) => {
            Some("/payload/thread_settings/model_provider_id")
        }
        _ => None,
    }
}

fn provider_only_record_change(before: &[u8], after: &[u8]) -> Result<bool, String> {
    if before == after {
        return Ok(false);
    }
    let mut before_value: serde_json::Value = serde_json::from_slice(before)
        .map_err(|error| format!("parse_provider_migration_backup_record_failed: {error}"))?;
    let after_value: serde_json::Value = serde_json::from_slice(after)
        .map_err(|error| format!("parse_provider_migration_current_record_failed: {error}"))?;
    let pointer = provider_state_pointer(&before_value)
        .ok_or_else(|| "provider_migration_changed_non_provider_record".to_string())?;
    let before_provider = before_value.pointer(pointer).cloned();
    let after_provider = after_value.pointer(pointer).cloned();
    if before_provider == after_provider {
        return Err("provider_migration_record_has_other_changes".to_string());
    }
    let destination = before_value
        .pointer_mut(pointer)
        .ok_or_else(|| "provider_migration_backup_missing_provider_field".to_string())?;
    *destination = after_provider
        .ok_or_else(|| "provider_migration_current_missing_provider_field".to_string())?;
    if before_value != after_value {
        return Err("provider_migration_record_has_other_changes".to_string());
    }
    Ok(true)
}

fn map_provider_migration_boundary(
    backup_path: &Path,
    current_path: &Path,
    backup_offset: u64,
) -> Result<Option<ProviderMigrationBoundaryMapping>, String> {
    if backup_offset == 0 {
        return Err("provider_migration_boundary_is_zero".to_string());
    }
    let backup = File::open(backup_path)
        .map_err(|error| format!("open_provider_migration_backup_failed: {error}"))?;
    let current = File::open(current_path)
        .map_err(|error| format!("open_provider_migration_current_failed: {error}"))?;
    let mut backup_reader = BufReader::new(backup);
    let mut current_reader = BufReader::new(current);
    let mut backup_line = Vec::new();
    let mut current_line = Vec::new();
    let mut last_backup_line = Vec::new();
    let mut backup_position = 0_u64;
    let mut current_position = 0_u64;
    let mut changed_provider_records = 0_usize;

    while backup_position < backup_offset {
        backup_line.clear();
        current_line.clear();
        let backup_read = backup_reader
            .read_until(b'\n', &mut backup_line)
            .map_err(|error| format!("read_provider_migration_backup_failed: {error}"))?;
        let current_read = current_reader
            .read_until(b'\n', &mut current_line)
            .map_err(|error| format!("read_provider_migration_current_failed: {error}"))?;
        if backup_read == 0 || current_read == 0 {
            return Err("provider_migration_prefix_ended_before_boundary".to_string());
        }
        backup_position = backup_position
            .checked_add(backup_read as u64)
            .ok_or_else(|| "provider_migration_backup_offset_overflow".to_string())?;
        current_position = current_position
            .checked_add(current_read as u64)
            .ok_or_else(|| "provider_migration_current_offset_overflow".to_string())?;
        if backup_position > backup_offset {
            return Err("provider_migration_backup_offset_not_record_boundary".to_string());
        }
        if provider_only_record_change(&backup_line, &current_line)? {
            changed_provider_records = changed_provider_records
                .checked_add(1)
                .ok_or_else(|| "provider_migration_change_count_overflow".to_string())?;
        }
        last_backup_line.clear();
        last_backup_line.extend_from_slice(&backup_line);
    }

    if changed_provider_records == 0 || current_position == backup_offset {
        return Ok(None);
    }
    let last_value: serde_json::Value = serde_json::from_slice(&last_backup_line)
        .map_err(|error| format!("parse_provider_migration_boundary_record_failed: {error}"))?;
    let backup_end_ordinal = last_value
        .get("ordinal")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "provider_migration_boundary_record_missing_ordinal".to_string())?;
    Ok(Some(ProviderMigrationBoundaryMapping {
        current_offset: current_position,
        changed_provider_records,
        backup_end_ordinal,
    }))
}

fn rewrite_history_base_offset_preserving_record_len(
    record: &[u8],
    expected_old_offset: u64,
    new_offset: u64,
) -> Result<Vec<u8>, String> {
    let (content, newline): (&[u8], &[u8]) = if let Some(content) = record.strip_suffix(b"\r\n") {
        (content, b"\r\n")
    } else if let Some(content) = record.strip_suffix(b"\n") {
        (content, b"\n")
    } else {
        return Err("history_base_record_missing_newline".to_string());
    };
    let mut value: serde_json::Value = serde_json::from_slice(content)
        .map_err(|error| format!("parse_history_base_record_failed: {error}"))?;
    let offset = value
        .pointer_mut("/payload/history_base/end_byte_offset")
        .ok_or_else(|| "history_base_record_missing_end_byte_offset".to_string())?;
    if offset.as_u64() != Some(expected_old_offset) {
        return Err("history_base_offset_changed_during_repair".to_string());
    }
    *offset = serde_json::Value::from(new_offset);
    let mut rewritten = serde_json::to_vec(&value)
        .map_err(|error| format!("serialize_history_base_record_failed: {error}"))?;
    if rewritten.len() > content.len() {
        return Err("history_base_rewrite_cannot_preserve_record_length".to_string());
    }
    rewritten.resize(content.len(), b' ');
    rewritten.extend_from_slice(newline);
    Ok(rewritten)
}

#[cfg(test)]
fn rollout_session_id(path: &Path) -> Result<String, String> {
    Ok(rollout_session_metadata(path)?.id)
}

#[cfg(test)]
fn resolve_active_rollout_lineage(
    expected_thread_id: &str,
    active_path: &Path,
    candidate_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    resolve_active_rollout_lineage_with_overrides(
        expected_thread_id,
        active_path,
        candidate_paths,
        &HashMap::new(),
    )
}

#[cfg(test)]
fn resolve_active_rollout_lineage_with_overrides(
    expected_thread_id: &str,
    active_path: &Path,
    candidate_paths: &[PathBuf],
    history_base_offsets: &HashMap<PathBuf, u64>,
) -> Result<Vec<PathBuf>, String> {
    let paths_by_rollout_id = rollout_paths_by_id(candidate_paths)?;
    resolve_active_rollout_lineage_from_map(
        expected_thread_id,
        active_path,
        &paths_by_rollout_id,
        history_base_offsets,
    )
}

fn rollout_paths_by_id(candidate_paths: &[PathBuf]) -> Result<HashMap<String, PathBuf>, String> {
    let mut paths_by_rollout_id = HashMap::new();
    for path in candidate_paths {
        let Some(rollout_id) = source_id_from_rollout_path(path) else {
            continue;
        };
        if let Some(previous) = paths_by_rollout_id.insert(rollout_id.clone(), path.clone()) {
            if previous != *path {
                return Err(format!(
                    "ambiguous_rollout_id: rollout_id={rollout_id}, first={}, second={}",
                    previous.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(paths_by_rollout_id)
}

fn resolve_active_rollout_lineage_from_map(
    expected_thread_id: &str,
    active_path: &Path,
    paths_by_rollout_id: &HashMap<String, PathBuf>,
    history_base_offsets: &HashMap<PathBuf, u64>,
) -> Result<Vec<PathBuf>, String> {
    let mut lineage = Vec::new();
    let mut seen = HashSet::new();
    let mut current = active_path.to_path_buf();
    loop {
        let rollout_id = source_id_from_rollout_path(&current)
            .ok_or_else(|| format!("invalid_rollout_filename: {}", current.display()))?;
        if !seen.insert(rollout_id.clone()) {
            return Err(format!("cyclic_history_base: rollout_id={rollout_id}"));
        }
        let metadata = rollout_session_metadata(&current)?;
        if lineage.is_empty() && metadata.id != expected_thread_id {
            return Err(format!(
                "rollout_session_id_mismatch: expected={expected_thread_id}, actual={}",
                metadata.id
            ));
        }
        if metadata.history_mode.as_deref() != Some("paginated") {
            return Err(format!(
                "rollout_lineage_is_not_paginated: path={}",
                current.display()
            ));
        }
        lineage.push(current.clone());
        let Some(base) = metadata.history_base else {
            break;
        };
        if base.end_ordinal_exclusive == 0 {
            return Err(format!(
                "invalid_history_base_cutoff: rollout_id={}",
                base.thread_id
            ));
        }
        let parent = paths_by_rollout_id
            .get(&base.thread_id.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing_history_base_rollout: rollout_id={}",
                    base.thread_id
                )
            })?;
        let parent_len = fs::metadata(&parent)
            .map_err(|error| format!("read_history_base_metadata_failed: {error}"))?
            .len();
        let end_byte_offset = history_base_offsets
            .get(&current)
            .copied()
            .unwrap_or(base.end_byte_offset);
        if end_byte_offset > parent_len {
            return Err(format!(
                "history_base_offset_past_end: rollout_id={}, offset={}, file_len={parent_len}",
                base.thread_id, end_byte_offset
            ));
        }
        let mut file = fs::File::open(&parent)
            .map_err(|error| format!("read_history_base_failed: {error}"))?;
        let mut preceding = [0_u8; 1];
        let boundary = end_byte_offset > 0
            && file.seek(SeekFrom::Start(end_byte_offset - 1)).is_ok()
            && file.read_exact(&mut preceding).is_ok()
            && preceding[0] == b'\n';
        if !boundary {
            return Err(format!(
                "history_base_offset_not_record_boundary: rollout_id={}, offset={}",
                base.thread_id, end_byte_offset
            ));
        }
        current = parent;
    }
    lineage.reverse();
    Ok(lineage)
}

#[cfg(test)]
fn write_coalesced_rollout(
    expected_session_id: &str,
    paths: &[PathBuf],
    output_path: &Path,
) -> Result<CoalescedRollout, String> {
    if paths.len() < 2 {
        return Err("rotated_rollout_chain_requires_multiple_segments".to_string());
    }
    let mut segments = Vec::with_capacity(paths.len());
    for path in paths {
        let session_id = rollout_session_id(path)?;
        if session_id != expected_session_id {
            return Err(format!(
                "rotated_rollout_session_id_mismatch: expected={expected_session_id}, actual={session_id}"
            ));
        }
        let scan = scan_rollout_ordinals(path)?;
        if scan.duplicate_count > 0 {
            return Err("rotated_rollout_segment_has_duplicate_ordinals".to_string());
        }
        segments.push(RolloutSegment {
            path: path.clone(),
            first_ordinal: scan
                .first_ordinal
                .ok_or_else(|| "rotated_rollout_segment_has_no_first_ordinal".to_string())?,
            last_ordinal: scan
                .last_original_ordinal
                .ok_or_else(|| "rotated_rollout_segment_has_no_last_ordinal".to_string())?,
        });
    }
    segments.sort_by_key(|segment| segment.first_ordinal);
    if segments[0].first_ordinal != 0 {
        return Err("rotated_rollout_chain_does_not_start_at_zero".to_string());
    }
    for pair in segments.windows(2) {
        if pair[1].first_ordinal <= pair[0].first_ordinal
            || pair[1].first_ordinal > pair[0].last_ordinal.saturating_add(1)
        {
            return Err(format!(
                "unsafe_rotated_rollout_lineage: previous={}..{}, next={}..{}",
                pair[0].first_ordinal,
                pair[0].last_ordinal,
                pair[1].first_ordinal,
                pair[1].last_ordinal
            ));
        }
    }

    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_path)
        .map_err(|error| format!("create_coalesced_rollout_failed: {error}"))?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, output);
    let write_result = (|| -> Result<(), String> {
        for (index, segment) in segments.iter().enumerate() {
            let cutoff = segments.get(index + 1).map(|next| next.first_ordinal);
            let file = File::open(&segment.path)
                .map_err(|error| format!("open_rotated_rollout_segment_failed: {error}"))?;
            let mut reader = BufReader::with_capacity(1024 * 1024, file);
            let mut line = Vec::new();
            loop {
                line.clear();
                let read = reader
                    .read_until(b'\n', &mut line)
                    .map_err(|error| format!("read_rotated_rollout_segment_failed: {error}"))?;
                if read == 0 {
                    break;
                }
                let (_, _, ordinal) = ordinal_span(&line)?;
                if cutoff.is_some_and(|cutoff| ordinal >= cutoff) {
                    break;
                }
                writer
                    .write_all(&line)
                    .map_err(|error| format!("write_coalesced_rollout_failed: {error}"))?;
            }
        }
        writer
            .flush()
            .map_err(|error| format!("flush_coalesced_rollout_failed: {error}"))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| format!("sync_coalesced_rollout_failed: {error}"))?;
        Ok(())
    })();
    drop(writer);
    if let Err(error) = write_result {
        let _ = fs::remove_file(output_path);
        return Err(error);
    }
    let scan = scan_rollout_ordinals(output_path)?;
    if scan.duplicate_count != 0 || scan.first_ordinal != Some(0) {
        let _ = fs::remove_file(output_path);
        return Err("coalesced_rollout_failed_integrity_check".to_string());
    }
    Ok(CoalescedRollout {
        segment_count: segments.len(),
        first_ordinal: 0,
        last_ordinal: scan
            .last_original_ordinal
            .ok_or_else(|| "coalesced_rollout_has_no_last_ordinal".to_string())?,
        byte_len: scan.byte_len,
    })
}

fn inspect_verified_duplicate_projection_cursor(
    projection_db: &Path,
    source_id: &str,
    rollout_path: &Path,
) -> Result<Option<ProjectionCursorRepair>, String> {
    let connection = Connection::open_with_flags(
        projection_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("open_thread_history_projection_for_inspection_failed: {error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("configure_thread_history_projection_timeout_failed: {error}"))?;
    let Some((current_offset, expected_ordinal)) = connection
        .query_row(
            "SELECT next_rollout_byte_offset, next_rollout_ordinal
             FROM thread_history_projection_state WHERE thread_id = ?1",
            [source_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()
        .map_err(|error| format!("read_thread_history_projection_cursor_failed: {error}"))?
    else {
        return Ok(None);
    };

    let mut file = File::open(rollout_path)
        .map_err(|error| format!("open_rollout_for_projection_repair_failed: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("read_rollout_projection_repair_metadata_failed: {error}"))?
        .len();
    if current_offset >= file_len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(current_offset))
        .map_err(|error| format!("seek_rollout_projection_repair_failed: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let read = reader
        .read_until(b'\n', &mut line)
        .map_err(|error| format!("read_rollout_projection_repair_record_failed: {error}"))?;
    if read == 0 {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_slice(&line)
        .map_err(|error| format!("parse_rollout_projection_repair_record_failed: {error}"))?;
    let actual_ordinal = value
        .get("ordinal")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "projection_repair_record_missing_ordinal".to_string())?;
    if actual_ordinal >= expected_ordinal {
        return Ok(None);
    }
    let is_verified_duplicate_metadata = actual_ordinal.saturating_add(1) == expected_ordinal
        && value.get("type").and_then(serde_json::Value::as_str) == Some("event_msg")
        && value
            .pointer("/payload/type")
            .and_then(serde_json::Value::as_str)
            == Some("thread_settings_applied");
    if !is_verified_duplicate_metadata {
        return Err(format!(
            "unsafe_projection_duplicate_record: expected={expected_ordinal}, actual={actual_ordinal}"
        ));
    }
    Ok(Some(ProjectionCursorRepair {
        skipped_duplicate_count: 1,
        stalled_byte_offset: current_offset,
        stalled_expected_ordinal: expected_ordinal,
        minimum_next_byte_offset: current_offset,
        minimum_next_ordinal: actual_ordinal,
    }))
}

fn repair_verified_duplicate_projection_cursor(
    projection_db: &Path,
    source_id: &str,
    rollout_path: &Path,
) -> Result<Option<ProjectionCursorRepair>, String> {
    let Some(repair) =
        inspect_verified_duplicate_projection_cursor(projection_db, source_id, rollout_path)?
    else {
        return Ok(None);
    };
    let mut connection = Connection::open(projection_db)
        .map_err(|error| format!("open_thread_history_projection_for_repair_failed: {error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("configure_thread_history_projection_timeout_failed: {error}"))?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("begin_thread_history_projection_repair_failed: {error}"))?;
    let updated = transaction
        .execute(
            "UPDATE thread_history_projection_state
             SET next_rollout_ordinal = ?1
             WHERE thread_id = ?2
               AND next_rollout_byte_offset = ?3
               AND next_rollout_ordinal = ?4",
            rusqlite::params![
                repair.minimum_next_ordinal,
                source_id,
                repair.stalled_byte_offset,
                repair.stalled_expected_ordinal
            ],
        )
        .map_err(|error| format!("update_thread_history_projection_cursor_failed: {error}"))?;
    if updated != 1 {
        return Err("thread_history_projection_cursor_changed_during_repair".to_string());
    }
    transaction
        .commit()
        .map_err(|error| format!("commit_thread_history_projection_repair_failed: {error}"))?;
    Ok(Some(repair))
}

fn ordinal_span(line: &[u8]) -> Result<(usize, usize, u64), String> {
    const KEY: &[u8] = b"\"ordinal\"";
    let key_start = line
        .windows(KEY.len())
        .position(|window| window == KEY)
        .ok_or_else(|| "rollout_record_missing_top_level_ordinal".to_string())?;
    let mut cursor = key_start + KEY.len();
    while line.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if line.get(cursor) != Some(&b':') {
        return Err("rollout_record_invalid_ordinal_field".to_string());
    }
    cursor += 1;
    while line.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let digits_start = cursor;
    while line.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == digits_start {
        return Err("rollout_record_invalid_ordinal_value".to_string());
    }
    let ordinal_text = std::str::from_utf8(&line[digits_start..cursor])
        .map_err(|error| format!("rollout_ordinal_is_not_utf8: {error}"))?;
    let ordinal = ordinal_text
        .parse::<u64>()
        .map_err(|error| format!("rollout_ordinal_is_not_u64: {error}"))?;
    Ok((digits_start, cursor, ordinal))
}

fn normalized_ordinal(
    previous_original: Option<u64>,
    current_original: u64,
    duplicate_count: &mut usize,
) -> Result<u64, String> {
    if let Some(previous) = previous_original {
        if current_original == previous {
            *duplicate_count = duplicate_count
                .checked_add(1)
                .ok_or_else(|| "rollout_duplicate_count_overflow".to_string())?;
        } else if current_original != previous.saturating_add(1) {
            return Err(format!(
                "unsafe_rollout_ordinal_sequence: previous={previous}, current={current_original}"
            ));
        }
    }
    current_original
        .checked_add(*duplicate_count as u64)
        .ok_or_else(|| "normalized_rollout_ordinal_overflow".to_string())
}

pub(crate) fn scan_rollout_ordinals(path: &Path) -> Result<RolloutOrdinalScan, String> {
    let file =
        File::open(path).map_err(|error| format!("open_rollout_for_scan_failed: {error}"))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut line = Vec::new();
    let mut byte_len = 0u64;
    let mut line_number = 0usize;
    let mut previous_original = None;
    let mut first_ordinal = None;
    let mut last_normalized_ordinal = None;
    let mut duplicate_count = 0usize;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| format!("read_rollout_for_scan_failed: {error}"))?;
        if read == 0 {
            break;
        }
        byte_len = byte_len
            .checked_add(read as u64)
            .ok_or_else(|| "rollout_byte_length_overflow".to_string())?;
        line_number += 1;
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Err(format!("empty_rollout_record_at_line_{line_number}"));
        }
        let (_, _, original) =
            ordinal_span(&line).map_err(|error| format!("{error}_at_line_{line_number}"))?;
        let normalized = normalized_ordinal(previous_original, original, &mut duplicate_count)?;
        first_ordinal.get_or_insert(normalized);
        last_normalized_ordinal = Some(normalized);
        previous_original = Some(original);
    }

    if first_ordinal.is_none() {
        return Err("rollout_contains_no_records".to_string());
    }

    Ok(RolloutOrdinalScan {
        duplicate_count,
        byte_len,
        first_ordinal,
        last_original_ordinal: previous_original,
        last_normalized_ordinal,
    })
}

#[cfg(test)]
fn unique_sibling_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "rollout_path_has_no_parent".to_string())?;
    let source_id =
        source_id_from_rollout_path(path).unwrap_or_else(|| "unknown-source".to_string());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system_time_before_unix_epoch: {error}"))?
        .as_nanos();
    for _ in 0..32 {
        let counter = REPAIR_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".ccsm-{source_id}-{label}-{}.{}.{}",
            std::process::id(),
            now,
            counter
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("could_not_allocate_rollout_repair_sibling_path".to_string())
}

#[cfg(test)]
fn rollback_rotated_segment_moves(moved: &[(PathBuf, PathBuf)]) {
    for (original, backup) in moved.iter().rev() {
        if backup.exists() && !original.exists() {
            let _ = fs::rename(backup, original);
        }
    }
}

#[cfg(test)]
fn install_coalesced_rollout(
    config_dir: &Path,
    candidate: &RotatedRolloutRepairCandidate,
) -> Result<CoalescedRollout, String> {
    let temp_path = unique_sibling_path(&candidate.canonical_path, "coalesced.tmp")?;
    let coalesced = write_coalesced_rollout(&candidate.thread_id, &candidate.segments, &temp_path)?;
    let backup_parent = config_dir
        .parent()
        .unwrap_or(config_dir)
        .join(".cc-switch")
        .join("backups")
        .join("codex-paginated-history-repair-v2");
    fs::create_dir_all(&backup_parent)
        .map_err(|error| format!("create_rotated_rollout_backup_parent_failed: {error}"))?;
    let backup_dir = unique_sibling_path(
        &backup_parent.join(format!("{}.backup", candidate.thread_id)),
        "segments",
    )?;
    fs::create_dir(&backup_dir)
        .map_err(|error| format!("create_rotated_rollout_backup_failed: {error}"))?;

    let mut moved = Vec::new();
    for (index, original) in candidate.segments.iter().enumerate() {
        let file_name = original
            .file_name()
            .ok_or_else(|| "rotated_rollout_segment_has_no_filename".to_string())?;
        let backup = backup_dir.join(format!("{index:03}-{}", file_name.to_string_lossy()));
        if let Err(error) = fs::rename(original, &backup) {
            rollback_rotated_segment_moves(&moved);
            let _ = fs::remove_file(&temp_path);
            return Err(format!("backup_rotated_rollout_segment_failed: {error}"));
        }
        moved.push((original.clone(), backup));
    }
    if let Err(error) = fs::rename(&temp_path, &candidate.canonical_path) {
        rollback_rotated_segment_moves(&moved);
        let _ = fs::remove_file(&temp_path);
        return Err(format!("install_coalesced_rollout_failed: {error}"));
    }

    let projection_result = (|| -> Result<(), String> {
        let mut connection = Connection::open(&candidate.projection_db).map_err(|error| {
            format!("open_thread_history_projection_for_rotation_repair_failed: {error}")
        })?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| {
                format!("configure_thread_history_projection_timeout_failed: {error}")
            })?;
        let transaction = connection.transaction().map_err(|error| {
            format!("begin_thread_history_rotation_projection_reset_failed: {error}")
        })?;
        let mut projection_ids = candidate
            .segments
            .iter()
            .filter_map(|path| source_id_from_rollout_path(path))
            .collect::<Vec<_>>();
        projection_ids.push(candidate.thread_id.clone());
        projection_ids.sort();
        projection_ids.dedup();
        for table in [
            "thread_history_projection_state",
            "thread_items",
            "thread_turns",
            "thread_realtime_items",
        ] {
            let exists = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| format!("inspect_projection_table_failed: {error}"))?;
            if !exists {
                continue;
            }
            let placeholders = std::iter::repeat_n("?", projection_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE thread_id IN ({placeholders})"),
                    rusqlite::params_from_iter(projection_ids.iter()),
                )
                .map_err(|error| format!("reset_rotated_thread_projection_failed: {error}"))?;
        }
        transaction.commit().map_err(|error| {
            format!("commit_thread_history_rotation_projection_reset_failed: {error}")
        })?;
        Ok(())
    })();
    if let Err(error) = projection_result {
        let _ = fs::remove_file(&candidate.canonical_path);
        rollback_rotated_segment_moves(&moved);
        return Err(error);
    }
    log::info!(
        "Coalesced Codex rotated rollout segments: thread={}, segments={}, backup={}",
        candidate.thread_id,
        coalesced.segment_count,
        backup_dir.display()
    );
    Ok(coalesced)
}

fn source_id_from_rollout_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    let matcher = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .ok()?;
    matcher
        .find_iter(file_name)
        .last()
        .map(|matched| matched.as_str().to_ascii_lowercase())
}

fn projection_db_path(config_dir: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(config_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let version = name
                .strip_prefix("thread_history_")?
                .strip_suffix(".sqlite")?
                .parse::<u64>()
                .ok()?;
            Some((version, entry.path()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(version, _)| *version);
    candidates.pop().map(|(_, path)| path)
}

fn projection_cursor(
    projection_db: Option<&Path>,
    source_id: &str,
) -> Result<Option<(u64, u64)>, String> {
    let Some(projection_db) = projection_db else {
        return Ok(None);
    };
    let conn = Connection::open_with_flags(
        projection_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("open_thread_history_projection_failed: {error}"))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|error| format!("configure_thread_history_projection_timeout_failed: {error}"))?;
    conn.query_row(
        "SELECT next_rollout_byte_offset, next_rollout_ordinal
         FROM thread_history_projection_state WHERE thread_id = ?1",
        [source_id],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
    )
    .optional()
    .map_err(|error| format!("read_thread_history_projection_cursor_failed: {error}"))
}

fn paginated_rollout_paths() -> Result<(PathBuf, Vec<(String, PathBuf)>), String> {
    let config_dir = crate::codex_config::get_codex_config_dir();
    let config_text =
        fs::read_to_string(crate::codex_config::get_codex_config_path()).unwrap_or_default();
    let Some(state_db) =
        crate::codex_state_db::resolve_active_codex_state_db_path(&config_dir, &config_text)
    else {
        return Ok((config_dir, Vec::new()));
    };
    let conn = Connection::open_with_flags(
        &state_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("open_codex_state_for_paginated_history_failed: {error}"))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|error| format!("configure_codex_state_history_timeout_failed: {error}"))?;
    let columns = conn
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("inspect_codex_thread_schema_failed: {error}"))?;
    if !columns.iter().any(|column| column == "history_mode")
        || !columns.iter().any(|column| column == "rollout_path")
    {
        return Ok((config_dir, Vec::new()));
    }
    let mut statement = conn
        .prepare(
            "SELECT id, rollout_path FROM threads
             WHERE history_mode = 'paginated' AND rollout_path IS NOT NULL AND rollout_path != ''",
        )
        .map_err(|error| format!("prepare_paginated_rollout_query_failed: {error}"))?;
    let mut paths = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                PathBuf::from(row.get::<_, String>(1)?),
            ))
        })
        .map_err(|error| format!("query_paginated_rollout_paths_failed: {error}"))?
        .filter_map(Result::ok)
        .filter(|(_, path)| path.is_file())
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    paths.dedup_by(|left, right| left.0 == right.0);
    Ok((config_dir, paths))
}

#[cfg(test)]
fn rotated_rollout_segments(
    thread_id: &str,
    canonical_path: &Path,
) -> Result<Vec<PathBuf>, String> {
    let Some(parent) = canonical_path.parent() else {
        return Ok(Vec::new());
    };
    let mut segments = vec![canonical_path.to_path_buf()];
    for entry in fs::read_dir(parent)
        .map_err(|error| format!("read_rotated_rollout_directory_failed: {error}"))?
    {
        let entry = entry
            .map_err(|error| format!("read_rotated_rollout_directory_entry_failed: {error}"))?;
        let path = entry.path();
        if path == canonical_path
            || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
            || !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(thread_id))
        {
            continue;
        }
        match rollout_session_id(&path) {
            Ok(session_id) if session_id == thread_id => segments.push(path),
            Ok(_) => {}
            Err(error) => {
                return Err(format!(
                    "inspect_named_rotated_rollout_segment_failed: path={}, error={error}",
                    path.display()
                ));
            }
        }
    }
    if segments.len() < 2 {
        return Ok(Vec::new());
    }
    Ok(segments)
}

fn collect_rollout_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("read_rollout_directory_failed: {error}"))?
        {
            let entry = entry.map_err(|error| format!("read_rollout_entry_failed: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("read_rollout_entry_type_failed: {error}"))?;
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && source_id_from_rollout_path(&path).is_some()
            {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

fn build_repair_plan_for_paths(
    projection_db: &Path,
    active_paths: &[(String, PathBuf)],
    all_rollout_paths: &[PathBuf],
    backup_generations: &[PathBuf],
) -> RolloutRepairPlan {
    let provider_migration = migration_recovery::build_plan(
        projection_db,
        active_paths,
        all_rollout_paths,
        backup_generations,
    );
    let history_base_offsets = migration_recovery::history_base_overrides(&provider_migration);
    let mut migrated_cursor_ids = provider_migration
        .cursor_repairs
        .iter()
        .map(|repair| repair.source_id.clone())
        .collect::<HashSet<_>>();
    migrated_cursor_ids.extend(provider_migration.blocked_cursor_ids.iter().cloned());
    let duplicate_candidate_ids = provider_migration.duplicate_candidate_ids.clone();
    let mut plan = RolloutRepairPlan {
        projection_db: Some(projection_db.to_path_buf()),
        blocked: provider_migration.blocked.clone(),
        provider_migration,
        ..Default::default()
    };
    let paths_by_rollout_id = match rollout_paths_by_id(all_rollout_paths) {
        Ok(paths) => paths,
        Err(error) => {
            plan.blocked.push(error);
            return plan;
        }
    };
    let mut inspected_rollouts = HashSet::new();
    for (thread_id, active_path) in active_paths {
        let lineage = match resolve_active_rollout_lineage_from_map(
            thread_id,
            active_path,
            &paths_by_rollout_id,
            &history_base_offsets,
        ) {
            Ok(lineage) => lineage,
            Err(error) => {
                plan.blocked.push(error);
                continue;
            }
        };
        for path in lineage {
            let Some(rollout_id) = source_id_from_rollout_path(&path) else {
                plan.blocked
                    .push(format!("invalid_rollout_filename: {}", path.display()));
                continue;
            };
            if !inspected_rollouts.insert(rollout_id.clone()) {
                continue;
            }
            if migrated_cursor_ids.contains(&rollout_id) {
                continue;
            }
            if !duplicate_candidate_ids.contains(&rollout_id) {
                continue;
            }
            let repair = match inspect_verified_duplicate_projection_cursor(
                projection_db,
                &rollout_id,
                &path,
            ) {
                Ok(Some(repair)) => repair,
                Ok(None) => continue,
                Err(error) => {
                    plan.blocked.push(error);
                    continue;
                }
            };
            #[cfg(not(any(target_os = "windows", test)))]
            let _ = &repair;
            plan.candidates.push(RolloutRepairCandidate {
                path,
                source_id: rollout_id,
                projection_db: projection_db.to_path_buf(),
                #[cfg(any(target_os = "windows", test))]
                repair,
            });
        }
    }
    plan
}

fn build_repair_plan() -> Result<RolloutRepairPlan, String> {
    let (config_dir, paths) = paginated_rollout_paths()?;
    let Some(projection_db) = projection_db_path(&config_dir) else {
        return Ok(RolloutRepairPlan::default());
    };
    let mut all_rollout_paths = collect_rollout_paths(&config_dir.join("sessions"))?;
    all_rollout_paths.extend(collect_rollout_paths(
        &config_dir.join("archived_sessions"),
    )?);
    let backup_generations = migration_recovery::configured_backup_generations();
    Ok(build_repair_plan_for_paths(
        &projection_db,
        &paths,
        &all_rollout_paths,
        &backup_generations,
    ))
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn inspect_paginated_history_repair() -> Result<PaginatedHistoryRepairPreflight, String>
{
    let mut plan = build_repair_plan()?;
    plan.blocked.sort();
    plan.blocked.dedup();
    let affected_paths = plan
        .candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .chain(
            plan.provider_migration
                .cursor_repairs
                .iter()
                .map(|repair| repair.rollout_path.clone()),
        )
        .chain(
            plan.provider_migration
                .history_base_repairs
                .iter()
                .map(|repair| repair.path.clone()),
        )
        .collect::<HashSet<_>>();
    Ok(PaginatedHistoryRepairPreflight {
        affected_rollout_count: affected_paths.len(),
        duplicate_ordinal_count: plan
            .candidates
            .iter()
            .map(|candidate| candidate.repair.skipped_duplicate_count)
            .sum(),
        provider_migration_cursor_count: plan.provider_migration.cursor_repairs.len(),
        provider_migration_history_base_count: plan.provider_migration.history_base_repairs.len(),
        // `thread/revert` legitimately creates multiple immutable rollout files joined by
        // `history_base`. They are not damaged "rotated" files and must never be flattened.
        rotated_thread_count: 0,
        rotated_segment_count: 0,
        affected_bytes: affected_paths
            .iter()
            .filter_map(|path| fs::metadata(path).ok().map(|metadata| metadata.len()))
            .sum(),
        blocked_rollout_count: plan.blocked.len(),
        blocked_reason: plan.blocked.first().cloned(),
    })
}

pub(crate) fn repair_paginated_history_after_codex_exit(
) -> Result<PaginatedHistoryRepairOutcome, String> {
    let plan = build_repair_plan()?;
    let mut outcome = PaginatedHistoryRepairOutcome::default();
    if !plan.provider_migration.cursor_repairs.is_empty()
        || !plan.provider_migration.history_base_repairs.is_empty()
    {
        let backup_root = crate::config::get_app_config_dir()
            .join("backups")
            .join("codex-paginated-history-migration-recovery-v1")
            .join(format!(
                "{}_{}_{}",
                chrono::Local::now().format("%Y%m%d_%H%M%S"),
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| format!("system_time_before_unix_epoch: {error}"))?
                    .subsec_nanos()
            ));
        let repaired = migration_recovery::apply_plan(
            plan.projection_db
                .as_deref()
                .ok_or_else(|| "provider_migration_recovery_projection_db_missing".to_string())?,
            &backup_root,
            &plan.provider_migration,
        )?;
        outcome.repaired_provider_migration_cursor_count = repaired.repaired_cursor_count;
        outcome.repaired_provider_migration_history_base_count =
            repaired.repaired_history_base_count;
        outcome.repaired_rollout_count += plan
            .provider_migration
            .cursor_repairs
            .iter()
            .map(|repair| repair.rollout_path.clone())
            .chain(
                plan.provider_migration
                    .history_base_repairs
                    .iter()
                    .map(|repair| repair.path.clone()),
            )
            .collect::<HashSet<_>>()
            .len();
        for repair in &plan.provider_migration.cursor_repairs {
            let scan = scan_rollout_ordinals(&repair.rollout_path)?;
            outcome.targets.push(ProjectionCatchUpTarget {
                source_id: repair.source_id.clone(),
                rollout_path: repair.rollout_path.clone(),
                minimum_next_ordinal: scan
                    .last_original_ordinal
                    .unwrap_or(repair.expected_ordinal.saturating_sub(1))
                    .saturating_add(1),
                minimum_next_byte_offset: scan.byte_len,
            });
        }
    }
    for candidate in plan.candidates {
        let scan = scan_rollout_ordinals(&candidate.path)?;
        let Some(repaired) = repair_verified_duplicate_projection_cursor(
            &candidate.projection_db,
            &candidate.source_id,
            &candidate.path,
        )?
        else {
            continue;
        };
        log::info!(
            "Advanced Codex paginated history projection past verified duplicate metadata: source={}, duplicates={}, next_offset={}, next_ordinal={}",
            candidate.source_id,
            repaired.skipped_duplicate_count,
            repaired.minimum_next_byte_offset,
            repaired.minimum_next_ordinal
        );
        outcome.repaired_rollout_count += 1;
        outcome.repaired_duplicate_count += repaired.skipped_duplicate_count;
        outcome.targets.push(ProjectionCatchUpTarget {
            source_id: candidate.source_id,
            rollout_path: candidate.path,
            minimum_next_ordinal: scan
                .last_original_ordinal
                .unwrap_or(repaired.minimum_next_ordinal.saturating_sub(1))
                .saturating_add(1),
            minimum_next_byte_offset: scan.byte_len,
        });
    }
    Ok(outcome)
}

pub(crate) fn repaired_projections_caught_up(
    outcome: &PaginatedHistoryRepairOutcome,
) -> Result<bool, String> {
    if outcome.targets.is_empty() {
        return Ok(true);
    }
    let config_dir = crate::codex_config::get_codex_config_dir();
    let Some(projection_db) = projection_db_path(&config_dir) else {
        return Ok(false);
    };
    for target in &outcome.targets {
        let Some((next_offset, next_ordinal)) =
            projection_cursor(Some(&projection_db), &target.source_id)?
        else {
            return Ok(false);
        };
        if next_ordinal < target.minimum_next_ordinal
            || next_offset < target.minimum_next_byte_offset
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn repair_newly_stalled_projection_cursors(
    outcome: &PaginatedHistoryRepairOutcome,
) -> Result<usize, String> {
    if outcome.targets.is_empty() {
        return Ok(0);
    }
    let config_dir = crate::codex_config::get_codex_config_dir();
    let Some(projection_db) = projection_db_path(&config_dir) else {
        return Ok(0);
    };
    repair_projection_cursors_at(projection_db.as_path(), outcome)
}

fn repair_projection_cursors_at(
    projection_db: &Path,
    outcome: &PaginatedHistoryRepairOutcome,
) -> Result<usize, String> {
    let mut repaired = 0;
    for target in &outcome.targets {
        if repair_verified_duplicate_projection_cursor(
            projection_db,
            &target.source_id,
            &target.rollout_path,
        )?
        .is_some()
        {
            repaired += 1;
        }
    }
    Ok(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_session_segment(path: &Path, thread_id: &str, records: &[(u64, &str)]) {
        let text = records
            .iter()
            .map(|(ordinal, payload_type)| {
                if *payload_type == "session_meta" {
                    format!(
                        "{{\"ordinal\":{ordinal},\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\"}}}}\n"
                    )
                } else {
                    format!(
                        "{{\"ordinal\":{ordinal},\"type\":\"event_msg\",\"payload\":{{\"type\":\"{payload_type}\"}}}}\n"
                    )
                }
            })
            .collect::<String>();
        std::fs::write(path, text).expect("write rotated rollout segment");
    }

    fn write_paginated_lineage_segment(
        path: &Path,
        thread_id: &str,
        first_ordinal: u64,
        history_base: Option<(&str, u64, u64)>,
    ) {
        let history_base = history_base.map(|(rollout_id, end_ordinal, end_offset)| {
            json!({
                "thread_id": rollout_id,
                "end_ordinal_exclusive": end_ordinal,
                "end_byte_offset": end_offset,
            })
        });
        let metadata = json!({
            "ordinal": first_ordinal,
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "history_mode": "paginated",
                "history_base": history_base,
            }
        });
        let next = json!({
            "ordinal": first_ordinal + 1,
            "type": "event_msg",
            "payload": { "type": "agent_message" }
        });
        std::fs::write(path, format!("{metadata}\n{next}\n"))
            .expect("write paginated lineage segment");
    }

    fn create_projection_fixture(path: &Path, ids: &[&str], malformed_items: bool) {
        let connection = Connection::open(path).expect("projection db");
        let item_schema = if malformed_items {
            "CREATE TABLE thread_items (wrong_id TEXT PRIMARY KEY);"
        } else {
            "CREATE TABLE thread_items (thread_id TEXT PRIMARY KEY);"
        };
        connection
            .execute_batch(&format!(
                "CREATE TABLE thread_history_projection_state (thread_id TEXT PRIMARY KEY);
                 {item_schema}
                 CREATE TABLE thread_turns (thread_id TEXT PRIMARY KEY);
                 CREATE TABLE thread_realtime_items (thread_id TEXT PRIMARY KEY);"
            ))
            .expect("projection schema");
        for id in ids {
            connection
                .execute(
                    "INSERT INTO thread_history_projection_state VALUES (?1)",
                    [id],
                )
                .expect("projection state row");
            if !malformed_items {
                connection
                    .execute("INSERT INTO thread_items VALUES (?1)", [id])
                    .expect("thread item row");
            }
            connection
                .execute("INSERT INTO thread_turns VALUES (?1)", [id])
                .expect("thread turn row");
            connection
                .execute("INSERT INTO thread_realtime_items VALUES (?1)", [id])
                .expect("thread realtime row");
        }
    }

    fn write_rollout(path: &Path, records: &[(u64, &str)]) {
        let mut text = String::new();
        for (ordinal, payload_type) in records {
            text.push_str(&format!(
                "{{\"timestamp\":\"2026-08-24T00:00:00Z\",\"ordinal\":{ordinal},\"type\":\"event_msg\",\"payload\":{{\"type\":\"{payload_type}\"}}}}\n"
            ));
        }
        std::fs::write(path, text).expect("write rollout fixture");
    }

    #[test]
    fn verified_duplicate_metadata_repair_rewinds_expected_ordinal_without_rewriting_rollout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rollout = temp
            .path()
            .join("rollout-2026-08-24T00-00-00-01a00000-0000-7000-8000-000000000001.jsonl");
        write_rollout(
            &rollout,
            &[
                (9, "agent_message"),
                (10, "token_count"),
                (10, "thread_settings_applied"),
                (11, "task_started"),
                (12, "agent_message"),
            ],
        );
        let original = std::fs::read(&rollout).expect("read original rollout");
        let duplicate_start = original
            .windows(b"\"ordinal\":10,\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_settings_applied\"".len())
            .position(|window| {
                window
                    == b"\"ordinal\":10,\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_settings_applied\""
            })
            .and_then(|marker| {
                original[..marker]
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map(|newline| newline + 1)
            })
            .expect("duplicate record start") as u64;
        let db = temp.path().join("thread_history_1.sqlite");
        let connection = Connection::open(&db).expect("projection db");
        connection
            .execute_batch(
                "CREATE TABLE thread_history_projection_state (
                    thread_id TEXT PRIMARY KEY,
                    next_rollout_byte_offset INTEGER NOT NULL,
                    next_rollout_ordinal INTEGER NOT NULL
                 );",
            )
            .expect("schema");
        connection
            .execute(
                "INSERT INTO thread_history_projection_state
                 (thread_id, next_rollout_byte_offset, next_rollout_ordinal)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "01a00000-0000-7000-8000-000000000001",
                    duplicate_start,
                    11_u64
                ],
            )
            .expect("stalled cursor");
        drop(connection);

        let repaired = repair_verified_duplicate_projection_cursor(
            &db,
            "01a00000-0000-7000-8000-000000000001",
            &rollout,
        )
        .expect("verified duplicate metadata repair")
        .expect("repair was needed");

        assert_eq!(repaired.skipped_duplicate_count, 1);
        assert_eq!(repaired.minimum_next_byte_offset, duplicate_start);
        assert_eq!(repaired.minimum_next_ordinal, 10);
        assert_eq!(
            std::fs::read(&rollout).expect("rollout after repair"),
            original
        );
        let connection = Connection::open(&db).expect("read projection db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT next_rollout_byte_offset, next_rollout_ordinal
                     FROM thread_history_projection_state WHERE thread_id = ?1",
                    ["01a00000-0000-7000-8000-000000000001"],
                    |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
                )
                .expect("cursor after repair"),
            (duplicate_start, 10)
        );
    }

    #[test]
    fn verification_can_repair_a_later_duplicate_reached_after_the_initial_rewind() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_id = "01a00000-0000-7000-8000-000000000006";
        let rollout = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{source_id}.jsonl"));
        write_rollout(
            &rollout,
            &[
                (9, "token_count"),
                (9, "thread_settings_applied"),
                (10, "task_started"),
                (11, "token_count"),
                (11, "thread_settings_applied"),
                (12, "task_complete"),
            ],
        );
        let bytes = std::fs::read(&rollout).expect("rollout bytes");
        let marker = b"\"ordinal\":11,\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_settings_applied\"";
        let marker_offset = bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("later duplicate marker");
        let duplicate_start = bytes[..marker_offset]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1) as u64;
        let db = temp.path().join("thread_history_1.sqlite");
        let connection = Connection::open(&db).expect("projection db");
        connection
            .execute_batch(
                "CREATE TABLE thread_history_projection_state (
                    thread_id TEXT PRIMARY KEY,
                    next_rollout_byte_offset INTEGER NOT NULL,
                    next_rollout_ordinal INTEGER NOT NULL
                 );",
            )
            .expect("schema");
        connection
            .execute(
                "INSERT INTO thread_history_projection_state VALUES (?1, ?2, ?3)",
                rusqlite::params![source_id, duplicate_start, 12_u64],
            )
            .expect("later stalled cursor");
        drop(connection);
        let outcome = PaginatedHistoryRepairOutcome {
            targets: vec![ProjectionCatchUpTarget {
                source_id: source_id.to_string(),
                rollout_path: rollout,
                minimum_next_ordinal: 13,
                minimum_next_byte_offset: bytes.len() as u64,
            }],
            ..Default::default()
        };

        assert_eq!(repair_projection_cursors_at(&db, &outcome), Ok(1));
        let connection = Connection::open(&db).expect("read projection db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT next_rollout_byte_offset, next_rollout_ordinal
                     FROM thread_history_projection_state WHERE thread_id = ?1",
                    [source_id],
                    |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
                )
                .expect("repaired later cursor"),
            (duplicate_start, 11)
        );
    }

    #[test]
    fn projection_repair_refuses_to_skip_a_duplicate_conversation_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rollout = temp
            .path()
            .join("rollout-2026-08-24T00-00-00-01a00000-0000-7000-8000-000000000002.jsonl");
        write_rollout(
            &rollout,
            &[
                (9, "token_count"),
                (9, "agent_message"),
                (10, "task_complete"),
            ],
        );
        let bytes = std::fs::read(&rollout).expect("rollout");
        let duplicate_start = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|newline| newline as u64 + 1)
            .expect("second record");
        let db = temp.path().join("thread_history_1.sqlite");
        let connection = Connection::open(&db).expect("projection db");
        connection
            .execute_batch(
                "CREATE TABLE thread_history_projection_state (
                    thread_id TEXT PRIMARY KEY,
                    next_rollout_byte_offset INTEGER NOT NULL,
                    next_rollout_ordinal INTEGER NOT NULL
                 );",
            )
            .expect("schema");
        connection
            .execute(
                "INSERT INTO thread_history_projection_state VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "01a00000-0000-7000-8000-000000000002",
                    duplicate_start,
                    10_u64
                ],
            )
            .expect("cursor");
        drop(connection);

        let error = repair_verified_duplicate_projection_cursor(
            &db,
            "01a00000-0000-7000-8000-000000000002",
            &rollout,
        )
        .expect_err("conversation records must never be skipped");

        assert!(error.contains("unsafe_projection_duplicate_record"));
        assert_eq!(std::fs::read(&rollout).expect("unchanged rollout"), bytes);
    }

    #[test]
    fn active_history_base_lineage_accepts_a_different_parent_thread() {
        let temp = tempfile::tempdir().expect("tempdir");
        let parent_id = "01a00000-0000-7000-8000-000000000060";
        let child_id = "01a00000-0000-7000-8000-000000000061";
        let parent = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{parent_id}.jsonl"));
        let child = temp
            .path()
            .join(format!("rollout-2026-08-24T00-10-00-{child_id}.jsonl"));
        write_paginated_lineage_segment(&parent, parent_id, 0, None);
        let end = fs::metadata(&parent).unwrap().len();
        write_paginated_lineage_segment(&child, child_id, 2, Some((parent_id, 2, end)));

        assert_eq!(
            resolve_active_rollout_lineage(child_id, &child, &[parent.clone(), child.clone()]),
            Ok(vec![parent, child])
        );
    }

    #[test]
    fn active_history_base_lineage_rejects_a_cutoff_inside_a_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let id = "01a00000-0000-7000-8000-000000000062";
        let next_id = "01a00000-0000-7000-8000-000000000063";
        let parent = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{id}.jsonl"));
        let child = temp
            .path()
            .join(format!("rollout-2026-08-24T00-10-00-{next_id}.jsonl"));
        write_paginated_lineage_segment(&parent, id, 0, None);
        let before = fs::read(&parent).unwrap();
        write_paginated_lineage_segment(&child, id, 2, Some((id, 2, before.len() as u64 - 1)));

        let error = resolve_active_rollout_lineage(id, &child, &[parent.clone(), child.clone()])
            .expect_err("a shifted byte cutoff must not be accepted");
        assert!(
            error.contains("history_base_offset_not_record_boundary"),
            "{error}"
        );
        assert_eq!(fs::read(parent).unwrap(), before);
    }

    #[test]
    fn active_history_base_lineage_still_rejects_a_foreign_active_thread() {
        let temp = tempfile::tempdir().expect("tempdir");
        let id = "01a00000-0000-7000-8000-000000000064";
        let foreign = "01a00000-0000-7000-8000-000000000065";
        let path = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{id}.jsonl"));
        write_paginated_lineage_segment(&path, foreign, 0, None);
        let error = resolve_active_rollout_lineage(id, &path, &[path.clone()])
            .expect_err("only ancestor identities may differ");
        assert!(error.contains("rollout_session_id_mismatch"));
    }

    #[test]
    fn active_history_base_lineage_ignores_sibling_revert_branches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000030";
        let active_rollout_id = "01a00000-0000-7000-8000-000000000031";
        let sibling_rollout_id = "01a00000-0000-7000-8000-000000000032";
        let root = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{thread_id}.jsonl"));
        let active = temp.path().join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_{active_rollout_id}.jsonl"
        ));
        let sibling = temp.path().join(format!(
            "rollout-2026-08-24T00-20-00-{thread_id}_{sibling_rollout_id}.jsonl"
        ));
        write_paginated_lineage_segment(&root, thread_id, 0, None);
        let root_len = std::fs::metadata(&root).expect("root metadata").len();
        write_paginated_lineage_segment(&active, thread_id, 3, Some((thread_id, 2, root_len)));
        write_paginated_lineage_segment(&sibling, thread_id, 3, Some((thread_id, 2, root_len)));

        let lineage = resolve_active_rollout_lineage(
            thread_id,
            &active,
            &[root.clone(), active.clone(), sibling],
        )
        .expect("valid active lineage");

        assert_eq!(lineage, vec![root, active]);
    }

    #[test]
    fn active_history_base_lineage_rejects_a_missing_parent_rollout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000040";
        let active_rollout_id = "01a00000-0000-7000-8000-000000000041";
        let missing_rollout_id = "01a00000-0000-7000-8000-000000000042";
        let active = temp.path().join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_{active_rollout_id}.jsonl"
        ));
        write_paginated_lineage_segment(&active, thread_id, 3, Some((missing_rollout_id, 2, 100)));

        let error = resolve_active_rollout_lineage(thread_id, &active, &[active.clone()])
            .expect_err("missing history base must block repair");

        assert!(error.contains("missing_history_base_rollout"));
        assert!(
            active.exists(),
            "inspection must not rewrite the active rollout"
        );
    }

    #[test]
    fn repair_plan_does_not_classify_a_valid_revert_lineage_as_damage() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000050";
        let active_rollout_id = "01a00000-0000-7000-8000-000000000051";
        let root = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{thread_id}.jsonl"));
        let active = temp.path().join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_{active_rollout_id}.jsonl"
        ));
        write_paginated_lineage_segment(&root, thread_id, 0, None);
        let root_len = std::fs::metadata(&root).expect("root metadata").len();
        write_paginated_lineage_segment(&active, thread_id, 3, Some((thread_id, 2, root_len)));
        let projection_db = temp.path().join("thread_history_1.sqlite");
        Connection::open(&projection_db)
            .expect("projection db")
            .execute_batch(
                "CREATE TABLE thread_history_projection_state (
                    thread_id TEXT PRIMARY KEY,
                    next_rollout_byte_offset INTEGER NOT NULL,
                    next_rollout_ordinal INTEGER NOT NULL
                 );",
            )
            .expect("projection schema");

        let plan = build_repair_plan_for_paths(
            &projection_db,
            &[(thread_id.to_string(), active.clone())],
            &[root, active],
            &[],
        );

        assert!(plan.candidates.is_empty());
        assert!(plan.blocked.is_empty());
    }

    #[test]
    fn rotated_rollout_segments_are_coalesced_by_ordinal_with_newer_segment_winning_overlap() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000003";
        let parent = temp
            .path()
            .join(format!("rollout-2026-08-24T00-00-00-{thread_id}.jsonl"));
        let second = temp.path().join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_01a00000-0000-7000-8000-000000000004.jsonl"
        ));
        let third = temp.path().join(format!(
            "rollout-2026-08-24T00-20-00-{thread_id}_01a00000-0000-7000-8000-000000000005.jsonl"
        ));
        let write_segment = |path: &Path, records: &[(u64, &str)]| {
            let text = records
                .iter()
                .map(|(ordinal, payload_type)| {
                    if *payload_type == "session_meta" {
                        format!(
                            "{{\"ordinal\":{ordinal},\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\"}}}}\n"
                        )
                    } else {
                        format!(
                            "{{\"ordinal\":{ordinal},\"type\":\"event_msg\",\"payload\":{{\"type\":\"{payload_type}\"}}}}\n"
                        )
                    }
                })
                .collect::<String>();
            std::fs::write(path, text).expect("segment");
        };
        write_segment(
            &parent,
            &[
                (0, "session_meta"),
                (1, "old_one"),
                (2, "old_two"),
                (3, "aborted_tail"),
            ],
        );
        write_segment(
            &second,
            &[
                (3, "session_meta"),
                (4, "continued_four"),
                (5, "aborted_again"),
            ],
        );
        write_segment(
            &third,
            &[(5, "session_meta"), (6, "continued_six"), (7, "completed")],
        );
        let output = temp.path().join("coalesced.jsonl");

        let result = write_coalesced_rollout(
            thread_id,
            &[parent.clone(), second.clone(), third.clone()],
            &output,
        )
        .expect("coalesce safe rotated segments");

        assert_eq!(result.segment_count, 3);
        assert_eq!(result.first_ordinal, 0);
        assert_eq!(result.last_ordinal, 7);
        let rows = std::fs::read_to_string(&output).expect("coalesced output");
        let values = rows
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json"))
            .collect::<Vec<_>>();
        assert_eq!(
            values
                .iter()
                .map(|value| value.get("ordinal").and_then(serde_json::Value::as_u64))
                .collect::<Vec<_>>(),
            (0_u64..=7).map(Some).collect::<Vec<_>>()
        );
        assert_eq!(
            values[3].get("type").and_then(serde_json::Value::as_str),
            Some("session_meta")
        );
        assert_eq!(
            values[5].get("type").and_then(serde_json::Value::as_str),
            Some("session_meta")
        );
        assert!(!rows.contains("aborted_tail"));
        assert!(!rows.contains("aborted_again"));
        assert!(rows.contains("continued_six"));
    }

    #[test]
    fn rotated_rollout_detection_reports_a_corrupt_named_continuation_instead_of_ignoring_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000007";
        let canonical = temp
            .path()
            .join(format!("rollout-parent-{thread_id}.jsonl"));
        let corrupt = temp.path().join(format!(
            "rollout-child-{thread_id}_01a00000-0000-7000-8000-000000000008.jsonl"
        ));
        write_session_segment(&canonical, thread_id, &[(0, "session_meta"), (1, "first")]);
        std::fs::write(&corrupt, b"not-json\n").expect("corrupt continuation");

        let error = rotated_rollout_segments(thread_id, &canonical)
            .expect_err("a corrupt continuation must block automatic repair");

        assert!(error.contains("parse_rollout_session_metadata_failed"));
    }

    #[test]
    fn installing_coalesced_rollout_backs_up_segments_and_clears_only_lineage_projection_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join(".codex");
        let sessions_dir = config_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("sessions");
        let thread_id = "01a00000-0000-7000-8000-000000000010";
        let child_id = "01a00000-0000-7000-8000-000000000011";
        let unrelated_id = "01a00000-0000-7000-8000-000000000099";
        let canonical_path =
            sessions_dir.join(format!("rollout-2026-08-24T00-00-00-{thread_id}.jsonl"));
        let child_path = sessions_dir.join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_{child_id}.jsonl"
        ));
        write_session_segment(
            &canonical_path,
            thread_id,
            &[(0, "session_meta"), (1, "first"), (2, "interrupted")],
        );
        write_session_segment(
            &child_path,
            thread_id,
            &[(2, "session_meta"), (3, "continued")],
        );
        let projection_db = config_dir.join("thread_history_1.sqlite");
        create_projection_fixture(&projection_db, &[thread_id, child_id, unrelated_id], false);
        let candidate = RotatedRolloutRepairCandidate {
            thread_id: thread_id.to_string(),
            canonical_path: canonical_path.clone(),
            segments: vec![canonical_path.clone(), child_path.clone()],
            projection_db: projection_db.clone(),
        };

        let installed =
            install_coalesced_rollout(&config_dir, &candidate).expect("install coalesced rollout");

        assert_eq!(installed.segment_count, 2);
        assert!(canonical_path.exists());
        assert!(!child_path.exists());
        let canonical = std::fs::read_to_string(&canonical_path).expect("canonical rollout");
        assert!(canonical.contains("continued"));
        assert!(!canonical.contains("interrupted"));
        let backup_root = temp
            .path()
            .join(".cc-switch/backups/codex-paginated-history-repair-v2");
        let backup_dir = std::fs::read_dir(&backup_root)
            .expect("backup root")
            .next()
            .expect("backup directory")
            .expect("backup entry")
            .path();
        assert_eq!(
            std::fs::read_dir(backup_dir).expect("backup files").count(),
            2
        );
        let connection = Connection::open(&projection_db).expect("projection db");
        for table in [
            "thread_history_projection_state",
            "thread_items",
            "thread_turns",
            "thread_realtime_items",
        ] {
            let remaining = connection
                .query_row(
                    &format!("SELECT group_concat(thread_id) FROM {table}"),
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .expect("remaining projection rows");
            assert_eq!(remaining.as_deref(), Some(unrelated_id));
        }
    }

    #[test]
    fn projection_reset_failure_restores_every_original_rotated_segment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join(".codex");
        let sessions_dir = config_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("sessions");
        let thread_id = "01a00000-0000-7000-8000-000000000020";
        let child_id = "01a00000-0000-7000-8000-000000000021";
        let canonical_path =
            sessions_dir.join(format!("rollout-2026-08-24T00-00-00-{thread_id}.jsonl"));
        let child_path = sessions_dir.join(format!(
            "rollout-2026-08-24T00-10-00-{thread_id}_{child_id}.jsonl"
        ));
        write_session_segment(
            &canonical_path,
            thread_id,
            &[(0, "session_meta"), (1, "first"), (2, "interrupted")],
        );
        write_session_segment(
            &child_path,
            thread_id,
            &[(2, "session_meta"), (3, "continued")],
        );
        let original_canonical = std::fs::read(&canonical_path).expect("canonical bytes");
        let original_child = std::fs::read(&child_path).expect("child bytes");
        let projection_db = config_dir.join("thread_history_1.sqlite");
        create_projection_fixture(&projection_db, &[thread_id, child_id], true);
        let candidate = RotatedRolloutRepairCandidate {
            thread_id: thread_id.to_string(),
            canonical_path: canonical_path.clone(),
            segments: vec![canonical_path.clone(), child_path.clone()],
            projection_db,
        };

        let error = install_coalesced_rollout(&config_dir, &candidate)
            .expect_err("malformed projection table must abort repair");

        assert!(error.contains("reset_rotated_thread_projection_failed"));
        assert_eq!(
            std::fs::read(&canonical_path).expect("restored canonical"),
            original_canonical
        );
        assert_eq!(
            std::fs::read(&child_path).expect("restored child"),
            original_child
        );
        let connection = Connection::open(&candidate.projection_db).expect("projection db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM thread_history_projection_state",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("projection rows remain"),
            2
        );
    }

    #[test]
    fn provider_migration_backup_maps_an_old_record_boundary_to_current_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let backup = temp.path().join("backup.jsonl");
        let current = temp.path().join("current.jsonl");
        let old_provider = "codex_model_router_v2";
        let new_provider = "openai";
        let backup_text = format!(
            "{{\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{{\"id\":\"01a00000-0000-7000-8000-000000000070\",\"history_mode\":\"paginated\",\"model_provider\":\"{old_provider}\"}}}}\n{{\"ordinal\":1,\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\"}}}}\n"
        );
        let current_text = backup_text.replace(old_provider, new_provider)
            + "{\"ordinal\":2,\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
        fs::write(&backup, backup_text.as_bytes()).expect("backup");
        fs::write(&current, current_text.as_bytes()).expect("current");

        let mapping = map_provider_migration_boundary(&backup, &current, backup_text.len() as u64)
            .expect("provider-only migration should be provable")
            .expect("changed boundary");

        assert_eq!(mapping.current_offset, (backup_text.len() - 15) as u64);
        assert_eq!(mapping.changed_provider_records, 1);
        assert_eq!(mapping.backup_end_ordinal, 1);
    }

    #[test]
    fn history_base_rewrite_preserves_the_first_record_byte_length() {
        let old_offset = 1_128_203_u64;
        let new_offset = 1_128_173_u64;
        let line = "{\"ordinal\":106,\"type\":\"session_meta\",\"payload\":{\"id\":\"01a00000-0000-7000-8000-000000000071\",\"history_mode\":\"paginated\",\"history_base\":{\"thread_id\":\"01a00000-0000-7000-8000-000000000070\",\"end_ordinal_exclusive\":106,\"end_byte_offset\":__OFFSET__}}}\n"
            .replace("__OFFSET__", &old_offset.to_string());

        let rewritten = rewrite_history_base_offset_preserving_record_len(
            line.as_bytes(),
            old_offset,
            new_offset,
        )
        .expect("same-width offset should be repairable");

        assert_eq!(rewritten.len(), line.len());
        let value: serde_json::Value = serde_json::from_slice(&rewritten).expect("valid json");
        assert_eq!(
            value
                .pointer("/payload/history_base/end_byte_offset")
                .and_then(serde_json::Value::as_u64),
            Some(new_offset)
        );
    }
}
