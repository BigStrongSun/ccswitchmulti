use super::*;

fn history_fixture(home: &Path, name: &str, paginated: bool) -> PathBuf {
    let directory = home.join("sessions");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{name}.jsonl"));
    let mut meta = serde_json::json!({
        "type": "session_meta",
        "payload": {"id": name, "model_provider": "codex_model_router_v2"}
    });
    if paginated {
        meta["ordinal"] = serde_json::json!(0);
        meta["payload"]["history_mode"] = serde_json::json!("paginated");
    }
    fs::write(&path, format!("{meta}\n")).unwrap();
    path
}

#[test]
fn provider_migration_rejects_paginated_history_before_any_write() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = history_fixture(temp.path(), "a-legacy", false);
    let paginated = history_fixture(temp.path(), "z-paginated", true);
    let before = (fs::read(&legacy).unwrap(), fs::read(&paginated).unwrap());
    let backup = temp.path().join("backup");
    let sources = BTreeSet::from(["codex_model_router_v2".to_string()]);

    let error = migrate_codex_jsonl_files_to_target(temp.path(), &sources, &backup, "openai")
        .expect_err("do not partially migrate a mixed history tree");
    assert!(error
        .to_string()
        .contains("codex_paginated_history_immutable"));
    assert_eq!(
        (fs::read(legacy).unwrap(), fs::read(paginated).unwrap()),
        before
    );
    assert!(!backup.exists());
}

#[test]
fn all_provider_migration_rejects_archived_paginated_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = history_fixture(temp.path(), "archived", true);
    let archived = temp.path().join("archived_sessions");
    fs::create_dir_all(&archived).unwrap();
    let path = {
        let destination = archived.join("archived.jsonl");
        fs::rename(path, &destination).unwrap();
        destination
    };
    let before = fs::read(&path).unwrap();
    let error = migrate_all_non_target_codex_jsonl_files(
        temp.path(),
        &temp.path().join("backup"),
        "openai",
    )
    .expect_err("archived ancestors also carry byte references");
    assert!(error
        .to_string()
        .contains("codex_paginated_history_immutable"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn direct_provider_rewrite_rejects_paginated_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = history_fixture(temp.path(), "paginated", true);
    let before = fs::read(&path).unwrap();
    let error =
        rewrite_codex_session_file_lines(&path, temp.path(), &temp.path().join("backup"), |line| {
            rewrite_codex_provider_state_line(line, "openai")
        })
        .expect_err("the low-level writer must retain the protection");
    assert!(error
        .to_string()
        .contains("codex_paginated_history_immutable"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn provider_visibility_plan_rejects_paginated_rollout() {
    let temp = tempfile::tempdir().unwrap();
    let path = history_fixture(temp.path(), "paginated", true);
    let row = ThreadHistoryRow {
        rollout_path: Some(path.to_string_lossy().to_string()),
        ..Default::default()
    };
    let error = prepare_rollout_provider_update(&row, "openai")
        .expect_err("visibility repair must not schedule a canonical rewrite");
    assert!(error
        .to_string()
        .contains("codex_paginated_history_immutable"));
}

#[test]
fn provider_migration_rejects_paginated_database_without_rollout_files() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads (id TEXT, model_provider TEXT, history_mode TEXT);
         INSERT INTO threads VALUES ('missing-file', 'custom', 'paginated');",
    )
    .unwrap();
    let error = migrate_codex_state_db_provider_bucket_to_target(
        &db_path,
        temp.path(),
        &BTreeSet::from(["custom".to_string()]),
        &temp.path().join("backup"),
        "openai",
    )
    .expect_err("database-only migration must not split paginated provider identity");
    assert!(error
        .to_string()
        .contains("codex_paginated_history_immutable"));
    let provider: String = conn
        .query_row("SELECT model_provider FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(provider, "custom");
    assert!(!temp.path().join("backup").exists());
}

#[test]
fn provider_migration_rejects_uninspected_compressed_history() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = history_fixture(temp.path(), "legacy", false);
    let compressed = temp.path().join("sessions/future.jsonl.zst");
    fs::write(&compressed, b"opaque compressed history").unwrap();
    let before = fs::read(&legacy).unwrap();
    let result = migrate_all_non_target_codex_jsonl_files(
        temp.path(),
        &temp.path().join("backup"),
        "openai",
    );
    assert!(
        result.is_err(),
        "cannot prove compressed history safe for a provider migration"
    );
    assert_eq!(fs::read(legacy).unwrap(), before);
}

#[test]
fn provider_migration_keeps_legacy_history_supported() {
    let temp = tempfile::tempdir().unwrap();
    let path = history_fixture(temp.path(), "legacy", false);
    let sources = BTreeSet::from(["codex_model_router_v2".to_string()]);
    assert_eq!(
        migrate_codex_jsonl_files_to_target(
            temp.path(),
            &sources,
            &temp.path().join("backup"),
            "openai"
        )
        .unwrap(),
        1
    );
    let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(value["payload"]["model_provider"], "openai");
}

#[test]
fn database_guard_prevents_partial_rollout_batch_migration() {
    let temp = tempfile::tempdir().unwrap();
    let path = history_fixture(temp.path(), "legacy", false);
    let before = fs::read(&path).unwrap();
    let conn = Connection::open(temp.path().join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads (history_mode TEXT);
         INSERT INTO threads VALUES ('paginated');",
    )
    .unwrap();
    assert!(migrate_all_non_target_codex_jsonl_files(
        temp.path(),
        &temp.path().join("backup"),
        "openai",
    )
    .is_err());
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(!temp.path().join("backup").exists());
}

#[test]
fn rollout_guard_rejects_independent_history_markers_and_unknown_headers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    for header in [
        serde_json::json!({"type":"session_meta","ordinal":0,"payload":{}}),
        serde_json::json!({"type":"session_meta","payload":{"history_base":{}}}),
        serde_json::json!({"type":"session_meta","payload":{"history_mode":"future"}}),
        serde_json::json!({"type":"future","payload":{}}),
    ] {
        fs::write(&path, format!("{header}\n")).unwrap();
        assert!(migration_guard::ensure_legacy_rollout(&path).is_err());
    }
    fs::write(&path, b"{invalid json\n").unwrap();
    assert!(migration_guard::ensure_legacy_rollout(&path).is_err());
}
