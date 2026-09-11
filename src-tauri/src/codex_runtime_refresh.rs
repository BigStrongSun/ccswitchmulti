use crate::codex_config_consistency::{
    CodexConfigConsistencyState, CodexConfigRuntimeActivationState,
};
use crate::store::AppState;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

#[path = "codex_paginated_history_repair.rs"]
mod paginated_history;

static CODEX_RUNTIME_REFRESH_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

const CODEX_RUNTIME_REFRESH_EVENT: &str = "codex-runtime-refresh-progress";
const CODEX_GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(120);
const CODEX_HISTORY_REBUILD_BYTES_PER_WINDOW: u64 = 100_000_000;
const CODEX_HISTORY_REBUILD_WINDOW: Duration = Duration::from_secs(30);
const CODEX_HISTORY_REBUILD_TIMEOUT_CAP: Duration = Duration::from_secs(15 * 60);
/// renderer 兼容层通过 1.5s 定时注入收敛。验证时必须等待它真正收敛，
/// 否则会在补丁尚未挂载时误报 `codex_history_compatibility_not_ready`，
/// 导致新版本地历史目录和全 Provider 查询被判定为未修复。
const CODEX_RENDERER_PATCH_READY_TIMEOUT: Duration = Duration::from_secs(10);
/// 长操作心跳：后端在该间隔内至少上报一次，前端据此区分“仍在执行”和“任务/事件链已卡死”。
const CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
/// 分页历史修复的硬超时，避免底层文件/SQLite 卡住时命令永久不返回。
const CODEX_HISTORY_REPAIR_TIMEOUT: Duration = Duration::from_secs(15 * 60);

fn runtime_verification_timeout(
    history_repair: Option<&paginated_history::PaginatedHistoryRepairOutcome>,
) -> Duration {
    let Some(history_repair) = history_repair else {
        return CODEX_RUNTIME_READY_TIMEOUT;
    };
    if history_repair.targets.is_empty() {
        return CODEX_RUNTIME_READY_TIMEOUT;
    }
    let bytes = history_repair
        .targets
        .iter()
        .map(|target| target.minimum_next_byte_offset)
        .max()
        .unwrap_or_default();
    let windows = bytes.saturating_add(CODEX_HISTORY_REBUILD_BYTES_PER_WINDOW - 1)
        / CODEX_HISTORY_REBUILD_BYTES_PER_WINDOW;
    CODEX_RUNTIME_READY_TIMEOUT
        .max(CODEX_HISTORY_REBUILD_WINDOW.saturating_mul(windows as u32))
        .min(CODEX_HISTORY_REBUILD_TIMEOUT_CAP)
}

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
    #[cfg(target_os = "windows")]
    fn process_count(&self) -> usize {
        self.desktop_shells.len() + self.app_servers.len()
    }

    fn processes(&self) -> impl Iterator<Item = &RawCodexRuntimeProcess> {
        self.desktop_shells.iter().chain(self.app_servers.iter())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CodexRuntimeLaunchTarget {
    #[cfg(target_os = "windows")]
    WindowsAumid(String),
    DesktopExecutable(PathBuf),
}

impl CodexRuntimeLaunchTarget {
    #[cfg(target_os = "windows")]
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
    pub paginated_history: paginated_history::PaginatedHistoryRepairPreflight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeRefreshStage {
    Closing,
    ForceClosing,
    RepairingHistory,
    ApplyingConfig,
    Launching,
    Verifying,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeRefreshProgressKind {
    Stage,
    Log,
    Heartbeat,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeRefreshProgress {
    pub stage: CodexRuntimeRefreshStage,
    pub kind: CodexRuntimeRefreshProgressKind,
    pub sequence: u64,
    pub emitted_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl CodexRuntimeRefreshProgress {
    fn stage(stage: CodexRuntimeRefreshStage) -> Self {
        Self {
            stage,
            kind: CodexRuntimeRefreshProgressKind::Stage,
            sequence: 0,
            emitted_at_ms: 0,
            code: None,
            message: None,
        }
    }

    fn log(stage: CodexRuntimeRefreshStage, code: &str, message: impl Into<String>) -> Self {
        Self {
            stage,
            kind: CodexRuntimeRefreshProgressKind::Log,
            sequence: 0,
            emitted_at_ms: 0,
            code: Some(code.to_string()),
            message: Some(message.into()),
        }
    }

    fn heartbeat(stage: CodexRuntimeRefreshStage) -> Self {
        Self {
            stage,
            kind: CodexRuntimeRefreshProgressKind::Heartbeat,
            sequence: 0,
            emitted_at_ms: 0,
            code: None,
            message: None,
        }
    }
}

fn progress_emitted_at_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

/// 跨 async 任务和 spawn_blocking 共享的 Tauri 事件发送器。
///
/// 序列号只由这里分配，保证前端可以丢弃乱序/迟到事件；`AppHandle` 和
/// `Arc<AtomicU64>` 均为 Send + Sync，可安全克隆进分页历史修复的阻塞任务。
#[derive(Clone)]
struct RuntimeRefreshProgressEmitter {
    app: AppHandle,
    sequence: Arc<AtomicU64>,
}

impl RuntimeRefreshProgressEmitter {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    fn emit(&self, mut progress: CodexRuntimeRefreshProgress) {
        progress.sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        progress.emitted_at_ms = progress_emitted_at_ms();
        if let Err(error) = self.app.emit(CODEX_RUNTIME_REFRESH_EVENT, &progress) {
            log::warn!("Codex runtime refresh progress event failed: {error}");
        }
    }
}

impl From<paginated_history::PaginatedHistoryRepairProgress> for CodexRuntimeRefreshProgress {
    fn from(event: paginated_history::PaginatedHistoryRepairProgress) -> Self {
        use paginated_history::PaginatedHistoryRepairProgress;
        match event {
            PaginatedHistoryRepairProgress::PlanScanStarted => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_scan_started",
                "正在扫描分页历史文件与投影游标",
            ),
            PaginatedHistoryRepairProgress::PlanReady {
                repair_candidate_count,
                provider_cursor_repair_count,
                provider_history_base_repair_count,
                blocked_count,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_plan_ready",
                format!(
                    "历史扫描完成：待修复文件 {repair_candidate_count} 个，迁移游标 {provider_cursor_repair_count} 个，父段引用 {provider_history_base_repair_count} 个，被阻止 {blocked_count} 个"
                ),
            ),
            PaginatedHistoryRepairProgress::ProviderMigrationStarted {
                cursor_count,
                history_base_count,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "provider_migration_started",
                format!("正在恢复 Provider 迁移游标 {cursor_count} 个、父段引用 {history_base_count} 个"),
            ),
            PaginatedHistoryRepairProgress::ProviderMigrationFinished {
                repaired_cursor_count,
                repaired_history_base_count,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "provider_migration_finished",
                format!("Provider 迁移恢复完成：游标 {repaired_cursor_count} 个，父段引用 {repaired_history_base_count} 个"),
            ),
            PaginatedHistoryRepairProgress::RepairFileStarted {
                index,
                total,
                source_id,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_file_started",
                format!("正在修复历史文件 {index}/{total}：{source_id}"),
            ),
            PaginatedHistoryRepairProgress::RepairFileSkipped {
                index,
                total,
                source_id,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_file_skipped",
                format!("历史文件 {index}/{total} 无需修复：{source_id}"),
            ),
            PaginatedHistoryRepairProgress::RepairFileFinished {
                index,
                total,
                source_id,
                skipped_duplicate_count,
            } => CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_file_finished",
                format!("已修复历史文件 {index}/{total}：{source_id}（跳过重复序号 {skipped_duplicate_count} 个）"),
            ),
        }
    }
}

/// 等待长操作完成，同时周期性地发出心跳。
///
/// 心跳只证明后端事件循环仍活着；具体步骤的进展由 `CodexRuntimeRefreshProgress::log`
/// 事件提供。两者分离后，前端可以在“有进展但较慢”和“完全无事件/疑似卡死”之间做判断。
async fn await_with_heartbeat<F, T>(
    future: F,
    stage: CodexRuntimeRefreshStage,
    heartbeat_interval: Duration,
    emit: &mut impl FnMut(CodexRuntimeRefreshProgress),
) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    tokio::pin!(future);
    let mut interval = tokio::time::interval(heartbeat_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // interval 的第一次 tick 立即返回；消费掉它，避免刚进入长操作就发心跳。
    interval.tick().await;
    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = interval.tick() => emit(CodexRuntimeRefreshProgress::heartbeat(stage)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeRefreshResult {
    pub outcome: CodexRuntimeRefreshOutcome,
    pub config_status: CodexRuntimeCheckStatus,
    pub paginated_history_status: CodexRuntimeCheckStatus,
    pub renderer_compatibility_status: CodexRuntimeCheckStatus,
    pub renderer_compatibility_message: Option<String>,
    pub force_terminated: bool,
    pub closed_process_count: usize,
    pub repaired_history_rollout_count: usize,
    pub repaired_history_duplicate_count: usize,
    pub repaired_history_provider_migration_cursor_count: usize,
    pub repaired_history_provider_migration_history_base_count: usize,
    pub repaired_history_rotated_thread_count: usize,
    pub repaired_history_rotated_segment_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeRefreshOutcome {
    Completed,
    CompletedWithWarnings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeCheckStatus {
    Ready,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CodexRuntimeVerification {
    outcome: CodexRuntimeRefreshOutcome,
    config_status: CodexRuntimeCheckStatus,
    paginated_history_status: CodexRuntimeCheckStatus,
    renderer_compatibility_status: CodexRuntimeCheckStatus,
    renderer_compatibility_message: Option<String>,
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
    async fn repair_paginated_history(
        &mut self,
    ) -> Result<paginated_history::PaginatedHistoryRepairOutcome, String>;
    async fn apply_ccsm_config(&mut self) -> Result<i64, String>;
    async fn launch_codex(&mut self) -> Result<(), String>;
    /// 安装/探测 Codex renderer 兼容层。默认走真实 CDP 注入；测试可注入收敛序列。
    async fn unlock_renderer_compatibility(
        &mut self,
    ) -> Result<crate::codex_desktop::CodexModelPickerUnlockResult, String> {
        crate::codex_desktop::unlock_codex_model_picker().await
    }
    async fn verify_fresh_runtime(
        &mut self,
        config_written_at_ms: i64,
    ) -> Result<CodexRuntimeVerification, String>;
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
    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::Closing,
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Closing,
        "refresh_prepare",
        "正在检查 Codex 运行进程",
    ));
    let targets = operations.current_targets().await?;
    if refresh_target_fingerprint(&targets) != expected_snapshot_token {
        return Err("runtime_changed_since_inspection".to_string());
    }
    let closed_process_count = targets.desktop_shells.len() + targets.app_servers.len();

    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Closing,
        "refresh_started",
        format!("开始刷新 Codex 状态：准备关闭 {closed_process_count} 个进程"),
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Closing,
        "closing_requested",
        "已向 Codex 桌面进程发送关闭请求",
    ));
    if let Err(error) = await_with_heartbeat(
        operations.request_graceful_close(&targets),
        CodexRuntimeRefreshStage::Closing,
        CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
        &mut emit,
    )
    .await
    {
        emit(CodexRuntimeRefreshProgress::log(
            CodexRuntimeRefreshStage::Closing,
            "closing_failed",
            format!("发送关闭请求失败：{error}"),
        ));
        return Err(error);
    }

    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Closing,
        "waiting_for_exit",
        "正在等待 Codex 进程退出",
    ));
    let mut survivors = match await_with_heartbeat(
        operations.wait_for_exit(&targets),
        CodexRuntimeRefreshStage::Closing,
        CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
        &mut emit,
    )
    .await
    {
        Ok(survivors) => survivors,
        Err(error) => {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::Closing,
                "waiting_for_exit_failed",
                format!("等待 Codex 进程退出失败：{error}"),
            ));
            return Err(error);
        }
    };
    let force_terminated = !survivors.is_empty();
    if force_terminated {
        emit(CodexRuntimeRefreshProgress::stage(
            CodexRuntimeRefreshStage::ForceClosing,
        ));
        emit(CodexRuntimeRefreshProgress::log(
            CodexRuntimeRefreshStage::ForceClosing,
            "force_closing",
            format!("有 {} 个进程未在时限内退出，正在强制结束", survivors.len()),
        ));
        if let Err(error) = await_with_heartbeat(
            operations.force_terminate(&survivors),
            CodexRuntimeRefreshStage::ForceClosing,
            CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
            &mut emit,
        )
        .await
        {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::ForceClosing,
                "force_close_failed",
                format!("强制结束进程失败：{error}"),
            ));
            return Err(error);
        }
        survivors = match await_with_heartbeat(
            operations.wait_for_exit(&targets),
            CodexRuntimeRefreshStage::ForceClosing,
            CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
            &mut emit,
        )
        .await
        {
            Ok(survivors) => survivors,
            Err(error) => {
                emit(CodexRuntimeRefreshProgress::log(
                    CodexRuntimeRefreshStage::ForceClosing,
                    "waiting_after_force_failed",
                    format!("等待强制结束后失败：{error}"),
                ));
                return Err(error);
            }
        };
        if !survivors.is_empty() {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::ForceClosing,
                "force_close_incomplete",
                format!("仍有 {} 个 Codex 进程未退出", survivors.len()),
            ));
            return Err("codex_runtime_still_running_after_forced_close".to_string());
        }
    }

    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::RepairingHistory,
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::RepairingHistory,
        "history_repair_started",
        "开始检查并修复分页历史",
    ));
    let history_repair = match await_with_heartbeat(
        operations.repair_paginated_history(),
        CodexRuntimeRefreshStage::RepairingHistory,
        CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
        &mut emit,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::RepairingHistory,
                "history_repair_failed",
                format!("历史修复失败：{error}"),
            ));
            emit(CodexRuntimeRefreshProgress::stage(
                CodexRuntimeRefreshStage::Launching,
            ));
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::Launching,
                "relaunch_after_history_failure_started",
                "历史修复失败，正在尝试重新打开 Codex",
            ));
            let relaunch = operations.launch_codex().await;
            return match relaunch {
                Ok(()) => Err(error),
                Err(launch_error) => Err(format!(
                    "{error}; codex_relaunch_after_history_repair_failure_failed: {launch_error}"
                )),
            };
        }
    };
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::RepairingHistory,
        "history_repair_finished",
        format!(
            "历史修复完成：文件 {} 个，重复序号 {} 个，迁移游标 {} 个，父段引用 {} 个",
            history_repair.repaired_rollout_count,
            history_repair.repaired_duplicate_count,
            history_repair.repaired_provider_migration_cursor_count,
            history_repair.repaired_provider_migration_history_base_count,
        ),
    ));

    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::ApplyingConfig,
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::ApplyingConfig,
        "config_apply_started",
        "正在重投影 CCSM 配置",
    ));
    let config_written_at_ms = match await_with_heartbeat(
        operations.apply_ccsm_config(),
        CodexRuntimeRefreshStage::ApplyingConfig,
        CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
        &mut emit,
    )
    .await
    {
        Ok(timestamp) => timestamp,
        Err(error) => {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::ApplyingConfig,
                "config_apply_failed",
                format!("应用 CCSM 配置失败：{error}"),
            ));
            emit(CodexRuntimeRefreshProgress::stage(
                CodexRuntimeRefreshStage::Launching,
            ));
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::Launching,
                "relaunch_after_config_failure_started",
                "配置应用失败，正在尝试重新打开 Codex",
            ));
            let relaunch = operations.launch_codex().await;
            return match relaunch {
                Ok(()) => Err(error),
                Err(launch_error) => Err(format!(
                    "{error}; codex_relaunch_after_config_failure_failed: {launch_error}"
                )),
            };
        }
    };
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::ApplyingConfig,
        "config_apply_finished",
        "CCSM 配置已写入",
    ));

    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::Launching,
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Launching,
        "launch_started",
        "正在重新启动 Codex",
    ));
    if let Err(error) = operations.launch_codex().await {
        emit(CodexRuntimeRefreshProgress::log(
            CodexRuntimeRefreshStage::Launching,
            "launch_failed",
            format!("启动 Codex 失败：{error}"),
        ));
        return Err(error);
    }
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Launching,
        "launch_finished",
        "Codex 启动请求已发送",
    ));

    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::Verifying,
    ));
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Verifying,
        "verification_started",
        "正在验证新的 Codex 运行状态",
    ));
    let verification = match await_with_heartbeat(
        operations.verify_fresh_runtime(config_written_at_ms),
        CodexRuntimeRefreshStage::Verifying,
        CODEX_RUNTIME_REFRESH_HEARTBEAT_INTERVAL,
        &mut emit,
    )
    .await
    {
        Ok(verification) => verification,
        Err(error) => {
            emit(CodexRuntimeRefreshProgress::log(
                CodexRuntimeRefreshStage::Verifying,
                "verification_failed",
                format!("验证 Codex 运行状态失败：{error}"),
            ));
            return Err(error);
        }
    };
    emit(CodexRuntimeRefreshProgress::log(
        CodexRuntimeRefreshStage::Verifying,
        "verification_finished",
        "Codex 运行状态验证完成",
    ));
    emit(CodexRuntimeRefreshProgress::stage(
        CodexRuntimeRefreshStage::Completed,
    ));

    Ok(CodexRuntimeRefreshResult {
        outcome: verification.outcome,
        config_status: verification.config_status,
        paginated_history_status: verification.paginated_history_status,
        renderer_compatibility_status: verification.renderer_compatibility_status,
        renderer_compatibility_message: verification.renderer_compatibility_message,
        force_terminated,
        closed_process_count,
        repaired_history_rollout_count: history_repair.repaired_rollout_count,
        repaired_history_duplicate_count: history_repair.repaired_duplicate_count,
        repaired_history_provider_migration_cursor_count: history_repair
            .repaired_provider_migration_cursor_count,
        repaired_history_provider_migration_history_base_count: history_repair
            .repaired_provider_migration_history_base_count,
        repaired_history_rotated_thread_count: history_repair.repaired_rotated_thread_count,
        repaired_history_rotated_segment_count: history_repair.repaired_rotated_segment_count,
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

fn select_launch_target(
    aumid: Option<String>,
    executable: Option<PathBuf>,
    timezone_injection_enabled: bool,
) -> Option<CodexRuntimeLaunchTarget> {
    if timezone_injection_enabled {
        if let Some(executable) = executable.clone() {
            return Some(CodexRuntimeLaunchTarget::DesktopExecutable(executable));
        }
    }
    #[cfg(target_os = "windows")]
    if let Some(aumid) = aumid {
        return Some(CodexRuntimeLaunchTarget::WindowsAumid(aumid));
    }
    #[cfg(not(target_os = "windows"))]
    let _ = aumid;
    executable.map(CodexRuntimeLaunchTarget::DesktopExecutable)
}

fn resolve_launch_target() -> Option<CodexRuntimeLaunchTarget> {
    let executable = crate::codex_desktop::resolve_codex_executable();
    let timezone_injection_enabled =
        crate::codex_egress_timezone::resolve_launch_timezone(&crate::settings::get_settings())
            .is_some();
    #[cfg(target_os = "windows")]
    let aumid = resolve_windows_codex_aumid();
    #[cfg(not(target_os = "windows"))]
    let aumid = None;
    select_launch_target(aumid, executable, timezone_injection_enabled)
}

#[cfg(not(target_os = "windows"))]
async fn build_preflight() -> Result<CodexRuntimeRefreshPreflight, String> {
    Ok(CodexRuntimeRefreshPreflight {
        supported: false,
        can_refresh: false,
        snapshot_token: String::new(),
        desktop_process_count: 0,
        app_server_process_count: 0,
        process_count: 0,
        launch_target: None,
        warning: Some("codex_runtime_refresh_windows_only".to_string()),
        paginated_history: Default::default(),
    })
}

#[cfg(target_os = "windows")]
async fn build_preflight() -> Result<CodexRuntimeRefreshPreflight, String> {
    let targets = query_refresh_targets().await?;
    let launch_target = resolve_launch_target();
    let paginated_history =
        tokio::task::spawn_blocking(paginated_history::inspect_paginated_history_repair)
            .await
            .map_err(|error| format!("paginated_history_inspection_join_failed: {error}"))??;
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
        paginated_history,
    })
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

#[cfg(any(target_os = "windows", test))]
fn force_terminate_process_arguments(pid: u32) -> Vec<String> {
    vec!["/PID".to_string(), pid.to_string(), "/F".to_string()]
}

fn runtime_verification_result(
    config_ready: bool,
    paginated_history_ready: bool,
    injected: bool,
    all_provider_history_patched: bool,
    history_refresh_requested: bool,
    renderer_message: Option<String>,
) -> Result<CodexRuntimeVerification, String> {
    if !config_ready {
        return Err("codex_runtime_config_not_current".to_string());
    }
    if !paginated_history_ready {
        return Err("codex_paginated_history_projection_not_caught_up".to_string());
    }

    let renderer_ready = injected && all_provider_history_patched && history_refresh_requested;
    Ok(CodexRuntimeVerification {
        outcome: if renderer_ready {
            CodexRuntimeRefreshOutcome::Completed
        } else {
            CodexRuntimeRefreshOutcome::CompletedWithWarnings
        },
        config_status: CodexRuntimeCheckStatus::Ready,
        paginated_history_status: CodexRuntimeCheckStatus::Ready,
        renderer_compatibility_status: if renderer_ready {
            CodexRuntimeCheckStatus::Ready
        } else {
            CodexRuntimeCheckStatus::Warning
        },
        renderer_compatibility_message: if renderer_ready {
            None
        } else {
            renderer_message
        },
    })
}

#[cfg(target_os = "windows")]
fn force_terminate_process(pid: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = Command::new("taskkill.exe");
    command.args(force_terminate_process_arguments(pid));
    let status = command
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
fn force_terminate_process(_pid: u32) -> Result<(), String> {
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
    history_repair_outcome: Option<paginated_history::PaginatedHistoryRepairOutcome>,
    progress: RuntimeRefreshProgressEmitter,
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
                if let Err(error) = force_terminate_process(process.pid) {
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
                if let Err(error) = force_terminate_process(process.pid) {
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

    async fn repair_paginated_history(
        &mut self,
    ) -> Result<paginated_history::PaginatedHistoryRepairOutcome, String> {
        let progress = self.progress.clone();
        let repair = tokio::time::timeout(
            CODEX_HISTORY_REPAIR_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                paginated_history::repair_paginated_history_after_codex_exit(|event| {
                    progress.emit(event.into());
                })
            }),
        )
        .await;
        let outcome = match repair {
            Ok(join_result) => join_result
                .map_err(|error| format!("paginated_history_repair_join_failed: {error}"))??,
            Err(_) => {
                self.progress.emit(CodexRuntimeRefreshProgress::log(
                    CodexRuntimeRefreshStage::RepairingHistory,
                    "history_repair_timed_out",
                    "历史修复超过 15 分钟，已停止等待；后台文件任务可能仍在收尾",
                ));
                return Err("codex_paginated_history_repair_timed_out".to_string());
            }
        };
        self.history_repair_outcome = Some(outcome.clone());
        Ok(outcome)
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

    async fn verify_fresh_runtime(
        &mut self,
        config_written_at_ms: i64,
    ) -> Result<CodexRuntimeVerification, String> {
        let deadline =
            Instant::now() + runtime_verification_timeout(self.history_repair_outcome.as_ref());
        let mut last_core_error = None;
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
                    let history_ready = self
                        .history_repair_outcome
                        .as_ref()
                        .map(paginated_history::repaired_projections_caught_up)
                        .transpose()?
                        .unwrap_or(true);
                    if !history_ready {
                        if let Some(outcome) = self.history_repair_outcome.clone() {
                            let repaired = tokio::task::spawn_blocking(move || {
                                paginated_history::repair_newly_stalled_projection_cursors(&outcome)
                            })
                            .await
                            .map_err(|error| {
                                format!("paginated_history_followup_repair_join_failed: {error}")
                            })??;
                            if repaired > 0 {
                                log::info!(
                                    "Rewound {repaired} later Codex paginated-history projection cursor(s) during verification"
                                );
                            }
                        }
                        last_core_error =
                            Some("codex_paginated_history_projection_not_caught_up".to_string());
                    } else {
                        return poll_renderer_compatibility(
                            CODEX_RENDERER_PATCH_READY_TIMEOUT,
                            CodexRendererProbe { operations: self },
                        )
                        .await;
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(last_core_error.unwrap_or_else(|| {
                    "codex_runtime_did_not_become_current_before_timeout".to_string()
                }));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// 可注入的 renderer 兼容层探测接口，供 `poll_renderer_compatibility` 使用；
/// 生产实现包装 `SystemCodexRuntimeRefreshOperations`，测试用 mock 提供收敛序列。
trait RendererCompatibilityProbe {
    async fn probe(&mut self)
        -> Result<crate::codex_desktop::CodexModelPickerUnlockResult, String>;
}

/// 生产探测包装：把真实 CCSM 操作借用给轮询器，避免闭包捕获 `&mut self`
/// 返回值借用外部状态的限制。
struct CodexRendererProbe<'a, 'b> {
    operations: &'a mut SystemCodexRuntimeRefreshOperations<'b>,
}

impl RendererCompatibilityProbe for CodexRendererProbe<'_, '_> {
    async fn probe(
        &mut self,
    ) -> Result<crate::codex_desktop::CodexModelPickerUnlockResult, String> {
        self.operations.unlock_renderer_compatibility().await
    }
}

/// 轮询 renderer 兼容层，直到全 Provider 历史查询补丁真正收敛或超时。
///
/// `unlock_codex_model_picker` 每次只读一份补丁快照；补丁通过 1.5s 定时注入逐步收敛，
/// 单纯读取一次会在补丁尚未挂载时报 `codex_history_compatibility_not_ready`。这里在
/// 收敛窗口内重复探测，收敛即判定 Ready，超时才降级为 CompletedWithWarnings。
async fn poll_renderer_compatibility<P>(
    timeout: Duration,
    mut probe: P,
) -> Result<CodexRuntimeVerification, String>
where
    P: RendererCompatibilityProbe,
{
    let deadline = Instant::now() + timeout;
    #[allow(unused_assignments)]
    let mut last_message = None;
    loop {
        match probe.probe().await {
            Ok(result) => {
                last_message = Some(result.message);
                if result.injected
                    && result.all_provider_history_patched
                    && result.history_refresh_requested
                {
                    return runtime_verification_result(true, true, true, true, true, None);
                }
            }
            Err(error) => {
                last_message = Some(format!(
                    "codex_history_compatibility_install_failed: {error}"
                ));
            }
        }
        if Instant::now() >= deadline {
            return runtime_verification_result(
                true,
                true,
                false,
                false,
                false,
                last_message.or_else(|| Some("codex_history_compatibility_not_ready".to_string())),
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    let progress = RuntimeRefreshProgressEmitter::new(app);
    let mut operations = SystemCodexRuntimeRefreshOperations {
        state: &state,
        launch_target,
        history_repair_outcome: None,
        progress: progress.clone(),
    };
    execute_refresh_transaction(&mut operations, &snapshot_token, |event| {
        progress.emit(event);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn long_running_stage_emits_heartbeat_progress() {
        let mut events = Vec::new();
        await_with_heartbeat(
            async {
                tokio::time::sleep(Duration::from_millis(40)).await;
                Ok::<_, String>(())
            },
            CodexRuntimeRefreshStage::RepairingHistory,
            Duration::from_millis(10),
            &mut |progress| events.push(progress),
        )
        .await
        .expect("operation should finish");

        assert!(events.iter().any(|progress| {
            progress.kind == CodexRuntimeRefreshProgressKind::Heartbeat
                && progress.stage == CodexRuntimeRefreshStage::RepairingHistory
        }));
    }

    #[test]
    fn timezone_injection_prefers_executable_over_aumid_launch() {
        let executable = PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\ChatGPT.exe");
        let target = select_launch_target(
            Some("OpenAI.Codex_123!App".to_string()),
            Some(executable.clone()),
            true,
        );

        assert_eq!(
            target,
            Some(CodexRuntimeLaunchTarget::DesktopExecutable(executable))
        );
    }
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
    fn runtime_verification_budget_scales_for_large_projection_catch_up() {
        assert_eq!(
            runtime_verification_timeout(None),
            CODEX_RUNTIME_READY_TIMEOUT
        );
        let large_rebuild = paginated_history::PaginatedHistoryRepairOutcome {
            repaired_provider_migration_cursor_count: 1,
            targets: vec![paginated_history::ProjectionCatchUpTarget {
                source_id: "thread".to_string(),
                rollout_path: PathBuf::from("rollout.jsonl"),
                minimum_next_ordinal: 10,
                minimum_next_byte_offset: 1_500_000_000,
            }],
            ..Default::default()
        };
        assert!(runtime_verification_timeout(Some(&large_rebuild)) >= Duration::from_secs(7 * 60));
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

    #[test]
    fn forced_close_targets_only_the_revalidated_pid_without_descendant_tree_kill() {
        assert_eq!(
            force_terminate_process_arguments(4242),
            vec!["/PID", "4242", "/F"]
        );
    }

    #[test]
    fn renderer_compatibility_is_reported_independently_from_core_runtime_readiness() {
        let warning = runtime_verification_result(
            true,
            true,
            true,
            false,
            false,
            Some("renderer request client was not found".to_string()),
        )
        .expect("config and paginated history are ready");

        assert_eq!(
            warning.outcome,
            CodexRuntimeRefreshOutcome::CompletedWithWarnings
        );
        assert_eq!(warning.config_status, CodexRuntimeCheckStatus::Ready);
        assert_eq!(
            warning.paginated_history_status,
            CodexRuntimeCheckStatus::Ready
        );
        assert_eq!(
            warning.renderer_compatibility_status,
            CodexRuntimeCheckStatus::Warning
        );
        assert_eq!(
            warning.renderer_compatibility_message.as_deref(),
            Some("renderer request client was not found")
        );

        assert!(runtime_verification_result(true, false, true, true, true, None).is_err());
        assert!(runtime_verification_result(false, true, true, true, true, None).is_err());
    }

    fn renderer_result(
        injected: bool,
        all_provider_history_patched: bool,
        history_refresh_requested: bool,
        message: String,
    ) -> crate::codex_desktop::CodexModelPickerUnlockResult {
        crate::codex_desktop::CodexModelPickerUnlockResult {
            attempted_ports: vec![9229],
            debug_port: Some(9229),
            target_id: Some("codex-main".to_string()),
            target_title: Some("Codex".to_string()),
            target_url: Some("app://-/index.html".to_string()),
            model_count: 1,
            model_names: vec!["gpt-5.6-sol".to_string()],
            injected,
            launched: false,
            codex_executable: None,
            history_sync_requested: true,
            history_catalog_complete: Some(true),
            history_catalog_count: Some(1308),
            all_provider_history_patched,
            history_refresh_requested,
            message,
        }
    }

    struct MockRendererProbe {
        responses: VecDeque<crate::codex_desktop::CodexModelPickerUnlockResult>,
        fallback: crate::codex_desktop::CodexModelPickerUnlockResult,
    }

    impl RendererCompatibilityProbe for MockRendererProbe {
        async fn probe(
            &mut self,
        ) -> Result<crate::codex_desktop::CodexModelPickerUnlockResult, String> {
            Ok(self
                .responses
                .pop_front()
                .unwrap_or_else(|| self.fallback.clone()))
        }
    }

    #[tokio::test]
    async fn renderer_patch_poll_waits_for_convergence_instead_of_early_not_ready() {
        let verification = poll_renderer_compatibility(
            Duration::from_millis(1500),
            MockRendererProbe {
                responses: VecDeque::from([
                    renderer_result(true, false, false, "patch not installed yet".to_string()),
                    renderer_result(true, false, false, "patch still converging".to_string()),
                    renderer_result(true, true, true, "all-provider history patched".to_string()),
                ]),
                fallback: renderer_result(
                    true,
                    true,
                    true,
                    "all-provider history patched".to_string(),
                ),
            },
        )
        .await
        .expect("the renderer patch should converge before the timeout");

        assert_eq!(verification.outcome, CodexRuntimeRefreshOutcome::Completed);
        assert_eq!(
            verification.renderer_compatibility_status,
            CodexRuntimeCheckStatus::Ready
        );
        assert!(verification.renderer_compatibility_message.is_none());
    }

    #[tokio::test]
    async fn renderer_patch_poll_degrades_to_warning_when_not_converged_by_timeout() {
        let verification = poll_renderer_compatibility(
            Duration::from_millis(130),
            MockRendererProbe {
                responses: VecDeque::new(),
                fallback: renderer_result(
                    true,
                    false,
                    false,
                    "renderer request client was not found".to_string(),
                ),
            },
        )
        .await
        .expect("a non-converged renderer returns a warning result");

        assert_eq!(
            verification.outcome,
            CodexRuntimeRefreshOutcome::CompletedWithWarnings
        );
        assert_eq!(
            verification.renderer_compatibility_status,
            CodexRuntimeCheckStatus::Warning
        );
        assert_eq!(
            verification.renderer_compatibility_message.as_deref(),
            Some("renderer request client was not found")
        );
    }

    #[derive(Default)]
    struct FakeRefreshOperations {
        targets: CodexRuntimeRefreshTargets,
        wait_results: VecDeque<Vec<RawCodexRuntimeProcess>>,
        log: Vec<&'static str>,
        history_repair_error: Option<String>,
        repaired_history_rollout_count: usize,
        repaired_history_duplicate_count: usize,
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

        async fn repair_paginated_history(
            &mut self,
        ) -> Result<paginated_history::PaginatedHistoryRepairOutcome, String> {
            self.log.push("repair_history");
            match self.history_repair_error.take() {
                Some(error) => Err(error),
                None => Ok(paginated_history::PaginatedHistoryRepairOutcome {
                    repaired_rollout_count: self.repaired_history_rollout_count,
                    repaired_duplicate_count: self.repaired_history_duplicate_count,
                    ..Default::default()
                }),
            }
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

        async fn verify_fresh_runtime(
            &mut self,
            _config_written_at_ms: i64,
        ) -> Result<CodexRuntimeVerification, String> {
            self.log.push("verify");
            runtime_verification_result(true, true, true, true, true, None)
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
            repaired_history_rollout_count: 1,
            repaired_history_duplicate_count: 3,
            ..Default::default()
        };
        let mut stages = Vec::new();
        let mut logs = Vec::new();

        let result = execute_refresh_transaction(&mut operations, &token, |progress| {
            if progress.kind == CodexRuntimeRefreshProgressKind::Stage {
                stages.push(progress.stage);
            }
            if progress.kind == CodexRuntimeRefreshProgressKind::Log {
                logs.push(progress.code.unwrap_or_default());
            }
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
                "repair_history",
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
                CodexRuntimeRefreshStage::RepairingHistory,
                CodexRuntimeRefreshStage::ApplyingConfig,
                CodexRuntimeRefreshStage::Launching,
                CodexRuntimeRefreshStage::Verifying,
                CodexRuntimeRefreshStage::Completed,
            ]
        );
        assert_eq!(result.repaired_history_rollout_count, 1);
        assert_eq!(result.repaired_history_duplicate_count, 3);
        assert!(logs.iter().any(|code| code == "refresh_started"));
        assert!(logs.iter().any(|code| code == "history_repair_finished"));
        assert!(logs.iter().any(|code| code == "verification_finished"));
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
                "repair_history",
                "apply_config",
                "launch",
            ]
        );
    }

    #[tokio::test]
    async fn history_repair_failure_reopens_codex_before_returning_the_error() {
        let targets = one_shell_target();
        let token = refresh_target_fingerprint(&targets);
        let mut operations = FakeRefreshOperations {
            targets,
            wait_results: VecDeque::from([Vec::new()]),
            history_repair_error: Some("history repair refused unsafe gap".to_string()),
            ..Default::default()
        };

        let error = execute_refresh_transaction(&mut operations, &token, |_| {})
            .await
            .expect_err("history repair failure should be reported");

        assert!(error.contains("history repair refused unsafe gap"));
        assert_eq!(
            operations.log,
            vec![
                "inspect",
                "graceful_close",
                "wait_for_exit",
                "repair_history",
                "launch",
            ]
        );
    }
}
