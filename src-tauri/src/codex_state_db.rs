//! Locating Codex's per-thread state SQLite databases.
//!
//! Codex stores thread metadata in `state_5.sqlite`, normally inside the Codex
//! config dir (`CODEX_HOME` / `~/.codex`). The SQLite location can be moved with
//! the `sqlite_home` key in `config.toml` or the `CODEX_SQLITE_HOME` env var;
//! when set, a second DB lives there. Both history migration and the session
//! list's title lookup need the same resolution, so it lives here once.

use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config::get_home_dir;

/// Filename of Codex's per-thread state database. Codex bumps the version
/// number across releases; update this single source of truth when a new state
/// DB version ships.
pub(crate) const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";

/// Env var that overrides the Codex SQLite state directory.
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

/// Resolve every candidate `state_5.sqlite` path: the config-dir DB plus, when
/// Codex is configured to keep its SQLite state elsewhere, that DB too.
///
/// `config_dir` is the Codex config dir (`~/.codex`); `config_text` is the raw
/// `config.toml` contents, used to detect a `sqlite_home` override.
pub(crate) fn codex_state_db_paths(config_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, config_dir.join(CODEX_STATE_DB_FILENAME));
    // Codex lets SQLite state move away from CODEX_HOME; config takes precedence.
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    }
    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn sqlite_home_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CODEX_SQLITE_HOME_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

/// Resolve the state DB currently used by Codex.
///
/// Configured `sqlite_home` and `CODEX_SQLITE_HOME` take precedence, then the
/// normal Codex root DB, with the legacy `sqlite/state_5.sqlite` subdirectory as
/// the final compatibility fallback.
pub(crate) fn resolve_active_codex_state_db_path(
    config_dir: &Path,
    config_text: &str,
) -> Option<PathBuf> {
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        let configured = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        if configured.exists() {
            return Some(configured);
        }
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        let configured = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        if configured.exists() {
            return Some(configured);
        }
    }

    let codex_root = config_dir.join(CODEX_STATE_DB_FILENAME);
    if codex_root.exists() {
        return Some(codex_root);
    }

    let sqlite_default = config_dir.join("sqlite").join(CODEX_STATE_DB_FILENAME);
    if sqlite_default.exists() {
        return Some(sqlite_default);
    }

    None
}

/// Read the current model for a Codex thread from its active state DB.
///
/// The proxy only consults this for compaction requests, where Codex may carry
/// the previous model in the request body even though the thread's current model
/// has already switched. Returning `None` keeps the existing body-based routing.
pub(crate) fn codex_thread_model(session_id: &str) -> Option<String> {
    let config_dir = crate::codex_config::get_codex_config_dir();
    let config_path = crate::codex_config::get_codex_config_path();
    let config_text = std::fs::read_to_string(config_path).unwrap_or_default();
    let db_path = resolve_active_codex_state_db_path(&config_dir, &config_text)?;
    codex_thread_model_from_db(session_id, &db_path)
}

fn codex_thread_model_from_db(session_id: &str, db_path: &Path) -> Option<String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(250));
    conn.query_row(
        "SELECT model FROM threads WHERE id = ?1",
        [session_id],
        |row| row.get(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn includes_config_sqlite_home() {
        let temp = tempdir().expect("tempdir");
        let sqlite_home = temp.path().join("sqlite-home");
        // TOML literal strings preserve Windows backslashes without escape parsing.
        let config_text = format!("sqlite_home = '''{}'''\n", sqlite_home.display());

        let paths = codex_state_db_paths(temp.path(), &config_text);

        assert_eq!(
            paths,
            vec![
                temp.path().join(CODEX_STATE_DB_FILENAME),
                sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
    }

    #[test]
    fn active_state_db_prefers_configured_home_and_query_reads_thread_model() {
        let temp = tempdir().expect("tempdir");
        let sqlite_home = temp.path().join("sqlite-home");
        std::fs::create_dir_all(&sqlite_home).expect("create sqlite home");
        let db_path = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        let conn = rusqlite::Connection::open(&db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model TEXT); \
             INSERT INTO threads (id, model) VALUES ('t1', 'deepseek-v4-flash');",
        )
        .expect("seed state db");
        drop(conn);

        let config_text = format!("sqlite_home = '''{}'''\n", sqlite_home.display());
        let active =
            resolve_active_codex_state_db_path(temp.path(), &config_text).expect("active state db");
        assert_eq!(active, db_path);
        assert_eq!(
            codex_thread_model_from_db("t1", &db_path).as_deref(),
            Some("deepseek-v4-flash")
        );
        assert_eq!(codex_thread_model_from_db("missing", &db_path), None);
    }

    #[test]
    fn active_state_db_prefers_codex_root_when_no_override() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join(CODEX_STATE_DB_FILENAME);
        std::fs::write(&db_path, b"root").expect("write root state db");

        let active = resolve_active_codex_state_db_path(temp.path(), "").expect("active state db");
        assert_eq!(active, db_path);
    }
}
