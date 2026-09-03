//! 数据库模块测试
//!
//! 包含 Schema 迁移和基本功能的测试。

use super::*;
use crate::app_config::MultiAppConfig;
use crate::provider::{Provider, ProviderManager};
use indexmap::IndexMap;
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashMap;
use tempfile::NamedTempFile;

const LEGACY_SCHEMA_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

// v3.8.x（schema v1）的真实表结构快照：用于验证从 v3.8.* 升级到当前版本的迁移链路
// 参考：tag v3.8.3 的 src-tauri/src/database/schema.rs
pub(super) const V3_8_SCHEMA_V1_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        website_url TEXT,
        category TEXT,
        created_at INTEGER,
        sort_index INTEGER,
        notes TEXT,
        icon TEXT,
        icon_color TEXT,
        meta TEXT NOT NULL DEFAULT '{}',
        is_current BOOLEAN NOT NULL DEFAULT 0,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL,
        added_at INTEGER,
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL,
        description TEXT,
        homepage TEXT,
        docs TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        enabled_claude BOOLEAN NOT NULL DEFAULT 0,
        enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        description TEXT,
        enabled BOOLEAN NOT NULL DEFAULT 1,
        created_at INTEGER,
        updated_at INTEGER,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0,
        installed_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        branch TEXT NOT NULL DEFAULT 'main',
        enabled BOOLEAN NOT NULL DEFAULT 1,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

#[derive(Debug)]
struct ColumnInfo {
    r#type: String,
    notnull: i64,
    default: Option<String>,
}

fn get_column_info(conn: &Connection, table: &str, column: &str) -> ColumnInfo {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\");"))
        .expect("prepare pragma");
    let mut rows = stmt.query([]).expect("query pragma");
    while let Some(row) = rows.next().expect("read row") {
        let column_name: String = row.get(1).expect("name");
        if column_name.eq_ignore_ascii_case(column) {
            return ColumnInfo {
                r#type: row.get::<_, String>(2).expect("type"),
                notnull: row.get::<_, i64>(3).expect("notnull"),
                default: row.get::<_, Option<String>>(4).ok().flatten(),
            };
        }
    }
    panic!("column {table}.{column} not found");
}

fn normalize_default(default: &Option<String>) -> Option<String> {
    default
        .as_ref()
        .map(|s| s.trim_matches('\'').trim_matches('"').to_string())
}

#[test]
fn deleted_default_skill_repo_is_not_restored() {
    let db = Database::memory().expect("create memory db");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 4);
    for repo in db.get_skill_repos().expect("get initialized repos") {
        db.delete_skill_repo(&repo.owner, &repo.name)
            .expect("delete repo");
    }
    assert!(db.get_skill_repos().expect("get deleted repos").is_empty());

    assert_eq!(
        db.init_default_skill_repos().expect("reinitialize repos"),
        0
    );
    assert!(db.get_skill_repos().expect("get repos").is_empty());
}

#[test]
fn existing_skill_repo_selection_is_not_supplemented() {
    let db = Database::memory().expect("create memory db");
    let default_store = crate::services::skill::SkillStore::default();
    db.save_skill_repo(&default_store.repos[0])
        .expect("save existing repo");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 0);
    assert_eq!(db.get_skill_repos().expect("get repos").len(), 1);
    assert!(db
        .get_bool_flag("default_skill_repos_initialized")
        .expect("get initialized flag"));
}

#[test]
fn schema_migration_sets_user_version_when_missing() {
    let conn = Connection::open_in_memory().expect("open memory db");

    Database::create_tables_on_conn(&conn).expect("create tables");
    assert_eq!(
        Database::get_user_version(&conn).expect("read version before"),
        0
    );

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migration");

    assert_eq!(
        Database::get_user_version(&conn).expect("read version after"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_rejects_future_version() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::set_user_version(&conn, SCHEMA_VERSION + 1).expect("set future version");

    let err =
        Database::apply_schema_migrations_on_conn(&conn).expect_err("should reject higher version");
    assert!(
        err.to_string().contains("数据库版本过新"),
        "unexpected error: {err}"
    );
}

#[test]
fn schema_migration_adds_missing_columns_for_providers() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 创建旧版 providers 表，缺少新增列
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 验证关键新增列已补齐
    for (table, column) in [
        ("providers", "meta"),
        ("providers", "is_current"),
        ("provider_endpoints", "added_at"),
        ("mcp_servers", "enabled_gemini"),
        ("prompts", "updated_at"),
        ("skills", "installed_at"),
        ("skill_repos", "enabled"),
    ] {
        assert!(
            Database::has_column(&conn, table, column).expect("check column"),
            "{table}.{column} should exist after migration"
        );
    }

    // 验证 meta 列约束保持一致
    let meta = get_column_info(&conn, "providers", "meta");
    assert_eq!(meta.notnull, 1, "meta should be NOT NULL");
    assert_eq!(
        normalize_default(&meta.default).as_deref(),
        Some("{}"),
        "meta default should be '{{}}'"
    );

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_aligns_column_defaults_and_types() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let is_current = get_column_info(&conn, "providers", "is_current");
    assert_eq!(is_current.r#type, "BOOLEAN");
    assert_eq!(is_current.notnull, 1);
    assert_eq!(normalize_default(&is_current.default).as_deref(), Some("0"));

    let tags = get_column_info(&conn, "mcp_servers", "tags");
    assert_eq!(tags.r#type, "TEXT");
    assert_eq!(tags.notnull, 1);
    assert_eq!(normalize_default(&tags.default).as_deref(), Some("[]"));

    let enabled = get_column_info(&conn, "prompts", "enabled");
    assert_eq!(enabled.r#type, "BOOLEAN");
    assert_eq!(enabled.notnull, 1);
    assert_eq!(normalize_default(&enabled.default).as_deref(), Some("1"));

    let installed_at = get_column_info(&conn, "skills", "installed_at");
    assert_eq!(installed_at.r#type, "INTEGER");
    assert_eq!(installed_at.notnull, 1);
    assert_eq!(
        normalize_default(&installed_at.default).as_deref(),
        Some("0")
    );

    let branch = get_column_info(&conn, "skill_repos", "branch");
    assert_eq!(branch.r#type, "TEXT");
    assert_eq!(normalize_default(&branch.default).as_deref(), Some("main"));

    let skill_repo_enabled = get_column_info(&conn, "skill_repos", "enabled");
    assert_eq!(skill_repo_enabled.r#type, "BOOLEAN");
    assert_eq!(skill_repo_enabled.notnull, 1);
    assert_eq!(
        normalize_default(&skill_repo_enabled.default).as_deref(),
        Some("1")
    );
}

#[test]
fn schema_v16_migration_creates_protocol_compatibility_profiles() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::set_user_version(&conn, 16).expect("set user_version=16");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='protocol_compatibility_profiles'",
            [],
            |row| row.get(0),
        )
        .expect("query table");
    assert_eq!(count, 1);
    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_v17_migration_creates_reasoning_manual_overrides() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::set_user_version(&conn, 17).expect("set user_version=17");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='codex_reasoning_manual_overrides'",
            [],
            |row| row.get(0),
        )
        .expect("query manual override table");
    assert_eq!(count, 1);
    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_v18_migration_creates_protocol_probe_observations() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::set_user_version(&conn, 18).expect("set user_version=18");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='protocol_probe_observations'",
            [],
            |row| row.get(0),
        )
        .expect("query observation table");
    assert_eq!(count, 1);
    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn protocol_compatibility_profile_round_trips_only_for_the_exact_target() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let target = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    let result = ProtocolCompatibilityProbeResult {
        selected_transport: Some(TransportKind::OpenAiChat),
        readiness: ProbeReadiness::Verified,
        branches: Vec::new(),
    };
    let record = ProtocolCompatibilityRecord::new(target.clone(), result, 1_000, 2_000);

    db.save_protocol_compatibility_result(&record)
        .expect("save profile");
    assert_eq!(
        db.get_protocol_compatibility_result(&target)
            .expect("read exact profile"),
        Some(record.clone())
    );

    let changed_endpoint = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://other.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    assert!(db
        .get_protocol_compatibility_result(&changed_endpoint)
        .expect("read changed target")
        .is_none());

    let listed = db
        .list_protocol_compatibility_profiles("provider-a")
        .expect("list provider profiles");
    assert_eq!(listed, vec![record]);
}

#[test]
fn protocol_probe_observations_round_trip_independently_for_each_transport() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let responses_target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();
    let chat_target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    let selected_transport = Some(TransportKind::OpenAiResponses);
    let responses = ProtocolCompatibilityRecord::new(
        responses_target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport,
            readiness: ProbeReadiness::Verified,
            branches: Vec::new(),
        },
        1_000,
        3_000,
    );
    let chat = ProtocolCompatibilityRecord::new(
        chat_target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport,
            readiness: ProbeReadiness::Partial,
            branches: Vec::new(),
        },
        1_001,
        2_000,
    );

    db.save_protocol_probe_observations(&[responses.clone(), chat.clone()])
        .expect("save both transport observations");

    assert_eq!(
        db.get_protocol_probe_observation(&responses_target)
            .expect("read Responses observation"),
        Some(responses.clone())
    );
    assert_eq!(
        db.get_protocol_probe_observation(&chat_target)
            .expect("read Chat observation"),
        Some(chat.clone())
    );
    let listed = db
        .list_protocol_probe_observations("provider-a")
        .expect("list provider observations");
    assert_eq!(listed.len(), 2);
    assert!(listed.contains(&responses));
    assert!(listed.contains(&chat));
}

#[test]
fn protocol_probe_observation_round_trips_numeric_usage_evidence() {
    use crate::protocol_compatibility::{
        ProbeTargetKey, ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let target = ProbeTargetKey::new(
        "provider-usage",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();
    let result = serde_json::from_value(serde_json::json!({
        "selected_transport": "open_ai_responses",
        "readiness": "verified",
        "branches": [{
            "assessment": {
                "transport": "open_ai_responses",
                "baseline": "passed",
                "streaming": "passed",
                "forced_tool": "passed",
                "continuation": "passed"
            },
            "reasoning_shape": {
                "semantic": "readable",
                "source": "native_responses",
                "pre_tool_visible_content": "absent"
            },
            "evidence": [{
                "status_code": 200,
                "paths": ["response.usage.total_tokens"],
                "fields": [],
                "event_types": ["response.completed"],
                "usage": {
                    "inputTokens": 80,
                    "outputTokens": 20,
                    "cacheReadTokens": 10,
                    "cacheCreationTokens": 0,
                    "totalTokens": 100
                }
            }],
            "failures": []
        }]
    }))
    .expect("deserialize compatibility result with usage evidence");
    let record = ProtocolCompatibilityRecord::new(target.clone(), result, 1_000, 3_000);

    db.save_protocol_probe_observations(std::slice::from_ref(&record))
        .expect("save usage evidence");
    let restored = db
        .get_protocol_probe_observation(&target)
        .expect("read usage evidence")
        .expect("stored observation");
    let serialized = serde_json::to_value(restored).expect("serialize restored observation");

    assert_eq!(
        serialized.pointer("/result/branches/0/evidence/0/usage/totalTokens"),
        Some(&serde_json::json!(100))
    );
    assert_eq!(
        serialized.pointer("/result/branches/0/evidence/0/usage/cacheReadTokens"),
        Some(&serde_json::json!(10))
    );
}

#[test]
fn protocol_probe_bundle_rolls_back_selection_when_observation_write_fails() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();
    let selection = ProtocolCompatibilityRecord::new(
        target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiResponses),
            readiness: ProbeReadiness::Verified,
            branches: Vec::new(),
        },
        1_000,
        3_000,
    );
    let observation = selection.clone();
    {
        let conn = db.conn.lock().expect("lock database");
        conn.execute_batch(
            "CREATE TRIGGER fail_protocol_observation_insert
             BEFORE INSERT ON protocol_probe_observations
             BEGIN
               SELECT RAISE(ABORT, 'injected observation failure');
             END;",
        )
        .expect("install observation failure trigger");
    }

    let error = db
        .save_protocol_probe_bundle(&selection, &[observation])
        .expect_err("observation failure must abort the diagnostic bundle");

    assert!(error.to_string().contains("injected observation failure"));
    assert!(db
        .get_protocol_compatibility_result(&target)
        .expect("read rolled-back selection")
        .is_none());
    assert!(db
        .get_protocol_probe_observation(&target)
        .expect("read rolled-back observation")
        .is_none());
}

#[test]
fn deleting_logical_provider_deletes_its_protocol_probe_observations() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let provider = crate::provider::Provider::with_id(
        "provider-a".to_string(),
        "Provider A".to_string(),
        serde_json::json!({"auth": {}, "config": ""}),
        None,
    );
    db.save_provider("codex", &provider)
        .expect("save logical provider");
    let target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();
    let observation = ProtocolCompatibilityRecord::new(
        target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiResponses),
            readiness: ProbeReadiness::Verified,
            branches: Vec::new(),
        },
        1_000,
        3_000,
    );
    db.save_protocol_probe_observations(&[observation])
        .expect("save observation");

    db.delete_provider("codex", "provider-a")
        .expect("delete logical provider");

    assert!(db
        .get_protocol_probe_observation(&target)
        .expect("query deleted observation")
        .is_none());
}

#[test]
fn pruning_protocol_state_removes_expired_observations() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let make_observation = |public_model: &str, expires_at: i64| {
        let target = ProbeTargetKey::new(
            "provider-a",
            None::<String>,
            public_model,
            public_model,
            TransportKind::OpenAiResponses,
            "https://example.test/v1/responses",
            "bearer",
        )
        .unwrap();
        ProtocolCompatibilityRecord::new(
            target,
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiResponses),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            1_000,
            expires_at,
        )
    };
    let expired = make_observation("expired", 1_999);
    let current = make_observation("current", 2_001);
    db.save_protocol_probe_observations(&[expired.clone(), current.clone()])
        .expect("save observations");

    assert_eq!(
        db.prune_protocol_compatibility_results(2_000)
            .expect("prune protocol state"),
        1
    );
    assert!(db
        .get_protocol_probe_observation(&expired.target)
        .expect("read expired observation")
        .is_none());
    assert_eq!(
        db.get_protocol_probe_observation(&current.target)
            .expect("read current observation"),
        Some(current)
    );
}

#[test]
fn runtime_shape_mismatch_expires_only_the_exact_protocol_profile() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    let record = ProtocolCompatibilityRecord::new(
        target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiChat),
            readiness: ProbeReadiness::Verified,
            branches: Vec::new(),
        },
        1_000,
        2_000,
    );
    db.save_protocol_compatibility_result(&record)
        .expect("save profile");

    assert!(db
        .expire_protocol_compatibility_result(&target, 1_500)
        .expect("expire exact profile"));
    let expired = db
        .get_protocol_compatibility_result(&target)
        .expect("read expired profile")
        .expect("profile retained for diagnostics");
    assert_eq!(expired.expires_at, 1_499);

    let changed_target = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "other-public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    assert!(!db
        .expire_protocol_compatibility_result(&changed_target, 1_500)
        .expect("unmatched target is a no-op"));
}

#[test]
fn reasoning_manual_override_uses_target_bound_compare_and_swap_revisions() {
    use crate::protocol_compatibility::{
        HistoryReplay, ManualReasoningOverride, ProbeTargetKey, ReasoningProjection,
        ReasoningSemantic, ReasoningSource, TransportKind,
    };

    let db = Database::memory().expect("create memory db");
    let target = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap()
    .with_credential("secret-a");
    let override_spec = ManualReasoningOverride::new(
        ReasoningSemantic::Readable,
        ReasoningSource::ReasoningContent,
        HistoryReplay::ChatReasoningContent,
    );

    let first = db
        .save_reasoning_manual_override(
            &target,
            override_spec,
            ReasoningProjection::RawReasoningText,
            "verified against provider documentation",
            100,
            0,
        )
        .expect("create override");
    assert_eq!(first.revision, 1);
    assert_eq!(first.target, target);
    assert_eq!(
        db.get_reasoning_manual_override(&target)
            .expect("read override"),
        Some(first.clone())
    );
    let backup = db.export_sql_string().expect("export SQL backup");
    assert!(backup.contains("codex_reasoning_manual_overrides"));
    assert!(backup.contains("verified against provider documentation"));

    let stale = db
        .save_reasoning_manual_override(
            &target,
            override_spec,
            ReasoningProjection::ReasoningSummary,
            "stale update",
            101,
            0,
        )
        .expect_err("stale revision must fail");
    assert!(stale.to_string().contains("revision_conflict"));

    let cleared_revision = db
        .clear_reasoning_manual_override(&target, 1, 102)
        .expect("clear override");
    assert_eq!(cleared_revision, 2);
    assert!(db
        .get_reasoning_manual_override(&target)
        .expect("read cleared override")
        .is_none());

    let stale_after_clear = db
        .save_reasoning_manual_override(
            &target,
            override_spec,
            ReasoningProjection::RawReasoningText,
            "stale create after clear",
            103,
            0,
        )
        .expect_err("clear tombstone must retain revision");
    assert!(stale_after_clear.to_string().contains("revision_conflict"));

    let recreated = db
        .save_reasoning_manual_override(
            &target,
            override_spec,
            ReasoningProjection::RawReasoningText,
            "recreate from current revision",
            104,
            cleared_revision,
        )
        .expect("recreate override");
    assert_eq!(recreated.revision, 3);
}

#[test]
fn provider_and_protocol_profile_commit_together_or_not_at_all() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("memory database");
    let provider = crate::provider::Provider::with_id(
        "provider-a".to_string(),
        "Provider A".to_string(),
        serde_json::json!({"auth": {}, "config": ""}),
        None,
    );
    let mismatched_target = ProbeTargetKey::new(
        "provider-b",
        None::<String>,
        "model-a",
        "model-a",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    let record = ProtocolCompatibilityRecord::new(
        mismatched_target.clone(),
        ProtocolCompatibilityProbeResult {
            selected_transport: None,
            readiness: ProbeReadiness::Unverified,
            branches: Vec::new(),
        },
        100,
        200,
    );

    assert!(db
        .save_provider_with_protocol_profile("codex", &provider, &record)
        .is_err());
    assert!(db
        .get_provider_by_id("provider-a", "codex")
        .unwrap()
        .is_none());
    assert!(db
        .get_protocol_compatibility_result(&mismatched_target)
        .unwrap()
        .is_none());
}

#[test]
fn provider_and_every_routed_protocol_profile_commit_in_one_transaction() {
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };

    let db = Database::memory().expect("memory database");
    let provider = crate::provider::Provider::with_id(
        "router-a".to_string(),
        "Router A".to_string(),
        serde_json::json!({"auth": {}, "config": ""}),
        None,
    );
    let records = ["route-a", "route-b"].map(|route_id| {
        let target = ProbeTargetKey::new(
            "router-a",
            Some(route_id),
            format!("public-{route_id}"),
            "upstream-model",
            TransportKind::OpenAiChat,
            "https://example.test/v1/chat/completions",
            "bearer",
        )
        .unwrap();
        ProtocolCompatibilityRecord::new(
            target,
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiChat),
                readiness: ProbeReadiness::Partial,
                branches: Vec::new(),
            },
            100,
            200,
        )
    });

    db.save_provider_with_protocol_profiles("codex", &provider, &records)
        .expect("save provider and routed profiles");

    assert!(db
        .get_provider_by_id("router-a", "codex")
        .unwrap()
        .is_some());
    for record in &records {
        assert_eq!(
            db.get_protocol_compatibility_result(&record.target)
                .expect("read routed profile"),
            Some(record.clone())
        );
    }
}

#[test]
fn schema_create_tables_include_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);
}

#[test]
fn schema_migration_v4_adds_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE providers (
            id TEXT NOT NULL,
            app_type TEXT NOT NULL,
            name TEXT NOT NULL,
            settings_config TEXT NOT NULL DEFAULT '{}',
            meta TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY (id, app_type)
        );
        CREATE TABLE proxy_config (app_type TEXT PRIMARY KEY);
        CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY, model TEXT NOT NULL);
        CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            server_config TEXT NOT NULL,
            enabled_claude INTEGER NOT NULL DEFAULT 0,
            enabled_codex INTEGER NOT NULL DEFAULT 0,
            enabled_gemini INTEGER NOT NULL DEFAULT 0,
            enabled_opencode INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .expect("seed v4 schema");

    Database::set_user_version(&conn, 4).expect("set user_version=4");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn migration_v10_to_v11_rebuilds_rollups_with_request_model_dimension() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟 v10 形状的 rollup 表（主键不含 request_model）+ 一行历史聚合数据，
    // 以及 v10 形状的明细表（无 pricing_model 列）
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_request_logs (
            request_id TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            request_model TEXT
        );
        CREATE TABLE usage_daily_rollups (
            date TEXT NOT NULL,
            app_type TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            model TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            success_count INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            total_cost_usd TEXT NOT NULL DEFAULT '0',
            avg_latency_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (date, app_type, provider_id, model)
        );
        INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_count, success_count,
             input_tokens, output_tokens, total_cost_usd, avg_latency_ms)
        VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 7, 7, 1000, 500, '0.07', 120);
        "#,
    )
    .expect("seed v10 rollup table");

    Database::set_user_version(&conn, 10).expect("set user_version=10");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 新列存在且 NOT NULL DEFAULT ''
    let request_model = get_column_info(&conn, "usage_daily_rollups", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 1);
    let rollup_pricing_model = get_column_info(&conn, "usage_daily_rollups", "pricing_model");
    assert_eq!(rollup_pricing_model.r#type, "TEXT");
    assert_eq!(rollup_pricing_model.notnull, 1);

    // 明细表补上 pricing_model 列（可空，历史行 NULL）
    let pricing_model = get_column_info(&conn, "proxy_request_logs", "pricing_model");
    assert_eq!(pricing_model.r#type, "TEXT");
    assert_eq!(pricing_model.notnull, 0);

    // 历史行保留，request_model 填 ''（未知）
    let (rm, count, input, cost): (String, i64, i64, String) = conn
        .query_row(
            "SELECT request_model, request_count, input_tokens, total_cost_usd
             FROM usage_daily_rollups WHERE model = 'kimi-k2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("migrated row");
    assert_eq!(rm, "");
    assert_eq!(count, 7);
    assert_eq!(input, 1000);
    assert_eq!(cost, "0.07");

    // 主键包含 request_model：同 model 不同别名可共存
    conn.execute(
        "INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_model, request_count)
         VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 'claude-sonnet-4-6', 1)",
        [],
    )
    .expect("insert row with same model but different request_model");

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_create_tables_repairs_dev_global_profile_marker() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟跑过未发布开发版的库：user_version 已是 12（迁移不会再跑），
    // 但 current 标记还是全局 key（现按应用分组）
    conn.execute_batch(
        r#"
        CREATE TABLE profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            payload TEXT NOT NULL,
            sort_order INTEGER,
            created_at INTEGER,
            updated_at INTEGER
        );
        INSERT INTO profiles (id, name, payload) VALUES ('p1', 'Project A', '{}');
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
        INSERT INTO settings (key, value) VALUES ('current_profile_id', 'p1');
        "#,
    )
    .expect("seed dev v12 shape");
    Database::set_user_version(&conn, 12).expect("set user_version=12");

    Database::create_tables_on_conn(&conn).expect("create tables should repair marker");

    // 全局 current 标记改名为 claude 组标记，旧 key 删除
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker");
    assert_eq!(claude_marker, "p1");
    let old_marker: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'current_profile_id'",
            [],
            |row| row.get(0),
        )
        .expect("count old marker");
    assert_eq!(old_marker, 0);

    // 修复必须幂等：再跑一遍不应破坏已迁移的标记
    Database::create_tables_on_conn(&conn).expect("repair is idempotent");
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker survives");
    assert_eq!(claude_marker, "p1");
}

#[test]
fn schema_create_tables_repairs_legacy_proxy_config_singleton_to_per_app() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟测试版 v2：user_version=2，但 proxy_config 仍是单例结构（无 app_type）
    Database::set_user_version(&conn, 2).expect("set user_version");
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_config (
            id INTEGER PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 5000,
            max_retries INTEGER NOT NULL DEFAULT 3,
            request_timeout INTEGER NOT NULL DEFAULT 300,
            enable_logging INTEGER NOT NULL DEFAULT 1,
            target_app TEXT NOT NULL DEFAULT 'claude',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO proxy_config (id, enabled) VALUES (1, 1);
        "#,
    )
    .expect("seed legacy proxy_config");

    Database::create_tables_on_conn(&conn).expect("create tables should repair proxy_config");

    assert!(
        Database::has_column(&conn, "proxy_config", "app_type").expect("check app_type"),
        "proxy_config should be migrated to per-app structure"
    );

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count rows");
    assert_eq!(count, 4, "per-app proxy_config should have 4 rows");

    // 新结构下应能按 app_type 查询
    let _: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("query by app_type");
}

#[test]
fn migration_from_v3_8_schema_v1_to_current_schema_v3() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute("PRAGMA foreign_keys = ON;", [])
        .expect("enable foreign keys");

    // 模拟 v3.8.* 用户的数据库（schema v1）
    conn.execute_batch(V3_8_SCHEMA_V1_SQL)
        .expect("seed v3.8 schema v1");
    Database::set_user_version(&conn, 1).expect("set user_version=1");

    // 插入一条旧版 Provider + Skill（用于验证迁移不会破坏既有数据）
    conn.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta, is_current
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            "p1",
            "claude",
            "Test Provider",
            serde_json::to_string(&json!({ "anthropicApiKey": "sk-test" })).unwrap(),
            Option::<String>::None,
            Option::<String>::None,
            Option::<i64>::None,
            Option::<usize>::None,
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            "{}",
            1,
        ],
    )
    .expect("seed provider");

    conn.execute(
        "INSERT INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
        params!["claude:demo-skill", 1, 1700000000i64],
    )
    .expect("seed legacy skill");

    // 按应用启动流程：先 create_tables（补齐新增表），再 apply_schema_migrations（按 user_version 迁移）
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    assert_eq!(
        Database::get_user_version(&conn).expect("user_version after migration"),
        SCHEMA_VERSION
    );

    // v1 -> v2：providers 新增字段必须补齐
    for column in [
        "cost_multiplier",
        "limit_daily_usd",
        "limit_monthly_usd",
        "provider_type",
        "in_failover_queue",
    ] {
        assert!(
            Database::has_column(&conn, "providers", column).expect("check column"),
            "providers.{column} should exist after migration"
        );
    }

    // 旧 provider 不应丢失，且新增字段应有默认值
    let provider_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("count providers");
    assert_eq!(provider_count, 1);

    let cost_multiplier: String = conn
        .query_row(
            "SELECT cost_multiplier FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("read cost_multiplier");
    assert_eq!(cost_multiplier, "1.0");

    // v2 -> v3：skills 表重建为统一结构，并设置 pending 标记（后续由启动时扫描文件系统重建数据）
    assert!(
        Database::has_column(&conn, "skills", "enabled_claude").expect("check skills v3 column"),
        "skills table should be migrated to v3 structure"
    );
    let skills_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
        .expect("count skills");
    assert_eq!(skills_count, 0, "skills table should be rebuilt empty");

    let pending: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_pending'",
            [],
            |r| r.get(0),
        )
        .ok();
    assert!(
        matches!(pending.as_deref(), Some("true") | Some("1")),
        "skills_ssot_migration_pending should be set after v2->v3 migration"
    );
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_snapshot'",
            [],
            |r| r.get(0),
        )
        .ok();
    let snapshot = snapshot.expect("skills migration snapshot should be recorded");
    let snapshot_rows: serde_json::Value =
        serde_json::from_str(&snapshot).expect("parse skills migration snapshot");
    assert!(
        snapshot_rows
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| {
                row.get("directory").and_then(|v| v.as_str()) == Some("demo-skill")
                    && row.get("app_type").and_then(|v| v.as_str()) == Some("claude")
            })),
        "skills migration snapshot should preserve legacy app mapping"
    );

    // v3.9+ 新增：proxy_config 三行 seed 必须存在（否则 UI 会查不到默认值）
    let proxy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count proxy_config rows");
    assert_eq!(proxy_rows, 4);

    // model_pricing 应具备默认数据（迁移时会 seed）
    let pricing_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |r| r.get(0))
        .expect("count model_pricing rows");
    assert!(pricing_rows > 0, "model_pricing should be seeded");
}

#[test]
fn schema_dry_run_does_not_write_to_disk() {
    // Create minimal valid config for migration
    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), ProviderManager::default());

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should succeed without any file I/O errors
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with valid config: {result:?}"
    );
}

#[test]
fn dry_run_validates_schema_compatibility() {
    // Create config with actual provider data
    let mut providers = IndexMap::new();
    providers.insert(
        "test-provider".to_string(),
        Provider {
            id: "test-provider".to_string(),
            name: "Test Provider".to_string(),
            settings_config: json!({
                "anthropicApiKey": "sk-test-123",
            }),
            website_url: None,
            category: None,
            created_at: Some(1234567890),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        },
    );

    let manager = ProviderManager {
        providers,
        current: "test-provider".to_string(),
    };

    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), manager);

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should validate the full migration path
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with provider data: {result:?}"
    );
}

#[test]
fn schema_model_pricing_is_seeded_on_init() {
    let db = Database::memory().expect("create memory db");

    let conn = db.conn.lock().expect("lock conn");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |row| row.get(0))
        .expect("count pricing");

    assert!(
        count > 0,
        "模型定价数据应该在初始化时自动填充，实际数量: {}",
        count
    );

    // 验证包含 Claude 模型
    let claude_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'claude-%'",
            [],
            |row| row.get(0),
        )
        .expect("check claude");
    assert!(
        claude_count > 0,
        "应该包含 Claude 模型定价，实际数量: {}",
        claude_count
    );

    // 验证包含 GPT 模型
    let gpt_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gpt-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gpt");
    assert!(
        gpt_count > 0,
        "应该包含 GPT 模型定价，实际数量: {}",
        gpt_count
    );

    // 验证包含 Gemini 模型
    let gemini_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gemini-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gemini");
    assert!(
        gemini_count > 0,
        "应该包含 Gemini 模型定价，实际数量: {}",
        gemini_count
    );
}

#[test]
fn model_pricing_seed_repairs_known_outdated_builtin_prices() {
    let db = Database::memory().expect("create memory db");

    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '1.68',
                 output_cost_per_million = '3.36',
                 cache_read_cost_per_million = '0.14',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'deepseek-v4-pro'",
            [],
        )
        .expect("restore old DeepSeek price");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '9',
                 output_cost_per_million = '9',
                 cache_read_cost_per_million = '9',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'glm-5.1'",
            [],
        )
        .expect("set custom GLM price");
    }

    db.ensure_model_pricing_seeded()
        .expect("ensure pricing seeded");

    let conn = db.conn.lock().expect("lock conn");
    let deepseek: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'deepseek-v4-pro'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query DeepSeek price");
    assert_eq!(
        deepseek,
        (
            "0.435".to_string(),
            "0.87".to_string(),
            "0.003625".to_string()
        )
    );

    let glm: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'glm-5.1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query GLM price");
    assert_eq!(glm, ("9".to_string(), "9".to_string(), "9".to_string()));
}

#[test]
fn schema_model_pricing_includes_new_codex_and_qwen_models() {
    // 新模型必须在首次初始化时即可计费，不能等用户手工导入价格表。
    let db = Database::memory().expect("create memory db");
    let conn = db.conn.lock().expect("lock conn");

    let expected = [
        ("gpt-5.6-sol", "5", "30", "0.50", "6.25"),
        ("gpt-5.6-terra", "2", "12", "0.20", "2.50"),
        ("gpt-5.6-luna", "0.20", "1.20", "0.02", "0.25"),
        ("gpt-5.3-codex-spark", "1.75", "14", "0.175", "0"),
        ("qwen3.6", "0.285", "2.40", "0.15", "0"),
    ];

    for (model_id, input, output, cache_read, cache_creation) in expected {
        let actual: (String, String, String, String) = conn
            .query_row(
                "SELECT input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million
                 FROM model_pricing WHERE model_id = ?1",
                [model_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("new model pricing should be seeded");
        assert_eq!(
            actual,
            (
                input.to_string(),
                output.to_string(),
                cache_read.to_string(),
                cache_creation.to_string()
            ),
            "{model_id} pricing mismatch"
        );
    }
}

#[test]
fn ensure_incremental_auto_vacuum_rebuilds_existing_file_db() {
    let temp = NamedTempFile::new().expect("create temp db file");
    let path = temp.path().to_path_buf();

    let conn = Connection::open(&path).expect("open temp db");
    conn.execute("PRAGMA auto_vacuum = NONE;", [])
        .expect("set none auto_vacuum");
    Database::create_tables_on_conn(&conn).expect("create tables");

    assert_eq!(
        Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum before rebuild"),
        0,
        "existing file db should start with NONE auto_vacuum"
    );

    let rebuilt =
        Database::ensure_incremental_auto_vacuum_on_conn(&conn).expect("enable incremental mode");
    assert!(rebuilt, "existing db should require rebuild via VACUUM");
    drop(conn);

    let reopened = Connection::open(&path).expect("reopen temp db");
    assert_eq!(
        Database::get_auto_vacuum_mode(&reopened).expect("auto_vacuum after rebuild"),
        2,
        "file db should persist INCREMENTAL auto_vacuum after VACUUM rebuild"
    );
}
