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

/// 上一次运行的证据分类。
///
/// 分类先判断旧 PID 是否仍活跃，再判断同一 PID 的 panic/crash 和计划退出证据；只有
/// 没有更强证据时才归为 `UncleanExit`。这样单独残留 marker 不会触发破坏性恢复。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousRunClassification {
    NoPreviousRun,
    ActivePreviousInstance,
    ConfirmedCrash,
    PlannedRestartOrUpdate,
    UncleanExit,
}

impl PreviousRunClassification {
    pub fn allows_recovery(self) -> bool {
        matches!(self, Self::ConfirmedCrash | Self::UncleanExit)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupRecoveryReport {
    pub previous: Option<PreviousRunReport>,
    pub classification: PreviousRunClassification,
}

/// 记录应用启动，并在覆盖 marker 前读取完整的旧运行证据。
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
    let classification = classify_previous_run(
        previous.as_ref().map(|report| &report.marker),
        &events,
        crash_log_modified_after_marker,
        previous
            .as_ref()
            .is_some_and(|report| process_is_active(report.marker.pid)),
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

    let marker = RunMarker {
        started_at: now_string(),
        pid: std::process::id(),
        version: APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cwd: std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    };

    if let Err(err) = write_run_marker(&marker) {
        log::warn!("写入应用运行 marker 失败: {err}");
    }

    StartupRecoveryReport {
        previous,
        classification,
    }
}

/// 兼容旧调用方：仍返回旧 marker，但内部使用新的证据分类流程。
#[allow(dead_code)]
pub fn record_startup() -> Option<PreviousRunReport> {
    record_startup_report().previous
}

/// 记录一次正常退出，并清理运行 marker。
///
/// 退出原因由调用方传入，便于区分托盘退出、窗口关闭、设置重启和更新安装等不同路径。
pub fn record_clean_exit(reason: &str, exit_code: i32) {
    append_event("clean_exit", reason, Some(exit_code), None);
    if let Err(err) = fs::remove_file(run_marker_path()) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::warn!("清理应用运行 marker 失败: {err}");
        }
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
    let _ = fs::remove_file(run_marker_path());
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
    pid_active: bool,
) -> PreviousRunClassification {
    let Some(marker) = marker else {
        return PreviousRunClassification::NoPreviousRun;
    };
    if pid_active {
        return PreviousRunClassification::ActivePreviousInstance;
    }

    let same_pid_events = events
        .iter()
        .filter(|event| event.pid == marker.pid && event.timestamp >= marker.started_at);
    let mut confirmed_crash = crash_log_modified_after_marker;
    let mut planned_restart = false;
    for event in same_pid_events {
        if matches!(event.kind.as_str(), "panic" | "forced_exit") {
            confirmed_crash = true;
        }
        if event.kind == "clean_exit" && is_planned_exit_reason(&event.reason) {
            planned_restart = true;
        }
    }
    if confirmed_crash {
        PreviousRunClassification::ConfirmedCrash
    } else if planned_restart {
        PreviousRunClassification::PlannedRestartOrUpdate
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
        .into_iter()
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<ExitEvent>(line).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn process_is_active(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // A read-only process handle is enough to distinguish an active old instance.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            false
        } else {
            unsafe { CloseHandle(handle) };
            true
        }
    }
    #[cfg(all(unix, not(target_os = "windows")))]
    {
        // kill(pid, 0) performs an existence/permission check without sending a signal.
        unsafe {
            libc::kill(pid as libc::pid_t, 0) == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        false
    }
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

    fn marker(pid: u32, started_at: &str) -> RunMarker {
        RunMarker {
            started_at: started_at.to_string(),
            pid,
            version: "test".to_string(),
            os: "test".to_string(),
            arch: "test".to_string(),
            cwd: "test".to_string(),
        }
    }

    fn event(pid: u32, timestamp: &str, kind: &str, reason: &str) -> ExitEvent {
        ExitEvent {
            timestamp: timestamp.to_string(),
            kind: kind.to_string(),
            reason: reason.to_string(),
            exit_code: None,
            version: "test".to_string(),
            os: "test".to_string(),
            arch: "test".to_string(),
            pid,
            details: None,
        }
    }

    #[test]
    fn classification_without_marker_is_no_previous_run() {
        assert_eq!(
            classify_previous_run(None, &[], false, false),
            PreviousRunClassification::NoPreviousRun
        );
    }

    #[test]
    fn active_previous_instance_takes_priority_over_crash_evidence() {
        let old = marker(42, "2026-08-24 10:00:00.000");
        let events = vec![event(42, "2026-08-24 10:01:00.000", "panic", "panic")];

        assert_eq!(
            classify_previous_run(Some(&old), &events, true, true),
            PreviousRunClassification::ActivePreviousInstance
        );
    }

    #[test]
    fn same_pid_panic_or_crash_log_is_confirmed_crash() {
        let old = marker(42, "2026-08-24 10:00:00.000");
        let events = vec![event(42, "2026-08-24 10:01:00.000", "panic", "panic")];

        assert_eq!(
            classify_previous_run(Some(&old), &events, false, false),
            PreviousRunClassification::ConfirmedCrash
        );
        assert_eq!(
            classify_previous_run(Some(&old), &[], true, false),
            PreviousRunClassification::ConfirmedCrash
        );
    }

    #[test]
    fn same_pid_planned_exit_is_planned_restart_or_update() {
        let old = marker(42, "2026-08-24 10:00:00.000");
        let events = vec![event(
            42,
            "2026-08-24 10:01:00.000",
            "clean_exit",
            "update and restart",
        )];

        assert_eq!(
            classify_previous_run(Some(&old), &events, false, false),
            PreviousRunClassification::PlannedRestartOrUpdate
        );
    }

    #[test]
    fn stale_marker_without_matching_evidence_is_unclean_exit() {
        let old = marker(42, "2026-08-24 10:00:00.000");
        let events = vec![event(42, "2026-08-24 09:59:00.000", "panic", "old")];

        assert_eq!(
            classify_previous_run(Some(&old), &events, false, false),
            PreviousRunClassification::UncleanExit
        );
    }

    #[test]
    fn different_pid_events_cannot_change_classification() {
        let old = marker(42, "2026-08-24 10:00:00.000");
        let events = vec![event(
            99,
            "2026-08-24 10:01:00.000",
            "clean_exit",
            "restart",
        )];

        assert_eq!(
            classify_previous_run(Some(&old), &events, false, false),
            PreviousRunClassification::UncleanExit
        );
    }
}
