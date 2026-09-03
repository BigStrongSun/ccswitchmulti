use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

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
    repair: ProjectionCursorRepair,
    #[cfg(any(target_os = "windows", test))]
    scan: RolloutOrdinalScan,
}

#[derive(Default)]
struct RolloutRepairPlan {
    candidates: Vec<RolloutRepairCandidate>,
    rotated: Vec<RotatedRolloutRepairCandidate>,
    blocked: Vec<String>,
}

#[derive(Clone, Debug)]
struct RotatedRolloutRepairCandidate {
    thread_id: String,
    canonical_path: PathBuf,
    segments: Vec<PathBuf>,
    projection_db: PathBuf,
    affected_bytes: u64,
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
struct CoalescedRollout {
    segment_count: usize,
    first_ordinal: u64,
    last_ordinal: u64,
    byte_len: u64,
}

#[derive(Clone, Debug)]
struct RolloutSegment {
    path: PathBuf,
    first_ordinal: u64,
    last_ordinal: u64,
}

fn rollout_session_id(path: &Path) -> Result<String, String> {
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
    value
        .pointer("/payload/id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "rollout_session_metadata_missing_id".to_string())
}

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

fn rollback_rotated_segment_moves(moved: &[(PathBuf, PathBuf)]) {
    for (original, backup) in moved.iter().rev() {
        if backup.exists() && !original.exists() {
            let _ = fs::rename(backup, original);
        }
    }
}

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

fn build_repair_plan() -> Result<RolloutRepairPlan, String> {
    let (config_dir, paths) = paginated_rollout_paths()?;
    let Some(projection_db) = projection_db_path(&config_dir) else {
        return Ok(RolloutRepairPlan::default());
    };
    let mut plan = RolloutRepairPlan::default();
    for (source_id, path) in paths {
        match rotated_rollout_segments(&source_id, &path) {
            Ok(segments) if !segments.is_empty() => {
                let affected_bytes = segments
                    .iter()
                    .filter_map(|segment| fs::metadata(segment).ok().map(|metadata| metadata.len()))
                    .sum();
                plan.rotated.push(RotatedRolloutRepairCandidate {
                    thread_id: source_id.clone(),
                    canonical_path: path.clone(),
                    segments,
                    projection_db: projection_db.clone(),
                    affected_bytes,
                });
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                plan.blocked.push(error);
                continue;
            }
        }
        let repair =
            match inspect_verified_duplicate_projection_cursor(&projection_db, &source_id, &path) {
                Ok(Some(repair)) => repair,
                Ok(None) => continue,
                Err(error) => {
                    plan.blocked.push(error);
                    continue;
                }
            };
        let scan = match scan_rollout_ordinals(&path) {
            Ok(scan) => scan,
            Err(error) => {
                plan.blocked.push(error);
                continue;
            }
        };
        plan.candidates.push(RolloutRepairCandidate {
            path,
            source_id,
            projection_db: projection_db.clone(),
            repair,
            #[cfg(any(target_os = "windows", test))]
            scan,
        });
    }
    Ok(plan)
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn inspect_paginated_history_repair() -> Result<PaginatedHistoryRepairPreflight, String>
{
    let plan = build_repair_plan()?;
    Ok(PaginatedHistoryRepairPreflight {
        affected_rollout_count: plan.candidates.len() + plan.rotated.len(),
        duplicate_ordinal_count: plan
            .candidates
            .iter()
            .map(|candidate| candidate.repair.skipped_duplicate_count)
            .sum(),
        rotated_thread_count: plan.rotated.len(),
        rotated_segment_count: plan.rotated.iter().map(|item| item.segments.len()).sum(),
        affected_bytes: plan
            .candidates
            .iter()
            .map(|candidate| candidate.scan.byte_len)
            .sum::<u64>()
            + plan
                .rotated
                .iter()
                .map(|candidate| candidate.affected_bytes)
                .sum::<u64>(),
        blocked_rollout_count: plan.blocked.len(),
        blocked_reason: plan.blocked.first().cloned(),
    })
}

pub(crate) fn repair_paginated_history_after_codex_exit(
) -> Result<PaginatedHistoryRepairOutcome, String> {
    let plan = build_repair_plan()?;
    let mut outcome = PaginatedHistoryRepairOutcome::default();
    let config_dir = crate::codex_config::get_codex_config_dir();
    for candidate in plan.rotated {
        let coalesced = install_coalesced_rollout(&config_dir, &candidate)?;
        outcome.repaired_rollout_count += 1;
        outcome.repaired_rotated_thread_count += 1;
        outcome.repaired_rotated_segment_count += coalesced.segment_count;
        outcome.targets.push(ProjectionCatchUpTarget {
            source_id: candidate.thread_id,
            rollout_path: candidate.canonical_path,
            minimum_next_ordinal: coalesced.last_ordinal.saturating_add(1),
            minimum_next_byte_offset: coalesced.byte_len,
        });
    }
    for candidate in plan.candidates {
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
            minimum_next_ordinal: candidate
                .scan
                .last_original_ordinal
                .unwrap_or(repaired.minimum_next_ordinal.saturating_sub(1))
                .saturating_add(1),
            minimum_next_byte_offset: candidate.scan.byte_len,
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
    repair_projection_cursors_at(&projection_db, outcome)
}

fn repair_projection_cursors_at(
    projection_db: &Path,
    outcome: &PaginatedHistoryRepairOutcome,
) -> Result<usize, String> {
    let mut repaired = 0;
    for target in &outcome.targets {
        if repair_verified_duplicate_projection_cursor(
            &projection_db,
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
            affected_bytes: 0,
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
            affected_bytes: 0,
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
}
