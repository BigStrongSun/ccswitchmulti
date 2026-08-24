//! Persisted, user-visible outcomes for startup configuration recovery.
//!
//! Recovery can run before the webview subscribes to Tauri events.  The last
//! outcome is therefore written first and emitted second; the frontend can
//! always query the file if it missed the event.

use crate::config::{atomic_write, get_app_config_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

pub const EVENT_RECOVERY_OUTCOME: &str = "codex-config-recovery-outcome";
const OUTCOME_FILE: &str = "codex-config-recovery-outcome.json";

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static OUTCOME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// A named recovery result.  The names are intentionally stable: they are
/// persisted on disk and consumed by the TypeScript UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryOutcomeKind {
    HealthyBackupRestored,
    LivePreservedProviderRepaired,
    ProviderOnlyRestored,
    UserBackupCandidateFound,
    UnrecoverableUserTables,
    ConcurrentModificationDeferred,
    PluginRegistrationRepairAvailable,
    PluginRegistrationRepairCompleted,
    PluginRegistrationRepairFailed,
    PortOwnedByCompatibleInstance,
    PortOwnedByUnknownOwner,
    ActivePreviousInstance,
    PlannedRestartOrUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryOutcome {
    pub kind: RecoveryOutcomeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_type: Option<String>,
    #[serde(default)]
    pub kept_fields: Vec<String>,
    #[serde(default)]
    pub lost_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl RecoveryOutcome {
    pub fn new(kind: RecoveryOutcomeKind) -> Self {
        Self {
            kind,
            app_type: None,
            kept_fields: Vec::new(),
            lost_fields: Vec::new(),
            next_step: None,
            timestamp: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            details: None,
        }
    }

    pub fn for_app(kind: RecoveryOutcomeKind, app_type: &str) -> Self {
        let mut outcome = Self::new(kind);
        outcome.app_type = Some(app_type.to_string());
        outcome
    }
}

/// Inject the application handle during Tauri setup.  Repeated calls are
/// harmless and are ignored after the first successful initialization.
pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::debug!("recovery_outcome::init 重复调用，已忽略");
    }
}

/// Persist an outcome atomically, then notify the webview.  A failed emit is
/// logged only: persistence is the source of truth and must not be rolled back
/// because a window is not ready yet.
pub fn record_recovery_outcome(outcome: RecoveryOutcome) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&outcome)
        .map_err(|error| format!("序列化恢复结果失败: {error}"))?;
    {
        let _guard = outcome_lock()
            .lock()
            .map_err(|_| "恢复结果锁已损坏".to_string())?;
        let path = recovery_outcome_path();
        atomic_write(&path, &bytes).map_err(|error| format!("保存恢复结果失败: {error}"))?;
    }

    if let Some(handle) = APP_HANDLE.get() {
        if let Err(error) = handle.emit(EVENT_RECOVERY_OUTCOME, &outcome) {
            log::warn!("发送 {EVENT_RECOVERY_OUTCOME} 事件失败: {error}");
        }
    }
    Ok(())
}

pub fn get_last_recovery_outcome() -> Result<Option<RecoveryOutcome>, String> {
    let _guard = outcome_lock()
        .lock()
        .map_err(|_| "恢复结果锁已损坏".to_string())?;
    let path = recovery_outcome_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取恢复结果失败: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("解析恢复结果失败: {error}"))
}

/// Remove only a stale startup warning after a normal startup classification.
///
/// Active and planned outcomes are intentionally transient: they describe the
/// previous launch and should not be shown again on every later launch.  Other
/// outcomes contain useful recovery history and must remain available.  The
/// read/compare/remove sequence is serialized with outcome writes and rechecks
/// the bytes before deletion so a newer in-process writer wins over cleanup.
pub fn clear_transient_startup_outcome_if_not_active_or_planned() -> Result<(), String> {
    let _guard = outcome_lock()
        .lock()
        .map_err(|_| "恢复结果锁已损坏".to_string())?;
    let path = recovery_outcome_path();
    let expected = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取恢复结果失败: {error}")),
    };
    let outcome: RecoveryOutcome =
        serde_json::from_slice(&expected).map_err(|error| format!("解析恢复结果失败: {error}"))?;
    if !matches!(
        outcome.kind,
        RecoveryOutcomeKind::ActivePreviousInstance | RecoveryOutcomeKind::PlannedRestartOrUpdate
    ) {
        return Ok(());
    }

    let current = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取恢复结果失败: {error}")),
    };
    if current != expected {
        return Ok(());
    }

    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("清理过期启动恢复结果失败: {error}")),
    }
}

pub fn recovery_outcome_path() -> PathBuf {
    get_app_config_dir().join("logs").join(OUTCOME_FILE)
}

fn outcome_lock() -> &'static Mutex<()> {
    OUTCOME_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    struct TempHome {
        _dir: TempDir,
        previous: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let previous = std::env::var("CC_SWITCH_TEST_HOME").ok();
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn recovery_outcome_serializes_stable_kind_and_fields() {
        let mut outcome = RecoveryOutcome::new(RecoveryOutcomeKind::ProviderOnlyRestored);
        outcome.app_type = Some("codex".to_string());
        outcome.kept_fields = vec!["desktop.max".to_string()];
        outcome.lost_fields = vec!["userTables".to_string()];
        outcome.next_step = Some("openLogs".to_string());

        let value: serde_json::Value = serde_json::to_value(&outcome).expect("serialize outcome");
        assert_eq!(value["kind"], "providerOnlyRestored");
        assert_eq!(value["appType"], "codex");
        assert_eq!(value["keptFields"][0], "desktop.max");
        assert_eq!(value["lostFields"][0], "userTables");
    }

    #[test]
    #[serial]
    fn recovery_outcome_persists_before_readback() {
        let _home = TempHome::new();
        let mut outcome =
            RecoveryOutcome::for_app(RecoveryOutcomeKind::LivePreservedProviderRepaired, "codex");
        outcome.kept_fields = vec!["desktop".to_string(), "plugins".to_string()];
        record_recovery_outcome(outcome.clone()).expect("record outcome");

        assert!(recovery_outcome_path().is_file());
        assert_eq!(
            get_last_recovery_outcome().expect("read outcome"),
            Some(outcome)
        );
    }

    #[test]
    #[serial]
    fn transient_startup_outcomes_are_conditionally_cleared() {
        let _home = TempHome::new();
        for kind in [
            RecoveryOutcomeKind::ActivePreviousInstance,
            RecoveryOutcomeKind::PlannedRestartOrUpdate,
        ] {
            record_recovery_outcome(RecoveryOutcome::new(kind)).expect("seed transient outcome");
            clear_transient_startup_outcome_if_not_active_or_planned()
                .expect("clear transient startup outcome");
            assert_eq!(
                get_last_recovery_outcome().expect("read cleared outcome"),
                None
            );
        }
    }

    #[test]
    #[serial]
    fn non_transient_recovery_outcome_is_not_cleared() {
        let _home = TempHome::new();
        let outcome = RecoveryOutcome::new(RecoveryOutcomeKind::ProviderOnlyRestored);
        record_recovery_outcome(outcome.clone()).expect("seed durable outcome");
        clear_transient_startup_outcome_if_not_active_or_planned()
            .expect("reconcile durable outcome");
        assert_eq!(
            get_last_recovery_outcome().expect("read durable outcome"),
            Some(outcome)
        );
    }
}
