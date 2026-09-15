//! 跨平台看门狗（supervisor）。
//!
//! 应用自身以 `--ccsm-supervise <parent_pid>` 方式再拉起一个同二进制进程；该进程
//! 只做一件事：盯着主进程。主进程异常退出（运行标记仍指向它、但进程已经不在了）时，
//! 它把安装态可执行文件重新拉起，等到本地代理端口重新可用后自行退出。
//!
//! 为什么不是进程内线程：线程会随主进程一起消失，无法实现“进程死了再拉起来”。
//! 为什么不用 PowerShell/计划任务/systemd/launchd：那是各平台各写一套的运维方案，
//! 不随软件分发，也无法做成产品开关。这里用同一份 Rust 代码覆盖 Windows/macOS/Linux。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 守护进程的启动标记；主程序带这个参数运行时进入守护模式，不创建窗口。
pub const SUPERVISE_FLAG: &str = "--ccsm-supervise";
pub const SUPERVISE_CONFIG_FLAG: &str = "--ccsm-supervise-config";
pub const SUPERVISE_PORT_FLAG: &str = "--ccsm-supervise-port";

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const READY_POLL: Duration = Duration::from_millis(500);
const RESTART_WINDOW_SECS: u64 = 30 * 60;
const MAX_RESTARTS_PER_WINDOW: usize = 5;
const RUN_MARKER_FILE: &str = "app-run-marker.json";
const WATCHDOG_LOG_FILE: &str = "watchdog.jsonl";
const WATCHDOG_STATE_FILE: &str = "watchdog-state.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperviseArgs {
    pub parent_pid: u32,
    pub config_dir: Option<String>,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentOutcome {
    /// 主进程仍在运行。
    StillRunning,
    /// 主进程异常消失（运行标记仍指向它），应当重启。
    Crashed,
    /// 主进程正常退出（运行标记已被应用自己删除或不属于它），不应打扰。
    CleanExit,
}

/// 解析守护模式参数；不是守护模式时返回 None。
pub fn parse_supervise_args(args: &[String]) -> Option<SuperviseArgs> {
    let position = args.iter().position(|arg| arg == SUPERVISE_FLAG)?;
    let parent_pid = args
        .get(position + 1)
        .and_then(|value| value.parse::<u32>().ok())?;
    let config_dir = flag_value(args, SUPERVISE_CONFIG_FLAG);
    let port = flag_value(args, SUPERVISE_PORT_FLAG)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    Some(SuperviseArgs {
        parent_pid,
        config_dir,
        port,
    })
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1).cloned()
}

/// 依据运行标记判断主进程是崩溃还是正常退出。
///
/// 应用只在**正常退出**时删除 `logs/app-run-marker.json`；标记仍在且 pid 等于主进程 pid
/// 并且该进程已经不存在 ⇒ 崩溃。标记缺失、pid 不匹配或内容不可解析时一律按正常退出处理
/// （宁可少重启，也不制造崩溃循环）。
pub fn classify_parent_outcome(marker_json: Option<&str>, parent_pid: u32) -> ParentOutcome {
    let Some(marker_json) = marker_json else {
        return ParentOutcome::CleanExit;
    };
    let Ok(marker) = serde_json::from_str::<serde_json::Value>(marker_json) else {
        return ParentOutcome::CleanExit;
    };
    let marker_pid = marker
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    if marker_pid != Some(parent_pid) {
        return ParentOutcome::CleanExit;
    }
    ParentOutcome::Crashed
}

/// 重启频率预算：默认 30 分钟内最多 5 次。
pub fn restart_budget_allows(now_secs: u64, recent_secs: &[u64]) -> bool {
    let window_start = now_secs.saturating_sub(RESTART_WINDOW_SECS);
    let recent = recent_secs
        .iter()
        .filter(|value| **value >= window_start)
        .count();
    recent < MAX_RESTARTS_PER_WINDOW
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn log_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("logs")
}

fn append_event(config_dir: &Path, level: &str, event: &str, detail: serde_json::Value) {
    let directory = log_dir(config_dir);
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let record = serde_json::json!({
        "timestamp": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "level": level,
        "event": event,
        "detail": detail,
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(WATCHDOG_LOG_FILE))
    {
        let _ = writeln!(file, "{record}");
    }
}

fn read_run_marker(config_dir: &Path) -> Option<String> {
    std::fs::read_to_string(log_dir(config_dir).join(RUN_MARKER_FILE)).ok()
}

fn read_recent_restarts(config_dir: &Path) -> Vec<u64> {
    let path = log_dir(config_dir).join(WATCHDOG_STATE_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("restarts")
                .and_then(serde_json::Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(serde_json::Value::as_u64)
                        .collect::<Vec<u64>>()
                })
        })
        .unwrap_or_default()
}

fn write_recent_restarts(config_dir: &Path, restarts: &[u64]) {
    let directory = log_dir(config_dir);
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let payload = serde_json::json!({ "restarts": restarts });
    let _ = std::fs::write(
        directory.join(WATCHDOG_STATE_FILE),
        serde_json::to_vec_pretty(&payload).unwrap_or_default(),
    );
}

/// 主进程可执行文件路径（守护用的就是同一个二进制）。
pub fn current_executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// 由主进程调用：按设置决定是否拉起守护进程。
pub fn spawn_supervisor(config_dir: &Path, port: u16) {
    if crate::lightweight::is_lightweight_mode() {
        return;
    }
    let settings = crate::settings::get_settings();
    if !settings.watchdog_enabled {
        log::debug!("内置看门狗已关闭，跳过 supervisor 启动");
        return;
    }
    let Some(executable) = current_executable() else {
        log::warn!("无法解析当前可执行文件路径，跳过 supervisor 启动");
        return;
    };
    let parent_pid = std::process::id();
    let mut command = std::process::Command::new(&executable);
    command
        .arg(SUPERVISE_FLAG)
        .arg(parent_pid.to_string())
        .arg(SUPERVISE_CONFIG_FLAG)
        .arg(config_dir)
        .arg(SUPERVISE_PORT_FLAG)
        .arg(port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    match command.spawn() {
        Ok(child) => {
            append_event(
                config_dir,
                "info",
                "supervisor-spawned",
                serde_json::json!({
                    "supervisorPid": child.id(),
                    "parentPid": parent_pid,
                    "port": port,
                    "executable": executable.to_string_lossy(),
                }),
            );
            log::info!(
                "已启动内置看门狗: supervisorPid={} parentPid={parent_pid} port={port}",
                child.id()
            );
        }
        Err(error) => {
            log::warn!("启动内置看门狗失败: {error}");
            append_event(
                config_dir,
                "error",
                "supervisor-spawn-failed",
                serde_json::json!({ "error": error.to_string() }),
            );
        }
    }
}

/// 守护进程入口：盯住主进程，必要时把它拉起来，然后退出。
pub fn run_supervisor(args: SuperviseArgs) -> ! {
    let config_dir = args
        .config_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(crate::config::get_app_config_dir);
    append_event(
        &config_dir,
        "info",
        "supervisor-started",
        serde_json::json!({ "parentPid": args.parent_pid, "port": args.port }),
    );

    let expected = crate::process_identity::process_identity(args.parent_pid);
    if let Some(identity) = expected.as_ref() {
        append_event(
            &config_dir,
            "info",
            "supervisor-watching",
            serde_json::json!({
                "parentPid": identity.pid,
                "startedAtTicks": identity.started_at_ticks,
                "executable": identity.executable_path,
            }),
        );
    }

    let mut unhealthy_logged = false;
    loop {
        let current = crate::process_identity::process_identity(args.parent_pid);
        let still_running = match (expected.as_ref(), current.as_ref()) {
            (Some(expected), Some(current)) => expected.matches(current),
            _ => false,
        };
        if still_running {
            let port_ready = args.port == 0
                || crate::process_identity::tcp_listener_owner_pid(args.port).is_some();
            if !port_ready {
                if !unhealthy_logged {
                    unhealthy_logged = true;
                    append_event(
                        &config_dir,
                        "warning",
                        "supervisor-parent-unhealthy",
                        serde_json::json!({ "parentPid": args.parent_pid, "port": args.port }),
                    );
                }
            } else {
                unhealthy_logged = false;
            }
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }
        break;
    }

    let marker = read_run_marker(&config_dir);
    match classify_parent_outcome(marker.as_deref(), args.parent_pid) {
        ParentOutcome::StillRunning => std::process::exit(0),
        ParentOutcome::CleanExit => {
            append_event(
                &config_dir,
                "info",
                "supervisor-clean-exit",
                serde_json::json!({ "parentPid": args.parent_pid }),
            );
            std::process::exit(0);
        }
        ParentOutcome::Crashed => {}
    }

    let mut restarts = read_recent_restarts(&config_dir);
    let now = now_secs();
    if !restart_budget_allows(now, &restarts) {
        append_event(
            &config_dir,
            "error",
            "supervisor-restart-limited",
            serde_json::json!({ "parentPid": args.parent_pid, "windowSecs": RESTART_WINDOW_SECS }),
        );
        std::process::exit(0);
    }
    restarts.push(now);
    write_recent_restarts(&config_dir, &restarts);

    let Some(executable) = current_executable() else {
        append_event(
            &config_dir,
            "error",
            "supervisor-restart-failed",
            serde_json::json!({ "error": "current executable unavailable" }),
        );
        std::process::exit(0);
    };
    let mut command = std::process::Command::new(&executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let spawned = command.spawn();
    let Ok(child) = spawned else {
        let error = spawned
            .err()
            .map(|value| value.to_string())
            .unwrap_or_default();
        append_event(
            &config_dir,
            "error",
            "supervisor-restart-failed",
            serde_json::json!({ "error": error }),
        );
        std::process::exit(0);
    };
    append_event(
        &config_dir,
        "warning",
        "supervisor-restarted",
        serde_json::json!({
            "parentPid": args.parent_pid,
            "newPid": child.id(),
            "port": args.port,
            "executable": executable.to_string_lossy(),
        }),
    );

    let deadline = SystemTime::now() + READY_TIMEOUT;
    while SystemTime::now() < deadline {
        if args.port == 0 || crate::process_identity::tcp_listener_owner_pid(args.port).is_some() {
            append_event(
                &config_dir,
                "info",
                "supervisor-restart-ready",
                serde_json::json!({ "newPid": child.id(), "port": args.port }),
            );
            std::process::exit(0);
        }
        std::thread::sleep(READY_POLL);
    }
    append_event(
        &config_dir,
        "error",
        "supervisor-restart-not-ready",
        serde_json::json!({ "newPid": child.id(), "port": args.port }),
    );
    std::process::exit(0);
}

/// 守护日志里的一条事件（供设置页的“看门狗 / 端口自检”面板展示）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogEventRecord {
    pub timestamp: String,
    pub level: String,
    pub event: String,
    pub detail: serde_json::Value,
}

/// 读取最近 limit 条守护事件（新到旧）。
pub fn recent_events(config_dir: &Path, limit: usize) -> Vec<WatchdogEventRecord> {
    let path = log_dir(config_dir).join(WATCHDOG_LOG_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for line in text.lines().rev() {
        if records.len() >= limit {
            break;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        records.push(WatchdogEventRecord {
            timestamp: value
                .get("timestamp")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            level: value
                .get("level")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("info")
                .to_string(),
            event: value
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            detail: value
                .get("detail")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        });
    }
    records
}

/// 最近一次为 parent_pid 拉起的守护进程 PID（来自守护日志的 supervisor-spawned 事件）。
pub fn last_supervisor_pid(config_dir: &Path, parent_pid: u32) -> Option<u32> {
    recent_events(config_dir, 64)
        .into_iter()
        .filter(|record| record.event == "supervisor-spawned")
        .find_map(|record| {
            let detail = record.detail;
            let detail_parent = detail
                .get("parentPid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            if detail_parent != Some(parent_pid) {
                return None;
            }
            detail
                .get("supervisorPid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
        })
}

/// 当前重启窗口内已经发生的自动拉起次数。
pub fn recent_restart_count(config_dir: &Path) -> usize {
    let now = now_secs();
    let window_start = now.saturating_sub(RESTART_WINDOW_SECS);
    read_recent_restarts(config_dir)
        .into_iter()
        .filter(|value| *value >= window_start)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn supervise_args_require_the_flag_and_a_pid() {
        assert!(parse_supervise_args(&args(&["cc-switch.exe"])).is_none());
        assert!(parse_supervise_args(&args(&[SUPERVISE_FLAG])).is_none());
        assert!(parse_supervise_args(&args(&[SUPERVISE_FLAG, "abc"])).is_none());
        let parsed = parse_supervise_args(&args(&[
            "cc-switch.exe",
            SUPERVISE_FLAG,
            "4242",
            SUPERVISE_CONFIG_FLAG,
            "C:/cfg",
            SUPERVISE_PORT_FLAG,
            "15721",
        ]))
        .expect("parsed");
        assert_eq!(parsed.parent_pid, 4242);
        assert_eq!(parsed.config_dir.as_deref(), Some("C:/cfg"));
        assert_eq!(parsed.port, 15721);
    }

    #[test]
    fn crash_is_detected_only_from_our_own_remaining_marker() {
        assert_eq!(classify_parent_outcome(None, 42), ParentOutcome::CleanExit);
        assert_eq!(
            classify_parent_outcome(Some("{ not json"), 42),
            ParentOutcome::CleanExit
        );
        assert_eq!(
            classify_parent_outcome(Some(r#"{"pid": 7}"#), 42),
            ParentOutcome::CleanExit
        );
        assert_eq!(
            classify_parent_outcome(Some(r#"{"pid": 42, "version": "1.0"}"#), 42),
            ParentOutcome::Crashed
        );
    }

    #[test]
    fn supervisor_log_round_trip_finds_events_and_last_pid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).expect("logs dir");
        let log = logs.join(WATCHDOG_LOG_FILE);
        let lines = [
            r#"{"timestamp":"t1","level":"info","event":"supervisor-spawned","detail":{"supervisorPid":11,"parentPid":100}}"#,
            r#"{"timestamp":"t2","level":"info","event":"supervisor-watching","detail":{"parentPid":100}}"#,
            r#"{"timestamp":"t3","level":"warning","event":"supervisor-restarted","detail":{"parentPid":100,"newPid":101}}"#,
            r#"{"timestamp":"t4","level":"info","event":"supervisor-spawned","detail":{"supervisorPid":22,"parentPid":999}}"#,
        ]
        .join("\n");
        std::fs::write(&log, lines).expect("write log");

        let events = recent_events(dir.path(), 10);
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event, "supervisor-spawned");
        assert_eq!(last_supervisor_pid(dir.path(), 100), Some(11));
        assert_eq!(last_supervisor_pid(dir.path(), 999), Some(22));
        assert_eq!(last_supervisor_pid(dir.path(), 1234), None);
    }
    #[test]
    fn restart_budget_limits_crash_loops() {
        let now = 1_000_000u64;
        assert!(restart_budget_allows(now, &[]));
        assert!(restart_budget_allows(now, &[now - 10, now - 20]));
        let exhausted = vec![now - 1, now - 2, now - 3, now - 4, now - 5];
        assert!(!restart_budget_allows(now, &exhausted));
        let stale = vec![now - RESTART_WINDOW_SECS - 1; 5];
        assert!(restart_budget_allows(now, &stale));
    }
}
