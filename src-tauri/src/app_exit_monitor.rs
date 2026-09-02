//! 应用退出与崩溃监控。
//!
//! 这个模块不依赖数据库，确保数据库初始化失败、panic hook 或更新安装器直接退出时
//! 仍能留下可排查的本地证据。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const RUN_MARKER_FILE: &str = "app-run-marker.json";
const EXIT_EVENTS_FILE: &str = "app-exit-events.jsonl";

static APP_CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 初始化退出监控使用的配置目录。
///
/// 必须在 Store 覆盖目录刷新后调用；若 panic 发生得更早，模块会回退到默认
/// `~/.cc-switch`，避免因为目录尚未初始化而丢失崩溃证据。
pub fn init_app_config_dir(dir: PathBuf) {
    let _ = APP_CONFIG_DIR.set(dir);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousRunClassification {
    NoPreviousRun,
    ActivePreviousInstance,
    ConfirmedCrash,
    PlannedRestartOrUpdate,
    CleanExit,
    UncleanExit,
}

impl PreviousRunClassification {
    pub fn allows_crash_recovery(self) -> bool {
        matches!(self, Self::ConfirmedCrash | Self::UncleanExit)
    }

    pub fn allows_proxy_startup(self) -> bool {
        !matches!(self, Self::ActivePreviousInstance)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupRecoveryReport {
    pub previous: Option<PreviousRunReport>,
    pub classification: PreviousRunClassification,
}

/// 记录本次启动，并在写入 marker 前对旧 PID、可执行文件与创建时间做身份核验。
pub fn record_startup_report() -> StartupRecoveryReport {
    let previous_marker = read_run_marker();
    let previous = previous_marker.as_ref().map(|marker| PreviousRunReport {
        marker: marker.clone(),
        crash_log_modified_at: file_modified_at(crash_log_path()),
    });
    let events = read_exit_events();
    let crash_log_modified_after_marker = previous.as_ref().is_some_and(|report| {
        report
            .crash_log_modified_at
            .as_deref()
            .is_some_and(|modified| modified > report.marker.started_at.as_str())
    });
    let observed_process = previous
        .as_ref()
        .and_then(|report| crate::process_identity::process_identity(report.marker.pid));
    let config_scope = crate::process_identity::config_scope_fingerprint();
    let classification = classify_previous_run(
        previous.as_ref().map(|report| &report.marker),
        &events,
        crash_log_modified_after_marker,
        observed_process.as_ref(),
        &config_scope,
    );

    if let Some(report) = previous.as_ref() {
        append_event(
            "abnormal_exit_detected",
            "previous run marker remained at startup",
            None,
            Some(json!({
                "previousRun": report.marker,
                "crashLogModifiedAt": report.crash_log_modified_at,
                "classification": classification,
            })),
        );
    }

    let identity = crate::process_identity::current_process_identity();
    let marker = RunMarker {
        started_at: now_string(),
        pid: std::process::id(),
        version: APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cwd: std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        executable_path: identity
            .as_ref()
            .map(|identity| identity.executable_path.clone()),
        process_started_at_ticks: identity.as_ref().map(|identity| identity.started_at_ticks),
        config_scope: Some(config_scope),
    };

    if classification != PreviousRunClassification::ActivePreviousInstance {
        if let Err(err) = write_run_marker(&marker) {
            log::warn!("写入应用运行 marker 失败: {err}");
        }
    }

    StartupRecoveryReport {
        previous,
        classification,
    }
}

#[allow(dead_code)]
pub fn record_startup() -> Option<PreviousRunReport> {
    record_startup_report().previous
}

/// 记录一次正常退出，并清理运行 marker。
///
/// 退出原因由调用方传入，便于区分托盘退出、窗口关闭、设置重启和更新安装等不同路径。
pub fn record_clean_exit(reason: &str, exit_code: i32) {
    append_event("clean_exit", reason, Some(exit_code), None);
    if let Err(err) = remove_run_marker_if_owned() {
        log::warn!("清理应用运行 marker 失败: {err}");
    }
}

/// 记录即将直接退出的错误路径。
///
/// 这类路径通常发生在数据库或配置加载阶段，不能假设 Tauri 事件循环和数据库都可用。
pub fn record_forced_exit(reason: &str, exit_code: i32, detail: impl Into<Option<String>>) {
    append_event(
        "forced_exit",
        reason,
        Some(exit_code),
        detail.into().map(|detail| json!({ "detail": detail })),
    );
    let _ = remove_run_marker_if_owned();
}

/// 记录 panic hook 捕获到的崩溃摘要。
///
/// 详细 backtrace 仍由 `panic_hook` 写入 `crash.log`；这里写一条结构化 JSONL，
/// 方便下次启动或用户汇总“崩溃原因”。
pub fn record_panic(message: &str, location: Option<String>, thread: Option<String>) {
    append_event(
        "panic",
        message,
        None,
        Some(json!({
            "location": location,
            "thread": thread,
        })),
    );
}

/// 打开日志目录。
///
/// 返回路径字符串供前端 toast 或调试使用；实际打开由命令层完成。
pub fn log_dir_path() -> PathBuf {
    get_app_config_dir().join("logs")
}

/// 异常退出历史文件路径。
pub fn exit_events_path() -> PathBuf {
    log_dir_path().join(EXIT_EVENTS_FILE)
}

/// 上次未正常退出的报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousRunReport {
    pub marker: RunMarker,
    pub crash_log_modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMarker {
    pub started_at: String,
    pub pid: u32,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub cwd: String,
    #[serde(default)]
    pub executable_path: Option<String>,
    #[serde(default)]
    pub process_started_at_ticks: Option<u64>,
    #[serde(default)]
    pub config_scope: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExitEvent {
    timestamp: String,
    kind: String,
    reason: String,
    exit_code: Option<i32>,
    version: String,
    os: String,
    arch: String,
    pid: u32,
    details: Option<Value>,
}

fn classify_previous_run(
    marker: Option<&RunMarker>,
    events: &[ExitEvent],
    crash_log_modified_after_marker: bool,
    observed_process: Option<&crate::process_identity::ProcessIdentity>,
    expected_config_scope: &str,
) -> PreviousRunClassification {
    let Some(marker) = marker else {
        return PreviousRunClassification::NoPreviousRun;
    };
    let marker_identity = marker
        .executable_path
        .as_ref()
        .zip(marker.process_started_at_ticks)
        .map(
            |(executable_path, started_at_ticks)| crate::process_identity::ProcessIdentity {
                pid: marker.pid,
                executable_path: executable_path.clone(),
                started_at_ticks,
            },
        );
    let same_scope = marker.config_scope.as_deref() == Some(expected_config_scope);
    if same_scope
        && marker_identity
            .as_ref()
            .zip(observed_process)
            .is_some_and(|(expected, observed)| expected.matches(observed))
    {
        return PreviousRunClassification::ActivePreviousInstance;
    }

    let same_pid_events = events
        .iter()
        .filter(|event| event.pid == marker.pid && event.timestamp >= marker.started_at);
    let mut confirmed_crash = crash_log_modified_after_marker;
    let mut planned_restart = false;
    let mut clean_exit = false;
    for event in same_pid_events {
        if matches!(event.kind.as_str(), "panic" | "forced_exit") {
            confirmed_crash = true;
        }
        if event.kind == "clean_exit" {
            clean_exit = true;
            planned_restart |= is_planned_exit_reason(&event.reason);
        }
    }
    if confirmed_crash {
        PreviousRunClassification::ConfirmedCrash
    } else if planned_restart {
        PreviousRunClassification::PlannedRestartOrUpdate
    } else if clean_exit {
        PreviousRunClassification::CleanExit
    } else {
        PreviousRunClassification::UncleanExit
    }
}

fn is_planned_exit_reason(reason: &str) -> bool {
    let lower = reason.trim().to_ascii_lowercase();
    lower.contains("restart") || lower.contains("update") || lower.contains("reload")
}

fn read_exit_events() -> Vec<ExitEvent> {
    fs::read_to_string(exit_events_path())
        .ok()
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<ExitEvent>(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn append_event(kind: &str, reason: &str, exit_code: Option<i32>, details: Option<Value>) {
    let event = ExitEvent {
        timestamp: now_string(),
        kind: kind.to_string(),
        reason: reason.to_string(),
        exit_code,
        version: APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        pid: std::process::id(),
        details,
    };

    let path = exit_events_path();
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    if let Ok(line) = serde_json::to_string(&event) {
        let _ = writeln!(file, "{line}");
    }
}

fn read_run_marker() -> Option<RunMarker> {
    let text = fs::read_to_string(run_marker_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_run_marker(marker: &RunMarker) -> std::io::Result<()> {
    let path = run_marker_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(marker).map_err(std::io::Error::other)?;
    fs::write(path, text)
}

fn marker_matches_process(
    marker: &RunMarker,
    process: &crate::process_identity::ProcessIdentity,
) -> bool {
    marker
        .executable_path
        .as_ref()
        .zip(marker.process_started_at_ticks)
        .is_some_and(|(executable_path, started_at_ticks)| {
            crate::process_identity::ProcessIdentity {
                pid: marker.pid,
                executable_path: executable_path.clone(),
                started_at_ticks,
            }
            .matches(process)
        })
}

fn remove_run_marker_if_owned() -> std::io::Result<()> {
    let Some(marker) = read_run_marker() else {
        return Ok(());
    };
    let Some(current) = crate::process_identity::current_process_identity() else {
        return Ok(());
    };
    if !marker_matches_process(&marker, &current) {
        return Ok(());
    }
    match fs::remove_file(run_marker_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn run_marker_path() -> PathBuf {
    log_dir_path().join(RUN_MARKER_FILE)
}

fn crash_log_path() -> PathBuf {
    get_app_config_dir().join("crash.log")
}

fn get_app_config_dir() -> PathBuf {
    APP_CONFIG_DIR
        .get()
        .cloned()
        .unwrap_or_else(default_app_config_dir)
}

fn default_app_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-switch")
}

fn now_string() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

fn file_modified_at(path: PathBuf) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Local> = modified.into();
    Some(datetime.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_events_path_uses_logs_directory() {
        let path = exit_events_path();
        assert!(path.ends_with(EXIT_EVENTS_FILE));
        assert!(path.to_string_lossy().contains("logs"));
    }

    #[test]
    fn clean_exit_event_serializes_exit_code() {
        let event = ExitEvent {
            timestamp: "2026-06-28 12:00:00.000".to_string(),
            kind: "clean_exit".to_string(),
            reason: "unit_test".to_string(),
            exit_code: Some(0),
            version: "test".to_string(),
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
            pid: 42,
            details: None,
        };

        let text = serde_json::to_string(&event).expect("serialize event");
        assert!(text.contains("\"kind\":\"clean_exit\""));
        assert!(text.contains("\"exitCode\":0"));
    }

    fn marker_with_identity() -> RunMarker {
        RunMarker {
            started_at: "2026-08-25 10:00:00.000".to_string(),
            pid: 42,
            version: "test".to_string(),
            os: "test".to_string(),
            arch: "test".to_string(),
            cwd: "test".to_string(),
            executable_path: Some(r"C:\Apps\cc-switch.exe".to_string()),
            process_started_at_ticks: Some(100),
            config_scope: Some(crate::process_identity::config_scope_fingerprint()),
        }
    }

    #[test]
    fn active_previous_instance_requires_the_complete_process_identity() {
        let marker = marker_with_identity();
        let exact = crate::process_identity::ProcessIdentity {
            pid: marker.pid,
            executable_path: marker.executable_path.clone().expect("marker executable"),
            started_at_ticks: marker.process_started_at_ticks.expect("marker start"),
        };
        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &[],
                false,
                Some(&exact),
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::ActivePreviousInstance
        );

        let reused_pid = crate::process_identity::ProcessIdentity {
            started_at_ticks: exact.started_at_ticks + 1,
            ..exact.clone()
        };
        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &[],
                false,
                Some(&reused_pid),
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::UncleanExit
        );
        let wrong_executable = crate::process_identity::ProcessIdentity {
            executable_path: r"C:\Other\cc-switch.exe".to_string(),
            ..exact
        };
        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &[],
                false,
                Some(&wrong_executable),
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::UncleanExit
        );
    }

    #[test]
    fn legacy_pid_only_marker_is_never_treated_as_an_active_instance() {
        let mut marker = marker_with_identity();
        marker.executable_path = None;
        marker.process_started_at_ticks = None;
        let observed = crate::process_identity::ProcessIdentity {
            pid: marker.pid,
            executable_path: r"C:\Apps\cc-switch.exe".to_string(),
            started_at_ticks: 100,
        };

        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &[],
                false,
                Some(&observed),
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::UncleanExit
        );
    }

    #[test]
    fn clean_exit_event_prevents_recovery_when_marker_cleanup_was_unavailable() {
        let marker = marker_with_identity();
        let events = vec![ExitEvent {
            timestamp: "2026-08-25 10:01:00.000".to_string(),
            kind: "clean_exit".to_string(),
            reason: "user_requested_exit".to_string(),
            exit_code: Some(0),
            version: "test".to_string(),
            os: "test".to_string(),
            arch: "test".to_string(),
            pid: marker.pid,
            details: None,
        }];

        let classification = classify_previous_run(
            Some(&marker),
            &events,
            false,
            None,
            marker.config_scope.as_deref().expect("marker scope"),
        );
        assert_eq!(classification, PreviousRunClassification::CleanExit);
        assert!(!classification.allows_crash_recovery());
    }

    #[test]
    fn clean_exit_with_restart_reason_remains_a_planned_restart() {
        let marker = marker_with_identity();
        let events = vec![ExitEvent {
            timestamp: "2026-08-25 10:01:00.000".to_string(),
            kind: "clean_exit".to_string(),
            reason: "restart_after_update".to_string(),
            exit_code: Some(0),
            version: "test".to_string(),
            os: "test".to_string(),
            arch: "test".to_string(),
            pid: marker.pid,
            details: None,
        }];

        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &events,
                false,
                None,
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::PlannedRestartOrUpdate
        );
    }

    #[test]
    fn crash_evidence_wins_over_an_earlier_clean_exit_event() {
        let marker = marker_with_identity();
        let events = vec![
            ExitEvent {
                timestamp: "2026-08-25 10:01:00.000".to_string(),
                kind: "clean_exit".to_string(),
                reason: "user_requested_exit".to_string(),
                exit_code: Some(0),
                version: "test".to_string(),
                os: "test".to_string(),
                arch: "test".to_string(),
                pid: marker.pid,
                details: None,
            },
            ExitEvent {
                timestamp: "2026-08-25 10:01:01.000".to_string(),
                kind: "panic".to_string(),
                reason: "panic after shutdown began".to_string(),
                exit_code: Some(1),
                version: "test".to_string(),
                os: "test".to_string(),
                arch: "test".to_string(),
                pid: marker.pid,
                details: None,
            },
        ];

        assert_eq!(
            classify_previous_run(
                Some(&marker),
                &events,
                false,
                None,
                marker.config_scope.as_deref().expect("marker scope"),
            ),
            PreviousRunClassification::ConfirmedCrash
        );
    }

    #[test]
    fn marker_ownership_requires_the_complete_current_process_identity() {
        let current = crate::process_identity::current_process_identity()
            .expect("current process identity must be queryable");
        let owned = RunMarker {
            pid: current.pid,
            executable_path: Some(current.executable_path.clone()),
            process_started_at_ticks: Some(current.started_at_ticks),
            ..marker_with_identity()
        };
        assert!(marker_matches_process(&owned, &current));

        let different_instance = RunMarker {
            process_started_at_ticks: Some(current.started_at_ticks.saturating_add(1)),
            ..owned
        };
        assert!(!marker_matches_process(&different_instance, &current));
    }
}
