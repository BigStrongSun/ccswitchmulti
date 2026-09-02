use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
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
    pub last_normalized_ordinal: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedRollout {
    pub backup_path: PathBuf,
    pub duplicate_count: usize,
    pub last_normalized_ordinal: u64,
    pub byte_len: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedHistoryRepairPreflight {
    pub affected_rollout_count: usize,
    pub duplicate_ordinal_count: usize,
    pub affected_bytes: u64,
    pub blocked_rollout_count: usize,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PaginatedHistoryRepairOutcome {
    pub repaired_rollout_count: usize,
    pub repaired_duplicate_count: usize,
    pub(super) targets: Vec<ProjectionCatchUpTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProjectionCatchUpTarget {
    source_id: String,
    minimum_next_ordinal: u64,
    minimum_next_byte_offset: u64,
}

#[derive(Clone, Debug)]
struct RolloutRepairCandidate {
    path: PathBuf,
    source_id: String,
    #[cfg(any(target_os = "windows", test))]
    scan: RolloutOrdinalScan,
}

#[derive(Default)]
struct RolloutRepairPlan {
    candidates: Vec<RolloutRepairCandidate>,
    blocked: Vec<String>,
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

#[cfg(windows)]
fn replace_rollout_with_backup(
    path: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let replaced: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let replacement_wide: Vec<u16> = replacement
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let backup_wide: Vec<u16> = backup
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: all three path buffers are NUL-terminated UTF-16 and remain alive
    // for the duration of the call. The application and exclusion pointers are unused.
    let replaced_ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced_ok == 0 {
        return Err(format!(
            "atomic_rollout_replace_failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_rollout_with_backup(
    path: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), String> {
    fs::rename(path, backup)
        .map_err(|error| format!("backup_rollout_before_replace_failed: {error}"))?;
    if let Err(error) = fs::rename(replacement, path) {
        let restore_error = fs::rename(backup, path).err();
        return Err(match restore_error {
            Some(restore_error) => format!(
                "install_normalized_rollout_failed: {error}; restore_original_failed: {restore_error}"
            ),
            None => format!("install_normalized_rollout_failed: {error}"),
        });
    }
    Ok(())
}

pub(crate) fn normalize_rollout_ordinals(path: &Path) -> Result<NormalizedRollout, String> {
    let scan = scan_rollout_ordinals(path)?;
    if scan.duplicate_count == 0 {
        return Err("rollout_has_no_duplicate_ordinals".to_string());
    }

    let temp_path = unique_sibling_path(path, "ordinal-normalized.tmp")?;
    let backup_path = unique_sibling_path(path, "before-ordinal-repair.bak.jsonl")?;
    let source = File::open(path)
        .map_err(|error| format!("open_rollout_for_normalization_failed: {error}"))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, source);
    let temp_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| format!("create_normalized_rollout_failed: {error}"))?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, temp_file);
    let mut line = Vec::new();
    let mut line_number = 0usize;
    let mut previous_original = None;
    let mut duplicate_count = 0usize;

    let write_result = (|| -> Result<(), String> {
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .map_err(|error| format!("read_rollout_for_normalization_failed: {error}"))?;
            if read == 0 {
                break;
            }
            line_number += 1;
            let (digits_start, digits_end, original) =
                ordinal_span(&line).map_err(|error| format!("{error}_at_line_{line_number}"))?;
            let normalized = normalized_ordinal(previous_original, original, &mut duplicate_count)?;
            writer
                .write_all(&line[..digits_start])
                .and_then(|_| writer.write_all(normalized.to_string().as_bytes()))
                .and_then(|_| writer.write_all(&line[digits_end..]))
                .map_err(|error| format!("write_normalized_rollout_failed: {error}"))?;
            previous_original = Some(original);
        }
        writer
            .flush()
            .map_err(|error| format!("flush_normalized_rollout_failed: {error}"))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| format!("sync_normalized_rollout_failed: {error}"))?;
        Ok(())
    })();
    drop(writer);
    drop(reader);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Ok(metadata) = fs::metadata(path) {
        let _ = fs::set_permissions(&temp_path, metadata.permissions());
    }

    if let Err(error) = replace_rollout_with_backup(path, &temp_path, &backup_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    let byte_len = fs::metadata(path)
        .map_err(|error| format!("read_normalized_rollout_metadata_failed: {error}"))?
        .len();

    Ok(NormalizedRollout {
        backup_path,
        duplicate_count: scan.duplicate_count,
        last_normalized_ordinal: scan
            .last_normalized_ordinal
            .expect("non-empty rollout scan has a last ordinal"),
        byte_len,
    })
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

fn normalized_ordinal_at_byte_offset(path: &Path, byte_offset: u64) -> Result<Option<u64>, String> {
    let file = File::open(path)
        .map_err(|error| format!("open_rollout_for_cursor_check_failed: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("read_rollout_cursor_metadata_failed: {error}"))?
        .len();
    if byte_offset >= file_len {
        return Ok(None);
    }
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut line = Vec::new();
    let mut current_offset = 0u64;
    let mut previous_original = None;
    let mut duplicate_count = 0usize;
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| format!("read_rollout_projection_cursor_failed: {error}"))?;
        if read == 0 {
            return Ok(None);
        }
        let (digits_start, digits_end, original) = ordinal_span(&line)?;
        let normalized = normalized_ordinal(previous_original, original, &mut duplicate_count)?;
        let next_offset = current_offset
            .checked_add(read as u64)
            .ok_or_else(|| "rollout_projection_cursor_offset_overflow".to_string())?;
        if byte_offset >= current_offset && byte_offset < next_offset {
            let normalized_digits = normalized.to_string();
            let replacement_changes_width = normalized_digits.len() != digits_end - digits_start;
            let cursor_after_ordinal = byte_offset > current_offset + digits_end as u64;
            if replacement_changes_width && cursor_after_ordinal {
                return Err("projection_cursor_would_move_after_ordinal_width_change".to_string());
            }
            return Ok(Some(normalized));
        }
        current_offset = next_offset;
        previous_original = Some(original);
    }
}

fn paginated_rollout_paths() -> Result<(PathBuf, Vec<PathBuf>), String> {
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
            "SELECT DISTINCT rollout_path FROM threads
             WHERE history_mode = 'paginated' AND rollout_path IS NOT NULL AND rollout_path != ''",
        )
        .map_err(|error| format!("prepare_paginated_rollout_query_failed: {error}"))?;
    let mut paths = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("query_paginated_rollout_paths_failed: {error}"))?
        .filter_map(Result::ok)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok((config_dir, paths))
}

fn build_repair_plan() -> Result<RolloutRepairPlan, String> {
    let (config_dir, paths) = paginated_rollout_paths()?;
    let projection_db = projection_db_path(&config_dir);
    let mut plan = RolloutRepairPlan::default();
    for path in paths {
        let source_id = match source_id_from_rollout_path(&path) {
            Some(source_id) => source_id,
            None => {
                plan.blocked
                    .push("paginated_rollout_source_id_unavailable".to_string());
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
        if scan.duplicate_count == 0 {
            continue;
        }
        let Some((next_offset, next_ordinal)) =
            projection_cursor(projection_db.as_deref(), &source_id)?
        else {
            continue;
        };
        let cursor_ordinal = match normalized_ordinal_at_byte_offset(&path, next_offset) {
            Ok(cursor_ordinal) => cursor_ordinal,
            Err(error) => {
                plan.blocked.push(error);
                continue;
            }
        };
        match cursor_ordinal {
            None => continue,
            Some(cursor_ordinal) if cursor_ordinal == next_ordinal => {}
            Some(cursor_ordinal) => {
                plan.blocked.push(format!(
                    "projection_cursor_does_not_match_normalized_rollout: expected={next_ordinal}, actual={cursor_ordinal}"
                ));
                continue;
            }
        }
        plan.candidates.push(RolloutRepairCandidate {
            path,
            source_id,
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
        affected_rollout_count: plan.candidates.len(),
        duplicate_ordinal_count: plan
            .candidates
            .iter()
            .map(|candidate| candidate.scan.duplicate_count)
            .sum(),
        affected_bytes: plan
            .candidates
            .iter()
            .map(|candidate| candidate.scan.byte_len)
            .sum(),
        blocked_rollout_count: plan.blocked.len(),
        blocked_reason: plan.blocked.first().cloned(),
    })
}

pub(crate) fn repair_paginated_history_after_codex_exit(
) -> Result<PaginatedHistoryRepairOutcome, String> {
    let plan = build_repair_plan()?;
    let mut outcome = PaginatedHistoryRepairOutcome::default();
    for candidate in plan.candidates {
        let normalized = normalize_rollout_ordinals(&candidate.path)?;
        log::info!(
            "Normalized Codex paginated history ordinals: source={}, duplicates={}, backup={}",
            candidate.source_id,
            normalized.duplicate_count,
            normalized.backup_path.display()
        );
        outcome.repaired_rollout_count += 1;
        outcome.repaired_duplicate_count += normalized.duplicate_count;
        outcome.targets.push(ProjectionCatchUpTarget {
            source_id: candidate.source_id,
            minimum_next_ordinal: normalized.last_normalized_ordinal.saturating_add(1),
            minimum_next_byte_offset: normalized.byte_len,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_rollout(path: &Path, records: &[(u64, &str)]) {
        let mut text = String::new();
        for (ordinal, payload_type) in records {
            text.push_str(&format!(
                "{{\"timestamp\":\"2026-08-24T00:00:00Z\",\"ordinal\":{ordinal},\"type\":\"event_msg\",\"payload\":{{\"type\":\"{payload_type}\"}}}}\n"
            ));
        }
        std::fs::write(path, text).expect("write rollout fixture");
    }

    fn read_ordinals(path: &Path) -> Vec<u64> {
        std::fs::read_to_string(path)
            .expect("read normalized rollout")
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .expect("valid JSONL")
                    .get("ordinal")
                    .and_then(serde_json::Value::as_u64)
                    .expect("top-level ordinal")
            })
            .collect()
    }

    #[test]
    fn normalization_preserves_duplicate_metadata_and_renumbers_every_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("rollout-source.jsonl");
        write_rollout(
            &path,
            &[
                (10306, "token_count"),
                (10307, "token_count"),
                (10307, "thread_settings_applied"),
                (10308, "agent_message"),
            ],
        );

        let result = normalize_rollout_ordinals(&path).expect("normalize duplicate-only file");

        assert_eq!(result.duplicate_count, 1);
        assert_eq!(read_ordinals(&path), vec![10306, 10307, 10308, 10309]);
        let normalized = std::fs::read_to_string(&path).expect("read normalized text");
        assert!(normalized.contains("thread_settings_applied"));
        assert!(result.backup_path.exists());
        assert_eq!(
            read_ordinals(&result.backup_path),
            vec![10306, 10307, 10307, 10308]
        );
    }

    #[test]
    fn normalization_keeps_duplicate_task_started_and_applies_cumulative_shift() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("rollout-source.jsonl");
        write_rollout(
            &path,
            &[
                (10, "token_count"),
                (10, "task_started"),
                (11, "agent_message"),
                (11, "thread_settings_applied"),
                (12, "task_complete"),
            ],
        );

        let result = normalize_rollout_ordinals(&path).expect("normalize both duplicates");

        assert_eq!(result.duplicate_count, 2);
        assert_eq!(read_ordinals(&path), vec![10, 11, 12, 13, 14]);
        let normalized = std::fs::read_to_string(&path).expect("read normalized text");
        assert!(normalized.contains("task_started"));
        assert!(normalized.contains("task_complete"));
    }

    #[test]
    fn gap_or_rewind_is_rejected_without_mutating_the_rollout() {
        for records in [vec![(10, "a"), (12, "b")], vec![(10, "a"), (9, "b")]] {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join("rollout-source.jsonl");
            write_rollout(&path, &records);
            let before = std::fs::read(&path).expect("read original");

            let error = normalize_rollout_ordinals(&path)
                .expect_err("non duplicate-only anomaly must be rejected");

            assert!(error.contains("unsafe_rollout_ordinal_sequence"));
            assert_eq!(std::fs::read(&path).expect("read unchanged file"), before);
        }
    }

    #[test]
    fn normalization_keeps_the_stalled_projection_cursor_at_the_same_record_boundary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("rollout-source.jsonl");
        write_rollout(
            &path,
            &[
                (9, "agent_message"),
                (10, "token_count"),
                (10, "task_started"),
                (11, "agent_message"),
            ],
        );
        let original = std::fs::read(&path).expect("read original");
        let duplicate_marker =
            b"\"ordinal\":10,\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"";
        let stalled_offset = original
            .windows(duplicate_marker.len())
            .position(|window| window == duplicate_marker)
            .expect("duplicate record marker");
        let record_start = original[..stalled_offset]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1);
        let stalled_offset = (record_start + b"{\"timestamp\":\"".len()) as u64;

        assert_eq!(
            normalized_ordinal_at_byte_offset(&path, stalled_offset)
                .expect("read normalized ordinal at existing cursor"),
            Some(11)
        );

        normalize_rollout_ordinals(&path).expect("normalize rollout");

        let normalized = std::fs::read(&path).expect("read normalized rollout");
        assert_eq!(
            &normalized[record_start..stalled_offset as usize],
            &original[record_start..stalled_offset as usize],
            "normalization must preserve every byte before the existing projection cursor"
        );
        assert!(std::str::from_utf8(&normalized)
            .expect("normalized rollout remains UTF-8")
            .contains("task_started"));
    }
}
