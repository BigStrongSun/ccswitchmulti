use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};

use crate::codex_desktop::{
    candidate_debug_ports, detect_running_codex_main_process, list_cdp_targets,
    load_cc_switch_model_catalog_projection, pick_codex_page_targets,
    try_inject_on_candidate_ports, DEFAULT_CODEX_DEBUG_PORT,
};

const GUARDIAN_CHECK_INTERVAL: Duration = Duration::from_secs(3);

/// 守护上报给前端的即时状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexGuardianStatus {
    pub active: bool,
    pub codex_running: bool,
    pub cdp_available: bool,
    pub injected_target_count: usize,
    pub injected: bool,
    pub last_event: String,
    pub message: String,
}

/// 守护内部持有的可变更状态。
struct GuardianInner {
    injected_target_ids: Vec<String>,
    inject_generation: u64,
}

/// 守护的外部句柄；丢弃后守护自动停止。
pub(crate) struct GuardianHandle {
    shutdown_tx: watch::Sender<bool>,
    pub(crate) status: Arc<Mutex<CodexGuardianStatus>>,
}

/// 启动 Codex 桌面模型菜单生命周期守护。
///
/// 守护不会静默终止正在工作的 Codex。
/// 仅在检测到新 renderer target 时幂等重新注入；
/// 若 CDP 不可用则记录原因并等待。
pub(crate) fn start_codex_guardian() -> GuardianHandle {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let status = Arc::new(Mutex::new(CodexGuardianStatus {
        active: true,
        codex_running: false,
        cdp_available: false,
        injected_target_count: 0,
        injected: false,
        last_event: "守护已启动".into(),
        message: "守护已启动，等待 Codex Desktop...".into(),
    }));

    let inner = Arc::new(Mutex::new(GuardianInner {
        injected_target_ids: Vec::new(),
        inject_generation: 0,
    }));

    let s = status.clone();
    let i = inner.clone();
    tokio::spawn(async move {
        guardian_loop(s, i, shutdown_rx).await;
    });

    GuardianHandle {
        shutdown_tx,
        status,
    }
}

impl GuardianHandle {
    pub(crate) fn stop(self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn guardian_loop(
    status: Arc<Mutex<CodexGuardianStatus>>,
    inner: Arc<Mutex<GuardianInner>>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            let mut s = status.lock().await;
            s.active = false;
            s.last_event = "守护已停止".into();
            s.message = "守护已停止".into();
            return;
        }

        run_guardian_cycle(&status, &inner).await;

        tokio::select! {
            _ = tokio::time::sleep(GUARDIAN_CHECK_INTERVAL) => {},
            _ = shutdown.changed() => {
                let mut s = status.lock().await;
                s.active = false;
                s.last_event = "守护已停止".into();
                s.message = "守护已停止".into();
                return;
            }
        }
    }
}

async fn run_guardian_cycle(
    status: &Arc<Mutex<CodexGuardianStatus>>,
    inner: &Arc<Mutex<GuardianInner>>,
) {
    // 1. 检查 Codex 主进程是否在运行。
    let running = detect_running_codex_main_process();
    if running.is_none() {
        let mut s = status.lock().await;
        s.codex_running = false;
        s.cdp_available = false;
        s.injected = false;
        s.injected_target_count = 0;
        s.last_event = "Codex 未运行".into();
        s.message = "Codex Desktop 未运行，等待用户启动...".into();
        // 进程消失时清理已知 target，下次启动不会跳过新 target。
        inner.lock().await.injected_target_ids.clear();
        return;
    }

    // 2. 查询 CDP target，收集当前存在的 Codex 页面 target ID。
    let ports = candidate_debug_ports(DEFAULT_CODEX_DEBUG_PORT);
    let mut current_ids: Vec<String> = Vec::new();
    let mut cdp_available = false;

    for port in &ports {
        match list_cdp_targets(*port).await {
            Ok(targets) => match pick_codex_page_targets(&targets, *port) {
                Ok(pages) => {
                    cdp_available = true;
                    for t in &pages {
                        if !current_ids.contains(&t.id) {
                            current_ids.push(t.id.clone());
                        }
                    }
                }
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }

    if !cdp_available {
        let mut s = status.lock().await;
        s.codex_running = true;
        s.cdp_available = false;
        s.injected = false;
        s.injected_target_count = 0;
        s.last_event = "Codex 运行中但无 CDP".into();
        s.message = "Codex Desktop 正在运行但未开放 CDP 调试端口；CCSM 不会静默终止 Codex，请退出后从 CCSM 启动或等待下次 CCSM 启动 Codex 时自动注入。".into();
        inner.lock().await.injected_target_ids.clear();
        return;
    }

    // 3. 比较当前 target ID 与已注入列表，找出新增的。
    let mut guard = inner.lock().await;
    let new_ids: Vec<String> = current_ids
        .iter()
        .filter(|id| !guard.injected_target_ids.contains(id))
        .cloned()
        .collect();

    if new_ids.is_empty() {
        // 没有新 target，无需注入。
        let s = &mut *status.lock().await;
        s.codex_running = true;
        s.cdp_available = true;
        s.injected = !guard.injected_target_ids.is_empty();
        s.injected_target_count = guard.injected_target_ids.len();
        s.last_event = "CDP target 无变化".into();
        if s.injected {
            s.message = format!(
                "已守护 {} 个 CDP renderer target；模型菜单注入有效。",
                guard.injected_target_ids.len()
            );
        } else {
            s.message = "CDP 可用但尚未注入（可能 target 启动中）".into();
        }
        // 清理已消失的 target
        guard
            .injected_target_ids
            .retain(|id| current_ids.contains(id));
        return;
    }

    // 4. 有新 target 出现，尝试注入。
    let gen = {
        guard.inject_generation += 1;
        guard.inject_generation
    };
    drop(guard);

    {
        let mut s = status.lock().await;
        s.last_event = format!("检测到新 CDP target，发起注入 (gen {gen})");
        s.message = format!(
            "检测到 {} 个新 CDP renderer target，正在注入...",
            new_ids.len()
        );
    }

    match load_cc_switch_model_catalog_projection() {
        Ok(catalog) => {
            match try_inject_on_candidate_ports(&catalog, &ports).await {
                Some(result) if result.injected => {
                    let mut guard = inner.lock().await;
                    // 记录注入成功的 target
                    for id in &current_ids {
                        if !guard.injected_target_ids.contains(id) {
                            guard.injected_target_ids.push(id.clone());
                        }
                    }
                    // 清理已消失的
                    guard
                        .injected_target_ids
                        .retain(|id| current_ids.contains(id));

                    let mut s = status.lock().await;
                    s.codex_running = true;
                    s.cdp_available = true;
                    s.injected = true;
                    s.injected_target_count = guard.injected_target_ids.len();
                    s.last_event = format!("注入成功 (gen {gen})");
                    s.message = format!(
                        "已将模型菜单兼容层注入 {} 个 renderer target (gen {gen})。",
                        guard.injected_target_ids.len()
                    );
                    log::info!(
                        "Codex 守护: 模型菜单已注入 target={:?}, models={}",
                        result.target_id,
                        result.model_count
                    );
                }
                Some(result) => {
                    let mut s = status.lock().await;
                    s.codex_running = true;
                    s.cdp_available = true;
                    s.last_event = format!("注入未完成 (gen {gen})");
                    s.message = format!("模型菜单注入尝试未完成: {}", result.message);
                    log::warn!("Codex 守护: 注入未完成: {}", result.message);
                }
                None => {
                    let mut s = status.lock().await;
                    s.codex_running = true;
                    s.cdp_available = true;
                    s.last_event = format!("注入失败 (gen {gen})");
                    s.message = "模型菜单注入失败：未找到可注入的 CDP target。".into();
                    log::warn!("Codex 守护: 注入失败，未找到可注入的 target");
                }
            }
        }
        Err(error) => {
            let mut s = status.lock().await;
            s.codex_running = true;
            s.cdp_available = true;
            s.last_event = format!("模型目录加载失败 (gen {gen})");
            s.message = format!("无法加载 CCSM 模型目录: {error}");
            log::warn!("Codex 守护: 模型目录加载失败: {error}");
        }
    }
}
