//! 看门狗 / 端口自检命令
//!
//! 只读：把守护进程状态、最近自动拉起记录和当前端口占用诊断暴露给设置页面板，
//! 让用户不用翻日志就能判断“现在到底有没有守护、端口是谁占的”。

use crate::store::AppState;
use crate::watchdog::WatchdogEventRecord;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogStatus {
    /// 设置里是否启用内置看门狗。
    pub enabled: bool,
    /// 当前应用主进程 PID。
    pub app_pid: u32,
    /// 当前应用对应的守护进程 PID（来自守护日志）。
    pub supervisor_pid: Option<u32>,
    /// 该守护进程当前是否仍在运行。
    pub supervisor_alive: bool,
    /// 守护进程可执行文件路径（校验用）。
    pub supervisor_path: Option<String>,
    /// 重启窗口内已发生的自动拉起次数。
    pub restarts_in_window: usize,
    /// 代理监听端口。
    pub port: u16,
    /// 端口当前监听者 PID。
    pub listener_pid: Option<u32>,
    /// 端口当前监听者可执行文件路径。
    pub listener_path: Option<String>,
    /// 监听者是否就是本安装态程序（即端口就绪且归属正确）。
    pub port_ready: bool,
    /// 端口占用诊断文本（无异常时是简短说明）。
    pub port_diagnosis: String,
    /// 最近若干条守护事件（新到旧）。
    pub recent_events: Vec<WatchdogEventRecord>,
}

#[tauri::command]
pub async fn get_watchdog_status(
    state: tauri::State<'_, AppState>,
) -> Result<WatchdogStatus, String> {
    let config_dir = crate::config::get_app_config_dir();
    let port = state
        .db
        .get_proxy_config()
        .await
        .map(|config| config.listen_port)
        .unwrap_or(0);
    let app_pid = std::process::id();
    let supervisor_pid = crate::watchdog::last_supervisor_pid(&config_dir, app_pid);
    let supervisor_identity = supervisor_pid.and_then(crate::process_identity::process_identity);
    let listener_pid = if port > 0 {
        crate::process_identity::tcp_listener_owner_pid(port)
    } else {
        None
    };
    let listener_identity = listener_pid.and_then(crate::process_identity::process_identity);
    let port_ready = listener_identity
        .as_ref()
        .is_some_and(|identity| crate::process_identity::executable_matches_current(&identity.executable_path));

    Ok(WatchdogStatus {
        enabled: crate::settings::get_settings().watchdog_enabled,
        app_pid,
        supervisor_pid,
        supervisor_alive: supervisor_identity.is_some(),
        supervisor_path: supervisor_identity.map(|identity| identity.executable_path),
        restarts_in_window: crate::watchdog::recent_restart_count(&config_dir),
        port,
        listener_pid,
        listener_path: listener_identity.map(|identity| identity.executable_path),
        port_ready,
        port_diagnosis: if port > 0 {
            crate::process_identity::describe_port_blockers(port)
        } else {
            String::new()
        },
        recent_events: crate::watchdog::recent_events(&config_dir, 8),
    })
}