use crate::config::{atomic_write, get_app_config_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

pub const EVENT_RECOVERY_OUTCOME: &str = "recovery-outcome-recorded";
const STORE_FILE: &str = "recovery-outcomes.json";
const MAX_OUTCOMES: usize = 64;

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CURRENT_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoverySeverity {
    Info,
    Warning,
    Error,
}

impl RecoverySeverity {
    fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Error => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryOutcomeKind {
    ActivePreviousInstance,
    ConfirmedCrash,
    UncleanExit,
    PlannedRestartOrUpdate,
    HealthyBackupRestored,
    LivePreservedProviderRepaired,
    ProviderOnlyRestored,
    UnrecoverableUserTables,
    PortOwnedByCompatibleInstance,
    PortOwnedByUnknownOwner,
    StartupTakeoverRestored,
    StartupTakeoverFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryOutcome {
    pub id: String,
    pub generation: u64,
    pub operation: String,
    pub kind: RecoveryOutcomeKind,
    pub severity: RecoverySeverity,
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
    pub acknowledged_at: Option<String>,
}

impl RecoveryOutcome {
    pub fn for_app(
        operation: impl Into<String>,
        kind: RecoveryOutcomeKind,
        severity: RecoverySeverity,
        app_type: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            generation: current_generation(),
            operation: operation.into(),
            kind,
            severity,
            app_type: Some(app_type.into()),
            kept_fields: Vec::new(),
            lost_fields: Vec::new(),
            next_step: None,
            timestamp: now_timestamp(),
            acknowledged_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryOutcomeStore {
    #[serde(default = "store_schema_version")]
    schema_version: u32,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    outcomes: Vec<RecoveryOutcome>,
}

fn store_schema_version() -> u32 {
    1
}

pub fn init(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

pub fn begin_generation() -> Result<u64, String> {
    let _guard = store_lock()
        .lock()
        .map_err(|_| "恢复结果存储锁已损坏".to_string())?;
    let mut store = read_store_unlocked()?;
    store.generation = store.generation.saturating_add(1).max(1);
    write_store_unlocked(&store)?;
    CURRENT_GENERATION.store(store.generation, Ordering::SeqCst);
    Ok(store.generation)
}

pub fn current_generation() -> u64 {
    CURRENT_GENERATION.load(Ordering::SeqCst).max(1)
}

pub fn record_recovery_outcome(mut outcome: RecoveryOutcome) -> Result<(), String> {
    if outcome.generation == 0 {
        outcome.generation = current_generation();
    }
    {
        let _guard = store_lock()
            .lock()
            .map_err(|_| "恢复结果存储锁已损坏".to_string())?;
        let mut store = read_store_unlocked()?;
        store.generation = store.generation.max(outcome.generation);
        store.outcomes.push(outcome.clone());
        trim_outcomes(&mut store.outcomes);
        write_store_unlocked(&store)?;
    }
    if let Some(handle) = APP_HANDLE.get() {
        if let Err(error) = handle.emit(EVENT_RECOVERY_OUTCOME, &outcome) {
            log::warn!("发送恢复结果事件失败: {error}");
        }
    }
    Ok(())
}

pub fn record_best_effort(outcome: RecoveryOutcome) {
    if let Err(error) = record_recovery_outcome(outcome) {
        log::warn!("保存恢复结果失败: {error}");
    }
}

pub fn get_pending_recovery_outcomes() -> Result<Vec<RecoveryOutcome>, String> {
    let _guard = store_lock()
        .lock()
        .map_err(|_| "恢复结果存储锁已损坏".to_string())?;
    let mut pending = read_store_unlocked()?
        .outcomes
        .into_iter()
        .filter(|outcome| {
            outcome.acknowledged_at.is_none() && !matches!(outcome.severity, RecoverySeverity::Info)
        })
        .collect::<Vec<_>>();
    pending.sort_by(|left, right| {
        right
            .severity
            .rank()
            .cmp(&left.severity.rank())
            .then_with(|| left.timestamp.cmp(&right.timestamp))
    });
    Ok(pending)
}

pub fn acknowledge_recovery_outcomes(generation: u64, ids: &[String]) -> Result<usize, String> {
    let _guard = store_lock()
        .lock()
        .map_err(|_| "恢复结果存储锁已损坏".to_string())?;
    let mut store = read_store_unlocked()?;
    let acknowledged_at = now_timestamp();
    let mut changed = 0usize;
    for outcome in &mut store.outcomes {
        if outcome.generation == generation
            && outcome.acknowledged_at.is_none()
            && ids.iter().any(|id| id == &outcome.id)
        {
            outcome.acknowledged_at = Some(acknowledged_at.clone());
            changed += 1;
        }
    }
    if changed > 0 {
        write_store_unlocked(&store)?;
    }
    Ok(changed)
}

fn trim_outcomes(outcomes: &mut Vec<RecoveryOutcome>) {
    while outcomes.len() > MAX_OUTCOMES {
        let remove_at = outcomes
            .iter()
            .enumerate()
            .min_by_key(|(_, outcome)| {
                (
                    outcome.acknowledged_at.is_none() as u8,
                    outcome.severity.rank(),
                    outcome.generation,
                    outcome.timestamp.clone(),
                )
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        outcomes.remove(remove_at);
    }
}

fn read_store_unlocked() -> Result<RecoveryOutcomeStore, String> {
    let path = recovery_outcome_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecoveryOutcomeStore {
                schema_version: store_schema_version(),
                ..Default::default()
            })
        }
        Err(error) => return Err(format!("读取恢复结果失败: {error}")),
    };
    match serde_json::from_slice(&bytes) {
        Ok(store) => Ok(store),
        Err(error) => {
            log::warn!("恢复结果文件不可解析，将从空记录继续: {error}");
            Ok(RecoveryOutcomeStore {
                schema_version: store_schema_version(),
                ..Default::default()
            })
        }
    }
}

fn write_store_unlocked(store: &RecoveryOutcomeStore) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(store).map_err(|error| format!("序列化恢复结果失败: {error}"))?;
    atomic_write(&recovery_outcome_path(), &bytes)
        .map_err(|error| format!("保存恢复结果失败: {error}"))
}

pub fn recovery_outcome_path() -> PathBuf {
    get_app_config_dir().join("logs").join(STORE_FILE)
}

fn store_lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

fn now_timestamp() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
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

    fn outcome(
        generation: u64,
        app: &str,
        severity: RecoverySeverity,
        kind: RecoveryOutcomeKind,
    ) -> RecoveryOutcome {
        let mut outcome = RecoveryOutcome::for_app("startup_restore", kind, severity, app);
        outcome.generation = generation;
        outcome
    }

    #[test]
    fn success_for_one_app_cannot_overwrite_another_apps_failure() {
        let mut outcomes = vec![
            outcome(
                1,
                "codex",
                RecoverySeverity::Error,
                RecoveryOutcomeKind::StartupTakeoverFailed,
            ),
            outcome(
                1,
                "claude",
                RecoverySeverity::Info,
                RecoveryOutcomeKind::StartupTakeoverRestored,
            ),
        ];
        trim_outcomes(&mut outcomes);

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().any(|outcome| {
            outcome.app_type.as_deref() == Some("codex")
                && outcome.severity == RecoverySeverity::Error
        }));
    }

    #[test]
    fn bounded_store_discards_low_severity_before_unacknowledged_errors() {
        let mut outcomes = (0..MAX_OUTCOMES)
            .map(|index| {
                outcome(
                    1,
                    &format!("info-{index}"),
                    RecoverySeverity::Info,
                    RecoveryOutcomeKind::StartupTakeoverRestored,
                )
            })
            .collect::<Vec<_>>();
        let error = outcome(
            1,
            "codex",
            RecoverySeverity::Error,
            RecoveryOutcomeKind::StartupTakeoverFailed,
        );
        let error_id = error.id.clone();
        outcomes.push(error);
        trim_outcomes(&mut outcomes);

        assert_eq!(outcomes.len(), MAX_OUTCOMES);
        assert!(outcomes.iter().any(|outcome| outcome.id == error_id));
    }

    #[test]
    #[serial]
    fn acknowledgment_is_scoped_to_the_observed_generation() {
        let _home = TempHome::new();
        let old = outcome(
            10,
            "codex",
            RecoverySeverity::Error,
            RecoveryOutcomeKind::StartupTakeoverFailed,
        );
        let new = outcome(
            11,
            "codex",
            RecoverySeverity::Error,
            RecoveryOutcomeKind::StartupTakeoverFailed,
        );
        record_recovery_outcome(old.clone()).expect("record old");
        record_recovery_outcome(new.clone()).expect("record new");

        assert_eq!(
            acknowledge_recovery_outcomes(10, &[old.id.clone(), new.id.clone()])
                .expect("ack old generation"),
            1
        );
        let pending = get_pending_recovery_outcomes().expect("read pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, new.id);
    }

    #[test]
    #[serial]
    fn later_success_for_another_app_does_not_hide_a_persisted_failure() {
        let _home = TempHome::new();
        let failure = outcome(
            20,
            "codex",
            RecoverySeverity::Error,
            RecoveryOutcomeKind::StartupTakeoverFailed,
        );
        let success = outcome(
            20,
            "claude",
            RecoverySeverity::Info,
            RecoveryOutcomeKind::StartupTakeoverRestored,
        );
        record_recovery_outcome(failure.clone()).expect("record failure");
        record_recovery_outcome(success).expect("record later success");

        let pending = get_pending_recovery_outcomes().expect("read pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, failure.id);
        assert_eq!(pending[0].app_type.as_deref(), Some("codex"));
    }
}
