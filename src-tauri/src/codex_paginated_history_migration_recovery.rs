use super::*;

use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProviderMigrationCursorRepair {
    pub(super) source_id: String,
    pub(super) rollout_path: PathBuf,
    pub(super) old_offset: u64,
    pub(super) new_offset: u64,
    pub(super) expected_ordinal: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProviderMigrationHistoryBaseRepair {
    pub(super) path: PathBuf,
    pub(super) old_offset: u64,
    pub(super) new_offset: u64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProviderMigrationRecoveryPlan {
    pub(super) cursor_repairs: Vec<ProviderMigrationCursorRepair>,
    pub(super) history_base_repairs: Vec<ProviderMigrationHistoryBaseRepair>,
    pub(super) blocked_cursor_ids: HashSet<String>,
    pub(super) duplicate_candidate_ids: HashSet<String>,
    pub(super) blocked: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ProviderMigrationRecoveryOutcome {
    pub(super) repaired_cursor_count: usize,
    pub(super) repaired_history_base_count: usize,
}

fn record_boundary(path: &Path, offset: u64) -> Result<bool, String> {
    let mut file =
        File::open(path).map_err(|error| format!("open_rollout_boundary_check_failed: {error}"))?;
    let len = file
        .metadata()
        .map_err(|error| format!("read_rollout_boundary_metadata_failed: {error}"))?
        .len();
    if offset == 0 || offset > len {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(offset - 1))
        .map_err(|error| format!("seek_rollout_boundary_check_failed: {error}"))?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)
        .map_err(|error| format!("read_rollout_boundary_check_failed: {error}"))?;
    Ok(byte[0] == b'\n')
}

fn ordinal_at_offset(path: &Path, offset: u64) -> Result<Option<u64>, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("open_rollout_ordinal_at_offset_failed: {error}"))?;
    let len = file
        .metadata()
        .map_err(|error| format!("read_rollout_ordinal_at_offset_metadata_failed: {error}"))?
        .len();
    if offset >= len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("seek_rollout_ordinal_at_offset_failed: {error}"))?;
    let mut record = Vec::new();
    BufReader::new(file)
        .read_until(b'\n', &mut record)
        .map_err(|error| format!("read_rollout_ordinal_at_offset_failed: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&record)
        .map_err(|error| format!("parse_rollout_ordinal_at_offset_failed: {error}"))?;
    value
        .get("ordinal")
        .and_then(serde_json::Value::as_u64)
        .map(Some)
        .ok_or_else(|| "rollout_record_at_offset_missing_ordinal".to_string())
}

fn ordinal_before_boundary(path: &Path, offset: u64) -> Result<Option<u64>, String> {
    if !record_boundary(path, offset)? {
        return Ok(None);
    }
    let mut file = File::open(path)
        .map_err(|error| format!("open_rollout_ordinal_before_boundary_failed: {error}"))?;
    let mut start = offset.saturating_sub(1);
    let mut buffer = [0_u8; 64 * 1024];
    while start > 0 {
        let chunk_start = start.saturating_sub(buffer.len() as u64);
        let chunk_len = usize::try_from(start - chunk_start)
            .map_err(|_| "rollout_boundary_chunk_length_overflow".to_string())?;
        file.seek(SeekFrom::Start(chunk_start))
            .map_err(|error| format!("seek_rollout_ordinal_before_boundary_failed: {error}"))?;
        file.read_exact(&mut buffer[..chunk_len])
            .map_err(|error| format!("read_rollout_ordinal_before_boundary_failed: {error}"))?;
        if let Some(newline) = buffer[..chunk_len].iter().rposition(|byte| *byte == b'\n') {
            start = chunk_start + newline as u64 + 1;
            break;
        }
        start = chunk_start;
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("seek_rollout_boundary_record_failed: {error}"))?;
    let record_len = usize::try_from(offset - start)
        .map_err(|_| "rollout_boundary_record_length_overflow".to_string())?;
    let mut record = vec![0_u8; record_len];
    file.read_exact(&mut record)
        .map_err(|error| format!("read_rollout_boundary_record_failed: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&record)
        .map_err(|error| format!("parse_rollout_boundary_record_failed: {error}"))?;
    value
        .get("ordinal")
        .and_then(serde_json::Value::as_u64)
        .map(Some)
        .ok_or_else(|| "rollout_boundary_record_missing_ordinal".to_string())
}

enum ProjectionCursorCondition {
    Healthy,
    DuplicateCandidate,
    MigrationCandidate,
}

fn projection_cursor_condition(
    path: &Path,
    offset: u64,
    expected_ordinal: u64,
) -> Result<ProjectionCursorCondition, String> {
    if !record_boundary(path, offset)? {
        return Ok(ProjectionCursorCondition::MigrationCandidate);
    }
    Ok(match ordinal_at_offset(path, offset)? {
        None => ProjectionCursorCondition::Healthy,
        Some(ordinal) if ordinal == expected_ordinal => ProjectionCursorCondition::Healthy,
        Some(ordinal) if ordinal < expected_ordinal => {
            ProjectionCursorCondition::DuplicateCandidate
        }
        Some(_) => ProjectionCursorCondition::MigrationCandidate,
    })
}

fn history_base_is_consistent(
    parent_path: &Path,
    base: &RolloutHistoryBase,
) -> Result<bool, String> {
    Ok(ordinal_before_boundary(parent_path, base.end_byte_offset)?
        .and_then(|ordinal| ordinal.checked_add(1))
        == Some(base.end_ordinal_exclusive))
}

fn first_record(path: &Path) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("open_rollout_first_record_failed: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut record = Vec::new();
    if reader
        .read_until(b'\n', &mut record)
        .map_err(|error| format!("read_rollout_first_record_failed: {error}"))?
        == 0
    {
        return Err("rollout_contains_no_records".to_string());
    }
    Ok(record)
}

fn collect_backup_rollouts(root: &Path) -> Result<HashMap<String, Vec<PathBuf>>, String> {
    let mut by_id: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in collect_rollout_paths(&root.join("jsonl"))? {
        if let Some(source_id) = source_id_from_rollout_path(&path) {
            by_id.entry(source_id).or_default().push(path);
        }
    }
    Ok(by_id)
}

fn unique_backup_for_id<'a>(
    backups: &'a HashMap<String, Vec<PathBuf>>,
    source_id: &str,
) -> Option<&'a PathBuf> {
    let candidates = backups.get(source_id)?;
    (candidates.len() == 1).then(|| &candidates[0])
}

fn mapped_offsets_for_cursor(
    generations: &[HashMap<String, Vec<PathBuf>>],
    source_id: &str,
    current_path: &Path,
    old_offset: u64,
    expected_ordinal: u64,
) -> Option<u64> {
    generations.iter().find_map(|generation| {
        let backup = unique_backup_for_id(generation, source_id)?;
        let mapping = map_provider_migration_boundary(backup, current_path, old_offset)
            .ok()
            .flatten()?;
        (mapping.backup_end_ordinal.checked_add(1) == Some(expected_ordinal))
            .then_some(mapping.current_offset)
    })
}

fn mapped_offsets_for_history_base(
    generations: &[HashMap<String, Vec<PathBuf>>],
    child_path: &Path,
    child_metadata: &RolloutSessionMetadata,
    parent_path: &Path,
    base: &RolloutHistoryBase,
) -> Option<u64> {
    let child_id = source_id_from_rollout_path(child_path)?;
    let current_child_record = match first_record(child_path) {
        Ok(record) => record,
        Err(_) => return None,
    };
    for generation in generations {
        let Some(child_backup) = unique_backup_for_id(generation, &child_id) else {
            continue;
        };
        let Some(parent_backup) = unique_backup_for_id(generation, &base.thread_id) else {
            continue;
        };
        let Ok(backup_child_metadata) = rollout_session_metadata(child_backup) else {
            continue;
        };
        if backup_child_metadata.id != child_metadata.id
            || backup_child_metadata.history_mode != child_metadata.history_mode
            || backup_child_metadata
                .history_base
                .as_ref()
                .is_none_or(|backup_base| {
                    backup_base.thread_id != base.thread_id
                        || backup_base.end_ordinal_exclusive != base.end_ordinal_exclusive
                        || backup_base.end_byte_offset != base.end_byte_offset
                })
        {
            continue;
        }
        let Ok(backup_child_record) = first_record(child_backup) else {
            continue;
        };
        if !matches!(
            provider_only_record_change(&backup_child_record, &current_child_record),
            Ok(true)
        ) {
            continue;
        }
        if let Ok(Some(mapping)) =
            map_provider_migration_boundary(parent_backup, parent_path, base.end_byte_offset)
        {
            if mapping.backup_end_ordinal.checked_add(1) == Some(base.end_ordinal_exclusive) {
                return Some(mapping.current_offset);
            }
        }
    }
    None
}

fn active_lineage_paths(
    thread_id: &str,
    active_path: &Path,
    paths_by_id: &HashMap<String, PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    let mut lineage = Vec::new();
    let mut seen = HashSet::new();
    let mut current = active_path.to_path_buf();
    loop {
        let source_id = source_id_from_rollout_path(&current)
            .ok_or_else(|| format!("invalid_rollout_filename: {}", current.display()))?;
        if !seen.insert(source_id.clone()) {
            return Err(format!("cyclic_history_base: rollout_id={source_id}"));
        }
        let metadata = rollout_session_metadata(&current)?;
        if lineage.is_empty() && metadata.id != thread_id {
            return Err(format!(
                "rollout_session_id_mismatch: expected={thread_id}, actual={}",
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
        current = paths_by_id
            .get(&base.thread_id.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing_history_base_rollout: rollout_id={}",
                    base.thread_id
                )
            })?;
    }
    lineage.reverse();
    Ok(lineage)
}

pub(super) fn build_plan(
    projection_db: &Path,
    active_paths: &[(String, PathBuf)],
    all_rollout_paths: &[PathBuf],
    backup_generations: &[PathBuf],
) -> ProviderMigrationRecoveryPlan {
    let mut plan = ProviderMigrationRecoveryPlan::default();
    let generations = backup_generations
        .iter()
        .filter_map(|generation| collect_backup_rollouts(generation).ok())
        .collect::<Vec<_>>();
    let mut paths_by_id = HashMap::new();
    for path in all_rollout_paths {
        if let Some(source_id) = source_id_from_rollout_path(path) {
            if paths_by_id
                .insert(source_id.clone(), path.clone())
                .is_some()
            {
                plan.blocked
                    .push(format!("ambiguous_rollout_id: rollout_id={source_id}"));
            }
        }
    }
    let mut inspected = HashSet::new();
    for (thread_id, active_path) in active_paths {
        let lineage = match active_lineage_paths(thread_id, active_path, &paths_by_id) {
            Ok(lineage) => lineage,
            Err(error) => {
                plan.blocked.push(error);
                continue;
            }
        };
        for path in lineage {
            let Some(source_id) = source_id_from_rollout_path(&path) else {
                continue;
            };
            if !inspected.insert(source_id.clone()) {
                continue;
            }
            if let Ok(Some((old_offset, expected_ordinal))) =
                projection_cursor(Some(projection_db), &source_id)
            {
                match projection_cursor_condition(&path, old_offset, expected_ordinal) {
                    Ok(ProjectionCursorCondition::Healthy) => {}
                    Ok(ProjectionCursorCondition::DuplicateCandidate) => {
                        plan.duplicate_candidate_ids.insert(source_id.clone());
                    }
                    Ok(ProjectionCursorCondition::MigrationCandidate) => {
                        let offsets = mapped_offsets_for_cursor(
                            &generations,
                            &source_id,
                            &path,
                            old_offset,
                            expected_ordinal,
                        );
                        if let Some(new_offset) = offsets {
                            plan.cursor_repairs.push(ProviderMigrationCursorRepair {
                                source_id: source_id.clone(),
                                rollout_path: path.clone(),
                                old_offset,
                                new_offset,
                                expected_ordinal,
                            });
                        } else {
                            plan.blocked_cursor_ids.insert(source_id.clone());
                            plan.blocked.push(format!(
                                "provider_migration_cursor_mapping_missing: rollout_id={source_id}"
                            ));
                        }
                    }
                    Err(error) => plan.blocked.push(error),
                }
            }

            let metadata = match rollout_session_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    plan.blocked.push(error);
                    continue;
                }
            };
            let Some(base) = metadata.history_base.as_ref() else {
                continue;
            };
            let Some(parent_path) = paths_by_id.get(&base.thread_id.to_ascii_lowercase()) else {
                continue;
            };
            match history_base_is_consistent(parent_path, base) {
                Ok(true) => {}
                Ok(false) => {
                    let offsets = mapped_offsets_for_history_base(
                        &generations,
                        &path,
                        &metadata,
                        parent_path,
                        base,
                    );
                    if let Some(new_offset) = offsets {
                        plan.history_base_repairs
                            .push(ProviderMigrationHistoryBaseRepair {
                                path: path.clone(),
                                old_offset: base.end_byte_offset,
                                new_offset,
                            });
                    } else {
                        plan.blocked.push(format!(
                            "provider_migration_history_base_mapping_missing: rollout_id={source_id}"
                        ));
                    }
                }
                Err(error) => plan.blocked.push(error),
            }
        }
    }
    plan.cursor_repairs
        .sort_by(|a, b| a.source_id.cmp(&b.source_id));
    plan.history_base_repairs
        .sort_by(|a, b| a.path.cmp(&b.path));
    plan.blocked.sort();
    plan.blocked.dedup();
    plan
}

pub(super) fn configured_backup_generations() -> Vec<PathBuf> {
    let mut generations = Vec::new();
    for parent in crate::codex_history_migration::codex_history_provider_migration_backup_parents()
    {
        let Ok(entries) = fs::read_dir(parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("jsonl").is_dir() {
                generations.push(path);
            }
        }
    }
    sort_backup_generations_newest_first(&mut generations);
    generations
}

fn sort_backup_generations_newest_first(generations: &mut [PathBuf]) {
    generations.sort_by(|a, b| b.file_name().cmp(&a.file_name()).then_with(|| b.cmp(a)));
}

pub(super) fn history_base_overrides(
    plan: &ProviderMigrationRecoveryPlan,
) -> HashMap<PathBuf, u64> {
    plan.history_base_repairs
        .iter()
        .map(|repair| (repair.path.clone(), repair.new_offset))
        .collect()
}

fn write_cursor_repairs_snapshot(
    backup_root: &Path,
    repairs: &[ProviderMigrationCursorRepair],
) -> Result<(), String> {
    let snapshot = repairs
        .iter()
        .map(|repair| {
            serde_json::json!({
                "sourceId": repair.source_id,
                "oldOffset": repair.old_offset,
                "newOffset": repair.new_offset,
                "expectedOrdinal": repair.expected_ordinal,
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::to_vec_pretty(&snapshot)
        .map_err(|error| format!("serialize_provider_migration_cursor_snapshot_failed: {error}"))?;
    crate::config::atomic_write(&backup_root.join("cursor-repairs.json"), &payload)
        .map_err(|error| format!("write_provider_migration_cursor_snapshot_failed: {error}"))
}

fn rewrite_history_base_file(
    repair: &ProviderMigrationHistoryBaseRepair,
    modified: Option<std::time::SystemTime>,
) -> Result<(), String> {
    let content = fs::read(&repair.path)
        .map_err(|error| format!("read_history_base_repair_target_failed: {error}"))?;
    let newline = content
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| "history_base_repair_target_has_no_complete_record".to_string())?;
    let record_end = newline + 1;
    let rewritten_record = rewrite_history_base_offset_preserving_record_len(
        &content[..record_end],
        repair.old_offset,
        repair.new_offset,
    )?;
    let mut rewritten = content;
    rewritten[..record_end].copy_from_slice(&rewritten_record);
    crate::config::atomic_write(&repair.path, &rewritten)
        .map_err(|error| format!("write_history_base_repair_target_failed: {error}"))?;
    if let Some(modified) = modified {
        filetime::set_file_mtime(&repair.path, filetime::FileTime::from_system_time(modified))
            .map_err(|error| format!("restore_history_base_repair_mtime_failed: {error}"))?;
    }
    Ok(())
}

fn restore_history_base_files(
    repairs: &[ProviderMigrationHistoryBaseRepair],
    backup_root: &Path,
    modified_times: &HashMap<PathBuf, Option<std::time::SystemTime>>,
) -> Result<(), String> {
    for repair in repairs {
        let file_name = repair
            .path
            .file_name()
            .ok_or_else(|| "history_base_repair_target_has_no_filename".to_string())?;
        let backup = backup_root.join("jsonl").join(file_name);
        if backup.is_file() {
            let bytes = fs::read(&backup)
                .map_err(|error| format!("read_history_base_rollback_backup_failed: {error}"))?;
            crate::config::atomic_write(&repair.path, &bytes)
                .map_err(|error| format!("restore_history_base_repair_target_failed: {error}"))?;
            if let Some(Some(modified)) = modified_times.get(&repair.path) {
                filetime::set_file_mtime(
                    &repair.path,
                    filetime::FileTime::from_system_time(*modified),
                )
                .map_err(|error| format!("restore_history_base_rollback_mtime_failed: {error}"))?;
            }
        }
    }
    Ok(())
}

pub(super) fn apply_plan(
    projection_db: &Path,
    backup_root: &Path,
    plan: &ProviderMigrationRecoveryPlan,
) -> Result<ProviderMigrationRecoveryOutcome, String> {
    if plan.cursor_repairs.is_empty() && plan.history_base_repairs.is_empty() {
        return Ok(ProviderMigrationRecoveryOutcome::default());
    }
    let backup_parent = backup_root
        .parent()
        .ok_or_else(|| "provider_migration_recovery_backup_has_no_parent".to_string())?;
    fs::create_dir_all(backup_parent)
        .map_err(|error| format!("create_provider_migration_recovery_backup_failed: {error}"))?;
    fs::create_dir(backup_root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            "provider_migration_recovery_backup_already_exists".to_string()
        } else {
            format!("create_provider_migration_recovery_backup_failed: {error}")
        }
    })?;
    fs::create_dir(backup_root.join("jsonl")).map_err(|error| {
        format!("create_provider_migration_recovery_jsonl_backup_failed: {error}")
    })?;
    // A full SQLite backup of a multi-GB projection database made a tiny cursor
    // repair spend minutes to hours in the backup step. The cursor updates below
    // are a single atomic compare-and-set transaction, and this snapshot preserves
    // the exact old/new rows for audit or manual recovery without copying the DB.
    write_cursor_repairs_snapshot(backup_root, &plan.cursor_repairs)?;
    let mut modified_times = HashMap::new();
    for repair in &plan.history_base_repairs {
        let file_name = repair
            .path
            .file_name()
            .ok_or_else(|| "history_base_repair_target_has_no_filename".to_string())?;
        fs::copy(&repair.path, backup_root.join("jsonl").join(file_name))
            .map_err(|error| format!("backup_history_base_repair_target_failed: {error}"))?;
        modified_times.insert(
            repair.path.clone(),
            fs::metadata(&repair.path)
                .map_err(|error| format!("read_history_base_repair_metadata_failed: {error}"))?
                .modified()
                .ok(),
        );
    }

    let apply_files = (|| -> Result<(), String> {
        for repair in &plan.history_base_repairs {
            rewrite_history_base_file(repair, modified_times.get(&repair.path).copied().flatten())?;
            if fs::metadata(&repair.path)
                .map_err(|error| format!("read_repaired_history_base_metadata_failed: {error}"))?
                .len()
                != fs::metadata(
                    backup_root
                        .join("jsonl")
                        .join(repair.path.file_name().expect("validated filename")),
                )
                .map_err(|error| format!("read_history_base_backup_metadata_failed: {error}"))?
                .len()
            {
                return Err("history_base_repair_changed_rollout_length".to_string());
            }
            if rollout_session_metadata(&repair.path)?
                .history_base
                .is_none_or(|base| base.end_byte_offset != repair.new_offset)
            {
                return Err("history_base_repair_postcheck_failed".to_string());
            }
        }
        Ok(())
    })();
    if let Err(error) = apply_files {
        let rollback =
            restore_history_base_files(&plan.history_base_repairs, backup_root, &modified_times);
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback_error) => format!("{error}; rollback_failed: {rollback_error}"),
        });
    }

    let database_result = (|| -> Result<(), String> {
        let mut connection = Connection::open(projection_db)
            .map_err(|error| format!("open_projection_for_migration_recovery_failed: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("configure_projection_recovery_timeout_failed: {error}"))?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("begin_projection_migration_recovery_failed: {error}"))?;
        for repair in &plan.cursor_repairs {
            let changed = transaction
                .execute(
                    "UPDATE thread_history_projection_state
                     SET next_rollout_byte_offset=?1
                     WHERE thread_id=?2
                       AND next_rollout_byte_offset=?3
                       AND next_rollout_ordinal=?4",
                    rusqlite::params![
                        repair.new_offset,
                        repair.source_id,
                        repair.old_offset,
                        repair.expected_ordinal
                    ],
                )
                .map_err(|error| {
                    format!("update_provider_shifted_projection_cursor_failed: {error}")
                })?;
            if changed != 1 {
                return Err(format!(
                    "provider_shifted_projection_cursor_changed_during_repair: rollout_id={}",
                    repair.source_id
                ));
            }
            if !record_boundary(&repair.rollout_path, repair.new_offset)? {
                return Err(format!(
                    "provider_shifted_projection_cursor_postcheck_failed: rollout_id={}",
                    repair.source_id
                ));
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("commit_projection_migration_recovery_failed: {error}"))
    })();
    if let Err(error) = database_result {
        let rollback =
            restore_history_base_files(&plan.history_base_repairs, backup_root, &modified_times);
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback_error) => format!("{error}; rollback_failed: {rollback_error}"),
        });
    }

    Ok(ProviderMigrationRecoveryOutcome {
        repaired_cursor_count: plan.cursor_repairs.len(),
        repaired_history_base_count: plan.history_base_repairs.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_generations_are_sorted_globally_newest_first() {
        let mut generations = vec![
            PathBuf::from("z-provider/20260901_120000"),
            PathBuf::from("a-provider/20260903_120000"),
            PathBuf::from("m-provider/20260902_120000"),
        ];

        sort_backup_generations_newest_first(&mut generations);

        assert_eq!(
            generations,
            vec![
                PathBuf::from("a-provider/20260903_120000"),
                PathBuf::from("m-provider/20260902_120000"),
                PathBuf::from("z-provider/20260901_120000"),
            ]
        );
    }

    fn write_projection(path: &Path, thread_id: &str, offset: u64, ordinal: u64) {
        let connection = Connection::open(path).expect("projection db");
        connection
            .execute_batch(
                "CREATE TABLE thread_history_projection_state (
                    thread_id TEXT PRIMARY KEY,
                    next_rollout_byte_offset INTEGER NOT NULL,
                    next_rollout_ordinal INTEGER NOT NULL
                 );",
            )
            .expect("projection schema");
        connection
            .execute(
                "INSERT INTO thread_history_projection_state VALUES (?1, ?2, ?3)",
                rusqlite::params![thread_id, offset, ordinal],
            )
            .expect("projection cursor");
    }

    #[test]
    fn recovery_plan_repairs_provider_shifted_cursor_and_history_base_together() {
        let temp = tempfile::tempdir().expect("tempdir");
        let codex_dir = temp.path().join(".codex");
        let sessions = codex_dir.join("sessions/2026/09/03");
        fs::create_dir_all(&sessions).expect("sessions");
        let parent_id = "01a00000-0000-7000-8000-000000000080";
        let child_id = "01a00000-0000-7000-8000-000000000081";
        let parent_name = format!("rollout-parent-{parent_id}.jsonl");
        let child_name = format!("rollout-child-{child_id}.jsonl");
        let parent = sessions.join(&parent_name);
        let child = sessions.join(&child_name);
        let old_provider = "codex_model_router_v2";
        let parent_backup_text = format!(
            "{{\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{{\"id\":\"{parent_id}\",\"history_mode\":\"paginated\",\"model_provider\":\"{old_provider}\"}}}}\n{{\"ordinal\":1,\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\"}}}}\n"
        );
        let old_parent_boundary = parent_backup_text.len() as u64;
        let parent_current_text = parent_backup_text.replace(old_provider, "openai")
            + "{\"ordinal\":2,\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
        fs::write(&parent, parent_current_text).expect("current parent");

        let child_backup_text = format!(
            "{{\"ordinal\":2,\"type\":\"session_meta\",\"payload\":{{\"id\":\"{child_id}\",\"history_mode\":\"paginated\",\"model_provider\":\"{old_provider}\",\"history_base\":{{\"thread_id\":\"{parent_id}\",\"end_ordinal_exclusive\":2,\"end_byte_offset\":{old_parent_boundary}}}}}}}\n{{\"ordinal\":3,\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"}}}}\n"
        );
        let child_current_text = child_backup_text.replace(old_provider, "openai");
        fs::write(&child, child_current_text.as_bytes()).expect("current child");
        let child_len_before = fs::metadata(&child).expect("child metadata").len();

        let generation = temp.path().join("migration/20260904_180600");
        let backup_sessions = generation.join("jsonl/sessions/2026/09/03");
        fs::create_dir_all(&backup_sessions).expect("backup sessions");
        fs::write(backup_sessions.join(parent_name), parent_backup_text).expect("backup parent");
        fs::write(backup_sessions.join(child_name), child_backup_text).expect("backup child");
        let projection = codex_dir.join("thread_history_1.sqlite");
        write_projection(&projection, parent_id, old_parent_boundary, 2);

        let plan = build_plan(
            &projection,
            &[(child_id.to_string(), child.clone())],
            &[parent.clone(), child.clone()],
            std::slice::from_ref(&generation),
        );
        assert!(plan.blocked.is_empty(), "{:?}", plan.blocked);
        assert_eq!(plan.cursor_repairs.len(), 1);
        assert_eq!(plan.history_base_repairs.len(), 1);
        let full_plan = super::super::build_repair_plan_for_paths(
            &projection,
            &[(child_id.to_string(), child.clone())],
            &[parent.clone(), child.clone()],
            &[generation],
        );
        assert!(full_plan.blocked.is_empty(), "{:?}", full_plan.blocked);
        assert_eq!(full_plan.provider_migration.cursor_repairs.len(), 1);
        assert_eq!(full_plan.provider_migration.history_base_repairs.len(), 1);

        let recovery_backup = temp.path().join("recovery-backup");
        let outcome = apply_plan(&projection, &recovery_backup, &plan).expect("apply recovery");
        assert_eq!(outcome.repaired_cursor_count, 1);
        assert_eq!(outcome.repaired_history_base_count, 1);
        assert!(!recovery_backup.join("projection.sqlite").exists());
        assert!(recovery_backup.join("cursor-repairs.json").is_file());
        assert!(recovery_backup
            .join("jsonl")
            .join(child.file_name().unwrap())
            .is_file());

        let connection = Connection::open(&projection).expect("projection db");
        let repaired_offset: u64 = connection
            .query_row(
                "SELECT next_rollout_byte_offset FROM thread_history_projection_state WHERE thread_id=?1",
                [parent_id],
                |row| row.get(0),
            )
            .expect("repaired cursor");
        assert_eq!(repaired_offset, old_parent_boundary - 15);
        let metadata = rollout_session_metadata(&child).expect("repaired child metadata");
        assert_eq!(
            metadata.history_base.expect("history base").end_byte_offset,
            old_parent_boundary - 15
        );
        assert_eq!(
            fs::metadata(child).expect("child metadata").len(),
            child_len_before
        );
    }

    #[test]
    fn apply_plan_records_cursor_snapshot_without_copying_projection_database() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rollout = temp.path().join("rollout.jsonl");
        fs::write(&rollout, b"x\n").expect("rollout");
        let projection = temp.path().join("thread_history_1.sqlite");
        write_projection(&projection, "thread", 1, 1);
        let plan = ProviderMigrationRecoveryPlan {
            cursor_repairs: vec![ProviderMigrationCursorRepair {
                source_id: "thread".to_string(),
                rollout_path: rollout,
                old_offset: 1,
                new_offset: 2,
                expected_ordinal: 1,
            }],
            ..Default::default()
        };
        let backup_root = temp.path().join("recovery");

        let outcome = apply_plan(&projection, &backup_root, &plan).expect("apply recovery");

        assert_eq!(outcome.repaired_cursor_count, 1);
        assert!(
            !backup_root.join("projection.sqlite").exists(),
            "small cursor recovery must not copy the whole projection database"
        );
        let snapshot = fs::read_to_string(backup_root.join("cursor-repairs.json"))
            .expect("cursor repair snapshot");
        assert!(snapshot.contains("\"thread\""), "{snapshot}");
        assert!(snapshot.contains("\"newOffset\": 2"), "{snapshot}");
    }

    #[test]
    fn recovery_plan_blocks_a_prefix_with_non_provider_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000082";
        let name = format!("rollout-{thread_id}.jsonl");
        let current = temp.path().join(&name);
        let backup_text = format!(
            "{{\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"history_mode\":\"paginated\",\"model_provider\":\"codex_model_router_v2\"}}}}\n{{\"ordinal\":1,\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"value\":1}}}}\n"
        );
        let current_text = backup_text
            .replace("codex_model_router_v2", "openai")
            .replace("\"value\":1", "\"value\":2")
            + "{\"ordinal\":2,\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
        fs::write(&current, current_text).expect("current");
        let generation = temp.path().join("migration/generation");
        fs::create_dir_all(generation.join("jsonl")).expect("backup dir");
        fs::write(generation.join("jsonl").join(name), backup_text.as_bytes()).expect("backup");
        let projection = temp.path().join("thread_history_1.sqlite");
        write_projection(&projection, thread_id, backup_text.len() as u64, 2);

        let plan = build_plan(
            &projection,
            &[(thread_id.to_string(), current.clone())],
            &[current],
            &[generation],
        );

        assert!(plan.cursor_repairs.is_empty());
        assert!(plan
            .blocked
            .iter()
            .any(|reason| reason.contains("provider_migration_cursor_mapping_missing")));
    }

    #[test]
    fn cursor_compare_and_set_failure_rolls_back_history_base_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let thread_id = "01a00000-0000-7000-8000-000000000083";
        let child = temp.path().join("child.jsonl");
        let old_offset = 200_u64;
        let new_offset = 185_u64;
        let child_text = format!(
            "{{\"ordinal\":2,\"type\":\"session_meta\",\"payload\":{{\"id\":\"child\",\"history_mode\":\"paginated\",\"history_base\":{{\"thread_id\":\"{thread_id}\",\"end_ordinal_exclusive\":2,\"end_byte_offset\":{old_offset}}}}}}}\n"
        );
        fs::write(&child, child_text.as_bytes()).expect("child");
        let rollout = temp.path().join("parent.jsonl");
        let parent_text = "x".repeat(new_offset as usize - 1) + "\n";
        fs::write(&rollout, parent_text).expect("parent");
        let projection = temp.path().join("thread_history_1.sqlite");
        write_projection(&projection, thread_id, old_offset + 1, 2);
        let plan = ProviderMigrationRecoveryPlan {
            cursor_repairs: vec![ProviderMigrationCursorRepair {
                source_id: thread_id.to_string(),
                rollout_path: rollout,
                old_offset,
                new_offset,
                expected_ordinal: 2,
            }],
            history_base_repairs: vec![ProviderMigrationHistoryBaseRepair {
                path: child.clone(),
                old_offset,
                new_offset,
            }],
            blocked_cursor_ids: HashSet::new(),
            duplicate_candidate_ids: HashSet::new(),
            blocked: Vec::new(),
        };

        let error = apply_plan(&projection, &temp.path().join("recovery"), &plan)
            .expect_err("stale plan must fail compare-and-set");

        assert!(error.contains("cursor_changed_during_repair"), "{error}");
        assert_eq!(
            fs::read(child).expect("rolled back child"),
            child_text.as_bytes()
        );
    }

    #[test]
    fn apply_plan_refuses_to_overwrite_an_existing_recovery_backup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let projection = temp.path().join("thread_history_1.sqlite");
        write_projection(&projection, "thread", 1, 1);
        let backup_root = temp.path().join("recovery");
        fs::create_dir_all(backup_root.join("jsonl")).expect("existing backup");
        fs::write(backup_root.join("projection.sqlite"), b"prior evidence")
            .expect("prior evidence");
        let plan = ProviderMigrationRecoveryPlan {
            cursor_repairs: vec![ProviderMigrationCursorRepair {
                source_id: "thread".to_string(),
                rollout_path: temp.path().join("rollout.jsonl"),
                old_offset: 1,
                new_offset: 2,
                expected_ordinal: 1,
            }],
            ..Default::default()
        };

        let error = apply_plan(&projection, &backup_root, &plan)
            .expect_err("existing recovery evidence must not be overwritten");

        assert!(error.contains("backup_already_exists"), "{error}");
        assert_eq!(
            fs::read(backup_root.join("projection.sqlite")).expect("prior evidence remains"),
            b"prior evidence"
        );
    }
}
