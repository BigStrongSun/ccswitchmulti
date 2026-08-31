use crate::codex_config_consistency::{
    CodexConfigConsistencyState, CodexConfigRuntimeActivationState,
};
use crate::store::AppState;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

static CODEX_RUNTIME_REFRESH_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

const CODEX_RUNTIME_REFRESH_EVENT: &str = "codex-runtime-refresh-progress";
const CODEX_GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RawCodexRuntimeProcess {
    pid: u32,
    parent_pid: u32,
    name: String,
    executable_path: String,
    command_line: String,
    started_at: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
struct CodexRuntimeRefreshTargets {
    desktop_shells: Vec<RawCodexRuntimeProcess>,
    app_servers: Vec<RawCodexRuntimeProcess>,
}

impl CodexRuntimeRefreshTargets {
    fn process_count(&self) -> usize {
        self.desktop_shells.len() + self.app_servers.len()
    }

    fn processes(&self) -> impl Iterator<Item = &RawCodexRuntimeProcess> {
        self.desktop_shells.iter().chain(self.app_servers.iter())
    }
}

#[derive(Clone, Debug)]
enum CodexRuntimeLaunchTarget {
    #[cfg(target_os = "windows")]
    WindowsAumid(String),
    DesktopExecutable(PathBuf),
}

impl CodexRuntimeLaunchTarget {
    fn label(&self) -> String {
        match self {
            #[cfg(target_os = "windows")]
            Self::WindowsAumid(aumid) => aumid.clone(),
            Self::DesktopExecutable(path) => path.display().to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeRefreshPreflight {
    pub supported: bool,
    pub can_refresh: bool,
    pub snapshot_token: String,
    pub desktop_process_count: usize,
    pub app_server_process_count: usize,
    pub process_count: usize,
    pub launch_target: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeRefreshStage {
    Closing,
    ForceClosing,
    ApplyingConfig,
    Launching,
    Verifying,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeRefreshProgress {
    pub stage: CodexRuntimeRefreshStage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeRefreshResult {
    pub force_terminated: bool,
    pub closed_process_count: usize,
}

trait CodexRuntimeRefreshOperations {
    async fn current_targets(&mut self) -> Result<CodexRuntimeRefreshTargets, String>;
    async fn request_graceful_close(
        &mut self,
        targets: &CodexRuntimeRefreshTargets,
    ) -> Result<(), String>;
    async fn wait_for_exit(
        &mut self,
        targets: &CodexRuntimeRefreshTargets,
    ) -> Result<Vec<RawCodexRuntimeProcess>, String>;
    async fn force_terminate(&mut self, survivors: &[RawCodexRuntimeProcess])
        -> Result<(), String>;
    async fn apply_ccsm_config(&mut self) -> Result<i64, String>;
    async fn launch_codex(&mut self) -> Result<(), String>;
    async fn verify_fresh_runtime(&mut self, config_written_at_ms: i64) -> Result<(), String>;
}

async fn execute_refresh_transaction<O, F>(
    operations: &mut O,
    expected_snapshot_token: &str,
    mut emit: F,
) -> Result<CodexRuntimeRefreshResult, String>
where
    O: CodexRuntimeRefreshOperations,
    F: FnMut(CodexRuntimeRefreshProgress),
{
    let targets = operations.current_targets().await?;
    if refresh_target_fingerprint(&targets) != expected_snapshot_token {
        return Err("runtime_changed_since_inspection".to_string());
    }
    let closed_process_count = targets.desktop_shells.len() + targets.app_servers.len();

    emit(CodexRuntimeRefreshProgress {
        stage: CodexRuntimeRefreshStage::Closing,
    });
    operations.request_graceful_close(&targets).await?;
    let mut survivors = operations.wait_for_exit(&targets).await?;
    let force_terminated = !survivors.is_empty();
    if force_terminated {
        emit(CodexRuntimeRefreshProgress {
            stage: CodexRuntimeRefreshStage::ForceClosing,
        });
        operations.force_terminate(&survivors).await?;
        survivors = operations.wait_for_exit(&targets).await?;
        if !survivors.is_empty() {
            return Err("codex_runtime_still_running_after_forced_close".to_string());
        }
    }

    emit(CodexRuntimeRefreshProgress {
        stage: CodexRuntimeRefreshStage::ApplyingConfig,
    });
    let config_written_at_ms = match operations.apply_ccsm_config().await {
        Ok(timestamp) => timestamp,
        Err(error) => {
            emit(CodexRuntimeRefreshProgress {
                stage: CodexRuntimeRefreshStage::Launching,
            });
            let relaunch = operations.launch_codex().await;
            return match relaunch {
                Ok(()) => Err(error),
                Err(launch_error) => Err(format!(
                    "{error}; codex_relaunch_after_config_failure_failed: {launch_error}"
                )),
            };
        }
    };

    emit(CodexRuntimeRefreshProgress {
        stage: CodexRuntimeRefreshStage::Launching,
    });
    operations.launch_codex().await?;
    emit(CodexRuntimeRefreshProgress {
        stage: CodexRuntimeRefreshStage::Verifying,
    });
    operations
        .verify_fresh_runtime(config_written_at_ms)
        .await?;
    emit(CodexRuntimeRefreshProgress {
        stage: CodexRuntimeRefreshStage::Completed,
    });

    Ok(CodexRuntimeRefreshResult {
        force_terminated,
        closed_process_count,
    })
}

fn normalized_windows_path(path: &str) -> String {
    path.trim().replace('/', "\\").to_ascii_lowercase()
}

fn has_complete_process_identity(process: &RawCodexRuntimeProcess) -> bool {
    process.pid != 0
        && !process.executable_path.trim().is_empty()
        && !process.started_at.trim().is_empty()
}

fn is_known_legacy_codex_desktop_path(path: &str) -> bool {
    path.contains("\\windowsapps\\openai.codex_")
        || path.contains("\\windowsapps\\openai.codex.preview_")
        || path.contains("\\appdata\\local\\openai\\codex\\")
        || path.contains("\\appdata\\local\\programs\\openai\\codex\\")
        || path.contains("\\appdata\\local\\programs\\codex\\")
        || path.contains("\\program files\\openai\\codex\\")
        || path.contains("\\program files\\codex\\")
        || path.contains("\\program files (x86)\\openai\\codex\\")
        || path.contains("\\scoop\\apps\\codex\\")
}

fn is_official_codex_desktop_shell(process: &RawCodexRuntimeProcess) -> bool {
    if !has_complete_process_identity(process) {
        return false;
    }
    if process
        .command_line
        .to_ascii_lowercase()
        .contains(" --type=")
    {
        return false;
    }
    let path = normalized_windows_path(&process.executable_path);
    match process.name.as_str() {
        "ChatGPT.exe" => {
            path.contains("\\windowsapps\\openai.codex_")
                || path.contains("\\windowsapps\\openai.codex.preview_")
        }
        "Codex.exe" => {
            is_known_legacy_codex_desktop_path(&path) && !path.ends_with("\\resources\\codex.exe")
        }
        _ => false,
    }
}

fn is_codex_app_server(process: &RawCodexRuntimeProcess) -> bool {
    has_complete_process_identity(process)
        && process.name.eq_ignore_ascii_case("codex.exe")
        && process
            .command_line
            .split_whitespace()
            .any(|argument| argument.eq_ignore_ascii_case("app-server"))
}

fn classify_refresh_targets(processes: &[RawCodexRuntimeProcess]) -> CodexRuntimeRefreshTargets {
    let mut desktop_shells = processes
        .iter()
        .filter(|process| is_official_codex_desktop_shell(process))
        .cloned()
        .collect::<Vec<_>>();
    desktop_shells.sort_by_key(|process| process.pid);

    let shell_pids = desktop_shells
        .iter()
        .map(|process| process.pid)
        .collect::<std::collections::BTreeSet<_>>();
    let mut app_servers = processes
        .iter()
        .filter(|process| is_codex_app_server(process))
        .filter(|process| {
            shell_pids.contains(&process.parent_pid)
                || normalized_windows_path(&process.executable_path)
                    .contains("\\appdata\\local\\openai\\codex\\bin\\")
        })
        .cloned()
        .collect::<Vec<_>>();
    app_servers.sort_by_key(|process| process.pid);

    CodexRuntimeRefreshTargets {
        desktop_shells,
        app_servers,
    }
}

fn refresh_target_fingerprint(targets: &CodexRuntimeRefreshTargets) -> String {
    let bytes = serde_json::to_vec(targets).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawWindowsCodexRuntimeProcess {
    process_id: Option<u32>,
    parent_process_id: Option<u32>,
    name: Option<String>,
    executable_path: Option<String>,
    command_line: Option<String>,
    started_at: Option<String>,
}

#[cfg(target_os = "windows")]
fn powershell_utf8_output(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("powershell_start_failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)
            .unwrap_or_else(|error| format!("PowerShell stderr was not UTF-8: {error}"));
        return Err(format!(
            "powershell_failed: {}",
            stderr.trim().replace(['\r', '\n'], " ")
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim_start_matches('\u{feff}').trim().to_string())
        .map_err(|error| format!("powershell_output_not_utf8: {error}"))
}

#[cfg(target_os = "windows")]
fn query_codex_runtime_processes() -> Result<Vec<RawCodexRuntimeProcess>, String> {
    let output = powershell_utf8_output(
        r#"
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$items = @(Get-CimInstance Win32_Process -Filter "Name = 'Codex.exe' OR Name = 'codex.exe' OR Name = 'ChatGPT.exe'" |
  Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine,@{Name='StartedAt';Expression={$_.CreationDate.ToLocalTime().ToString('o')}})
ConvertTo-Json -InputObject $items -Compress
"#,
    )?;
    if output.is_empty() || output == "null" {
        return Ok(Vec::new());
    }
    let raw = serde_json::from_str::<Vec<RawWindowsCodexRuntimeProcess>>(&output)
        .map_err(|error| format!("codex_runtime_process_json_invalid: {error}"))?;
    Ok(raw
        .into_iter()
        .filter_map(|process| {
            Some(RawCodexRuntimeProcess {
                pid: process.process_id?,
                parent_pid: process.parent_process_id.unwrap_or_default(),
                name: process.name?,
                executable_path: process.executable_path.unwrap_or_default(),
                command_line: process.command_line.unwrap_or_default(),
                started_at: process.started_at.unwrap_or_default(),
            })
        })
        .collect())
}

#[cfg(not(target_os = "windows"))]
fn query_codex_runtime_processes() -> Result<Vec<RawCodexRuntimeProcess>, String> {
    Ok(Vec::new())
}

async fn query_refresh_targets() -> Result<CodexRuntimeRefreshTargets, String> {
    let processes = tokio::task::spawn_blocking(query_codex_runtime_processes)
        .await
        .map_err(|error| format!("codex_runtime_process_query_join_failed: {error}"))??;
    Ok(classify_refresh_targets(&processes))
}

#[cfg(target_os = "windows")]
fn resolve_windows_codex_aumid() -> Option<String> {
    let output = powershell_utf8_output(
        r#"
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Get-StartApps |
  Where-Object { $_.AppID -match '^OpenAI\.Codex(?:\.Preview)?_.*!App$' } |
  Select-Object -First 1 -ExpandProperty AppID
"#,
    )
    .ok()?;
    let aumid = output.lines().next()?.trim();
    (!aumid.is_empty()).then(|| aumid.to_string())
}

fn resolve_launch_target() -> Option<CodexRuntimeLaunchTarget> {
    #[cfg(target_os = "windows")]
    if let Some(aumid) = resolve_windows_codex_aumid() {
        return Some(CodexRuntimeLaunchTarget::WindowsAumid(aumid));
    }
    crate::codex_desktop::resolve_codex_executable()
        .map(CodexRuntimeLaunchTarget::DesktopExecutable)
}

async fn build_preflight() -> Result<CodexRuntimeRefreshPreflight, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Ok(CodexRuntimeRefreshPreflight {
            supported: false,
            can_refresh: false,
            snapshot_token: String::new(),
            desktop_process_count: 0,
            app_server_process_count: 0,
            process_count: 0,
            launch_target: None,
            warning: Some("codex_runtime_refresh_windows_only".to_string()),
        });
    }

    #[cfg(target_os = "windows")]
    {
        let targets = query_refresh_targets().await?;
        let launch_target = resolve_launch_target();
        Ok(CodexRuntimeRefreshPreflight {
            supported: true,
            can_refresh: launch_target.is_some(),
            snapshot_token: refresh_target_fingerprint(&targets),
            desktop_process_count: targets.desktop_shells.len(),
            app_server_process_count: targets.app_servers.len(),
            process_count: targets.process_count(),
            launch_target: launch_target.as_ref().map(CodexRuntimeLaunchTarget::label),
            warning: (targets.process_count() > 0)
                .then(|| "active_tasks_will_be_interrupted".to_string()),
        })
    }
}

fn same_process_identity(
    expected: &RawCodexRuntimeProcess,
    observed: &RawCodexRuntimeProcess,
) -> bool {
    expected.pid == observed.pid
        && expected.name == observed.name
        && normalized_windows_path(&expected.executable_path)
            == normalized_windows_path(&observed.executable_path)
        && expected.started_at == observed.started_at
        && expected.command_line == observed.command_line
}

fn surviving_processes(
    expected: &CodexRuntimeRefreshTargets,
    observed: &[RawCodexRuntimeProcess],
) -> Vec<RawCodexRuntimeProcess> {
    expected
        .processes()
        .filter(|expected_process| {
            observed
                .iter()
                .any(|process| same_process_identity(expected_process, process))
        })
        .cloned()
        .collect()
}

#[cfg(target_os = "windows")]
fn request_windows_close(pid: u32) {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    struct CloseWindowContext {
        pid: u32,
    }

    unsafe extern "system" fn close_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = unsafe { &*(lparam as *const CloseWindowContext) };
        let mut window_pid = 0_u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut window_pid) };
        if window_pid == context.pid {
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        }
        1
    }

    let context = CloseWindowContext { pid };
    unsafe {
        EnumWindows(
            Some(close_window),
            &context as *const CloseWindowContext as LPARAM,
        )
    };
}

#[cfg(not(target_os = "windows"))]
fn request_windows_close(_pid: u32) {}

#[cfg(target_os = "windows")]
fn force_terminate_process_tree(pid: u32, include_descendants: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = Command::new("taskkill.exe");
    command.args(["/PID", &pid.to_string()]);
    if include_descendants {
        command.arg("/T");
    }
    let status = command
        .arg("/F")
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("codex_taskkill_start_failed_for_pid_{pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "codex_taskkill_failed_for_pid_{pid}: exit_status={status}"
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn force_terminate_process_tree(_pid: u32, _include_descendants: bool) -> Result<(), String> {
    Err("codex_runtime_refresh_windows_only".to_string())
}

#[cfg(target_os = "windows")]
fn launch_windows_aumid(aumid: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, AO_NONE,
    };

    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| -> Result<(), String> {
        let manager: IApplicationActivationManager =
            unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) }
                .map_err(|error| format!("codex_aumid_activation_manager_failed: {error}"))?;
        let arguments = HSTRING::from(format!(
            "--remote-debugging-port={} --remote-allow-origins=http://127.0.0.1:{}",
            crate::codex_desktop::DEFAULT_CODEX_DEBUG_PORT,
            crate::codex_desktop::DEFAULT_CODEX_DEBUG_PORT
        ));
        unsafe { manager.ActivateApplication(&HSTRING::from(aumid), &arguments, AO_NONE) }
            .map(|_| ())
            .map_err(|error| format!("codex_aumid_activation_failed: {error}"))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

fn launch_codex_target(target: &CodexRuntimeLaunchTarget) -> Result<(), String> {
    match target {
        #[cfg(target_os = "windows")]
        CodexRuntimeLaunchTarget::WindowsAumid(aumid) => launch_windows_aumid(aumid),
        CodexRuntimeLaunchTarget::DesktopExecutable(path) => {
            crate::codex_desktop::launch_codex_with_debug_port(
                path,
                crate::codex_desktop::DEFAULT_CODEX_DEBUG_PORT,
            )
        }
    }
}

fn system_time_to_millis(time: SystemTime) -> Result<i64, String> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system_time_before_unix_epoch: {error}"))?;
    i64::try_from(duration.as_millis()).map_err(|_| "system_time_millis_overflow".to_string())
}

struct SystemCodexRuntimeRefreshOperations<'a> {
    state: &'a AppState,
    launch_target: CodexRuntimeLaunchTarget,
}

impl CodexRuntimeRefreshOperations for SystemCodexRuntimeRefreshOperations<'_> {
    async fn current_targets(&mut self) -> Result<CodexRuntimeRefreshTargets, String> {
        query_refresh_targets().await
    }

    async fn request_graceful_close(
        &mut self,
        targets: &CodexRuntimeRefreshTargets,
    ) -> Result<(), String> {
        for process in &targets.desktop_shells {
            request_windows_close(process.pid);
        }
        Ok(())
    }

    async fn wait_for_exit(
        &mut self,
        targets: &CodexRuntimeRefreshTargets,
    ) -> Result<Vec<RawCodexRuntimeProcess>, String> {
        let deadline = Instant::now() + CODEX_GRACEFUL_CLOSE_TIMEOUT;
        loop {
            let observed = tokio::task::spawn_blocking(query_codex_runtime_processes)
                .await
                .map_err(|error| format!("codex_runtime_process_query_join_failed: {error}"))??;
            let survivors = surviving_processes(targets, &observed);
            if survivors.is_empty() || Instant::now() >= deadline {
                return Ok(survivors);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn force_terminate(
        &mut self,
        survivors: &[RawCodexRuntimeProcess],
    ) -> Result<(), String> {
        for process in survivors
            .iter()
            .filter(|process| is_official_codex_desktop_shell(process))
        {
            let observed = tokio::task::spawn_blocking(query_codex_runtime_processes)
                .await
                .map_err(|error| format!("codex_runtime_process_query_join_failed: {error}"))??;
            if observed
                .iter()
                .any(|candidate| same_process_identity(process, candidate))
            {
                if let Err(error) = force_terminate_process_tree(process.pid, true) {
                    let after = tokio::task::spawn_blocking(query_codex_runtime_processes)
                        .await
                        .map_err(|join_error| {
                            format!("codex_runtime_process_query_join_failed: {join_error}")
                        })??;
                    if after
                        .iter()
                        .any(|candidate| same_process_identity(process, candidate))
                    {
                        return Err(error);
                    }
                }
            }
        }
        for process in survivors
            .iter()
            .filter(|process| is_codex_app_server(process))
        {
            let observed = tokio::task::spawn_blocking(query_codex_runtime_processes)
                .await
                .map_err(|error| format!("codex_runtime_process_query_join_failed: {error}"))??;
            if observed
                .iter()
                .any(|candidate| same_process_identity(process, candidate))
            {
                if let Err(error) = force_terminate_process_tree(process.pid, false) {
                    let after = tokio::task::spawn_blocking(query_codex_runtime_processes)
                        .await
                        .map_err(|join_error| {
                            format!("codex_runtime_process_query_join_failed: {join_error}")
                        })??;
                    if after
                        .iter()
                        .any(|candidate| same_process_identity(process, candidate))
                    {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }

    async fn apply_ccsm_config(&mut self) -> Result<i64, String> {
        crate::codex_config_consistency::reproject_current_ccsm_config(self.state)
            .await
            .map_err(String::from)?;
        let modified = std::fs::metadata(crate::codex_config::get_codex_config_path())
            .and_then(|metadata| metadata.modified())
            .map_err(|error| format!("codex_config_modified_time_unavailable: {error}"))?;
        system_time_to_millis(modified)
    }

    async fn launch_codex(&mut self) -> Result<(), String> {
        launch_codex_target(&self.launch_target)
    }

    async fn verify_fresh_runtime(&mut self, config_written_at_ms: i64) -> Result<(), String> {
        let deadline = Instant::now() + CODEX_RUNTIME_READY_TIMEOUT;
        loop {
            let targets = query_refresh_targets().await?;
            let fresh_app_server = targets.app_servers.iter().any(|process| {
                chrono::DateTime::parse_from_rfc3339(&process.started_at)
                    .ok()
                    .is_some_and(|started_at| started_at.timestamp_millis() >= config_written_at_ms)
            });
            if !targets.desktop_shells.is_empty() && fresh_app_server {
                let consistency =
                    crate::codex_config_consistency::inspect(self.state).map_err(String::from)?;
                if consistency.state != CodexConfigConsistencyState::ExternalDrift
                    && consistency.runtime_activation.state
                        == CodexConfigRuntimeActivationState::Current
                {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err("codex_runtime_did_not_become_current_before_timeout".to_string());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

#[tauri::command]
pub async fn inspect_codex_runtime_refresh() -> Result<CodexRuntimeRefreshPreflight, String> {
    build_preflight().await
}

#[tauri::command]
pub async fn refresh_codex_runtime_state(
    app: AppHandle,
    state: State<'_, AppState>,
    snapshot_token: String,
) -> Result<CodexRuntimeRefreshResult, String> {
    let _refresh_guard = CODEX_RUNTIME_REFRESH_LOCK
        .try_lock()
        .map_err(|_| "codex_runtime_refresh_already_running".to_string())?;
    let launch_target = resolve_launch_target()
        .ok_or_else(|| "codex_desktop_launch_target_not_found".to_string())?;
    let mut operations = SystemCodexRuntimeRefreshOperations {
        state: &state,
        launch_target,
    };
    execute_refresh_transaction(&mut operations, &snapshot_token, |progress| {
        if let Err(error) = app.emit(CODEX_RUNTIME_REFRESH_EVENT, &progress) {
            log::warn!("Codex runtime refresh progress event failed: {error}");
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn process(
        pid: u32,
        parent_pid: u32,
        name: &str,
        executable_path: &str,
        command_line: &str,
    ) -> RawCodexRuntimeProcess {
        RawCodexRuntimeProcess {
            pid,
            parent_pid,
            name: name.to_string(),
            executable_path: executable_path.to_string(),
            command_line: command_line.to_string(),
            started_at: "2026-09-01T02:00:00+08:00".to_string(),
        }
    }

    #[test]
    fn refresh_targets_include_the_new_packaged_shell_and_its_app_server_only() {
        let processes = vec![
            process(
                100,
                10,
                "ChatGPT.exe",
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
                r#""ChatGPT.exe""#,
            ),
            process(
                101,
                100,
                "codex.exe",
                r"C:\Users\sunda\AppData\Local\OpenAI\Codex\bin\hash\codex.exe",
                r#""codex.exe" app-server"#,
            ),
            process(
                200,
                20,
                "ChatGPT.exe",
                r"C:\Program Files\WindowsApps\OpenAI.ChatGPT_1.0.0.0_x64__other\app\ChatGPT.exe",
                r#""ChatGPT.exe""#,
            ),
            process(
                201,
                20,
                "codex.exe",
                r"C:\Tools\codex.exe",
                r#""codex.exe" app-server"#,
            ),
            process(
                202,
                20,
                "Codex.exe",
                r"C:\Tools\Codex.exe",
                r#""Codex.exe""#,
            ),
        ];

        let targets = classify_refresh_targets(&processes);

        assert_eq!(targets.desktop_shells.len(), 1);
        assert_eq!(targets.desktop_shells[0].pid, 100);
        assert_eq!(targets.app_servers.len(), 1);
        assert_eq!(targets.app_servers[0].pid, 101);
    }

    #[test]
    fn renderer_children_are_not_mistaken_for_a_second_desktop_shell() {
        let processes = vec![
            process(
                100,
                10,
                "ChatGPT.exe",
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
                r#""ChatGPT.exe""#,
            ),
            process(
                102,
                100,
                "ChatGPT.exe",
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
                r#""ChatGPT.exe" --type=renderer"#,
            ),
        ];

        let targets = classify_refresh_targets(&processes);

        assert_eq!(targets.desktop_shells.len(), 1);
        assert_eq!(targets.desktop_shells[0].pid, 100);
    }

    #[test]
    fn known_legacy_desktop_install_is_still_a_refresh_target() {
        let targets = classify_refresh_targets(&[process(
            100,
            10,
            "Codex.exe",
            r"C:\Users\sunda\AppData\Local\Programs\Codex\app-0.1.0\Codex.exe",
            r#""Codex.exe""#,
        )]);

        assert_eq!(targets.desktop_shells.len(), 1);
        assert_eq!(targets.desktop_shells[0].pid, 100);
    }

    #[test]
    fn incomplete_process_identity_is_never_a_refresh_target() {
        let mut shell = process(
            100,
            10,
            "ChatGPT.exe",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
            r#""ChatGPT.exe""#,
        );
        shell.started_at.clear();

        let targets = classify_refresh_targets(&[shell]);

        assert!(targets.desktop_shells.is_empty());
        assert!(targets.app_servers.is_empty());
    }

    #[test]
    fn preflight_token_changes_when_a_verified_process_identity_changes() {
        let first = classify_refresh_targets(&[process(
            100,
            10,
            "ChatGPT.exe",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
            r#""ChatGPT.exe""#,
        )]);
        let second = classify_refresh_targets(&[process(
            100,
            10,
            "ChatGPT.exe",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
            r#""ChatGPT.exe" --new-instance"#,
        )]);

        assert_ne!(
            refresh_target_fingerprint(&first),
            refresh_target_fingerprint(&second)
        );
    }

    #[derive(Default)]
    struct FakeRefreshOperations {
        targets: CodexRuntimeRefreshTargets,
        wait_results: VecDeque<Vec<RawCodexRuntimeProcess>>,
        log: Vec<&'static str>,
        apply_error: Option<String>,
    }

    impl CodexRuntimeRefreshOperations for FakeRefreshOperations {
        async fn current_targets(&mut self) -> Result<CodexRuntimeRefreshTargets, String> {
            self.log.push("inspect");
            Ok(self.targets.clone())
        }

        async fn request_graceful_close(
            &mut self,
            _targets: &CodexRuntimeRefreshTargets,
        ) -> Result<(), String> {
            self.log.push("graceful_close");
            Ok(())
        }

        async fn wait_for_exit(
            &mut self,
            _targets: &CodexRuntimeRefreshTargets,
        ) -> Result<Vec<RawCodexRuntimeProcess>, String> {
            self.log.push("wait_for_exit");
            Ok(self.wait_results.pop_front().unwrap_or_default())
        }

        async fn force_terminate(
            &mut self,
            _survivors: &[RawCodexRuntimeProcess],
        ) -> Result<(), String> {
            self.log.push("force_terminate");
            Ok(())
        }

        async fn apply_ccsm_config(&mut self) -> Result<i64, String> {
            self.log.push("apply_config");
            match self.apply_error.take() {
                Some(error) => Err(error),
                None => Ok(1_788_200_000_000),
            }
        }

        async fn launch_codex(&mut self) -> Result<(), String> {
            self.log.push("launch");
            Ok(())
        }

        async fn verify_fresh_runtime(&mut self, _config_written_at_ms: i64) -> Result<(), String> {
            self.log.push("verify");
            Ok(())
        }
    }

    fn one_shell_target() -> CodexRuntimeRefreshTargets {
        classify_refresh_targets(&[process(
            100,
            10,
            "ChatGPT.exe",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.825.6671.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
            r#""ChatGPT.exe""#,
        )])
    }

    #[tokio::test]
    async fn refresh_transaction_forces_verified_survivors_before_rewriting_and_launching() {
        let targets = one_shell_target();
        let token = refresh_target_fingerprint(&targets);
        let mut operations = FakeRefreshOperations {
            targets: targets.clone(),
            wait_results: VecDeque::from([targets.desktop_shells.clone(), Vec::new()]),
            ..Default::default()
        };
        let mut stages = Vec::new();

        let result = execute_refresh_transaction(&mut operations, &token, |progress| {
            stages.push(progress.stage)
        })
        .await
        .expect("refresh should complete");

        assert!(result.force_terminated);
        assert_eq!(
            operations.log,
            vec![
                "inspect",
                "graceful_close",
                "wait_for_exit",
                "force_terminate",
                "wait_for_exit",
                "apply_config",
                "launch",
                "verify",
            ]
        );
        assert_eq!(
            stages,
            vec![
                CodexRuntimeRefreshStage::Closing,
                CodexRuntimeRefreshStage::ForceClosing,
                CodexRuntimeRefreshStage::ApplyingConfig,
                CodexRuntimeRefreshStage::Launching,
                CodexRuntimeRefreshStage::Verifying,
                CodexRuntimeRefreshStage::Completed,
            ]
        );
    }

    #[tokio::test]
    async fn stale_preflight_stops_before_any_process_is_closed() {
        let mut operations = FakeRefreshOperations {
            targets: one_shell_target(),
            ..Default::default()
        };

        let error = execute_refresh_transaction(&mut operations, "stale-token", |_| {})
            .await
            .expect_err("stale inspection must be rejected");

        assert!(error.contains("runtime_changed_since_inspection"));
        assert_eq!(operations.log, vec!["inspect"]);
    }

    #[tokio::test]
    async fn config_failure_reopens_codex_after_the_verified_runtime_was_closed() {
        let targets = one_shell_target();
        let token = refresh_target_fingerprint(&targets);
        let mut operations = FakeRefreshOperations {
            targets,
            wait_results: VecDeque::from([Vec::new()]),
            apply_error: Some("projection failed".to_string()),
            ..Default::default()
        };

        let error = execute_refresh_transaction(&mut operations, &token, |_| {})
            .await
            .expect_err("config failure should be reported");

        assert!(error.contains("projection failed"));
        assert_eq!(
            operations.log,
            vec![
                "inspect",
                "graceful_close",
                "wait_for_exit",
                "apply_config",
                "launch",
            ]
        );
    }
}
