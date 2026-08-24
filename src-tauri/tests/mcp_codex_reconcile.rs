use std::collections::{HashMap, HashSet};
use std::fs;

use serde_json::json;

use cc_switch_lib::sync_enabled_to_codex_with_ownership;

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_live_config(text: &str) -> std::path::PathBuf {
    let home = ensure_test_home();
    let path = home.join(".codex").join("config.toml");
    fs::create_dir_all(path.parent().expect("config parent")).expect("create Codex dir");
    fs::write(&path, text).expect("seed config.toml");
    path
}

fn reconcile(live: &str, owned_ids: &[&str], enabled: &[(&str, serde_json::Value)]) -> String {
    reset_test_fs();
    let path = seed_live_config(live);
    let owned = owned_ids
        .iter()
        .map(|id| (*id).to_string())
        .collect::<HashSet<_>>();
    let enabled = enabled
        .iter()
        .map(|(id, spec)| ((*id).to_string(), spec.clone()))
        .collect::<HashMap<_, _>>();
    sync_enabled_to_codex_with_ownership(&owned, &enabled).expect("reconcile Codex MCP");
    fs::read_to_string(path).expect("read reconciled config.toml")
}

#[test]
fn empty_database_preserves_live_only_mcp() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "[desktop]\nfoo = true\n\n[mcp_servers.external-only]\ntype = \"stdio\"\ncommand = \"external\"\n",
        &[],
        &[],
    );
    assert!(text.contains("[mcp_servers.external-only]"));
    assert!(text.contains("command = \"external\""));
    assert!(text.contains("[desktop]"));
}

#[test]
fn managed_and_live_only_entries_are_reconciled_together() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "[mcp_servers.external-only]\ntype = \"stdio\"\ncommand = \"external\"\n",
        &["managed"],
        &[(
            "managed",
            json!({"type": "stdio", "command": "managed-from-db"}),
        )],
    );
    assert!(text.contains("[mcp_servers.external-only]"));
    assert!(text.contains("[mcp_servers.managed]"));
    assert!(text.contains("command = \"managed-from-db\""));
}

#[test]
fn same_content_is_idempotent_and_does_not_rewrite_live_bytes() {
    let _guard = test_mutex().lock().expect("test mutex");
    reset_test_fs();
    let initial =
        "# keep this comment\n[mcp_servers.same-id]\ntype = \"stdio\"\ncommand = \"same\"\n";
    let path = seed_live_config(initial);
    let owned = HashSet::from(["same-id".to_string()]);
    let enabled = HashMap::from([(
        "same-id".to_string(),
        json!({"type": "stdio", "command": "same"}),
    )]);
    sync_enabled_to_codex_with_ownership(&owned, &enabled).expect("first reconcile");
    assert_eq!(fs::read_to_string(path).expect("read config"), initial);
}

#[test]
fn semantically_same_managed_entry_preserves_comments_and_formatting() {
    let _guard = test_mutex().lock().expect("test mutex");
    reset_test_fs();
    let initial = concat!(
        "# keep this comment\n",
        "[mcp_servers.managed]\n",
        "# explain why this server is managed\n",
        "command = \"same\" # keep the operator note\n",
        "args = [\"--quiet\"]\n",
        "type = \"stdio\"\n",
    );
    let path = seed_live_config(initial);
    let owned = HashSet::from(["managed".to_string()]);
    let enabled = HashMap::from([(
        "managed".to_string(),
        json!({
            "type": "stdio",
            "command": "same",
            "args": ["--quiet"]
        }),
    )]);

    sync_enabled_to_codex_with_ownership(&owned, &enabled).expect("reconcile Codex MCP");

    assert_eq!(
        fs::read_to_string(path).expect("read reconciled config"),
        initial
    );
}

#[test]
fn managed_content_from_database_wins_over_external_edit() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "[mcp_servers.same-id]\ntype = \"stdio\"\ncommand = \"manual-edit\"\n",
        &["same-id"],
        &[(
            "same-id",
            json!({"type": "stdio", "command": "database-version"}),
        )],
    );
    assert!(text.contains("command = \"database-version\""));
    assert!(!text.contains("manual-edit"));
}

#[test]
fn explicit_disable_removes_only_owned_id() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "[mcp_servers.managed]\ntype = \"stdio\"\ncommand = \"managed\"\n\n[mcp_servers.external-only]\ntype = \"stdio\"\ncommand = \"external\"\n",
        &["managed"],
        &[],
    );
    assert!(!text.contains("[mcp_servers.managed]"));
    assert!(text.contains("[mcp_servers.external-only]"));
}

#[test]
fn legacy_servers_are_migrated_before_cleanup() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "[mcp]\nkeep = \"value\"\n\n[mcp.servers.ghost-legacy]\ntype = \"stdio\"\ncommand = \"ghost\"\n\n[mcp.servers.managed]\ntype = \"stdio\"\ncommand = \"old\"\n",
        &["managed"],
        &[(
            "managed",
            json!({"type": "stdio", "command": "database"}),
        )],
    );
    assert!(!text.contains("[mcp.servers]"));
    assert!(text.contains("[mcp_servers.ghost-legacy]"));
    assert!(text.contains("command = \"ghost\""));
    assert!(text.contains("command = \"database\""));
    assert!(text.contains("keep = \"value\""));
}

#[test]
fn invalid_toml_keeps_original_bytes() {
    let _guard = test_mutex().lock().expect("test mutex");
    reset_test_fs();
    let original = "[mcp_servers.external-only\ncommand = \"broken\"\n";
    let path = seed_live_config(original);
    let owned = HashSet::new();
    let enabled = HashMap::new();
    assert!(sync_enabled_to_codex_with_ownership(&owned, &enabled).is_err());
    assert_eq!(fs::read_to_string(path).expect("read original"), original);
}

#[test]
fn sensitive_values_are_written_but_not_used_as_identity() {
    let _guard = test_mutex().lock().expect("test mutex");
    let text = reconcile(
        "",
        &["secret-server"],
        &[(
            "secret-server",
            json!({
                "type": "http",
                "url": "https://example.test/mcp",
                "headers": {"Authorization": "Bearer secret-token"}
            }),
        )],
    );
    assert!(text.contains("[mcp_servers.secret-server]"));
    assert!(text.contains("Authorization"));
    assert!(!text.contains("[mcp_servers.secret-token]"));
}
