use super::*;

fn protected(path: &Path, reason: &str) -> AppError {
    AppError::Message(format!(
        "codex_paginated_history_immutable: {}: {reason}; provider migration cannot safely rewrite byte-addressed history",
        path.display()
    ))
}

pub(super) fn ensure_legacy_rollout(path: &Path) -> Result<(), AppError> {
    if path.to_string_lossy().ends_with(".jsonl.zst") {
        return Err(protected(path, "compressed history cannot be inspected"));
    }
    let file = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
    ensure_legacy_header(path, BufReader::new(file))
}

pub(super) fn ensure_legacy_content(path: &Path, content: &str) -> Result<(), AppError> {
    ensure_legacy_header(path, content.as_bytes())
}

fn ensure_legacy_header(path: &Path, reader: impl BufRead) -> Result<(), AppError> {
    for line in reader.lines() {
        let line = line.map_err(|e| AppError::io(path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).map_err(|_| protected(path, "invalid history header"))?;
        let payload = &value["payload"];
        if value["type"] != "session_meta" || !payload.is_object() {
            return Err(protected(path, "missing session metadata"));
        }
        if !value["ordinal"].is_null()
            || !payload["history_base"].is_null()
            || (!payload["history_mode"].is_null() && payload["history_mode"] != "legacy")
        {
            return Err(protected(path, "non-legacy history envelope"));
        }
        return Ok(());
    }
    Ok(())
}

fn ensure_legacy_tree(root: &Path) -> Result<(), AppError> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| AppError::io(root, e))? {
        let entry = entry.map_err(|e| AppError::io(root, e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| AppError::io(&path, e))?;
        if kind.is_symlink() {
            return Err(protected(
                &path,
                "linked history cannot be inspected safely",
            ));
        }
        if kind.is_dir() {
            ensure_legacy_tree(&path)?;
        } else if path.extension().is_some_and(|ext| ext == "jsonl")
            || path.to_string_lossy().ends_with(".jsonl.zst")
        {
            ensure_legacy_rollout(&path)?;
        }
    }
    Ok(())
}

pub(super) fn ensure_legacy_db(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Database(format!("inspect history database: {e}")))?;
    if Database::table_exists(&conn, "threads")?
        && Database::has_column(&conn, "threads", "history_mode")?
    {
        let nonlegacy: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE history_mode IS NOT NULL AND history_mode != 'legacy')",
            [],
            |row| row.get(0),
        ).map_err(|e| AppError::Database(format!("inspect history mode: {e}")))?;
        if nonlegacy {
            return Err(protected(path, "non-legacy thread rows"));
        }
    }
    Ok(())
}

// Preflight the entire batch before the first backup, rollout rewrite, or DB update.
pub(super) fn ensure_legacy_history(codex_dir: &Path) -> Result<(), AppError> {
    ensure_legacy_tree(&codex_dir.join("sessions"))?;
    ensure_legacy_tree(&codex_dir.join("archived_sessions"))?;
    let config_path = codex_dir.join("config.toml");
    let config = match fs::read_to_string(&config_path) {
        Ok(config) => config,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io(&config_path, error)),
    };
    for path in codex_state_db_paths(codex_dir, &config) {
        ensure_legacy_db(&path)?;
    }
    Ok(())
}
