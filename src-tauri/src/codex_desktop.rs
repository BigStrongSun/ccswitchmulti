use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Child, Output};
use std::process::{Command, Stdio};
use std::time::Duration;
use std::time::Instant;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub(crate) const DEFAULT_CODEX_DEBUG_PORT: u16 = 9229;
pub(crate) const CDP_HTTP_TIMEOUT: Duration = Duration::from_secs(2);
const CDP_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const MODEL_PICKER_PATCH_KEY: &str = "__ccSwitchCodexAppCompatibilityV10";
type ModelPickerDocumentScripts = HashMap<String, Vec<(String, String)>>;
static MODEL_PICKER_DOCUMENT_SCRIPTS: std::sync::LazyLock<
    std::sync::Mutex<ModelPickerDocumentScripts>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static MODEL_PICKER_UNTRACKED_DOCUMENTS: std::sync::LazyLock<std::sync::Mutex<BTreeSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(BTreeSet::new()));
static MODEL_PICKER_TIMEZONES: std::sync::LazyLock<std::sync::Mutex<BTreeSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(BTreeSet::new()));
static RENDERER_EFFECTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RENDERER_GENERATION: std::sync::LazyLock<tokio::sync::watch::Sender<u64>> =
    std::sync::LazyLock::new(|| tokio::sync::watch::channel(0).0);

#[derive(Clone, Copy)]
pub(crate) struct CodexRendererWork(u64);

impl CodexRendererWork {
    pub(crate) fn new() -> Self {
        Self(*RENDERER_GENERATION.borrow())
    }

    pub(crate) fn check(self) -> Result<(), String> {
        if self.0 != *RENDERER_GENERATION.borrow() {
            Err("Codex renderer work cancelled by a provider transition".to_string())
        } else {
            Ok(())
        }
    }

    /// Only use this for discovery/connection futures with no issued effects.
    pub(crate) async fn discover<F: std::future::Future>(
        self,
        future: F,
    ) -> Result<F::Output, String> {
        let mut generation = RENDERER_GENERATION.subscribe();
        self.check()?;
        tokio::select! {
            biased;
            _ = generation.changed() => Err("Codex renderer discovery cancelled".to_string()),
            value = future => { self.check()?; Ok(value) }
        }
    }
}

pub(crate) fn cancel_codex_renderer_work() {
    RENDERER_GENERATION.send_modify(|generation| *generation = generation.wrapping_add(1));
}
#[cfg(test)]
pub(crate) static TEST_RENDERER_RELEASE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>>,
> = std::sync::Mutex::new(None);
#[cfg(test)]
type RendererUnlockTest = dyn Fn() -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<CodexModelPickerUnlockResult, String>> + Send>,
    > + Send
    + Sync;
#[cfg(test)]
pub(crate) static TEST_RENDERER_UNLOCK: std::sync::Mutex<
    Option<std::sync::Arc<RendererUnlockTest>>,
> = std::sync::Mutex::new(None);
const REMEMBERED_CODEX_DESKTOP_EXECUTABLE_FILENAME: &str = "codex-desktop-executable.json";
#[cfg(test)]
pub(crate) static TEST_CDP_PORTS: std::sync::Mutex<Option<Vec<u16>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_CODEX_RUNNING: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);
pub(crate) fn isolated_cdp_fixture_enabled() -> bool {
    #[cfg(test)]
    {
        return TEST_CDP_PORTS.lock().unwrap().is_some();
    }
    #[cfg(not(test))]
    {
        false
    }
}
#[cfg(any(target_os = "macos", test))]
const CODEX_DESKTOP_BUNDLE_IDENTIFIER: &str = "com.openai.codex";
/// Codex App 历史目录与模型兼容层安装命令的执行结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelPickerUnlockResult {
    pub attempted_ports: Vec<u16>,
    pub debug_port: Option<u16>,
    pub target_id: Option<String>,
    pub target_title: Option<String>,
    pub target_url: Option<String>,
    pub model_count: usize,
    pub model_names: Vec<String>,
    pub injected: bool,
    pub launched: bool,
    pub codex_executable: Option<String>,
    pub history_sync_requested: bool,
    pub history_catalog_complete: Option<bool>,
    pub history_catalog_count: Option<usize>,
    pub all_provider_history_patched: bool,
    pub history_refresh_requested: bool,
    pub message: String,
}

/// renderer 脚本返回的新版历史目录同步证据。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CodexAppCompatibilityEvidence {
    history_sync_requested: bool,
    history_catalog_complete: Option<bool>,
    history_catalog_count: Option<usize>,
    all_provider_history_patched: bool,
    history_refresh_requested: bool,
}

/// 注入脚本需要的模型目录投影，避免把整个 catalog 私有字段泄漏到 renderer。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexModelCatalogProjection {
    default_model: Option<String>,
    model_provider: Option<String>,
    model_names: Vec<String>,
    models: Vec<Value>,
}

impl CodexModelCatalogProjection {
    /// 构造不含路由模型的兼容投影，使历史目录修复不依赖 CCSM 模型目录是否已生成。
    fn empty() -> Self {
        Self {
            default_model: None,
            model_provider: None,
            model_names: Vec::new(),
            models: Vec::new(),
        }
    }

    /// 返回本次 renderer 注入实际使用的目录指纹。
    pub(crate) fn fingerprint(&self) -> String {
        let payload = serde_json::to_vec(self).unwrap_or_default();
        hex::encode(Sha256::digest(payload))
    }
}

/// Chrome DevTools Protocol `/json` 返回的页面 target 摘要。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct CdpTarget {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) target_type: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub(crate) web_socket_debugger_url: Option<String>,
}

/// 尝试为当前或新启动的 Codex App 安装 renderer 兼容层。
///
/// 副作用：
/// - 若发现已开放 CDP 的 Codex renderer，会触发新版历史目录同步并修复模型菜单；
/// - 若未发现 CDP 且 Codex 未运行，会以 remote debugging 参数启动 Codex。
pub async fn unlock_codex_model_picker() -> Result<CodexModelPickerUnlockResult, String> {
    unlock_codex_model_picker_for_work(CodexRendererWork::new()).await
}

pub(crate) async fn unlock_codex_model_picker_for_work(
    work: CodexRendererWork,
) -> Result<CodexModelPickerUnlockResult, String> {
    work.check()?;
    let intent = match read_codex_desktop_projection_intent() {
        Ok(intent) => intent,
        Err(reason) => return Ok(inactive_codex_model_picker_result(reason)),
    };
    #[cfg(test)]
    {
        let unlock = TEST_RENDERER_UNLOCK.lock().unwrap().clone();
        if let Some(unlock) = unlock {
            return work.discover(unlock()).await?;
        }
    }
    #[cfg(test)]
    if std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        return Err(
            "Codex Desktop process control is disabled for isolated test homes".to_string(),
        );
    }
    let catalog = load_codex_desktop_catalog_for_intent(&intent).unwrap_or_else(|_| {
        let mut projection = CodexModelCatalogProjection::empty();
        projection.model_provider = codex_desktop_config_string(&intent.config, "model_provider");
        projection
    });
    let attempted_ports = candidate_debug_ports(DEFAULT_CODEX_DEBUG_PORT);

    if let Some(result) =
        try_inject_on_candidate_ports_for_work(&catalog, &attempted_ports, work).await
    {
        return Ok(result);
    }
    work.check()?;
    if let Err(reason) = read_codex_desktop_projection_intent() {
        return Ok(inactive_codex_model_picker_result(reason));
    }

    let running_main = detect_running_codex_main_process();
    if let Some(running_main) = running_main {
        let remembered = remember_codex_desktop_executable(&running_main).ok();
        return Ok(CodexModelPickerUnlockResult {
            attempted_ports,
            debug_port: None,
            target_id: None,
            target_title: None,
            target_url: None,
            model_count: catalog.model_names.len(),
            model_names: catalog.model_names,
            injected: false,
            launched: false,
            codex_executable: Some(
                remembered
                    .as_ref()
                    .unwrap_or(&running_main)
                    .display()
                    .to_string(),
            ),
            history_sync_requested: false,
            history_catalog_complete: None,
            history_catalog_count: None,
            all_provider_history_patched: false,
            history_refresh_requested: false,
            message: "Codex App is already running without an injectable CDP port. Fully quit Codex, then launch it from CCSwitchMulti so the history catalog and model compatibility layer can be installed.".to_string(),
        });
    }

    let executable =
        resolve_codex_executable().ok_or_else(codex_desktop_executable_not_found_message)?;
    work.check()?;
    if let Err(reason) = read_codex_desktop_projection_intent() {
        return Ok(inactive_codex_model_picker_result(reason));
    }
    launch_codex_with_debug_port(&executable, DEFAULT_CODEX_DEBUG_PORT)?;

    let mut last_result = None;
    for _ in 0..30 {
        if let Some(result) =
            try_inject_on_candidate_ports_for_work(&catalog, &[DEFAULT_CODEX_DEBUG_PORT], work)
                .await
        {
            return Ok(CodexModelPickerUnlockResult {
                launched: true,
                codex_executable: Some(executable.display().to_string()),
                ..result
            });
        }
        last_result = Some(CodexModelPickerUnlockResult {
            attempted_ports: vec![DEFAULT_CODEX_DEBUG_PORT],
            debug_port: Some(DEFAULT_CODEX_DEBUG_PORT),
            target_id: None,
            target_title: None,
            target_url: None,
            model_count: catalog.model_names.len(),
            model_names: catalog.model_names.clone(),
            injected: false,
            launched: true,
            codex_executable: Some(executable.display().to_string()),
            history_sync_requested: false,
            history_catalog_complete: None,
            history_catalog_count: None,
            all_provider_history_patched: false,
            history_refresh_requested: false,
            message: "Codex was launched with remote debugging; waiting for the renderer target."
                .to_string(),
        });
        work.discover(tokio::time::sleep(Duration::from_millis(500)))
            .await?;
    }

    Ok(last_result.unwrap_or(CodexModelPickerUnlockResult {
        attempted_ports: vec![DEFAULT_CODEX_DEBUG_PORT],
        debug_port: Some(DEFAULT_CODEX_DEBUG_PORT),
        target_id: None,
        target_title: None,
        target_url: None,
        model_count: catalog.model_names.len(),
        model_names: catalog.model_names,
        injected: false,
        launched: true,
        codex_executable: Some(executable.display().to_string()),
        history_sync_requested: false,
        history_catalog_complete: None,
        history_catalog_count: None,
        all_provider_history_patched: false,
        history_refresh_requested: false,
        message: "Codex was launched, but no injectable renderer target appeared before timeout."
            .to_string(),
    }))
}

pub(crate) fn inactive_codex_model_picker_result(message: String) -> CodexModelPickerUnlockResult {
    CodexModelPickerUnlockResult {
        attempted_ports: Vec::new(),
        debug_port: None,
        target_id: None,
        target_title: None,
        target_url: None,
        model_count: 0,
        model_names: Vec::new(),
        injected: false,
        launched: false,
        codex_executable: None,
        history_sync_requested: false,
        history_catalog_complete: None,
        history_catalog_count: None,
        all_provider_history_patched: false,
        history_refresh_requested: false,
        message,
    }
}

/// 在用户显式开启的“随 CCSwitchMulti 启动 Codex Desktop”设置下启动 Codex。
///
/// 这个入口不属于系统自启注册流程；调用方必须先读取独立设置。已有 Codex
/// 主进程时保持幂等，不重复拉起或干扰现有会话。
pub(crate) fn launch_codex_desktop_with_ccswitch(enabled: bool) -> Result<bool, String> {
    let codex_running = is_codex_desktop_running();
    if !should_launch_codex_desktop_with_ccswitch(enabled, codex_running) {
        return Ok(false);
    }

    let executable =
        resolve_codex_executable().ok_or_else(codex_desktop_executable_not_found_message)?;
    launch_codex_with_debug_port(&executable, DEFAULT_CODEX_DEBUG_PORT)?;
    Ok(true)
}

/// Start native Codex without adding CCSM renderer or timezone arguments.
pub(crate) fn launch_native_codex_desktop_with_ccswitch(enabled: bool) -> Result<bool, String> {
    if !should_launch_codex_desktop_with_ccswitch(enabled, is_codex_desktop_running()) {
        return Ok(false);
    }
    let executable =
        resolve_codex_executable().ok_or_else(codex_desktop_executable_not_found_message)?;
    native_codex_launch_command(&executable)
        .spawn()
        .map_err(|error| format!("failed to launch {}: {error}", executable.display()))?;
    Ok(true)
}

pub(crate) fn native_codex_launch_command(executable: &Path) -> Command {
    #[cfg(target_os = "macos")]
    if let Some(bundle) = macos_codex_bundle_for_executable(executable) {
        let mut command = Command::new("open");
        command.arg(bundle);
        return command;
    }
    let mut command = Command::new(executable);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

pub(crate) fn is_codex_desktop_running() -> bool {
    detect_running_codex_main_process().is_some()
}

fn should_launch_codex_desktop_with_ccswitch(enabled: bool, codex_running: bool) -> bool {
    enabled && !codex_running
}

/// 返回当前平台的 Desktop 可执行文件发现失败说明，避免 Windows-only 文案误导 macOS/Linux 用户。
fn codex_desktop_executable_not_found_message() -> String {
    let platform_sources = if cfg!(target_os = "windows") {
        "running Desktop process, remembered Desktop path, MSIX/Appx package metadata and manifest, App Paths registry entries, PATH commands, and common local install folders"
    } else if cfg!(target_os = "macos") {
        "running Desktop process, remembered Desktop path, /Applications, ~/Applications, and Spotlight-discovered com.openai.codex bundles (Codex.app or ChatGPT.app)"
    } else if cfg!(target_os = "linux") {
        "running Desktop process, remembered Desktop path, PATH entries for Codex/Codex.AppImage, .desktop entries with absolute Exec paths, and common AppImage or /opt install folders"
    } else {
        "running Desktop process, remembered Desktop path, and platform-specific install folders"
    };
    format!(
        "Codex Desktop executable was not found. CCSwitchMulti checked {platform_sources}. Install or start the Codex Desktop app once. The Desktop menu unlock flow will not launch CLI/app-server codex as an Electron shell; Codex CLI/app-server is still supported through live config.toml, model_catalog_json, the local /v1/models endpoint, and MultiRouter request routing."
    )
}

/// 从 cc-switch 生成的 catalog 中读取模型名和 renderer 需要的最小描述。
pub(crate) fn load_cc_switch_model_catalog_projection(
) -> Result<CodexModelCatalogProjection, String> {
    let intent = read_codex_desktop_projection_intent()?;
    load_codex_desktop_catalog_for_intent(&intent)
}

struct CodexDesktopProjectionIntent {
    config: toml::Value,
    catalog_path: PathBuf,
}

/// Only current live configuration can authorize reading the routed catalog.
/// Residual files and inactive provider tables cannot opt native Codex back in.
fn read_codex_desktop_projection_intent() -> Result<CodexDesktopProjectionIntent, String> {
    let text = crate::codex_config::read_codex_config_text().map_err(|error| error.to_string())?;
    let config = text
        .parse::<toml::Value>()
        .map_err(|error| format!("Cannot read Codex Desktop live configuration: {error}"))?;
    let owned_catalog = crate::codex_config::resolve_cc_switch_catalog_path(
        &text,
        &crate::codex_config::get_codex_config_dir(),
    );
    let active_custom_provider = codex_desktop_config_string(&config, "model_provider")
        .filter(|provider| provider != "openai")
        .is_some_and(|provider| {
            config
                .get("model_providers")
                .and_then(|providers| providers.get(provider.as_str()))
                .is_some_and(toml::Value::is_table)
        });
    if owned_catalog.is_none() && !active_custom_provider {
        return Err("native_codex_config".to_string());
    }
    Ok(CodexDesktopProjectionIntent {
        config,
        catalog_path: owned_catalog
            .unwrap_or_else(crate::codex_config::get_codex_model_catalog_path),
    })
}

fn load_codex_desktop_catalog_for_intent(
    intent: &CodexDesktopProjectionIntent,
) -> Result<CodexModelCatalogProjection, String> {
    let catalog_path = &intent.catalog_path;
    let default_model = codex_desktop_config_string(&intent.config, "model");
    let model_provider = codex_desktop_config_string(&intent.config, "model_provider");
    let mut candidates = Vec::new();

    if let Ok(catalog) = crate::config::read_json_file::<Value>(catalog_path) {
        candidates.push(catalog);
    }

    let cache_path = crate::codex_config::get_codex_config_dir().join("models_cache.json");
    if let Ok(cache) = crate::config::read_json_file::<Value>(&cache_path) {
        if cache.get("etag").and_then(Value::as_str) == Some("cc-switch-model-catalog") {
            candidates.push(cache);
        }
    }

    if let Some(inline_catalog) = read_active_codex_provider_inline_model_catalog(&intent.config) {
        candidates.push(inline_catalog);
    }

    for candidate in &candidates {
        if let Some(projection) = codex_model_catalog_projection_from_value(
            candidate,
            default_model.clone(),
            model_provider.clone(),
        ) {
            return Ok(projection);
        }
    }

    Err(format!(
        "No routed models were found in {}, the CCSwitchMulti models cache, or the active provider inline catalog",
        catalog_path.display()
    ))
}

/// 从单个目录值构造 renderer 投影；空目录返回 `None` 以便调用方继续尝试回退源。
fn codex_model_catalog_projection_from_value(
    catalog: &Value,
    default_model: Option<String>,
    model_provider: Option<String>,
) -> Option<CodexModelCatalogProjection> {
    let (model_names, models) = codex_model_entries_from_catalog_value(catalog);
    if model_names.is_empty() {
        return None;
    }
    Some(CodexModelCatalogProjection {
        default_model: default_model.or_else(|| model_names.first().cloned()),
        model_provider,
        model_names,
        models,
    })
}

/// 从当前活动 provider 的 TOML 内联 `models` 读取最后一道模型菜单回退。
///
/// 生成 live config 时，CCSM 会把同一份路由目录同时投射到 JSON catalog、models cache
/// 和活动 provider 内联数组。任一 JSON 文件短暂不可读时，内联数组仍能避免整次 Desktop
/// 会话安装空白名单；这里仅读取活动 provider，不能把未启用 provider 的模型重新注入。
fn read_active_codex_provider_inline_model_catalog(config: &toml::Value) -> Option<Value> {
    let provider_id = codex_desktop_config_string(config, "model_provider")?;
    let models = config
        .get("model_providers")
        .and_then(|providers| providers.get(provider_id.as_str()))
        .and_then(|provider| provider.get("models"))
        .and_then(toml::Value::as_array)?
        .clone();
    serde_json::to_value(models)
        .ok()
        .map(|models| json!({ "models": models }))
}

/// 提取 catalog 内所有 Codex Desktop 可识别的模型条目。
fn codex_model_entries_from_catalog_value(catalog: &Value) -> (Vec<String>, Vec<Value>) {
    let Some(entries) = catalog
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| catalog.as_array())
    else {
        return (Vec::new(), Vec::new());
    };

    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    let mut projected = Vec::new();

    for entry in entries {
        let Some(model_name) = codex_model_name(entry) else {
            continue;
        };
        if !seen.insert(model_name.clone()) {
            continue;
        }
        names.push(model_name.clone());
        projected.push(project_codex_model_descriptor(entry, &model_name));
    }

    (names, projected)
}

/// 从单个 catalog 条目中提取稳定模型名，兼容官方/旧版字段别名。
fn codex_model_name(entry: &Value) -> Option<String> {
    ["model", "slug", "id", "name"].into_iter().find_map(|key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

/// 将 catalog 条目补齐成 renderer 可直接消费的模型描述。
fn project_codex_model_descriptor(entry: &Value, model_name: &str) -> Value {
    let mut object = entry.as_object().cloned().unwrap_or_default();
    for key in ["model", "slug", "id", "name"] {
        object.insert(key.to_string(), Value::String(model_name.to_string()));
    }
    if !object.contains_key("displayName") {
        let display = object
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(model_name);
        object.insert(
            "displayName".to_string(),
            Value::String(display.to_string()),
        );
    }
    if !object.contains_key("display_name") {
        let display = object
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(model_name);
        object.insert(
            "display_name".to_string(),
            Value::String(display.to_string()),
        );
    }
    object.insert("hidden".to_string(), Value::Bool(false));
    Value::Object(object)
}

fn codex_desktop_config_string(config: &toml::Value, key: &str) -> Option<String> {
    config
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 在候选 CDP 端口中寻找 Codex renderer 并安装模型白名单补丁。
pub(crate) async fn try_inject_on_candidate_ports_for_work(
    catalog: &CodexModelCatalogProjection,
    ports: &[u16],
    work: CodexRendererWork,
) -> Option<CodexModelPickerUnlockResult> {
    work.check().ok()?;
    read_codex_desktop_projection_intent().ok()?;
    let launch_timezone =
        crate::codex_egress_timezone::resolve_launch_timezone(&crate::settings::get_settings());
    let discovered = work
        .discover(discover_codex_page_targets(ports))
        .await
        .ok()?;
    for port in ports {
        let targets: Vec<_> = discovered
            .iter()
            .filter(|(candidate, _)| candidate == port)
            .map(|(_, target)| target)
            .collect();
        let script = build_model_picker_unlock_script(catalog);
        let mut injected_target = None;
        let mut compatibility_evidence = CodexAppCompatibilityEvidence::default();
        for target in targets {
            read_codex_desktop_projection_intent().ok()?;
            let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
                continue;
            };
            if let Ok(evidence) =
                install_script_for_work(websocket_url, &script, launch_timezone.as_deref(), work)
                    .await
            {
                if evidence.history_sync_requested
                    || evidence.history_catalog_complete.is_some()
                    || evidence.all_provider_history_patched
                {
                    compatibility_evidence = evidence;
                }
                injected_target.get_or_insert(target);
            }
        }
        let Some(target) = injected_target else {
            continue;
        };
        return Some(CodexModelPickerUnlockResult {
            attempted_ports: ports.to_vec(),
            debug_port: Some(*port),
            target_id: Some(target.id.clone()),
            target_title: Some(target.title.clone()),
            target_url: Some(target.url.clone()),
            model_count: catalog.model_names.len(),
            model_names: catalog.model_names.clone(),
            injected: true,
            launched: false,
            codex_executable: detect_running_codex_main_process()
                .map(|path| path.display().to_string()),
            history_sync_requested: compatibility_evidence.history_sync_requested,
            history_catalog_complete: compatibility_evidence.history_catalog_complete,
            history_catalog_count: compatibility_evidence.history_catalog_count,
            all_provider_history_patched: compatibility_evidence.all_provider_history_patched,
            history_refresh_requested: compatibility_evidence.history_refresh_requested,
            message: if compatibility_evidence.all_provider_history_patched {
                "Codex App compatibility layer was injected; all-provider history queries were restored and the native history catalog was refreshed without rewriting history data.".to_string()
            } else if compatibility_evidence.history_sync_requested {
                "Codex App compatibility layer was injected; the native history catalog sync was requested and model visibility was refreshed.".to_string()
            } else {
                "Codex App compatibility layer was injected, but the all-provider history query patch was not installed in this renderer; model visibility was refreshed without rewriting history metadata.".to_string()
            },
        });
    }
    None
}

/// 生成去重后的 CDP 端口探测列表。
pub(crate) fn candidate_debug_ports(preferred: u16) -> Vec<u16> {
    #[cfg(test)]
    if let Some(ports) = TEST_CDP_PORTS.lock().unwrap().clone() {
        return ports;
    }
    let mut ports = vec![preferred, DEFAULT_CODEX_DEBUG_PORT, 9222, 9223, 9230, 9231];
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// 查询 CDP 页面 target。
pub(crate) async fn list_cdp_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(CDP_HTTP_TIMEOUT)
        .build()
        .map_err(|error| format!("failed to build CDP client: {error}"))?;
    let urls = [
        format!("http://127.0.0.1:{debug_port}/json"),
        format!("http://[::1]:{debug_port}/json"),
    ];
    let mut errors = Vec::new();
    let mut requests = futures::stream::FuturesUnordered::new();
    for url in urls {
        let client = client.clone();
        requests.push(async move {
            let response = client.get(&url).send().await;
            (url, response)
        });
    }
    while let Some((url, response)) = requests.next().await {
        match response {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.json::<Vec<CdpTarget>>().await {
                    Ok(targets) => return Ok(targets),
                    Err(error) => errors.push(format!("{url}: invalid target JSON: {error}")),
                },
                Err(error) => errors.push(format!("{url}: {error}")),
            },
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(errors.join("; "))
}

/// Discover independent debug endpoints concurrently, keeping the HTTP timeout
/// as the total discovery budget instead of multiplying it by port count.
pub(crate) async fn discover_codex_page_targets(ports: &[u16]) -> Vec<(u16, CdpTarget)> {
    futures::future::join_all(ports.iter().map(|port| async move {
        let targets = list_cdp_targets(*port).await.ok()?;
        let targets = pick_codex_page_targets(&targets, *port).ok()?;
        Some(
            targets
                .into_iter()
                .map(|target| (*port, target))
                .collect::<Vec<_>>(),
        )
    }))
    .await
    .into_iter()
    .flatten()
    .flatten()
    .collect()
}

/// 选择最可能属于 Codex Desktop 的 renderer 页面。
///
/// 共享调试端口如 9222 可能属于 Chrome 或其它 Electron 应用，必须看到 Codex
/// 标识才注入；CCSwitchMulti 自己启动的默认 9229 允许在标题还没初始化时退回
/// 到第一个 page target。
pub(crate) fn pick_codex_page_targets(
    targets: &[CdpTarget],
    debug_port: u16,
) -> Result<Vec<CdpTarget>, String> {
    let pages = targets.iter().filter(|target| {
        target.target_type == "page"
            && target
                .web_socket_debugger_url
                .as_deref()
                .is_some_and(|url| !url.is_empty())
    });
    let mut all_pages = Vec::new();
    let mut codex_pages = Vec::new();
    for target in pages {
        all_pages.push(target.clone());
        if target_matches_codex_desktop(target) {
            codex_pages.push(target.clone());
        }
    }
    if !codex_pages.is_empty() {
        return Ok(codex_pages);
    }
    if debug_port == DEFAULT_CODEX_DEBUG_PORT && !all_pages.is_empty() {
        return Ok(all_pages);
    }
    Err("No Codex Desktop page target was found on shared CDP port".to_string())
}

/// 判断 CDP target 的标题或 URL 是否明确指向 Codex Desktop。
fn target_matches_codex_desktop(target: &CdpTarget) -> bool {
    let haystack = format!("{} {}", target.title, target.url).to_ascii_lowercase();
    haystack.contains("codex") || haystack.contains("app://")
}

/// 使用 CDP 同时安装新文档脚本并立即 patch 当前页面。
#[cfg(test)]
pub(crate) async fn install_script(
    websocket_url: &str,
    script: &str,
    timezone: Option<&str>,
) -> Result<CodexAppCompatibilityEvidence, String> {
    install_script_for_work(websocket_url, script, timezone, CodexRendererWork::new()).await
}

async fn install_script_for_work(
    websocket_url: &str,
    script: &str,
    timezone: Option<&str>,
    work: CodexRendererWork,
) -> Result<CodexAppCompatibilityEvidence, String> {
    let _effects = RENDERER_EFFECTS.lock().await;
    work.check()?;
    read_codex_desktop_projection_intent()?;
    let (socket, _) = work
        .discover(tokio::time::timeout(
            CDP_CONNECT_TIMEOUT,
            connect_async(websocket_url),
        ))
        .await?
        .map_err(|_| "timed out connecting CDP websocket".to_string())?
        .map_err(|error| format!("failed to connect CDP websocket: {error}"))?;
    let mut session = CdpSession::new(socket);
    session.send_command(1, "Runtime.enable", json!({})).await?;
    work.check()?;
    read_codex_desktop_projection_intent()?;
    let authorized = session
        .send_command(
            6,
            "Runtime.evaluate",
            json!({
                "expression": renderer_authorize_core_script(), "returnByValue": true,
            }),
        )
        .await?;
    let activation_epoch = authorized
        .pointer("/result/result/value")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Codex renderer did not authorize an activation epoch".to_string())?;
    work.check()?;
    if let Some(timezone) = timezone {
        MODEL_PICKER_TIMEZONES
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(websocket_url.to_string());
        // `TZ` is inherited by a directly launched Desktop/app-server process,
        // while Chromium has its own ICU timezone controller. Apply the same
        // user-selected IANA identifier to the renderer through the documented
        // CDP command as well. This remains best-effort so the experimental
        // override can never block the model/history compatibility layer.
        if let Err(error) = session
            .send_command(
                2,
                "Emulation.setTimezoneOverride",
                codex_timezone_override_parameters(timezone),
            )
            .await
        {
            log::warn!("Codex renderer timezone override was not applied: {error}");
        }
    }
    work.check()?;
    session.send_command(3, "Page.enable", json!({})).await?;
    work.check()?;
    read_codex_desktop_projection_intent()?;
    // Replace acknowledged registrations instead of accumulating old catalogs
    // that would race the latest generation after a normal document navigation.
    let previous = MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(websocket_url)
        .cloned()
        .unwrap_or_default();
    for (index, (nonce, identifier)) in previous.iter().enumerate() {
        session
            .send_command(
                10 + index as u64,
                "Page.removeScriptToEvaluateOnNewDocument",
                json!({"identifier":identifier}),
            )
            .await?;
        if let Some(known) = MODEL_PICKER_DOCUMENT_SCRIPTS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(websocket_url)
        {
            known.retain(|(known_nonce, _)| known_nonce != nonce);
        }
        work.check()?;
    }
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let nonce_json = serde_json::to_string(&nonce).map_err(|error| error.to_string())?;
    let timezone_owned = timezone.is_some();
    let document_registration = format!("(window.__ccSwitchCodexDocumentScripts ||= new Map()).set({nonce_json}, null); window.__ccSwitchCodexTimezoneOwned = {timezone_owned};");
    let document_source =
        build_epoch_fenced_document_script(script, activation_epoch, &document_registration);
    MODEL_PICKER_UNTRACKED_DOCUMENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(websocket_url.to_string());
    let registered = session
        .send_command(
            4,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": document_source }),
        )
        .await?;
    let identifier = registered
        .pointer("/result/identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| "Codex renderer returned no document script identifier".to_string())?
        .to_string();
    MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .entry(websocket_url.to_string())
        .or_default()
        .push((nonce.clone(), identifier.clone()));
    MODEL_PICKER_UNTRACKED_DOCUMENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(websocket_url);
    work.check()?;
    read_codex_desktop_projection_intent()?;
    let identifier_json = serde_json::to_string(&identifier).map_err(|error| error.to_string())?;
    let registration = format!("(window.__ccSwitchCodexDocumentScripts ||= new Map()).set({nonce_json}, {identifier_json}); window.__ccSwitchCodexTimezoneOwned = {timezone_owned};");
    let expression = build_epoch_fenced_document_script(script, activation_epoch, &registration);
    // 只有 Runtime.evaluate 会返回当前 renderer 的执行结果；新文档脚本注册命令只返回
    // identifier，不能用来判断 localThreadCatalog 是否真正触发。
    let evaluation = session
        .send_command(
            5,
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true,
                "allowUnsafeEvalBlockedByCSP": true
            }),
        )
        .await?;
    codex_app_compatibility_evidence_from_evaluation(&evaluation)
}

fn codex_timezone_override_parameters(timezone: &str) -> Value {
    json!({ "timezoneId": timezone })
}

fn renderer_authorize_core_script() -> &'static str {
    r#"(() => {
      const current = Number.isSafeInteger(window.__ccSwitchCodexOwnershipEpoch) ? window.__ccSwitchCodexOwnershipEpoch : 0;
      window.__ccSwitchCodexOwnershipEpoch = current + 1;
      return window.__ccSwitchCodexOwnershipEpoch;
    })()"#
}

fn renderer_activation_guard_core_script() -> &'static str {
    r#"
  const activationEpoch = typeof ccSwitchActivationEpoch === "number" ? ccSwitchActivationEpoch : null;
  const rendererActivationCurrent = () => activationEpoch === null || window.__ccSwitchCodexOwnershipEpoch === activationEpoch;
  if (!rendererActivationCurrent()) return { cancelled: true };
"#
}

fn renderer_release_fence_core_script() -> &'static str {
    r#"const currentEpoch = Number.isSafeInteger(window.__ccSwitchCodexOwnershipEpoch) ? window.__ccSwitchCodexOwnershipEpoch : 0;
    window.__ccSwitchCodexOwnershipEpoch = currentEpoch + 1;"#
}

fn build_epoch_fenced_document_script(script: &str, epoch: u64, registration: &str) -> String {
    format!(
        r#"(async () => {{
      const ccSwitchActivationEpoch = {epoch};
      const currentEpoch = window.__ccSwitchCodexOwnershipEpoch;
      if (currentEpoch !== undefined && currentEpoch > ccSwitchActivationEpoch) return {{ cancelled: true }};
      // A registered script may run in a fresh document. Unknown registrations
      // that cannot be removed are reported pending and require a restart.
      if (currentEpoch === undefined || currentEpoch < ccSwitchActivationEpoch) window.__ccSwitchCodexOwnershipEpoch = ccSwitchActivationEpoch;
      {registration}
      return await {script};
    }})()"#
    )
}

fn build_native_model_picker_release_script() -> String {
    let fence = renderer_release_fence_core_script();
    let release_script = build_model_picker_release_script();
    format!(
        r#"(async () => {{
      {fence}
      return await {release_script};
    }})()"#
    )
}

/// 从 CDP `Runtime.evaluate` 结果中提取原生历史同步状态，并把脚本异常转为明确错误。
fn codex_app_compatibility_evidence_from_evaluation(
    evaluation: &Value,
) -> Result<CodexAppCompatibilityEvidence, String> {
    if let Some(exception) = evaluation
        .get("result")
        .and_then(|result| result.get("exceptionDetails"))
    {
        return Err(format!(
            "Codex App compatibility script failed: {exception}"
        ));
    }
    let value = evaluation
        .pointer("/result/result/value")
        .ok_or_else(|| "Codex App compatibility script returned no value".to_string())?;
    let history_sync = value.get("historySync").unwrap_or(&Value::Null);
    let history_query_patch = value.get("historyQueryPatch").unwrap_or(&Value::Null);
    let history_hydration_failed = history_query_patch
        .pointer("/lineageHydration/failed")
        .and_then(Value::as_array)
        .is_some_and(|failures| !failures.is_empty());
    Ok(CodexAppCompatibilityEvidence {
        history_sync_requested: history_sync
            .get("requested")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        history_catalog_complete: history_sync.get("complete").and_then(Value::as_bool),
        history_catalog_count: history_sync
            .get("afterCount")
            .and_then(Value::as_u64)
            .and_then(|count| usize::try_from(count).ok()),
        all_provider_history_patched: history_query_patch
            .get("patched")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        history_refresh_requested: history_query_patch
            .get("refreshRequested")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !history_hydration_failed,
    })
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRuntimeTimezone {
    pub(crate) timezone: String,
    pub(crate) utc_offset: String,
}

fn codex_runtime_timezone_from_evaluation(
    evaluation: &Value,
) -> Result<CodexRuntimeTimezone, String> {
    if let Some(exception) = evaluation
        .get("result")
        .and_then(|result| result.get("exceptionDetails"))
    {
        return Err(format!(
            "Codex runtime timezone inspection failed: {exception}"
        ));
    }
    let value = evaluation
        .pointer("/result/result/value")
        .ok_or_else(|| "Codex runtime timezone inspection returned no value".to_string())?;
    let timezone = value
        .get("timezone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|timezone| !timezone.is_empty())
        .ok_or_else(|| "Codex renderer did not report an IANA timezone".to_string())?;
    let minutes = value
        .get("utcOffsetMinutes")
        .and_then(Value::as_i64)
        .ok_or_else(|| "Codex renderer did not report a UTC offset".to_string())?;
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.unsigned_abs();
    Ok(CodexRuntimeTimezone {
        timezone: timezone.to_string(),
        utc_offset: format!("{sign}{:02}:{:02}", absolute / 60, absolute % 60),
    })
}

async fn evaluate_cdp_expression(websocket_url: &str, expression: &str) -> Result<Value, String> {
    let (socket, _) = tokio::time::timeout(CDP_CONNECT_TIMEOUT, connect_async(websocket_url))
        .await
        .map_err(|_| "timed out connecting CDP websocket".to_string())?
        .map_err(|error| format!("failed to connect CDP websocket: {error}"))?;
    let mut session = CdpSession::new(socket);
    session.send_command(1, "Runtime.enable", json!({})).await?;
    session
        .send_command(
            2,
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
                "allowUnsafeEvalBlockedByCSP": true
            }),
        )
        .await
}

pub(crate) fn has_owned_codex_renderer_state() -> bool {
    !MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_empty()
        || !MODEL_PICKER_TIMEZONES
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
        || !MODEL_PICKER_UNTRACKED_DOCUMENTS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
}

/// Release the running renderer's CCSM hooks without launching or closing Codex.
pub(crate) async fn release_codex_model_picker_projection() -> Result<(), String> {
    #[cfg(test)]
    if let Some(release) = TEST_RENDERER_RELEASE.lock().unwrap().clone() {
        return release();
    }
    // The test home is an isolation boundary for both files and running applications.
    if std::env::var_os("CC_SWITCH_TEST_HOME").is_some() && !isolated_cdp_fixture_enabled() {
        return Ok(());
    }
    let _effects = RENDERER_EFFECTS.lock().await;
    let script = build_native_model_picker_release_script();
    let mut known_urls: BTreeSet<String> = MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .keys()
        .cloned()
        .collect();
    known_urls.extend(
        MODEL_PICKER_TIMEZONES
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned(),
    );
    known_urls.extend(
        MODEL_PICKER_UNTRACKED_DOCUMENTS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned(),
    );
    let targets =
        discover_codex_page_targets(&candidate_debug_ports(DEFAULT_CODEX_DEBUG_PORT)).await;
    let mut urls = BTreeSet::new();
    for (_, target) in targets {
        if let Some(url) = target.web_socket_debugger_url {
            urls.insert(url);
        }
    }
    if urls.is_empty() && is_codex_desktop_running() {
        return Err("CCSM renderer ownership cannot be inspected without CDP; restart Codex to finish releasing compatibility hooks".to_string());
    }
    // Cleanup includes draining issued renderer work and preserving untracked
    // registrations as an explicit pending/restart result.
    let results = futures::future::join_all(urls.iter().map(|url| {
        let script = &script;
        async move {
            let documents = release_owned_codex_document_state(url).await;
            let renderer = match evaluate_cdp_expression(url, script).await {
                Ok(result) => verify_codex_renderer_release(&result),
                Err(error) => Err(error),
            };
            documents.and(renderer)
        }
    }))
    .await;
    let mut errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
    if known_urls.difference(&urls).next().is_some() {
        // A remembered socket may now belong to a different application. Keep
        // its registrations pending instead of clearing an unverified target.
        errors.push("Previously owned Codex targets could not be verified; restart Codex to finish releasing compatibility hooks".to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Failed to release Codex Desktop compatibility hooks: {}",
            errors.join("; ")
        ))
    }
}

fn codex_document_release_plan(
    snapshot: &Value,
    known: &[(String, String)],
) -> (bool, BTreeSet<String>, bool) {
    let known_by_nonce: HashMap<_, _> = known.iter().cloned().collect();
    let mut identifiers: BTreeSet<String> = known.iter().map(|(_, id)| id.clone()).collect();
    let mut unknown = false;
    let entries = snapshot.get("registrations").and_then(Value::as_array);
    for entry in entries.into_iter().flatten() {
        let id = entry.get(1).and_then(Value::as_str).or_else(|| {
            entry
                .get(0)
                .and_then(Value::as_str)
                .and_then(|nonce| known_by_nonce.get(nonce).map(String::as_str))
        });
        if let Some(id) = id {
            identifiers.insert(id.to_string());
        } else {
            unknown = true;
        }
    }
    let owned = snapshot.get("owned").and_then(Value::as_bool) == Some(true)
        || !identifiers.is_empty()
        || unknown;
    (owned, identifiers, unknown)
}

async fn release_owned_codex_document_state(websocket_url: &str) -> Result<(), String> {
    let snapshot = evaluate_cdp_expression(websocket_url, &format!(r#"(() => ({{
      owned: Boolean(window["{MODEL_PICKER_PATCH_KEY}"] || window.__ccSwitchCodexAppCompatibilityV9),
      registrations: Array.from(window.__ccSwitchCodexDocumentScripts?.entries() || []),
      timezoneOwned: window.__ccSwitchCodexTimezoneOwned
    }}))()"#)).await?;
    let snapshot = snapshot
        .pointer("/result/result/value")
        .ok_or_else(|| "Codex renderer document ownership inspection failed".to_string())?;
    let known = MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(websocket_url)
        .cloned()
        .unwrap_or_default();
    let (mut owned, identifiers, mut unknown) = codex_document_release_plan(snapshot, &known);
    let timezone_owned = snapshot.get("timezoneOwned").and_then(Value::as_bool) == Some(true)
        || MODEL_PICKER_TIMEZONES
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(websocket_url);
    unknown |= MODEL_PICKER_UNTRACKED_DOCUMENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(websocket_url);
    unknown |= owned
        && known.is_empty()
        && snapshot
            .get("timezoneOwned")
            .and_then(Value::as_bool)
            .is_none();
    owned |= timezone_owned || unknown;
    if !owned {
        return Ok(());
    }
    let (socket, _) = tokio::time::timeout(CDP_CONNECT_TIMEOUT, connect_async(websocket_url))
        .await
        .map_err(|_| "timed out connecting CDP document cleanup".to_string())?
        .map_err(|error| format!("failed to connect CDP document cleanup: {error}"))?;
    let mut session = CdpSession::new(socket);
    session.send_command(1, "Page.enable", json!({})).await?;
    if timezone_owned {
        session
            .send_command(
                2,
                "Emulation.setTimezoneOverride",
                codex_timezone_override_parameters(""),
            )
            .await?;
        MODEL_PICKER_TIMEZONES
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(websocket_url);
    }
    let mut request_id = 3;
    for identifier in identifiers {
        session
            .send_command(
                request_id,
                "Page.removeScriptToEvaluateOnNewDocument",
                json!({"identifier": identifier}),
            )
            .await?;
        request_id += 1;
    }
    if unknown {
        return Err("Untracked CCSM document scripts remain; restart Codex to release legacy compatibility scripts".to_string());
    }
    session.send_command(request_id, "Runtime.evaluate", json!({"expression": "delete window.__ccSwitchCodexDocumentScripts; delete window.__ccSwitchCodexTimezoneOwned", "returnByValue": true})).await?;
    MODEL_PICKER_DOCUMENT_SCRIPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(websocket_url);
    Ok(())
}

fn verify_codex_renderer_release(evaluation: &Value) -> Result<(), String> {
    if evaluation.pointer("/result/exceptionDetails").is_some() {
        return Err("Codex renderer hook release raised an exception".to_string());
    }
    let value = evaluation.pointer("/result/result/value").ok_or_else(|| {
        "Codex renderer did not return completed hook release evidence".to_string()
    })?;
    if value.get("disposed").and_then(Value::as_bool).is_none()
        || value.get("incomplete").and_then(Value::as_bool) != Some(false)
    {
        return Err("Codex renderer hook release is incomplete; restart Codex to release legacy or pending compatibility work".to_string());
    }
    Ok(())
}

/// Read the timezone from a currently running Codex renderer. This verifies the
/// child-process `TZ` result instead of inferring it from Windows settings.
pub(crate) async fn inspect_codex_runtime_timezone() -> Result<CodexRuntimeTimezone, String> {
    let ports = candidate_debug_ports(DEFAULT_CODEX_DEBUG_PORT);
    let expression = r#"(() => ({
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      utcOffsetMinutes: -new Date().getTimezoneOffset()
    }))()"#;
    let mut errors = Vec::new();
    for port in ports {
        let targets = match list_cdp_targets(port).await {
            Ok(targets) => targets,
            Err(error) => {
                errors.push(format!("port {port}: {error}"));
                continue;
            }
        };
        let targets = match pick_codex_page_targets(&targets, port) {
            Ok(targets) => targets,
            Err(error) => {
                errors.push(format!("port {port}: {error}"));
                continue;
            }
        };
        for target in targets {
            let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
                continue;
            };
            match evaluate_cdp_expression(websocket_url, expression).await {
                Ok(evaluation) => return codex_runtime_timezone_from_evaluation(&evaluation),
                Err(error) => errors.push(format!("port {port}: {error}")),
            }
        }
    }
    if detect_running_codex_main_process().is_some() {
        Err("Codex is running without an injectable CDP port; fully quit it and relaunch it from CCSwitchMulti before verifying the runtime timezone".to_string())
    } else if errors.is_empty() {
        Err("Codex Desktop is not running".to_string())
    } else {
        Err("Codex Desktop is not running on a CCSwitchMulti-managed debug port".to_string())
    }
}

/// 轻量 CDP websocket 会话，只处理 request/response。
struct CdpSession<S> {
    socket: S,
    responses: HashMap<u64, Value>,
}

impl<S> CdpSession<S>
where
    S: SinkExt<Message>
        + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    <S as futures::Sink<Message>>::Error: std::fmt::Display,
{
    fn new(socket: S) -> Self {
        Self {
            socket,
            responses: HashMap::new(),
        }
    }

    /// 发送 CDP 命令并等待对应 id 的响应。
    async fn send_command(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| format!("failed to send CDP command {method}: {error}"))?;
        tokio::time::timeout(CDP_COMMAND_TIMEOUT, self.wait_for_id(id, method))
            .await
            .map_err(|_| format!("timed out waiting for CDP command {method}"))?
    }

    /// 跳过无关 CDP 事件，直到拿到当前命令响应。
    async fn wait_for_id(&mut self, id: u64, method: &str) -> Result<Value, String> {
        loop {
            if let Some(response) = self.responses.remove(&id) {
                return cdp_command_result(response, method);
            }
            let Some(message) = self.socket.next().await else {
                return Err(format!("CDP websocket closed before {method} response"));
            };
            let message =
                message.map_err(|error| format!("failed to read CDP message: {error}"))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)
                .map_err(|error| format!("failed to parse CDP message: {error}"))?;
            if let Some(response_id) = value.get("id").and_then(Value::as_u64) {
                if response_id == id {
                    return cdp_command_result(value, method);
                }
                self.responses.insert(response_id, value);
            }
        }
    }
}

/// 将 CDP error 响应转成普通错误。
fn cdp_command_result(response: Value, method: &str) -> Result<Value, String> {
    if let Some(error) = response.get("error") {
        Err(format!("CDP command {method} failed: {error}"))
    } else {
        Ok(response)
    }
}

/// 模型目录 patch 的可执行 JavaScript 核心。
///
/// 这段逻辑会同时嵌入 Codex renderer，并由 QuickJS 回归直接执行。任何字段写入都必须
/// 先通过明确的模型门特征；不能把 React Fiber、Intl context 或普通 API 对象当成模型
/// 容器遍历和改写。
fn model_picker_patch_core_script() -> &'static str {
    r#"
  const modelProjectionEnabled = () => state.modelProjectionEnabled !== false;
  const setProjectedValue = (target, key, value) => {
    const owned = Object.prototype.hasOwnProperty.call(target, key);
    const original = target[key];
    target[key] = value;
    (state.modelRestorations ||= []).push(() => {
      if (target[key] !== value) return;
      if (owned) target[key] = original;
      else delete target[key];
    });
  };
  const appendProjectedValue = (target, value) => {
    target.push(value);
    (state.modelRestorations ||= []).push(() => {
      const index = target.indexOf(value);
      if (index >= 0) target.splice(index, 1);
    });
  };
  const currentPayload = () => state.payload || {};
  const modelNames = () => {
    const payload = currentPayload();
    return Array.from(new Set([payload.defaultModel, ...(payload.modelNames || [])].filter((name) => typeof name === "string" && name.trim()).map((name) => name.trim())));
  };
  const descriptorFor = (name) => {
    const payload = currentPayload();
    const existing = (payload.models || []).find((model) => model && model.model === name);
    return {
      model: name,
      id: name,
      slug: name,
      name,
      displayName: name,
      hidden: false,
      ...(existing || {}),
      hidden: false,
    };
  };
  const stringArray = (value) => Array.isArray(value) && value.every((item) => typeof item === "string");
  const modelArray = (value, allowEmpty = false) => Array.isArray(value) && (allowEmpty || value.length > 0) && value.every((item) => item && typeof item === "object" && typeof item.model === "string");
  const patchModelNameArray = (models) => {
    if (!modelProjectionEnabled() || !stringArray(models)) return false;
    let changed = false;
    for (const name of modelNames()) {
      if (!models.includes(name)) {
        appendProjectedValue(models, name);
        changed = true;
      }
    }
    return changed;
  };
  const patchModelArray = (models, allowEmpty = false) => {
    if (!modelProjectionEnabled() || !modelArray(models, allowEmpty)) return false;
    const names = modelNames();
    const existing = new Map(models.map((model) => [model.model, model]));
    let changed = false;
    for (const model of models) {
      if (names.includes(model.model) && model.hidden !== false) {
        setProjectedValue(model, "hidden", false);
        changed = true;
      }
    }
    for (const name of names) {
      if (!existing.has(name)) {
        appendProjectedValue(models, descriptorFor(name));
        changed = true;
      }
    }
    return changed;
  };
  const removeHiddenNames = (container, key) => {
    if (!modelProjectionEnabled() || !Array.isArray(container?.[key])) return false;
    const names = new Set(modelNames());
    const before = container[key].length;
    const remaining = container[key].filter((name) => !names.has(name));
    if (remaining.length === before) return false;
    setProjectedValue(container, key, remaining);
    return before !== container[key].length;
  };
  const patchNameSet = (setLike) => {
    if (!modelProjectionEnabled() || !(setLike instanceof Set)) return false;
    let changed = false;
    for (const name of modelNames()) {
      if (!setLike.has(name)) {
        setLike.add(name);
        (state.modelRestorations ||= []).push(() => setLike.delete(name));
        changed = true;
      }
    }
    return changed;
  };
  const patchModelContainer = (value) => {
    if (!modelProjectionEnabled() || !value || typeof value !== "object") return false;
    const looksLikeModelGate = "availableModels" in value || "available_models" in value || "useHiddenModels" in value || "use_hidden_models" in value || "defaultModel" in value || "default_model" in value;
    if (!looksLikeModelGate) return false;

    let changed = false;
    if (patchModelArray(value.models, "defaultModel" in value || "availableModels" in value || "available_models" in value)) changed = true;
    if (patchModelNameArray(value.models)) changed = true;
    if (patchModelArray(value.data)) changed = true;
    if (patchModelArray(value.result)) changed = true;
    if (patchModelArray(value.pages?.[0]?.data)) changed = true;
    if (patchModelArray(value.result?.data)) changed = true;
    if (patchModelArray(value.result?.models)) changed = true;
    if (patchModelArray(value.message?.result?.data)) changed = true;
    if (patchModelArray(value.message?.result?.models)) changed = true;
    if (patchNameSet(value.availableModels)) changed = true;
    if (patchNameSet(value.available_models)) changed = true;
    if (patchModelNameArray(value.availableModels)) changed = true;
    if (patchModelNameArray(value.available_models)) changed = true;
    if (removeHiddenNames(value, "hiddenModels")) changed = true;
    if (removeHiddenNames(value, "hidden_models")) changed = true;
    if ("useHiddenModels" in value && value.useHiddenModels !== false) {
      setProjectedValue(value, "useHiddenModels", false);
      changed = true;
    }
    if ("use_hidden_models" in value && value.use_hidden_models !== false) {
      setProjectedValue(value, "use_hidden_models", false);
      changed = true;
    }
    if ("default_model" in value && typeof value.default_model === "string" && modelNames().length && !modelNames().includes(value.default_model)) {
      setProjectedValue(value, "default_model", modelNames()[0]);
      changed = true;
    }
    if ("defaultModel" in value && value.defaultModel == null && modelNames().length > 0) {
      setProjectedValue(value, "defaultModel", descriptorFor(modelNames()[0]));
      changed = true;
    }
    return changed;
  };
"#
}

/// Keep renderer hooks reversible when live routing returns to Codex's native catalog.
fn model_picker_lifecycle_core_script() -> &'static str {
    r#"
  state.modelProjectionEnabled = true;
  state.lifecycleVersion = "2";
  state.generation ||= 1;
  const compatibilityGeneration = state.generation;
  const compatibilityActive = () => !state.disposed && state.generation === compatibilityGeneration;
  state.pendingRequests ||= new Set();
  state.historyRestorations ||= new Set();
  const trackCompatibilityRequest = (work) => {
    if (!compatibilityActive()) return Promise.resolve();
    let request;
    try { request = Promise.resolve(work()); } catch (error) { request = Promise.reject(error); }
    state.pendingRequests.add(request);
    request.then(() => state.pendingRequests.delete(request), () => state.pendingRequests.delete(request));
    return request;
  };
  const rememberHistoryRestoration = (restore) => {
    state.historyRestorations.add(restore);
    return () => state.historyRestorations.delete(restore);
  };
  state.hookRestorations ||= [];
  const installCompatibilityHook = (target, key, value) => {
    if (!compatibilityActive()) return;
    const owned = Object.prototype.hasOwnProperty.call(target, key);
    const original = target[key];
    target[key] = value;
    state.hookRestorations.push(() => {
      if (target[key] !== value) return;
      if (owned) target[key] = original;
      else delete target[key];
    });
  };
  state.dispose = () => {
    if (state.disposalPromise) return state.disposalPromise;
    state.modelProjectionEnabled = false;
    state.disposed = true;
    state.generation += 1;
    const failures = [];
    const release = (work) => { try { work(); } catch (error) { failures.push(String(error?.message || error)); } };
    if (state.interval) release(() => clearInterval(state.interval));
    state.interval = null;
    release(() => state.reactRootObserver?.disconnect());
    state.reactRootObserver = null;
    if (state.messageListener) release(() => window.removeEventListener("message", state.messageListener, true));
    state.messageListener = null;
    for (const restore of state.hookRestorations.splice(0).reverse()) {
      release(restore);
    }
    state.requestIds?.clear();
    state.pendingReactNodes?.clear();
    state.payload = { modelNames: [], models: [] };
    // Restore our fields before waiting: the CDP caller can time out while a
    // native RPC remains in flight. Its later settlement must never trigger
    // a delayed rollback over a new native conversation or model selection.
    for (const restore of state.historyRestorations) release(restore);
    state.historyRestorations.clear();
    state.historyManagerCandidates?.clear();
    for (const restore of (state.modelRestorations || []).splice(0).reverse()) release(restore);
    const pending = [...state.pendingRequests, state.runPromise, state.reactDiscoveryPromise,
      state.historyQueryRefreshPromise, state.historyLineageHydrationPromise, state.historySyncPromise].filter(Boolean);
    state.disposalPromise = Promise.allSettled(pending).then(() => {
      if (window[patchKey] === state) delete window[patchKey];
      return { disposed: true, drained: true, incomplete: failures.length > 0 || state.inheritedIncomplete === true, failures };
    });
    return state.disposalPromise;
  };
"#
}

fn build_model_picker_release_script() -> String {
    format!(
        r#"(async () => {{
  const current = window["{MODEL_PICKER_PATCH_KEY}"];
  let disposed = false;
  let incomplete = Boolean(current && typeof current.dispose !== "function");
  if (typeof current?.dispose === "function") {{
    const pending = [current.runPromise, current.reactDiscoveryPromise, current.historyQueryRefreshPromise,
      current.historyLineageHydrationPromise, current.historySyncPromise].filter(Boolean);
    const result = await current.dispose();
    await Promise.allSettled(pending);
    disposed = result?.disposed === true;
    incomplete = result?.drained !== true || result?.incomplete === true;
  }}
  const legacy = window.__ccSwitchCodexAppCompatibilityV9;
  if (!legacy) return {{ disposed, incomplete }};
  if (legacy.interval) clearInterval(legacy.interval);
  legacy.interval = null;
  try {{ legacy.reactRootObserver?.disconnect(); }} catch {{}}
  legacy.payload = {{ modelNames: [], models: [] }};
  const root = window.__STATSIG__ || globalThis.__STATSIG__;
  const clients = [root?.firstInstance, ...(Object.values(root?.instances || {{}}))];
  try {{ if (typeof root?.instance === "function") clients.push(root.instance()); }} catch {{}}
  for (const client of clients) {{
    if (typeof client?.__ccSwitchOriginalGetDynamicConfig !== "function") continue;
    client.getDynamicConfig = client.__ccSwitchOriginalGetDynamicConfig;
    delete client.__ccSwitchStatsigPatchVersion;
  }}
  delete window.__ccSwitchCodexAppCompatibilityV9;
  await Promise.allSettled([legacy.runPromise, legacy.reactDiscoveryPromise, legacy.historyQueryRefreshPromise,
    legacy.historyLineageHydrationPromise, legacy.historySyncPromise].filter(Boolean));
  return {{ disposed: true, incomplete: true, legacy: true }};
}})()"#
    )
}

/// The V5 renderer patch accidentally added model-whitelist fields to every
/// Statsig dynamic config instead of only the model catalog config. When those
/// fields reached Guardian V2, the bundled app-server rejected the request as
/// an invalid `FeatureToml` even though the on-disk `config.toml` was valid.
/// Keep the repair and exact-error fallback independent from model/history code
/// so the compatibility boundary can be executed directly in QuickJS tests.
fn guardian_v2_compatibility_patch_core_script() -> &'static str {
    r#"
  const guardianV2DynamicConfigName = "2553103476";
  const guardianV2FlatFeatureKey = "features.guardianv2";
  const statsigModelWhitelistConfigName = "107580212";
  const legacyModelWhitelistFields = ["available_models", "use_hidden_models", "default_model"];
  const guardianV2Enabled = (value) => value?.enabled === true;
  const isGuardianV2StructuredValue = (value) => Boolean(value) && typeof value === "object" && !Array.isArray(value);
  const stripLegacyModelWhitelistPollution = (config) => {
    const value = config?.value;
    if (!isGuardianV2StructuredValue(value) || !legacyModelWhitelistFields.some((key) => Object.prototype.hasOwnProperty.call(value, key))) return config;
    const nextValue = { ...value };
    for (const key of legacyModelWhitelistFields) delete nextValue[key];
    return { ...config, value: nextValue };
  };
  const patchGuardianV2DynamicConfig = (name, config) => {
    if (String(name || "") !== guardianV2DynamicConfigName) return config;
    return stripLegacyModelWhitelistPollution(config);
  };
  const prepareStatsigDynamicConfig = (name, config, repairLegacyPollution = false) => {
    const normalizedName = String(name || "");
    config = patchGuardianV2DynamicConfig(normalizedName, config);
    if (normalizedName === statsigModelWhitelistConfigName) {
      return { config, patchModelWhitelist: true };
    }
    return {
      config: repairLegacyPollution ? stripLegacyModelWhitelistPollution(config) : config,
      patchModelWhitelist: false,
    };
  };
  const isGuardianV2FeatureTomlError = (error) => {
    const message = String(error?.message || error || "");
    return message.includes("FeatureToml") && message.includes("features.guardianv2");
  };
  const downgradeGuardianV2Config = (config) => {
    if (!config || typeof config !== "object" || Array.isArray(config)) return config;
    let next = config;
    let changed = false;
    const flat = config[guardianV2FlatFeatureKey];
    if (isGuardianV2StructuredValue(flat)) {
      next = { ...next, [guardianV2FlatFeatureKey]: guardianV2Enabled(flat) };
      changed = true;
    }
    const features = config.features;
    if (isGuardianV2StructuredValue(features) && isGuardianV2StructuredValue(features.guardianv2)) {
      if (!changed) next = { ...next };
      next.features = { ...features, guardianv2: guardianV2Enabled(features.guardianv2) };
      changed = true;
    }
    return changed ? next : config;
  };
  const guardianV2FallbackRequestParams = (method, params) => {
    const wrapped = method === "send-cli-request-for-host" && params?.params && typeof params.params === "object";
    const requestParams = wrapped ? params.params : params;
    if (!requestParams || typeof requestParams !== "object" || Array.isArray(requestParams)) return params;
    const nextConfig = downgradeGuardianV2Config(requestParams.config);
    if (nextConfig === requestParams.config) return params;
    const nextRequestParams = { ...requestParams, config: nextConfig };
    return wrapped ? { ...params, params: nextRequestParams } : nextRequestParams;
  };
"#
}

/// App-server 请求兼容核心，同时嵌入 renderer 并由 QuickJS 行为测试直接执行。
///
/// `thread/list` 继续显式查询所有 Provider；`thread/resume` 则以当前 live Provider
/// 覆盖历史线程保存的旧入口标签，使跨 Provider 历史在冷恢复后重新经过 CCSM Router。
fn app_server_request_normalization_core_script() -> &'static str {
    r#"
  const appServerRequestPatchVersion = "10";
  const appServerRequestClientNeedsPatch = (client) => client?.__ccSwitchModelRequestPatch !== appServerRequestPatchVersion;
  const appServerMethod = (method, params) => method === "send-cli-request-for-host" && params?.method ? String(params.method) : String(method || "");
  const currentModelProvider = () => {
    const value = state.payload?.modelProvider;
    return typeof value === "string" && value.trim() ? value.trim() : null;
  };
  const normalizeAppServerRequestParams = (method, params) => {
    const actualMethod = appServerMethod(method, params);
    if (actualMethod === "thread/list") {
      if (method === "send-cli-request-for-host" && params?.params?.modelProviders == null) {
        return { ...params, params: { ...(params?.params || {}), modelProviders: [] } };
      }
      if (params == null) return { modelProviders: [] };
      return params?.modelProviders == null ? { ...params, modelProviders: [] } : params;
    }
    if (actualMethod !== "thread/resume") return params;
    const modelProvider = currentModelProvider();
    if (!modelProvider) return params;
    if (method === "send-cli-request-for-host") {
      const requestParams = params?.params;
      if (!requestParams || typeof requestParams !== "object" || Array.isArray(requestParams) || requestParams.modelProvider === modelProvider) return params;
      return { ...params, params: { ...requestParams, modelProvider } };
    }
    if (!params || typeof params !== "object" || Array.isArray(params) || params.modelProvider === modelProvider) return params;
    return { ...params, modelProvider };
  };
"#
}

/// Codex Desktop refreshes thread metadata independently from the loaded transcript. After
/// `thread/revert`, the stable thread ID remains unchanged while `thread.path` moves to a new
/// rollout. An already loaded conversation can therefore retain the old rollout path and turns.
/// Rehydrate only idle conversations whose cached path is demonstrably stale; active turns are
/// deferred so the compatibility layer never interrupts live work.
fn history_lineage_hydration_core_script() -> &'static str {
    r#"
  const historyWorkActive = () => typeof compatibilityActive !== "function" || compatibilityActive();
  const issueHistoryRequest = (work) => typeof trackCompatibilityRequest === "function" ? trackCompatibilityRequest(work) : work();
  const normalizedRolloutPath = (value) => typeof value === "string"
    ? value.replaceAll("\\", "/").replace(/^\/\/?\?\//, "").toLowerCase()
    : "";
  const conversationHasActiveTurn = (conversation) => {
    if (Array.isArray(conversation?.turns)
        && conversation.turns.some((turn) => turn?.status === "inProgress")) return true;
    const history = conversation?.turnHistory?.kind === "canonical"
      ? conversation.turnHistory.history
      : null;
    const entities = history?.entitiesByKey;
    return entities && typeof entities === "object"
      ? Object.values(entities).some((turn) => turn?.status === "inProgress")
      : false;
  };
  const canonicalHistoryIsFragmented = (conversation) => {
    const history = conversation?.turnHistory?.kind === "canonical"
      ? conversation.turnHistory.history
      : null;
    return history?.isComplete !== true
      && Array.isArray(history?.islands)
      && history.islands.length > 1;
  };
  const planHistoryLineageRehydration = (manager) => {
    const repair = [];
    const deferred = [];
    const conversations = typeof manager?.getCachedConversations === "function"
      ? manager.getCachedConversations()
      : [];
    const metadata = manager?.threadStore?.threadsById;
    for (const conversation of Array.isArray(conversations) ? conversations : []) {
      const id = typeof conversation?.id === "string" ? conversation.id : "";
      if (!id || !metadata || typeof metadata.get !== "function") continue;
      const currentPath = normalizedRolloutPath(conversation.rolloutPath);
      const activePath = normalizedRolloutPath(metadata.get(id)?.path);
      if (!activePath) continue;
      const pathChanged = currentPath !== activePath;
      if (!pathChanged && !canonicalHistoryIsFragmented(conversation)) continue;
      const streaming = typeof manager?.isConversationStreaming === "function"
        && manager.isConversationStreaming(id);
      if (streaming || conversationHasActiveTurn(conversation)) deferred.push(id);
      else repair.push(id);
    }
    return { repair, deferred };
  };
  const rehydrateStaleHistoryLineages = async (manager) => {
    if (!historyWorkActive()) return { repaired: [], deferred: [], failed: [] };
    const plan = planHistoryLineageRehydration(manager);
    const repaired = [];
    const failed = [];
    for (const id of plan.repair) {
      if (!historyWorkActive()) break;
      const previous = manager.getConversation(id);
      const activePath = manager.threadStore.threadsById.get(id)?.path;
      const owned = [];
      const owns = (value, entry) => Object.prototype.hasOwnProperty.call(value, entry.key) === entry.afterOwn && value[entry.key] === entry.after;
      const restoreOwned = () => {
        const current = manager.getConversation(id);
        if (!current) return;
        // These cache fields form one lineage. A native resume or user update
        // that replaced any of them owns the new lineage; do not mix old turns
        // or an old rollout path into that result.
        if (owned.some((entry) => !owns(current, entry))) return;
        const unchanged = owned.filter((entry) => owns(current, entry));
        if (!unchanged.length) return;
        manager.updateConversationState(id, (value) => {
          for (const entry of unchanged) {
            if (entry.beforeOwn) value[entry.key] = entry.before;
            else delete value[entry.key];
          }
        });
      };
      const retireRestoration = typeof rememberHistoryRestoration === "function"
        ? rememberHistoryRestoration(restoreOwned) : () => {};
      try {
        if (!historyWorkActive()) break;
        manager.updateConversationState(id, (conversation) => {
          if (!historyWorkActive()) return;
          const replace = (key, value, present = true) => {
            owned.push({ key, beforeOwn: Object.prototype.hasOwnProperty.call(conversation, key), before: conversation[key], afterOwn: present, after: value });
            if (present) conversation[key] = value;
            else delete conversation[key];
          };
          replace("rolloutPath", activePath);
          replace("turns", []);
          // Codex 26.901 renders canonical turnHistory when present. Clearing only the
          // legacy turns overlay leaves the stale islands visible and causes resume to
          // merge the new tail into the broken cache instead of replacing it.
          replace("turnHistory", undefined, false);
          replace("turnsPagination", {
            olderCursor: null,
            oldestLoadedTurnId: null,
            isLoadingOlder: false,
            hasLoadedOldest: false,
          });
          replace("resumeState", "needs_resume");
        });
        if (!historyWorkActive()) break;
        const resume = await issueHistoryRequest(() => manager.resumeConversationForUnavailableOwner({
          conversationId: id,
          model: null,
          serviceTier: null,
          reasoningEffort: null,
          workspaceRoots: previous?.cwd ? [previous.cwd] : ["/"],
          collaborationMode: previous?.latestCollaborationMode || null,
        }));
        if (!historyWorkActive()) break;
        if (resume?.status === "not-ready") {
          throw new Error(`Codex App could not resume history: ${String(resume.reason || "not-ready")}`);
        }
        // Resume intentionally hydrates only the newest page for paginated histories.
        // Ask Codex's own pagination layer to close every remaining boundary so a
        // fragmented middle cannot remain hidden behind a stale virtual-list island.
        if (typeof manager.getCompleteConversationTurns === "function") {
          if (!historyWorkActive()) break;
          await issueHistoryRequest(() => manager.getCompleteConversationTurns(id));
          if (!historyWorkActive()) break;
        }
        repaired.push(id);
        retireRestoration();
      } catch (error) {
        if (!historyWorkActive()) break;
        restoreOwned();
        retireRestoration();
        failed.push({ id, error: String(error?.message || error) });
      }
    }
    return { repaired, deferred: plan.deferred, failed };
  };
"#
}

/// Locate the current app-server request client from Codex's cached signal
/// module. The signal getter can replace its client without replacing the
/// module object, so this function must run on every lightweight heartbeat.
/// Kept as an executable core so the replacement behavior is regression-tested.
fn app_server_module_client_discovery_core_script() -> &'static str {
    r#"
  const patchAppServerModuleClients = (module) => {
    let patched = 0;
    for (const candidate of Object.values(module || {}).filter((item) => item && typeof item === "object")) {
      if (patchRequestClient(candidate)) patched += 1;
      if (typeof candidate.sendRequest !== "function" && typeof candidate.get === "function") {
        try { if (patchRequestClient(candidate.get())) patched += 1; } catch {}
      }
    }
    return patched;
  };
"#
}

/// 构造 renderer 注入脚本：触发新版本地历史目录同步，并修复模型白名单和缓存。
fn build_model_picker_unlock_script(catalog: &CodexModelCatalogProjection) -> String {
    let payload = serde_json::to_string(catalog).unwrap_or_else(|_| "{}".to_string());
    let model_patch_core = model_picker_patch_core_script();
    let guardian_v2_patch_core = guardian_v2_compatibility_patch_core_script();
    let request_normalization_core = app_server_request_normalization_core_script();
    let history_lineage_hydration_core = history_lineage_hydration_core_script();
    let module_client_discovery_core = app_server_module_client_discovery_core_script();
    let lifecycle_core = model_picker_lifecycle_core_script();
    let release_script = build_model_picker_release_script();
    let activation_guard = renderer_activation_guard_core_script();
    format!(
        r#"
(async () => {{
  const payload = {payload};
  const patchKey = "{MODEL_PICKER_PATCH_KEY}";
{activation_guard}
  let inheritedIncomplete = false;
  if (window.__ccSwitchCodexAppCompatibilityV9 || (window[patchKey] && window[patchKey].lifecycleVersion !== "2")) {{
    const released = await {release_script};
    if (!rendererActivationCurrent()) return {{ cancelled: true }};
    inheritedIncomplete = released.incomplete === true;
  }}
  const state = window[patchKey] || {{}};
  state.inheritedIncomplete = state.inheritedIncomplete === true || inheritedIncomplete;
  state.payload = payload;
  state.requestIds = state.requestIds || new Set();
  state.modulePromises = state.modulePromises || new Map();
  state.failures = state.failures || [];
  const rendererSchedulerVersion = "2";
  if (state.rendererSchedulerVersion !== rendererSchedulerVersion) {{
    if (state.interval) clearInterval(state.interval);
    try {{ state.reactRootObserver?.disconnect(); }} catch {{}}
    state.interval = null;
    state.runPromise = null;
    state.reactRootObserver = null;
    state.reactDiscoveryPromise = null;
    state.pendingReactNodes = new Set();
    state.lastReactFullScanAt = 0;
    state.lastReactIncrementalScanAt = 0;
    state.rendererSchedulerVersion = rendererSchedulerVersion;
  }}
  state.pendingReactNodes = state.pendingReactNodes || new Set();
  state.historyManagerCandidates = state.historyManagerCandidates || new Set();
  const recordFailure = (error) => {{
    const message = String(error?.message || error);
    if (state.failures.at(-1) !== message) state.failures.push(message);
    state.failures = state.failures.slice(-20);
  }};
  window[patchKey] = state;
{lifecycle_core}
{model_patch_core}
{guardian_v2_patch_core}
{request_normalization_core}
{history_lineage_hydration_core}
{module_client_discovery_core}
  const patchStatsigConfig = (name, config, repairLegacyPollution = false) => {{
    const prepared = prepareStatsigDynamicConfig(name, config, repairLegacyPollution);
    config = prepared.config;
    if (!modelProjectionEnabled() || !prepared.patchModelWhitelist) return config;
    const value = config?.value;
    if (!value || typeof value !== "object") return config;
    const available = Array.isArray(value.available_models) ? [...value.available_models] : [];
    let changed = false;
    for (const name of modelNames()) {{
      if (!available.includes(name)) {{
        available.push(name);
        changed = true;
      }}
    }}
    const nextValue = {{ ...value, available_models: available, use_hidden_models: false, default_model: modelNames()[0] || value.default_model }};
    if (changed || nextValue.default_model !== value.default_model || value.use_hidden_models !== false) {{
      try {{
        setProjectedValue(config, "value", nextValue);
      }} catch {{
        return {{ ...config, value: nextValue }};
      }}
    }}
    return config;
  }};
  const statsigClients = () => {{
    const root = window.__STATSIG__ || globalThis.__STATSIG__;
    if (!root || typeof root !== "object") return [];
    const clients = [root.firstInstance, typeof root.instance === "function" ? root.instance() : null];
    if (root.instances && typeof root.instances === "object") clients.push(...Object.values(root.instances));
    return clients.filter((client, index, array) => client && typeof client === "object" && array.indexOf(client) === index);
  }};
  const patchStatsig = () => {{
    if (!modelProjectionEnabled()) return;
    for (const client of statsigClients()) {{
      if (typeof client.getDynamicConfig !== "function") continue;
      if (client.__ccSwitchStatsigPatchVersion !== "2") {{
        const repairingLegacyPatch = client.__ccSwitchModelWhitelistPatched === true;
        const original = client.__ccSwitchOriginalGetDynamicConfig || client.getDynamicConfig.bind(client);
        installCompatibilityHook(client, "__ccSwitchOriginalGetDynamicConfig", original);
        installCompatibilityHook(client, "getDynamicConfig", (name, options) => patchStatsigConfig(name, original(name, options), repairingLegacyPatch));
        installCompatibilityHook(client, "__ccSwitchStatsigPatchVersion", "2");
      }}
      try {{ patchStatsigConfig("107580212", client.getDynamicConfig("107580212", {{ disableExposureLog: true }})); }} catch {{}}
      try {{ patchStatsigConfig(guardianV2DynamicConfigName, client.getDynamicConfig(guardianV2DynamicConfigName, {{ disableExposureLog: true }})); }} catch {{}}
    }}
  }};
  const visibleAssetUrls = () => [
      ...Array.from(document.scripts || []).map((script) => script.src),
      ...Array.from(document.querySelectorAll("link[href]") || []).map((link) => link.href),
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
    ].filter((url, index, urls) => Boolean(url) && urls.indexOf(url) === index);
  const assetUrl = async (namePart) => {{
    const urls = visibleAssetUrls();
    const direct = urls.find((url) => url.includes("/assets/") && url.includes(namePart) && url.split("?")[0].endsWith(".js"));
    if (direct) return direct;

    // Newer Codex builds keep only index-*.js visible and reference rpc/app chunks
    // from the Vite dependency table. Follow those same-origin module references
    // instead of depending on one hashed filename or Performance API behavior.
    const inspected = new Set();
    for (let index = 0; index < urls.length && index < 32; index += 1) {{
      const url = urls[index];
      if (!url || inspected.has(url) || !url.split("?")[0].endsWith(".js")) continue;
      inspected.add(url);
      let source = "";
      try {{
        source = await fetch(url).then((response) => response.ok ? response.text() : "");
      }} catch {{}}
      for (const match of source.matchAll(/["'](\.\/[^"']+\.js)["']/g)) {{
        const candidate = new URL(match[1], url).href;
        if (candidate.includes(namePart)) return candidate;
        if (!urls.includes(candidate)) urls.push(candidate);
      }}
    }}
    return "";
  }};
  const loadAppModule = async (namePart) => {{
    if (!compatibilityActive()) throw new Error("CCSM renderer compatibility was disposed");
    if (!state.modulePromises.has(namePart)) {{
      state.modulePromises.set(namePart, Promise.resolve().then(async () => {{
        const url = await assetUrl(namePart);
        if (!compatibilityActive()) throw new Error("CCSM renderer compatibility was disposed");
        if (!url) throw new Error(`Codex App asset not found: ${{namePart}}`);
        return await import(url);
      }}).catch((error) => {{
        state.modulePromises.delete(namePart);
        throw error;
      }}));
    }}
    return await state.modulePromises.get(namePart);
  }};
  const localCatalogFromModule = (module) => {{
    // 26.825+ exports both an unresolved RpcPromise (`appHost`) and the
    // initialized RpcStub (`appServices`). Prefer the initialized service;
    // otherwise the first shallow export looks valid but fails inside
    // readStatus/readSnapshot.
    const initialized = module?.appServices?.localThreadCatalog;
    if (initialized && typeof initialized.requestStartupSync === "function") return initialized;
    const roots = Object.values(module).filter((item) => item && (typeof item === "object" || typeof item === "function"));
    for (const root of roots) {{
      const catalog = root.localThreadCatalog;
      if (catalog && typeof catalog.requestStartupSync === "function") return catalog;
    }}
    return null;
  }};
  const summarizeCatalogStatus = (status, requested) => {{
    const local = Array.isArray(status?.hosts) ? status.hosts.find((host) => host?.hostId === "local") : null;
    return {{
      requested,
      complete: local?.isComplete ?? null,
      revision: local?.revision ?? status?.revision ?? null,
      source: "status",
    }};
  }};
  // 新版 Codex/ChatGPT App 用 localThreadCatalog 保存统一侧边栏目录。这里只调用
  // App 自己的 RPC 同步服务，不直接改 codex-dev.db 或历史 provider 元数据。
  const triggerLocalThreadCatalogSync = async () => {{
    if (!compatibilityActive()) return {{ requested: false }};
    if (state.historySyncPromise) return await state.historySyncPromise;
    state.historySyncPromise = Promise.resolve().then(async () => {{
      const module = await loadAppModule("rpc-");
      if (!compatibilityActive()) return {{ requested: false }};
      const catalog = localCatalogFromModule(module);
      if (!catalog || typeof catalog.requestStartupSync !== "function") throw new Error("Codex App localThreadCatalog RPC service was not found");

      // 26.825+ exposes readStatus and can report a complete local catalog before
      // requestStartupSync. Do not wait on a redundant startup sync RPC forever.
      try {{
        const status = await trackCompatibilityRequest(() => catalog.readStatus());
        if (!compatibilityActive()) return {{ requested: false }};
        const summary = summarizeCatalogStatus(status, false);
        if (summary.complete === true) {{
          state.historySync = summary;
          return summary;
        }}
        state.historySync = summarizeCatalogStatus(status, true);
        if (!compatibilityActive()) return {{ requested: false }};
        trackCompatibilityRequest(() => catalog.requestStartupSync()).then(async () => {{
          if (!compatibilityActive()) return;
          try {{
            const status = await trackCompatibilityRequest(() => catalog.readStatus());
            if (!compatibilityActive()) return;
            state.historySync = summarizeCatalogStatus(status, true);
          }} catch (error) {{
            recordFailure(error);
          }}
        }}).catch(recordFailure);
        return state.historySync;
      }} catch (error) {{
        if (!compatibilityActive()) return {{ requested: false }};
        recordFailure(error);
      }}

      // Older builds expose readSnapshot instead of readStatus. Preserve that
      // official cold-sweep path as a capability fallback.
      try {{
          if (!compatibilityActive()) return {{ requested: false }};
          const before = typeof catalog.readSnapshot === "function" ? await trackCompatibilityRequest(() => catalog.readSnapshot()) : null;
          if (!compatibilityActive()) return {{ requested: false }};
          await trackCompatibilityRequest(() => catalog.requestStartupSync());
          if (!compatibilityActive()) return {{ requested: false }};
          const after = typeof catalog.readSnapshot === "function" ? await trackCompatibilityRequest(() => catalog.readSnapshot()) : null;
          if (!compatibilityActive()) return {{ requested: false }};
          state.historySync = {{
            requested: true,
            beforeCount: Array.isArray(before?.entries) ? before.entries.length : null,
            afterCount: Array.isArray(after?.entries) ? after.entries.length : null,
            complete: after?.isComplete ?? null,
            source: "snapshot",
          }};
          if (after?.isComplete !== true) {{
            setTimeout(() => {{ if (compatibilityActive()) state.historySyncPromise = null; }}, 10000);
          }}
          return state.historySync;
      }} catch (error) {{
        if (!compatibilityActive()) return {{ requested: false }};
        recordFailure(error);
      }}
      throw new Error("Codex App localThreadCatalog RPC service was not found");
    }}).catch((error) => {{
      state.historySync = {{ requested: false, error: String(error?.message || error) }};
      recordFailure(error);
      state.historySyncPromise = null;
      return state.historySync;
    }});
    return await state.historySyncPromise;
  }};
  const isModelListMethod = (method) => method === "list-models-for-host" || method === "model/list";
  const patchModelListResult = (result) => {{
    if (!modelProjectionEnabled() || result == null) return false;
    let changed = false;
    if (Array.isArray(result) && patchModelArray(result, true)) changed = true;
    if (Array.isArray(result?.data) && patchModelArray(result.data, true)) changed = true;
    if (Array.isArray(result?.models) && patchModelArray(result.models, true)) changed = true;
    if (Array.isArray(result?.result) && patchModelArray(result.result, true)) changed = true;
    if (Array.isArray(result?.result?.data) && patchModelArray(result.result.data, true)) changed = true;
    if (Array.isArray(result?.result?.models) && patchModelArray(result.result.models, true)) changed = true;
    if (Array.isArray(result?.pages?.[0]?.data) && patchModelArray(result.pages[0].data, true)) changed = true;
    if (Array.isArray(result?.message?.result?.data) && patchModelArray(result.message.result.data, true)) changed = true;
    if (Array.isArray(result?.message?.result?.models) && patchModelArray(result.message.result.models, true)) changed = true;
    if (patchModelContainer(result)) changed = true;
    return changed;
  }};
  const patchAppServerResult = (method, result) => {{
    if (!isModelListMethod(method)) return result;
    patchModelListResult(result);
    return result;
  }};
  const patchRequestClient = (client) => {{
    if (!compatibilityActive() || !client || typeof client.sendRequest !== "function") return false;
    if (!appServerRequestClientNeedsPatch(client)) return true;
    try {{
      const original = client.__ccSwitchOriginalSendRequest || client.sendRequest.bind(client);
      installCompatibilityHook(client, "__ccSwitchOriginalSendRequest", original);
      installCompatibilityHook(client, "sendRequest", async function ccSwitchPatchedSendRequest(method, params, options) {{
        if (!compatibilityActive()) return original(method, params, options);
        const normalizedParams = normalizeAppServerRequestParams(method, params);
        const actualMethod = appServerMethod(method, params);
        try {{
          const result = await trackCompatibilityRequest(() => original(method, normalizedParams, options));
          if (!compatibilityActive()) return result;
          return patchAppServerResult(actualMethod, result);
        }} catch (error) {{
          if (!compatibilityActive()) throw error;
          if (!isGuardianV2FeatureTomlError(error)) throw error;
          const fallbackParams = guardianV2FallbackRequestParams(method, normalizedParams);
          if (fallbackParams === normalizedParams) throw error;
          state.guardianV2FallbackCount = (state.guardianV2FallbackCount || 0) + 1;
          const result = await trackCompatibilityRequest(() => original(method, fallbackParams, options));
          if (!compatibilityActive()) return result;
          return patchAppServerResult(actualMethod, result);
        }}
      }});
      installCompatibilityHook(client, "__ccSwitchModelRequestPatch", appServerRequestPatchVersion);
      return true;
    }} catch (error) {{
      // rpc-BaVz... also exposes immutable RpcStub objects whose method shape
      // resembles AppServerRequestClient. Skip them and keep scanning React.
      recordFailure(error);
      return false;
    }}
  }};
  const looksLikeAppServerRequestClient = (value) => {{
    try {{
      return value
        && typeof value.sendRequest === "function"
        && typeof value.getCompatibleThreadSortKey === "function"
        && typeof value.supportsPaginatedThreadHistory === "function";
    }} catch {{
      return false;
    }}
  }};
  const reactRootValue = (value) => {{
    let current = value;
    let hops = 0;
    try {{
      while (current?.return && hops < 2048) {{
        current = current.return;
        hops += 1;
      }}
    }} catch {{}}
    return current || value;
  }};
  const appendReactRootsFromNode = (node, values, seen) => {{
    for (const key of Object.keys(node || {{}})) {{
      if (!key.startsWith("__reactContainer") && !key.startsWith("__reactFiber") && !key.startsWith("__reactInternalInstance")) continue;
      const value = reactRootValue(node[key]);
      if (value && !seen.has(value)) {{
        seen.add(value);
        values.push(value);
      }}
    }}
  }};
  const initialReactNodes = () => [
    document.getElementById("root") || document.body,
    document.documentElement,
  ].filter(Boolean);
  const drainPendingReactNodes = () => {{
    const pending = Array.from(state.pendingReactNodes || []);
    state.pendingReactNodes.clear();
    return pending;
  }};
  const scheduleHistoryLineageHydration = (manager) => {{
    if (!compatibilityActive() || !state.historyQueryRefreshCompleted
        || state.historyLineageHydrationPromise
        || state.historyLineageHydrationCompleted
        || typeof manager?.getCachedConversations !== "function"
        || typeof manager?.getConversation !== "function"
        || typeof manager?.updateConversationState !== "function"
        || typeof manager?.setConversation !== "function"
        || typeof manager?.resumeConversationForUnavailableOwner !== "function") return;
    const wallNow = Date.now();
    if (wallNow < (state.historyLineageHydrationRetryAt || 0)) return;
    state.historyLineageHydrationRetryAt = wallNow + 30000;
    state.historyLineageHydrationPromise = Promise.resolve()
      .then(() => compatibilityActive() ? rehydrateStaleHistoryLineages(manager) : null)
      .then((result) => {{
        if (!compatibilityActive() || !result) return result;
        state.historyLineageHydration = result;
        for (const failure of result.failed || []) recordFailure(failure.error);
        state.historyLineageHydrationCompleted = (result.deferred || []).length === 0 && (result.failed || []).length === 0;
        if (state.historyLineageHydrationCompleted) state.historyManagerCandidates.clear();
        return result;
      }})
      .catch((error) => {{
        state.historyLineageHydration = {{ repaired: [], deferred: [], failed: [{{ error: String(error?.message || error) }}] }};
        recordFailure(error);
      }})
      .finally(() => {{ state.historyLineageHydrationPromise = null; }});
  }};
  const yieldReactDiscovery = () => new Promise((resolve) => {{
    if (typeof globalThis.requestIdleCallback === "function") {{
      globalThis.requestIdleCallback(() => resolve(), {{ timeout: 50 }});
    }} else {{
      setTimeout(resolve, 0);
    }}
  }});
  const monotonicNow = () => typeof performance?.now === "function" ? performance.now() : Date.now();
  const patchReactAppServerClients = async (startingNodes) => {{
    if (!compatibilityActive()) return 0;
    if (state.reactDiscoveryPromise) return await state.reactDiscoveryPromise;
    state.reactDiscoveryPromise = (async () => {{
      const roots = [];
      const rootSeen = new Set();
      const nodeQueue = startingNodes.filter(Boolean);
      let nodeCursor = 0;
      while (nodeCursor < nodeQueue.length) {{
        if (!compatibilityActive()) return 0;
        const sliceDeadline = monotonicNow() + 4;
        let sliceCount = 0;
        while (nodeCursor < nodeQueue.length && sliceCount < 800 && monotonicNow() < sliceDeadline) {{
          sliceCount += 1;
          const node = nodeQueue[nodeCursor++];
          appendReactRootsFromNode(node, roots, rootSeen);
          try {{
            for (const child of Array.from(node?.childNodes || [])) nodeQueue.push(child);
          }} catch {{}}
        }}
        if (nodeCursor < nodeQueue.length) await yieldReactDiscovery();
      }}

      const queue = roots.map((value) => ({{ value, depth: 0 }}));
      const seen = new Set();
      let cursor = 0;
      let patched = 0;
      while (cursor < queue.length && seen.size < 140000) {{
        if (!compatibilityActive()) return 0;
        const sliceDeadline = monotonicNow() + 4;
        let sliceCount = 0;
        while (cursor < queue.length && seen.size < 140000 && sliceCount < 800 && monotonicNow() < sliceDeadline) {{
          sliceCount += 1;
          const {{ value, depth }} = queue[cursor++];
          if (!value || (typeof value !== "object" && typeof value !== "function") || seen.has(value)) continue;
          seen.add(value);

          if (looksLikeAppServerRequestClient(value) && patchRequestClient(value)) patched += 1;
          const nestedClient = value?.requestClient;
          if (looksLikeAppServerRequestClient(nestedClient) && patchRequestClient(nestedClient)) {{
            patched += 1;
            if (state.historyManagerCandidates.size >= 8 && !state.historyManagerCandidates.has(value)) state.historyManagerCandidates.clear();
            state.historyManagerCandidates.add(value);
            if (!state.historyQueryRefreshPromise && !state.historyQueryRefreshCompleted && typeof value?.threadStore?.refreshRecentConversations === "function") {{
              state.historyQueryRefreshPromise = Promise.resolve().then(() =>
                compatibilityActive() ? trackCompatibilityRequest(() => value.threadStore.refreshRecentConversations({{ mode: "expanded" }})) : null
              ).then(() => {{
                if (!compatibilityActive()) return;
                state.historyQueryRefreshCompleted = true;
              }}).catch((error) => {{
                state.historyQueryRefreshPromise = null;
                state.historyQueryRefreshCompleted = false;
                recordFailure(error);
                throw error;
              }});
            }}
            scheduleHistoryLineageHydration(value);
          }}

          if (depth >= 14) continue;
          let descriptors = {{}};
          try {{ descriptors = Object.getOwnPropertyDescriptors(value); }} catch {{}}
          for (const [key, descriptor] of Object.entries(descriptors)) {{
            if (!["window", "ownerDocument", "parentNode", "parentElement", "nextSibling", "previousSibling"].includes(key) && "value" in descriptor) {{
              const child = descriptor.value;
              if (child && ((typeof child === "object" && !(typeof Node !== "undefined" && child instanceof Node)) || typeof child === "function")) {{
                queue.push({{ value: child, depth: depth + 1 }});
              }}
            }}
          }}
        }}
        if (cursor < queue.length && seen.size < 140000) await yieldReactDiscovery();
      }}
      state.reactRequestClientPatched = state.reactRequestClientPatched || patched > 0;
      state.historyQueryPatch = {{
        patched: Boolean(state.reactRequestClientPatched),
        clientCount: patched,
        refreshRequested: Boolean(state.historyQueryRefreshCompleted),
        lineageHydration: state.historyLineageHydration || null,
      }};
      return patched;
    }})().finally(() => {{ state.reactDiscoveryPromise = null; }});
    return await state.reactDiscoveryPromise;
  }};
  const installReactRootObserver = () => {{
    if (!compatibilityActive() || state.reactRootObserver || typeof MutationObserver !== "function") return;
    const target = document.documentElement || document.body;
    if (!target) return;
    state.reactRootObserver = new MutationObserver((records) => {{
      if (!compatibilityActive()) return;
      for (const record of records) {{
        for (const node of record.addedNodes || []) {{
          if (!node || (typeof node !== "object" && typeof node !== "function")) continue;
          state.pendingReactNodes.add(node);
          if (state.pendingReactNodes.size > 128) {{
            state.pendingReactNodes.clear();
            state.pendingReactNodes.add(document.getElementById("root") || document.body);
          }}
        }}
      }}
    }});
    state.reactRootObserver.observe(target, {{ childList: true, subtree: true }});
  }};
  const installAppServerPatch = async () => {{
    if (!compatibilityActive()) return;
    let patched = 0;
    let modulePatched = 0;
    const wallNow = Date.now();
    if (!state.appServerModuleRetryAt || wallNow >= state.appServerModuleRetryAt) {{
      try {{
        const module = await loadAppModule("app-server-manager-signals-");
        if (!compatibilityActive()) return;
        modulePatched = patchAppServerModuleClients(module);
        patched += modulePatched;
        state.appServerModuleRetryAt = 0;
      }} catch (error) {{
        if (!compatibilityActive()) return;
        recordFailure(error);
        state.appServerModuleRetryAt = wallNow + 30000;
      }}
    }}
    installReactRootObserver();
    const fullScanDue = !state.reactInitialDiscoveryCompleted
      || (modulePatched === 0 && wallNow - (state.lastReactFullScanAt || 0) >= 30000);
    const incrementalScanDue = modulePatched === 0
      && state.pendingReactNodes.size > 0
      && wallNow - (state.lastReactIncrementalScanAt || 0) >= 5000;
    if (fullScanDue) {{
      state.pendingReactNodes.clear();
      state.lastReactFullScanAt = wallNow;
      patched += await patchReactAppServerClients(initialReactNodes());
      if (!compatibilityActive()) return;
      state.reactInitialDiscoveryCompleted = true;
    }} else if (incrementalScanDue) {{
      state.lastReactIncrementalScanAt = wallNow;
      patched += await patchReactAppServerClients(drainPendingReactNodes());
      if (!compatibilityActive()) return;
    }} else if (modulePatched > 0) {{
      state.pendingReactNodes.clear();
    }}
    if (state.historyQueryRefreshPromise) {{
      try {{
        await state.historyQueryRefreshPromise;
        if (!compatibilityActive()) return;
      }} catch {{}}
    }}
    if (!compatibilityActive()) return;
    for (const manager of state.historyManagerCandidates) scheduleHistoryLineageHydration(manager);
    if (state.historyLineageHydrationPromise) {{
      try {{
        await state.historyLineageHydrationPromise;
        if (!compatibilityActive()) return;
      }} catch {{}}
    }}
    state.historyQueryPatch = {{
      ...(state.historyQueryPatch || {{}}),
      refreshRequested: Boolean(state.historyQueryRefreshCompleted),
      lineageHydration: state.historyLineageHydration || null,
    }};
    state.appServerPatchInstalled = patched > 0;
    if (patched > 0 && state.historyQueryRefreshCompleted) {{
      state.reactRequestClientPatchVersion = appServerRequestPatchVersion;
      state.appServerPatchVersion = appServerRequestPatchVersion;
    }}
  }};
  const patchMcpModelResponseData = (data) => {{
    if (data?.type !== "mcp-response") return false;
    const message = data.message || data.response;
    const requestId = message?.id != null ? String(message.id) : "";
    if (!requestId || !state.requestIds.has(requestId)) return false;
    state.requestIds.delete(requestId);
    return patchModelListResult(message?.result) || patchModelListResult(message?.result?.data);
  }};
  const installMessagePatch = () => {{
    if (state.disposed || state.messagePatchInstalled) return;
    state.messagePatchInstalled = true;
    const originalDispatchEvent = window.dispatchEvent;
    installCompatibilityHook(window, "dispatchEvent", function ccSwitchPatchedDispatchEvent(event) {{
      try {{
        const detail = event?.detail;
        const request = detail?.request;
        if (modelProjectionEnabled() && event?.type === "codex-message-from-view" && detail?.type === "mcp-request" && request?.method === "model/list") {{
          request.params = {{ ...(request.params || {{}}), includeHidden: true }};
          if (request.id != null) state.requestIds.add(String(request.id));
        }}
        if (event?.type === "message") patchMcpModelResponseData(event.data);
      }} catch (error) {{
        recordFailure(error);
      }}
      return originalDispatchEvent.call(this, event);
    }});
    state.messageListener = (event) => {{
      try {{ patchMcpModelResponseData(event?.data); }} catch (error) {{ recordFailure(error); }}
    }};
    window.addEventListener("message", state.messageListener, true);
  }};
  const reactFiberKeys = (element) => Object.keys(element || {{}}).filter((key) => key.startsWith("__reactFiber") || key.startsWith("__reactInternalInstance") || key.startsWith("__reactProps"));
  // Codex app-server 会根据 requires_openai_auth 暴露 OAuth 状态；旧配置或缓存状态
  // 可能把 renderer 留在非 chatgpt 模式，这里只修复前端 context，不改请求路由。
  const authContextValueFrom = (element) => {{
    for (const key of reactFiberKeys(element)) {{
      for (let fiber = element?.[key]; fiber; fiber = fiber.return) {{
        for (const value of [fiber.memoizedProps?.value, fiber.pendingProps?.value]) {{
          if (value && typeof value === "object" && typeof value.setAuthMethod === "function" && "authMethod" in value) return value;
        }}
      }}
    }}
    return null;
  }};
  const spoofChatGPTAuthMethod = (element) => {{
    if (!modelProjectionEnabled()) return false;
    const auth = authContextValueFrom(element);
    if (!auth || auth.authMethod === "chatgpt") return false;
    try {{
      const originalMethod = auth.authMethod;
      auth.setAuthMethod("chatgpt");
      (state.modelRestorations ||= []).push(() => {{
        const current = authContextValueFrom(element) || auth;
        if (current.authMethod === "chatgpt") current.setAuthMethod(originalMethod);
      }});
      return true;
    }} catch (error) {{
      recordFailure(error);
      return false;
    }}
  }};
  const patchReactState = () => {{
    const nodes = [document.body, ...document.querySelectorAll("button, [role='menu'], [role='dialog'], [data-radix-popper-content-wrapper]")].filter(Boolean);
    for (const node of nodes.slice(0, 220)) {{
      spoofChatGPTAuthMethod(node);
    }}
  }};
  const run = async () => {{
    if (state.disposed) return;
    if (state.runPromise) return await state.runPromise;
    state.runPromise = (async () => {{
      installMessagePatch();
      await installAppServerPatch();
      if (state.disposed) return;
      void triggerLocalThreadCatalogSync();
      patchStatsig();
      patchReactState();
    }})().finally(() => {{ state.runPromise = null; }});
    return await state.runPromise;
  }};
  await run();
  if (!compatibilityActive()) return {{ status: "disposed", modelCount: 0 }};
  if (!state.disposed && !state.interval) state.interval = setInterval(() => {{ void run(); }}, 1500);
  const historySync = await triggerLocalThreadCatalogSync();
  return {{ status: "ok", modelCount: modelNames().length, available_models: modelNames(), historySync, historyQueryPatch: state.historyQueryPatch || null, patchKey }};
}})()
"#
    )
}

/// 启动 Codex Desktop，并传入 remote debugging 参数。
pub(crate) fn launch_codex_with_debug_port(
    executable: &Path,
    debug_port: u16,
) -> Result<(), String> {
    if let Some(running) = detect_running_codex_main_process() {
        return Err(format!(
            "Codex Desktop is already running at {}. Fully quit Codex, then launch it from CCSwitchMulti so the model picker patch can be installed.",
            running.display()
        ));
    }
    let launch_timezone =
        crate::codex_egress_timezone::resolve_launch_timezone(&crate::settings::get_settings());
    #[cfg(target_os = "macos")]
    {
        if launch_timezone.is_none() {
            if let Some(bundle) = macos_codex_bundle_for_executable(executable) {
                let mut command = Command::new("open");
                command.arg(bundle).arg("--args");
                append_codex_debug_args(&mut command, debug_port);
                return command
                    .spawn()
                    .map(|_| {
                        crate::codex_egress_timezone::mark_codex_timezone_applied();
                    })
                    .map_err(|error| {
                        format!("failed to launch {}: {error}", executable.display())
                    });
            }
        } else {
            log::warn!(
                "Codex egress timezone injection is not supported through macOS LaunchServices; launching the bundle executable directly"
            );
        }
    }
    let mut command = Command::new(executable);
    append_codex_debug_args(&mut command, debug_port);
    apply_codex_launch_timezone(&mut command, launch_timezone.as_deref());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map(|_| {
            crate::codex_egress_timezone::mark_codex_timezone_applied();
        })
        .map_err(|error| format!("failed to launch {}: {error}", executable.display()))
}

fn apply_codex_launch_timezone(command: &mut Command, timezone: Option<&str>) {
    if let Some(timezone) = timezone {
        command.env("TZ", timezone);
    }
}

/// 为 Desktop 启动命令追加 Chromium remote-debugging 参数。
fn append_codex_debug_args(command: &mut Command, debug_port: u16) {
    command
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg(format!(
            "--remote-allow-origins=http://127.0.0.1:{debug_port}"
        ));
}

/// Windows 下查找 Codex App 主进程的脚本。
///
/// 新版 MSIX 将统一应用壳改名为 `ChatGPT.exe`，但只允许来自 `OpenAI.Codex`
/// 包目录的同名进程，避免误把独立 ChatGPT 应用接入 Codex 的 CDP 修复链路。
#[cfg(target_os = "windows")]
const DETECT_CODEX_MAIN_PROCESS_SCRIPT: &str = r#"
Get-CimInstance Win32_Process -Filter "Name = 'Codex.exe' OR Name = 'ChatGPT.exe'" |
  Where-Object {
    if (-not $_.ExecutablePath -or $_.CommandLine -match ' --type=') { return $false }
    $leaf = Split-Path -Leaf $_.ExecutablePath
    $legacyCodex = $leaf -ceq 'Codex.exe'
    $unifiedCodex = $leaf -ceq 'ChatGPT.exe' -and $_.ExecutablePath -match '\\WindowsApps\\OpenAI\.Codex(?:\.Preview)?_'
    return $legacyCodex -or $unifiedCodex
  } |
  Select-Object -First 1 -ExpandProperty ExecutablePath
"#;

/// 查找 Codex Desktop 主进程路径；已运行但未开放 CDP 时不能原地注入。
pub(crate) fn detect_running_codex_main_process() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(running) = *TEST_CODEX_RUNNING.lock().unwrap() {
        return running.then(|| PathBuf::from("fixture-codex.exe"));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let script = DETECT_CODEX_MAIN_PROCESS_SCRIPT;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(PathBuf::from(text))
        }
    }

    #[cfg(target_os = "macos")]
    {
        detect_running_macos_codex_main_process()
    }

    #[cfg(target_os = "linux")]
    {
        detect_running_linux_codex_main_process()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// 按常见安装位置寻找 Codex Desktop 可执行文件。
pub(crate) fn resolve_codex_executable() -> Option<PathBuf> {
    detect_running_codex_main_process()
        .filter(|path| path.exists())
        .and_then(|path| remember_codex_desktop_executable(&path).ok().or(Some(path)))
        .or_else(read_remembered_codex_desktop_executable)
        .or_else(find_platform_codex_executable)
}

/// 按当前操作系统查找 Codex Desktop 主程序，避免把 CLI 路径混进 Desktop/CDP 解锁链路。
fn find_platform_codex_executable() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        find_latest_windows_codex_executable()
    }

    #[cfg(target_os = "macos")]
    {
        find_macos_codex_executable()
    }

    #[cfg(target_os = "linux")]
    {
        find_linux_codex_executable()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// 存储最近确认过的 Codex App shell 路径，支持 Desktop 不在常见安装目录的场景。
fn remembered_codex_desktop_executable_path() -> PathBuf {
    crate::config::get_app_config_dir().join(REMEMBERED_CODEX_DESKTOP_EXECUTABLE_FILENAME)
}

/// 判断路径文件名是否是当前平台的 Desktop shell，而不是小写 CLI/app-server。
#[cfg(target_os = "windows")]
fn is_codex_desktop_executable_name(name: &str) -> bool {
    name == "Codex.exe" || name == "ChatGPT.exe"
}

/// Windows 下进一步约束统一 `ChatGPT.exe` 必须来自 `OpenAI.Codex` MSIX 包目录。
///
/// 旧版 `Codex.exe` 仍兼容独立安装目录；新版壳使用了通用文件名，若不校验祖先目录，
/// 会把用户安装的独立 ChatGPT 应用错误地当成 Codex App。
#[cfg(target_os = "windows")]
fn is_codex_desktop_executable_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !is_codex_desktop_executable_name(name) {
        return false;
    }
    if name == "Codex.exe" {
        return true;
    }
    path.ancestors().any(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("OpenAI.Codex_") || name.starts_with("OpenAI.Codex.Preview_")
            })
    })
}

/// macOS 统一壳可能叫 Codex 或 ChatGPT；必须回到 bundle 元数据校验身份与实际主程序。
#[cfg(target_os = "macos")]
fn is_codex_desktop_executable_path(path: &Path) -> bool {
    macos_codex_bundle_for_executable(path).is_some()
}

/// Linux 等平台沿用文件名级别的 Desktop shell 校验。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn is_codex_desktop_executable_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_codex_desktop_executable_name)
}

/// 判断路径文件名是否是当前平台的 Desktop shell，而不是小写 CLI/app-server。
#[cfg(target_os = "linux")]
fn is_codex_desktop_executable_name(name: &str) -> bool {
    name == "Codex" || (name.starts_with("Codex") && name.ends_with(".AppImage"))
}

/// 判断路径文件名是否是当前平台的 Desktop shell，而不是小写 CLI/app-server。
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn is_codex_desktop_executable_name(_name: &str) -> bool {
    false
}

/// 校验 Desktop 主程序路径，避免把小写 CLI/app-server `codex` 当成 Electron shell。
///
/// 这不是禁用 CLI 支持；CLI/app-server 修复走 `config.toml`、`model_catalog_json`
/// 和 `/v1/models`，而不是 Desktop renderer 的 CDP 注入入口。
fn canonical_codex_desktop_executable_path(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!(
            "Codex Desktop executable does not exist: {}",
            path.display()
        ));
    }
    if !is_codex_desktop_executable_path(path) {
        return Err(format!(
            "Detected file is not Codex Desktop's platform shell executable: {}",
            path.display()
        ));
    }
    path.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve Codex Desktop executable {}: {error}",
            path.display()
        )
    })
}

/// 读取上次从运行中 Desktop 捕获到的已校验主程序路径。
fn read_remembered_codex_desktop_executable() -> Option<PathBuf> {
    read_remembered_codex_desktop_executable_from(&remembered_codex_desktop_executable_path())
}

/// 从指定状态文件读取记住的 Desktop 主程序路径，便于测试隔离。
fn read_remembered_codex_desktop_executable_from(state_path: &Path) -> Option<PathBuf> {
    let value = crate::config::read_json_file::<Value>(state_path).ok()?;
    let executable = value
        .get("codex_executable")
        .or_else(|| value.get("codexExecutable"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)?;
    canonical_codex_desktop_executable_path(&executable).ok()
}

/// 记住已运行的 Desktop shell 路径；失败只影响下一次自动启动，不影响本次诊断返回。
fn remember_codex_desktop_executable(path: &Path) -> Result<PathBuf, String> {
    let executable = canonical_codex_desktop_executable_path(path)?;
    remember_codex_desktop_executable_at(&remembered_codex_desktop_executable_path(), &executable)
}

/// 将已校验的 Desktop 主程序路径写入指定状态文件，便于测试隔离。
fn remember_codex_desktop_executable_at(
    state_path: &Path,
    executable: &Path,
) -> Result<PathBuf, String> {
    let executable = canonical_codex_desktop_executable_path(executable)?;
    crate::config::write_json_file(
        state_path,
        &json!({
            "codex_executable": executable.display().to_string(),
        }),
    )
    .map_err(|error| {
        format!(
            "Failed to remember Codex Desktop executable at {}: {error}",
            state_path.display()
        )
    })?;
    Ok(executable)
}

/// macOS 上通过稳定 bundle identifier 找到运行中的 Codex/ChatGPT 统一壳。
#[cfg(target_os = "macos")]
fn detect_running_macos_codex_main_process() -> Option<PathBuf> {
    let script = format!(
        r#"
tell application "System Events"
  set matches to application processes whose bundle identifier is "{}"
  if (count of matches) is 0 then return ""
  try
    return POSIX path of (application file of item 1 of matches)
  end try
end tell
"#,
        CODEX_DESKTOP_BUNDLE_IDENTIFIER
    );
    if let Some(bundle) = command_stdout_trimmed(Command::new("osascript").arg("-e").arg(script))
        .filter(|path| !path.is_empty())
    {
        if let Some(executable) = macos_codex_bundle_executable(Path::new(&bundle)) {
            if let Ok(executable) = canonical_codex_desktop_executable_path(&executable) {
                return Some(executable);
            }
        }
    }

    let mut saw_codex_process = false;
    for process_name in ["Codex", "ChatGPT"] {
        for pid in command_stdout_lines(Command::new("pgrep").args(["-x", process_name])) {
            saw_codex_process = true;
            let path = command_stdout_trimmed(Command::new("ps").args(["-p", &pid, "-o", "comm="]));
            let Some(path) = path.filter(|path| !path.is_empty()) else {
                continue;
            };
            if let Ok(executable) = canonical_codex_desktop_executable_path(Path::new(&path)) {
                return Some(executable);
            }
        }
    }
    if saw_codex_process {
        return find_macos_codex_executable();
    }
    None
}

/// Linux 上通过 `/proc/<pid>/exe` 找到运行中的大写 Desktop/AppImage 主进程。
#[cfg(target_os = "linux")]
fn detect_running_linux_codex_main_process() -> Option<PathBuf> {
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name
            .to_str()
            .filter(|pid| pid.as_bytes().iter().all(|byte| byte.is_ascii_digit()))
        else {
            continue;
        };
        let Ok(executable) = std::fs::read_link(Path::new("/proc").join(pid).join("exe")) else {
            continue;
        };
        if let Ok(executable) = canonical_codex_desktop_executable_path(&executable) {
            return Some(executable);
        }
    }
    None
}

/// 查找 macOS 常见 Codex.app 安装位置和 Spotlight 索引结果。
#[cfg(target_os = "macos")]
fn find_macos_codex_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for bundle in macos_codex_common_bundle_candidates() {
        if let Some(executable) = macos_codex_bundle_executable(&bundle) {
            push_codex_desktop_executable_candidate(&mut candidates, Vec::new(), executable);
        }
    }
    for bundle in command_stdout_lines(Command::new("mdfind").arg(format!(
        "kMDItemCFBundleIdentifier == '{}' || kMDItemFSName == 'Codex.app'",
        CODEX_DESKTOP_BUNDLE_IDENTIFIER
    ))) {
        if let Some(executable) = macos_codex_bundle_executable(Path::new(&bundle)) {
            push_codex_desktop_executable_candidate(&mut candidates, Vec::new(), executable);
        }
    }

    candidates.pop().map(|(_, executable)| executable)
}

/// macOS 常见应用目录候选，覆盖系统级和用户级安装。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_codex_common_bundle_candidates() -> Vec<PathBuf> {
    let mut bundles = vec![
        PathBuf::from("/Applications/Codex.app"),
        PathBuf::from("/Applications/ChatGPT.app"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let applications = PathBuf::from(home).join("Applications");
        bundles.push(applications.join("Codex.app"));
        bundles.push(applications.join("ChatGPT.app"));
    }
    bundles
}

/// 以 bundle 的稳定身份和声明的主程序名构造候选，拒绝路径穿越与独立 ChatGPT。
#[cfg(any(target_os = "macos", test))]
fn macos_codex_bundle_executable_from_metadata(
    bundle: &Path,
    bundle_identifier: &str,
    executable_name: &str,
) -> Option<PathBuf> {
    if bundle_identifier.trim() != CODEX_DESKTOP_BUNDLE_IDENTIFIER {
        return None;
    }
    let executable_name = executable_name.trim();
    let executable_path = Path::new(executable_name);
    if executable_name.is_empty()
        || executable_path.file_name().and_then(|name| name.to_str()) != Some(executable_name)
        || executable_path.components().count() != 1
    {
        return None;
    }
    Some(bundle.join("Contents").join("MacOS").join(executable_name))
}

/// 读取 macOS bundle 的 Info.plist 字段；使用系统绝对路径，避免 GUI PATH 差异。
#[cfg(target_os = "macos")]
fn macos_bundle_info_value(bundle: &Path, key: &str) -> Option<String> {
    let info_plist = bundle.join("Contents").join("Info.plist");
    command_stdout_trimmed(
        Command::new("/usr/bin/plutil")
            .arg("-extract")
            .arg(key)
            .arg("raw")
            .arg("-o")
            .arg("-")
            .arg(info_plist),
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

/// 从 macOS `.app` 的真实元数据解析 Desktop 主二进制路径。
#[cfg(target_os = "macos")]
fn macos_codex_bundle_executable(bundle: &Path) -> Option<PathBuf> {
    let bundle_identifier = macos_bundle_info_value(bundle, "CFBundleIdentifier")?;
    let executable_name = macos_bundle_info_value(bundle, "CFBundleExecutable")?;
    macos_codex_bundle_executable_from_metadata(bundle, &bundle_identifier, &executable_name)
}

/// 找到受支持的顶层包装；Framework 内部 Service.app 不会提前截断搜索。
#[cfg(any(target_os = "macos", test))]
fn macos_codex_bundle_ancestor(executable: &Path) -> Option<PathBuf> {
    executable.ancestors().find_map(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| *name == "Codex.app" || *name == "ChatGPT.app")
            .map(|_| path.to_path_buf())
    })
}

/// 如果路径属于 bundle 元数据声明的主程序，返回对应 bundle，便于使用 `open` 启动。
#[cfg(target_os = "macos")]
fn macos_codex_bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let bundle = macos_codex_bundle_ancestor(executable)?;
    let declared_executable = macos_codex_bundle_executable(&bundle)?;
    (declared_executable == executable).then_some(bundle)
}

/// 查找 Linux 常见 Desktop/AppImage 安装位置，避免把小写 CLI `codex` 当成 Desktop。
#[cfg(target_os = "linux")]
fn find_linux_codex_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for binary in ["Codex", "Codex.AppImage"] {
        for path in command_stdout_lines(Command::new("which").args(["-a", binary])) {
            push_codex_desktop_executable_candidate(
                &mut candidates,
                Vec::new(),
                PathBuf::from(path),
            );
        }
    }
    for path in linux_codex_common_executable_candidates() {
        push_codex_desktop_executable_candidate(&mut candidates, Vec::new(), path);
    }
    for path in linux_desktop_entry_executable_candidates() {
        push_codex_desktop_executable_candidate(&mut candidates, Vec::new(), path);
    }

    candidates.pop().map(|(_, executable)| executable)
}

/// Linux 常见安装目录候选，覆盖 PATH 之外的 AppImage 和 `/opt` 安装。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_codex_common_executable_candidates() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/usr/local/bin/Codex"),
        PathBuf::from("/usr/bin/Codex"),
        PathBuf::from("/opt/Codex/Codex"),
        PathBuf::from("/opt/OpenAI/Codex/Codex"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.extend([
            home.join(".local").join("bin").join("Codex"),
            home.join(".local").join("bin").join("Codex.AppImage"),
            home.join("Applications").join("Codex"),
            home.join("Applications").join("Codex.AppImage"),
        ]);
    }
    paths
}

/// 从 Linux `.desktop` 文件中提取绝对路径 Exec 候选。
#[cfg(target_os = "linux")]
fn linux_desktop_entry_executable_candidates() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/usr/share/applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("applications"),
        );
    }

    let mut candidates = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().contains("codex"))
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            candidates.extend(
                linux_desktop_entry_exec_values(&text)
                    .into_iter()
                    .map(PathBuf::from),
            );
        }
    }
    candidates
}

/// 解析 `.desktop` 文件中的绝对路径 Exec 值，忽略 flatpak/snap 包装命令。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_desktop_entry_exec_values(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("Exec="))
        .filter_map(|exec| exec.split_whitespace().next())
        .filter(|path| path.starts_with('/'))
        .map(ToOwned::to_owned)
        .collect()
}

/// 运行命令并返回非空输出行，供平台探测脚本复用。
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_stdout_lines(command: &mut Command) -> Vec<String> {
    command_stdout_trimmed(command)
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// 运行命令并返回去空白后的标准输出，命令失败时按无结果处理。
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_stdout_trimmed(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 在 WindowsApps 中选择版本最新的 Codex Desktop。
#[cfg(target_os = "windows")]
fn find_latest_windows_codex_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    collect_registry_codex_executable_candidates(&mut candidates);
    collect_path_codex_executable_candidates(&mut candidates);
    collect_windowsapps_codex_executable_candidates(&mut candidates);
    collect_appx_codex_executable_candidates(&mut candidates);
    collect_local_windows_codex_executable_candidates(&mut candidates);

    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.pop().map(|(_, executable)| executable)
}

/// 从 Windows App Paths 注册表读取显式应用路径，覆盖独立安装器注册的 Desktop。
#[cfg(target_os = "windows")]
fn collect_registry_codex_executable_candidates(candidates: &mut Vec<(Vec<u32>, PathBuf)>) {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for subkey in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Codex.exe",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\Codex.exe",
        ] {
            let Ok(key) = root.open_subkey(subkey) else {
                continue;
            };
            let Ok(path) = key.get_value::<String, _>("") else {
                continue;
            };
            push_codex_desktop_executable_candidate(candidates, Vec::new(), PathBuf::from(path));
        }
    }
}

/// 从 PATH 中查找大写 Desktop `Codex.exe`，兼容用户手动加入 PATH 的独立安装。
#[cfg(target_os = "windows")]
fn collect_path_codex_executable_candidates(candidates: &mut Vec<(Vec<u32>, PathBuf)>) {
    let script = r#"
Get-Command Codex.exe -All -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -ceq 'Codex.exe' -and $_.Source } |
  Select-Object -ExpandProperty Source |
  ConvertTo-Json -Compress
"#;
    for path in powershell_json_string_list(script) {
        push_codex_desktop_executable_candidate(candidates, Vec::new(), PathBuf::from(path));
    }
}

/// 扫描 WindowsApps 包目录，收集可启动 Codex App renderer 的 shell 候选。
#[cfg(target_os = "windows")]
fn collect_windowsapps_codex_executable_candidates(candidates: &mut Vec<(Vec<u32>, PathBuf)>) {
    let mut roots = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    if let Some(program_files) = std::env::var_os("ProgramW6432") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    roots.push(PathBuf::from(r"C:\Program Files\WindowsApps"));
    roots.sort();
    roots.dedup();

    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.contains("Codex") {
                continue;
            }
            for executable in windows_codex_package_executable_candidates(&path) {
                if push_codex_desktop_executable_candidate(
                    candidates,
                    version_tuple_from_package_name(name),
                    executable,
                ) {
                    break;
                }
            }
        }
    }
}

/// `Get-AppxPackage` 返回的 Codex Windows App 安装摘要。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsCodexAppxPackage {
    package_full_name: Option<String>,
    version: Option<String>,
    install_location: Option<String>,
}

/// 通过 MSIX 安装元数据寻找 Codex Desktop，覆盖 WindowsApps 目录不可枚举的场景。
#[cfg(target_os = "windows")]
fn collect_appx_codex_executable_candidates(candidates: &mut Vec<(Vec<u32>, PathBuf)>) {
    let script = r#"
$packages = @()
$packages += Get-AppxPackage -Name OpenAI.Codex -ErrorAction SilentlyContinue
$packages += Get-AppxPackage -Name *Codex* -ErrorAction SilentlyContinue
try { $packages += Get-AppxPackage -AllUsers -Name *Codex* -ErrorAction SilentlyContinue } catch {}
$packages |
  Where-Object { $_.InstallLocation -and ($_.Name -like '*Codex*' -or $_.PackageFullName -like '*Codex*' -or $_.PackageFamilyName -like '*Codex*') } |
  Sort-Object PackageFullName -Unique |
  Select-Object Name,PackageFullName,PackageFamilyName,Version,InstallLocation |
  ConvertTo-Json -Compress
"#;
    let Some(value) = powershell_json_value(script) else {
        return;
    };
    let packages = if value.is_array() {
        serde_json::from_value::<Vec<WindowsCodexAppxPackage>>(value)
    } else {
        serde_json::from_value::<WindowsCodexAppxPackage>(value).map(|package| vec![package])
    };
    let Ok(packages) = packages else {
        return;
    };
    for package in packages {
        let Some(root) = package
            .install_location
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
        else {
            continue;
        };
        let sort_key = package
            .version
            .as_deref()
            .map(version_tuple_from_text)
            .or_else(|| {
                package
                    .package_full_name
                    .as_deref()
                    .map(version_tuple_from_package_name)
                    .filter(|version| !version.is_empty())
            })
            .unwrap_or_default();
        for executable in windows_codex_package_executable_candidates(&root) {
            if push_codex_desktop_executable_candidate(candidates, sort_key.clone(), executable) {
                break;
            }
        }
    }
}

/// WindowsApps 包内的 Codex Desktop 可执行文件候选路径。
///
/// MSIX 包布局随版本变化过：新版统一应用壳是 `app/ChatGPT.exe`，旧版则可能在
/// `app/Codex.exe` 或 `app/resources/Codex.exe`。小写 `codex.exe` 仍是 CLI/app-server，
/// 不参与 renderer/CDP 修复链路。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_codex_package_executable_candidates(package_root: &Path) -> Vec<PathBuf> {
    let mut candidates = appx_manifest_codex_executable_candidates(package_root);
    candidates.extend([
        package_root.join("app").join("ChatGPT.exe"),
        package_root.join("app").join("Codex.exe"),
        package_root.join("app").join("resources").join("Codex.exe"),
        package_root.join("ChatGPT.exe"),
        package_root.join("Codex.exe"),
    ]);
    dedupe_paths(candidates)
}

#[cfg(target_os = "windows")]
fn collect_local_windows_codex_executable_candidates(candidates: &mut Vec<(Vec<u32>, PathBuf)>) {
    let mut roots = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let local_app_data = PathBuf::from(local_app_data);
        let root = local_app_data.join("OpenAI").join("Codex");
        roots.push(root.clone());
        roots.push(root.join("app"));
        roots.push(local_app_data.join("Programs").join("OpenAI").join("Codex"));
        roots.push(local_app_data.join("Programs").join("Codex"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let program_files = PathBuf::from(program_files);
        roots.push(program_files.join("OpenAI").join("Codex"));
        roots.push(program_files.join("Codex"));
    }
    if let Some(program_w6432) = std::env::var_os("ProgramW6432") {
        roots.push(PathBuf::from(program_w6432).join("OpenAI").join("Codex"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        roots.push(
            PathBuf::from(program_files_x86)
                .join("OpenAI")
                .join("Codex"),
        );
    }
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        roots.push(
            PathBuf::from(user_profile)
                .join("scoop")
                .join("apps")
                .join("codex")
                .join("current"),
        );
    }

    for root in roots {
        push_codex_desktop_executable_candidate(candidates, Vec::new(), root.join("Codex.exe"));
    }
}

/// 从 AppxManifest.xml 读取 Desktop 应用声明的 Executable，减少对包内部目录结构的猜测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn appx_manifest_codex_executable_candidates(package_root: &Path) -> Vec<PathBuf> {
    let manifest_path = package_root.join("AppxManifest.xml");
    let Ok(text) = std::fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    appx_manifest_codex_executable_values(&text)
        .into_iter()
        .map(|relative| package_root.join(relative.replace('/', "\\")))
        .collect()
}

/// 提取 manifest 中 Codex App shell 的相对路径；小写 CLI/app-server `codex.exe` 不参与。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn appx_manifest_codex_executable_values(text: &str) -> Vec<String> {
    let Ok(regex) = regex::Regex::new(r#"Executable\s*=\s*"([^"]+)""#) else {
        return Vec::new();
    };
    regex
        .captures_iter(text)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().trim()))
        .filter(|value| {
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "Codex.exe" || name == "ChatGPT.exe")
        })
        .map(ToOwned::to_owned)
        .collect()
}

/// 校验并加入候选，统一过滤小写 CLI/app-server 路径和不存在的路径。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn push_codex_desktop_executable_candidate(
    candidates: &mut Vec<(Vec<u32>, PathBuf)>,
    version: Vec<u32>,
    path: PathBuf,
) -> bool {
    let Ok(path) = canonical_codex_desktop_executable_path(&path) else {
        return false;
    };
    if !candidates
        .iter()
        .any(|(_, existing)| paths_equal_ignore_ascii_case(existing, &path))
    {
        candidates.push((version, path));
    }
    true
}

/// 去重候选路径，保留原始优先级顺序。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped
            .iter()
            .any(|existing: &PathBuf| paths_equal_ignore_ascii_case(existing, &path))
        {
            deduped.push(path);
        }
    }
    deduped
}

/// 执行 PowerShell 并解析 JSON 输出，统一处理空输出和脚本失败。
#[cfg(target_os = "windows")]
fn powershell_json_value(script: &str) -> Option<Value> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const POWERSHELL_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW);
    let output = command_output_with_timeout(&mut command, POWERSHELL_COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&stdout).ok()
}

/// Wait for a command without leaving a timed-out child behind.
///
/// Output pipes are drained concurrently so a verbose command cannot block on a
/// full pipe while this thread polls its process state.
pub(crate) fn command_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Option<Output> {
    use std::io::Read;
    use std::sync::mpsc;
    use std::thread;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take().expect("stdout was configured as piped");
    let mut stderr = child.stderr.take().expect("stderr was configured as piped");
    let Some(deadline) = Instant::now().checked_add(timeout) else {
        terminate_and_reap_child(&mut child);
        return None;
    };
    let (sender, receiver) = mpsc::channel();
    let stdout_sender = sender.clone();
    thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout_sender.send((true, stdout.read_to_end(&mut output).map(|_| output)));
    });
    thread::spawn(move || {
        let mut output = Vec::new();
        let _ = sender.send((false, stderr.read_to_end(&mut output).map(|_| output)));
    });

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // `try_wait` already observed termination. `wait` is retained
                // here to make the reap guarantee explicit for every exit path.
                let _ = child.wait();
                break status;
            }
            Ok(None) => {
                let now = Instant::now();
                if now >= deadline {
                    terminate_and_reap_child(&mut child);
                    return None;
                }
                thread::sleep(Duration::from_millis(20).min(deadline - now));
            }
            Err(_) => {
                terminate_and_reap_child(&mut child);
                return None;
            }
        }
    };

    // A descendant may inherit a pipe even after PowerShell exits. Bound the
    // output drain by the same deadline instead of joining a stuck reader.
    let mut stdout = None;
    let mut stderr = None;
    for _ in 0..2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (is_stdout, result) = receiver.recv_timeout(remaining).ok()?;
        if is_stdout {
            stdout = Some(result.ok()?);
        } else {
            stderr = Some(result.ok()?);
        }
    }
    Some(Output {
        status,
        stdout: stdout?,
        stderr: stderr?,
    })
}

fn terminate_and_reap_child(child: &mut Child) {
    // If the process exited between `try_wait` and `kill`, `wait` still reaps it.
    let _ = child.kill();
    let _ = child.wait();
}

/// 解析 PowerShell 输出的字符串或字符串数组 JSON。
#[cfg(target_os = "windows")]
fn powershell_json_string_list(script: &str) -> Vec<String> {
    match powershell_json_value(script) {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Windows 路径比较需要忽略大小写；非 Windows 测试也复用该规则验证候选去重。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn paths_equal_ignore_ascii_case(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

/// 从版本号文本解析排序 key，兼容 MSIX 包名和 `Get-AppxPackage Version` 字段。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn version_tuple_from_text(version: &str) -> Vec<u32> {
    version
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>()
}

/// 从 WindowsApps 包目录名解析版本号，供排序使用。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn version_tuple_from_package_name(name: &str) -> Vec<u32> {
    name.split('_')
        .nth(1)
        .map(version_tuple_from_text)
        .unwrap_or_default()
}

/// 探测已安装 Codex Desktop 的版本（MSIX 包版本，如 26.915.4065.0）。
///
/// 先扫 WindowsApps 包目录（纯 fs，快）；目录扫描拿不到版本时才回退
/// `Get-AppxPackage`。探测不到版本时返回 `None`；调用方必须按"版本未知"
/// 回落旧行为，不得当作旧版本处理。
#[cfg(target_os = "windows")]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn installed_codex_desktop_version() -> Option<Vec<u32>> {
    let mut candidates = Vec::new();
    collect_windowsapps_codex_executable_candidates(&mut candidates);
    if let Some(version) = candidates
        .iter()
        .filter(|(version, _)| !version.is_empty())
        .map(|(version, _)| version.clone())
        .max()
    {
        return Some(version);
    }
    let mut appx_candidates = Vec::new();
    collect_appx_codex_executable_candidates(&mut appx_candidates);
    appx_candidates
        .iter()
        .filter(|(version, _)| !version.is_empty())
        .map(|(version, _)| version.clone())
        .max()
}

/// 探测已安装 Codex Desktop 的版本（Info.plist `CFBundleShortVersionString`）。
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn installed_codex_desktop_version() -> Option<Vec<u32>> {
    macos_codex_common_bundle_candidates()
        .iter()
        .find_map(|bundle| {
            macos_bundle_info_value(bundle, "CFBundleShortVersionString").and_then(|version| {
                let version = version_tuple_from_text(&version);
                (!version.is_empty()).then_some(version)
            })
        })
}

/// 其他平台没有可靠的 Desktop 版本探测通道，返回 `None`（按版本未知处理）。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn installed_codex_desktop_version() -> Option<Vec<u32>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovery_contacts_isolated_ports_concurrently() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let arrived = std::sync::Arc::new(tokio::sync::Barrier::new(5));
        let mut ports = Vec::new();
        let mut servers = Vec::new();
        for index in 0..5 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            ports.push(port);
            let arrived = arrived.clone();
            servers.push(tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).await.unwrap();
                // No response is possible until all five production requests
                // arrive. A sequential implementation cannot pass this gate.
                arrived.wait().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                let body = json!([{"id":format!("page-{index}"),"type":"page","title":"Codex","url":"app://codex","webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/page")}]).to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }));
        }
        let result =
            tokio::time::timeout(Duration::from_secs(2), discover_codex_page_targets(&ports)).await;
        for server in servers {
            server.abort();
        }
        assert_eq!(
            result
                .expect("all ports must be discovered concurrently")
                .len(),
            5
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn native_release_drains_issued_effect_before_disposal() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _home = TestHomeGuard::new();
        let config_dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(config_dir).unwrap();
        std::fs::write(
            crate::codex_config::get_codex_config_path(),
            "model_provider='fixture'\n[model_providers.fixture]\nbase_url='http://127.0.0.1:1'\n",
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("ws://127.0.0.1:{port}/page");
        *TEST_CDP_PORTS.lock().unwrap() = Some(vec![port]);
        *TEST_CODEX_RUNNING.lock().unwrap() = Some(false);
        let issued = std::sync::Arc::new(tokio::sync::Notify::new());
        let settle = std::sync::Arc::new(tokio::sync::Notify::new());
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (issued_server, settle_server, events_server) =
            (issued.clone(), settle.clone(), events.clone());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let issued = issued_server.clone();
                let settle = settle_server.clone();
                let events = events_server.clone();
                tokio::spawn(async move {
                    let mut peek = [0; 4096];
                    let n = socket.peek(&mut peek).await.unwrap();
                    if !String::from_utf8_lossy(&peek[..n])
                        .to_ascii_lowercase()
                        .contains("upgrade: websocket")
                    {
                        socket.read(&mut peek).await.unwrap();
                        let body = json!([{"id":"fixture","type":"page","title":"Codex","url":"app://codex","webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/page")}]).to_string();
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                        return;
                    }
                    let mut websocket = tokio_tungstenite::accept_async(socket).await.unwrap();
                    while let Some(Ok(message)) = websocket.next().await {
                        let Ok(text) = message.to_text() else {
                            continue;
                        };
                        let request: Value = serde_json::from_str(text).unwrap();
                        let method = request["method"].as_str().unwrap();
                        let expression = request["params"]["expression"].as_str().unwrap_or("");
                        let result = if method == "Page.addScriptToEvaluateOnNewDocument" {
                            json!({"identifier":"fixture-script"})
                        } else if method == "Runtime.evaluate" {
                            let value = if expression.contains("fixtureEffect") {
                                events.lock().unwrap().push("issued");
                                issued.notify_one();
                                settle.notified().await;
                                events.lock().unwrap().push("settled");
                                json!({})
                            } else if expression.contains("owned:") {
                                json!({"owned":true,"registrations":[],"timezoneOwned":false})
                            } else if expression.contains("current + 1") {
                                json!(1)
                            } else if expression.contains("currentEpoch + 1") {
                                events.lock().unwrap().push("disposed");
                                json!({"disposed":true,"incomplete":false})
                            } else {
                                json!({})
                            };
                            json!({"result":{"value":value}})
                        } else {
                            json!({})
                        };
                        if websocket
                            .send(Message::Text(
                                json!({"id":request["id"],"result":result})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
        });
        let install_url = url.clone();
        let install = tokio::spawn(async move {
            install_script(&install_url, "({fixtureEffect:true})", None).await
        });
        tokio::time::timeout(Duration::from_secs(3), issued.notified())
            .await
            .expect("issued real Runtime.evaluate");
        cancel_codex_renderer_work();
        let release = tokio::spawn(release_codex_model_picker_projection());
        tokio::time::sleep(Duration::from_millis(100)).await;
        let released_early = release.is_finished();
        settle.notify_one();
        let installed = install.await.unwrap();
        let released = tokio::time::timeout(Duration::from_secs(3), release).await;
        server.abort();
        *TEST_CDP_PORTS.lock().unwrap() = None;
        *TEST_CODEX_RUNNING.lock().unwrap() = None;
        assert!(
            !released_early,
            "native disposal must wait for the already issued effect"
        );
        installed.unwrap();
        released.unwrap().unwrap().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec!["issued", "settled", "disposed"]
        );
        assert!(!has_owned_codex_renderer_state());
    }

    struct TestHomeGuard {
        _dir: tempfile::TempDir,
        original: Option<std::ffi::OsString>,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create isolated desktop home");
            let original = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                original,
            }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn seed_desktop_projection_files(config: Option<&str>) {
        let dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&dir).expect("create isolated codex directory");
        let config_path = crate::codex_config::get_codex_config_path();
        if let Some(config) = config {
            std::fs::write(config_path, config).expect("write live config");
        } else if config_path.exists() {
            std::fs::remove_file(config_path).expect("remove live config");
        }
        std::fs::write(
            crate::codex_config::get_codex_model_catalog_path(),
            json!({ "models": [{ "model": "stale-routed-model" }] }).to_string(),
        )
        .expect("write stale catalog");
        std::fs::write(
            dir.join("models_cache.json"),
            json!({
                "etag": "cc-switch-model-catalog",
                "models": [{ "model": "stale-cached-model" }]
            })
            .to_string(),
        )
        .expect("write stale owned cache");
    }

    #[test]
    #[serial_test::serial]
    fn native_official_live_loader_rejects_residual_projection_sources() {
        let _home = TestHomeGuard::new();
        for provider in ["", "model_provider = \"openai\"\n"] {
            let config = format!(
                "model = \"gpt-5.5\"\n{provider}\n[model_providers.custom]\nbase_url = \"http://127.0.0.1:15721/v1\"\nmodels = [{{ model = \"inactive-routed-model\" }}]\n"
            );
            seed_desktop_projection_files(Some(&config));
            assert!(
                load_cc_switch_model_catalog_projection().is_err(),
                "native live config must not authorize a residual catalog, cache, or inactive provider: {provider:?}"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_official_desktop_loader_rejects_projection_without_valid_live_config() {
        let _home = TestHomeGuard::new();
        for config in [None, Some("model_provider = [invalid")] {
            seed_desktop_projection_files(config);
            assert!(
                load_cc_switch_model_catalog_projection().is_err(),
                "unreadable live intent must not authorize stale routed models"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_official_desktop_loader_rejects_external_catalog_as_injection_authority() {
        let _home = TestHomeGuard::new();
        for catalog in [
            "external-models.json",
            "../outside/cc-switch-model-catalog.json",
        ] {
            seed_desktop_projection_files(Some(&format!(
                "model_provider = \"openai\"\nmodel_catalog_json = \"{catalog}\"\n"
            )));
            assert!(
                load_cc_switch_model_catalog_projection().is_err(),
                "a user-owned catalog must not authorize residual CCSwitchMulti projection: {catalog}"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_official_desktop_loader_keeps_explicit_openai_catalog_projection() {
        let _home = TestHomeGuard::new();
        seed_desktop_projection_files(Some(
            "model = \"stale-routed-model\"\nmodel_provider = \"openai\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n",
        ));
        let projection = load_cc_switch_model_catalog_projection()
            .expect("explicit managed OpenAI catalog projection remains supported");
        assert_eq!(projection.model_names, vec!["stale-routed-model"]);
        assert_eq!(projection.model_provider.as_deref(), Some("openai"));
    }

    #[test]
    #[serial_test::serial]
    fn native_official_desktop_loader_keeps_active_router_inline_catalog_fallback() {
        let _home = TestHomeGuard::new();
        seed_desktop_projection_files(Some(
            "model = \"router-model\"\nmodel_provider = \"codex_model_router_v2\"\n\n[model_providers.codex_model_router_v2]\nbase_url = \"http://127.0.0.1:15721/v1\"\nmodels = [{ model = \"router-model\" }]\n",
        ));
        std::fs::remove_file(crate::codex_config::get_codex_model_catalog_path())
            .expect("remove JSON catalog to exercise inline fallback");
        std::fs::remove_file(crate::codex_config::get_codex_config_dir().join("models_cache.json"))
            .expect("remove cache to exercise inline fallback");
        let projection = load_cc_switch_model_catalog_projection()
            .expect("active managed router inline projection remains supported");
        assert_eq!(projection.model_names, vec!["router-model"]);
        assert_eq!(
            projection.model_provider.as_deref(),
            Some("codex_model_router_v2")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn native_official_unlock_skips_cdp_process_and_launch_actions() {
        let _home = TestHomeGuard::new();
        for config in [
            Some("model = \"gpt-5.5\"\n"),
            Some("model_provider = \"openai\"\n"),
            None,
            Some("model_provider = [invalid"),
        ] {
            seed_desktop_projection_files(config);
            let result = unlock_codex_model_picker()
                .await
                .expect("native or unreadable live intent exits before desktop process control");
            assert!(!result.injected);
            assert!(!result.launched);
            assert!(result.attempted_ports.is_empty());
            assert!(result.model_names.is_empty());
            assert!(!result.history_sync_requested);
        }
    }

    /// 返回当前测试平台的 Desktop 主程序文件名。
    fn desktop_test_executable_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "Codex.exe"
        } else {
            "Codex"
        }
    }

    /// 返回当前测试平台应被拒绝的小写 CLI/app-server 文件名。
    fn cli_test_executable_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "codex.exe"
        } else {
            "codex"
        }
    }

    #[test]
    fn codex_startup_toggle_is_explicit_and_idempotent() {
        assert!(!should_launch_codex_desktop_with_ccswitch(false, false));
        assert!(!should_launch_codex_desktop_with_ccswitch(false, true));
        assert!(should_launch_codex_desktop_with_ccswitch(true, false));
        assert!(!should_launch_codex_desktop_with_ccswitch(true, true));
    }

    #[test]
    fn codex_launch_timezone_is_scoped_to_the_child_command() {
        let mut command = Command::new("codex-desktop-placeholder");
        apply_codex_launch_timezone(&mut command, Some("Asia/Taipei"));

        let timezone = command
            .get_envs()
            .find_map(|(key, value)| {
                (key == "TZ").then(|| value.map(|value| value.to_string_lossy().to_string()))
            })
            .flatten();
        assert_eq!(timezone.as_deref(), Some("Asia/Taipei"));

        let mut disabled = Command::new("codex-desktop-placeholder");
        apply_codex_launch_timezone(&mut disabled, None);
        assert!(!disabled.get_envs().any(|(key, _)| key == "TZ"));
    }

    #[test]
    fn codex_runtime_timezone_is_read_from_the_renderer_value() {
        let evaluation = json!({
            "result": {
                "result": {
                    "value": {
                        "timezone": "Asia/Taipei",
                        "utcOffsetMinutes": 480
                    }
                }
            }
        });

        let timezone =
            codex_runtime_timezone_from_evaluation(&evaluation).expect("valid runtime timezone");
        assert_eq!(timezone.timezone, "Asia/Taipei");
        assert_eq!(timezone.utc_offset, "+08:00");
    }

    #[test]
    fn codex_renderer_timezone_override_uses_the_configured_iana_identifier() {
        assert_eq!(
            codex_timezone_override_parameters("America/Los_Angeles"),
            json!({ "timezoneId": "America/Los_Angeles" })
        );
    }

    /// 按当前平台构造一个能通过 Desktop 主程序校验的测试文件。
    ///
    /// Windows/Linux 是文件名级校验，直接写文件即可；macOS 要求 bundle 元数据
    /// （CFBundleIdentifier=com.openai.codex + CFBundleExecutable），构造最小
    /// Codex.app 结构（44b9de42 起 macOS 校验以 bundle 身份为准）。
    fn write_desktop_test_executable(dir: &Path) -> PathBuf {
        let name = desktop_test_executable_name();
        if cfg!(target_os = "macos") {
            let bundle = dir.join("Codex.app");
            let contents = bundle.join("Contents");
            let macos_dir = contents.join("MacOS");
            std::fs::create_dir_all(&macos_dir).expect("create fake bundle MacOS dir");
            std::fs::write(
                contents.join("Info.plist"),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{name}</string>
    <key>CFBundleIdentifier</key>
    <string>{CODEX_DESKTOP_BUNDLE_IDENTIFIER}</string>
</dict>
</plist>
"#
                ),
            )
            .expect("write fake bundle Info.plist");
            let executable = macos_dir.join(name);
            std::fs::write(&executable, "").expect("write fake bundle executable");
            executable
        } else {
            let executable = dir.join(name);
            std::fs::write(&executable, "").expect("write desktop exe");
            executable
        }
    }

    #[test]
    fn catalog_projection_accepts_model_and_slug_fields() {
        let value = json!({
            "models": [
                { "model": "qwen3.6", "display_name": "Qwen 3.6" },
                { "slug": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash" },
                { "id": "gpt-5.4-mini" }
            ]
        });
        let (names, models) = codex_model_entries_from_catalog_value(&value);
        assert_eq!(names, vec!["qwen3.6", "deepseek-v4-flash", "gpt-5.4-mini"]);
        assert!(models.iter().all(|model| model["hidden"] == false));
        assert_eq!(models[0]["displayName"], "Qwen 3.6");
    }

    #[test]
    fn catalog_projection_does_not_invent_unknown_reasoning_defaults() {
        let value = json!({
            "models": [{ "model": "unknown-third-party", "display_name": "Unknown" }]
        });
        let (_, models) = codex_model_entries_from_catalog_value(&value);

        assert!(models[0].get("defaultReasoningEffort").is_none());
        assert!(models[0].get("supportedReasoningEfforts").is_none());
    }

    #[test]
    fn native_official_disabled_model_projection_preserves_native_model_gate() {
        let result = run_model_picker_patch_core_probe(
            r#"
state.modelProjectionEnabled = false;
const gate = {
  models: [{ model: "official-model", hidden: true }],
  availableModels: ["official-model"],
  hiddenModels: ["qwen3.6"],
  useHiddenModels: true,
  default_model: "official-model"
};
const before = JSON.stringify(gate);
const changed = patchModelContainer(gate);
JSON.stringify({ changed, unchanged: JSON.stringify(gate) === before });
"#,
        );
        assert_eq!(result["changed"], false);
        assert_eq!(result["unchanged"], true);
    }

    #[test]
    fn native_official_dispose_restores_owned_renderer_hooks_and_model_changes() {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            r#"
(async () => {{
const window = globalThis;
const patchKey = "{MODEL_PICKER_PATCH_KEY}";
let timerCleared = false;
let observerDisconnected = false;
let listenerRemoved = false;
const clearInterval = () => {{ timerCleared = true; }};
window.removeEventListener = (name, listener, capture) => {{ listenerRemoved = name === "message" && listener === state.messageListener && capture; }};
const state = {{ payload: {{ modelNames: ["routed-model"] }}, interval: 7, reactRootObserver: {{ disconnect() {{ observerDisconnected = true; }} }}, messageListener() {{}} }};
window[patchKey] = state;
{lifecycle}
{model_core}
const originalSendRequest = () => "native";
const client = {{ sendRequest: originalSendRequest }};
installCompatibilityHook(client, "sendRequest", () => "managed");
installCompatibilityHook(client, "__ccSwitchModelRequestPatch", "9");
const gate = {{ availableModels: ["official-model"], models: [{{ model: "routed-model", hidden: true }}], hiddenModels: ["routed-model"], useHiddenModels: true, default_model: "official-model" }};
patchModelContainer(gate);
gate.availableModels.push("new-official-model");
gate.default_model = "new-official-model";
const released = await {release};
const changedAfterDispose = patchModelContainer(gate);
globalThis.__rendererDisposeResult = JSON.stringify({{ released, timerCleared, observerDisconnected, listenerRemoved, restoredMethod: client.sendRequest === originalSendRequest, removedMarker: !("__ccSwitchModelRequestPatch" in client), removedState: !window[patchKey], changedAfterDispose, gate }});
}})();
"#,
            lifecycle = model_picker_lifecycle_core_script(),
            model_core = model_picker_patch_core_script(),
            release = build_model_picker_release_script(),
        );
        context.with(|ctx| {
            ctx.eval::<(), _>(script).expect("start renderer disposal");
        });
        while runtime
            .execute_pending_job()
            .expect("drain renderer disposal jobs")
        {}
        let result: Value = context.with(|ctx| {
            let output: String = ctx
                .eval("globalThis.__rendererDisposeResult")
                .expect("execute renderer disposal");
            serde_json::from_str(&output).expect("parse renderer disposal result")
        });
        for key in [
            "timerCleared",
            "observerDisconnected",
            "listenerRemoved",
            "restoredMethod",
            "removedMarker",
            "removedState",
        ] {
            assert_eq!(result[key], true, "dispose must satisfy {key}");
        }
        assert_eq!(result["changedAfterDispose"], false);
        assert_eq!(
            result["gate"],
            json!({
                "availableModels": ["official-model", "new-official-model"],
                "models": [{ "model": "routed-model", "hidden": true }],
                "hiddenModels": ["routed-model"],
                "useHiddenModels": true,
                "default_model": "new-official-model"
            })
        );
    }

    fn run_renderer_history_disposal_probe(
        preserve_native_update: bool,
        cancel_before_start: bool,
    ) -> Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            r#"
(async () => {{
const window = globalThis;
const patchKey = "{MODEL_PICKER_PATCH_KEY}";
const clearInterval = () => {{}};
window.removeEventListener = () => {{}};
const state = {{ payload: {{}} }};
window[patchKey] = state;
{lifecycle}
{history}
let conversation = {{ id: "synthetic-thread", rolloutPath: "old.jsonl", turns: [{{id: "old-turn"}}], resumeState: "resumed" }};
let mutations = 0;
let completeLoads = 0;
let finishResume;
const manager = {{
  getCachedConversations: () => [conversation],
  getConversation: () => conversation,
  isConversationStreaming: () => false,
  threadStore: {{ threadsById: new Map([["synthetic-thread", {{path: "active.jsonl"}}]]) }},
  updateConversationState: (_id, update) => {{ mutations += 1; const next = {{...conversation}}; update(next); conversation = next; }},
  resumeConversationForUnavailableOwner: () => new Promise((resolve) => {{ finishResume = () => resolve({{status: "ready"}}); }}),
  getCompleteConversationTurns: async () => {{ completeLoads += 1; }},
}};
if ({cancel_before_start}) await state.dispose();
const hydration = rehydrateStaleHistoryLineages(manager);
state.historyLineageHydrationPromise = hydration;
let released = false;
const disposal = Promise.resolve(state.dispose()).then(() => {{ released = true; }});
await Promise.resolve();
const releasedBeforeRequestSettled = released;
if ({preserve_native_update}) {{
  conversation.turns = [{{id: "native-turn"}}];
  conversation.resumeState = "native-resumed";
  conversation.rolloutPath = "native.jsonl";
}}
if (finishResume) finishResume();
await hydration;
await disposal;
globalThis.__rendererHistoryDisposeResult = JSON.stringify({{releasedBeforeRequestSettled, mutations, completeLoads, conversation}});
}})();
"#,
            lifecycle = model_picker_lifecycle_core_script(),
            history = history_lineage_hydration_core_script(),
        );
        context.with(|ctx| {
            ctx.eval::<(), _>(script)
                .expect("start history disposal probe");
        });
        while runtime
            .execute_pending_job()
            .expect("drain history disposal jobs")
        {}
        context.with(|ctx| {
            let output: String = ctx
                .eval("globalThis.__rendererHistoryDisposeResult")
                .expect("read history disposal result");
            serde_json::from_str(&output).expect("parse history disposal result")
        })
    }

    #[test]
    fn native_official_dispose_drains_history_and_restores_owned_pending_lineage() {
        let result = run_renderer_history_disposal_probe(false, false);
        assert_eq!(result["releasedBeforeRequestSettled"], false);
        assert_eq!(result["completeLoads"], 0);
        assert_eq!(
            result["conversation"],
            json!({
                "id": "synthetic-thread", "rolloutPath": "old.jsonl", "turns": [{"id": "old-turn"}], "resumeState": "resumed"
            })
        );
    }

    #[test]
    fn native_official_dispose_preserves_a_superseding_native_history_update() {
        let result = run_renderer_history_disposal_probe(true, false);
        assert_eq!(result["completeLoads"], 0);
        assert_eq!(result["conversation"]["rolloutPath"], "native.jsonl");
        assert_eq!(
            result["conversation"]["turns"],
            json!([{"id": "native-turn"}])
        );
        assert_eq!(result["conversation"]["resumeState"], "native-resumed");
    }

    #[test]
    fn native_official_disposed_history_work_cannot_start_or_mutate_a_conversation() {
        let result = run_renderer_history_disposal_probe(false, true);
        assert_eq!(result["mutations"], 0);
        assert_eq!(result["completeLoads"], 0);
    }

    #[test]
    fn native_official_release_requires_completed_renderer_evidence() {
        assert!(verify_codex_renderer_release(
            &json!({"result":{"result":{"type":"object","subtype":"promise"}}})
        )
        .is_err());
        assert!(verify_codex_renderer_release(
            &json!({"result":{"result":{"value":{"disposed":true,"incomplete":true}}}})
        )
        .is_err());
        assert!(verify_codex_renderer_release(
            &json!({"result":{"result":{"value":{"disposed":false,"incomplete":false}}}})
        )
        .is_ok());
        assert!(verify_codex_renderer_release(
            &json!({"result":{"result":{"value":{"disposed":true,"incomplete":false}}}})
        )
        .is_ok());
    }

    #[test]
    fn native_official_launch_command_has_no_ccsm_debug_or_timezone_overrides() {
        let command = native_codex_launch_command(Path::new("codex-desktop-placeholder"));
        assert_eq!(command.get_args().count(), 0);
        assert!(!command.get_envs().any(|(key, _)| key == "TZ"));
    }

    #[test]
    fn native_official_document_release_removes_only_owned_registrations_and_resets_timezone() {
        let ordinary = codex_document_release_plan(&json!({"owned":false,"registrations":[]}), &[]);
        assert_eq!(ordinary, (false, BTreeSet::new(), false));
        let owned = codex_document_release_plan(
            &json!({"owned":true,"registrations":[["reload-nonce",null],["persisted-nonce","script-2"]]}),
            &[("reload-nonce".to_string(), "script-1".to_string())],
        );
        assert_eq!(
            owned,
            (
                true,
                BTreeSet::from(["script-1".to_string(), "script-2".to_string()]),
                false
            )
        );
        let unknown = codex_document_release_plan(
            &json!({"owned":true,"registrations":[["lost-nonce",null]]}),
            &[],
        );
        assert_eq!(unknown, (true, BTreeSet::new(), true));
        assert_eq!(
            codex_timezone_override_parameters(""),
            json!({"timezoneId":""})
        );
    }

    #[test]
    /// 主目录为空时，投影 helper 应允许调用方继续使用 models cache 或内联目录回退。
    fn catalog_projection_skips_empty_source_and_accepts_fallback_source() {
        let empty = json!({ "models": [] });
        assert!(
            codex_model_catalog_projection_from_value(&empty, None, None).is_none(),
            "empty primary catalog must not freeze an empty renderer payload"
        );

        let fallback = json!({
            "models": [
                { "model": "gpt-5.6-sol", "display_name": "GPT-5.6-Sol" },
                { "model": "qwen3.6", "display_name": "Qwen3.6 Local" }
            ]
        });
        let projection = codex_model_catalog_projection_from_value(
            &fallback,
            Some("qwen3.6".to_string()),
            Some("codex_model_router_v2".to_string()),
        )
        .expect("fallback catalog projection");

        assert_eq!(
            projection.model_names,
            vec!["gpt-5.6-sol".to_string(), "qwen3.6".to_string()]
        );
        assert_eq!(projection.default_model.as_deref(), Some("qwen3.6"));
    }

    fn run_model_picker_patch_core_probe(probe: &str) -> serde_json::Value {
        run_model_picker_patch_core_probe_with_payload(
            json!({
                "defaultModel": "qwen3.6",
                "modelNames": ["qwen3.6", "deepseek-v4-flash"],
                "models": [
                    {"model": "qwen3.6", "displayName": "Qwen 3.6", "hidden": false},
                    {"model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash", "hidden": false}
                ]
            }),
            probe,
        )
    }

    fn run_model_picker_patch_core_probe_with_payload(
        payload: Value,
        probe: &str,
    ) -> serde_json::Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            "const payload = {};\nconst state = {{ payload }};\n{}\n{}",
            payload,
            model_picker_patch_core_script(),
            probe
        );

        context.with(|ctx| {
            let json_text: String = ctx.eval(script).expect("execute model picker patch core");
            serde_json::from_str(&json_text).expect("parse model picker patch probe result")
        })
    }

    fn run_guardian_v2_compatibility_probe(probe: &str) -> serde_json::Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            "{}\n{}",
            guardian_v2_compatibility_patch_core_script(),
            probe
        );

        context.with(|ctx| {
            let json_text: String = ctx
                .eval(script)
                .expect("execute Guardian V2 compatibility core");
            serde_json::from_str(&json_text).expect("parse Guardian V2 compatibility result")
        })
    }

    fn run_app_server_request_normalization_probe_with_payload(
        payload: Value,
        probe: &str,
    ) -> serde_json::Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            "const payload = {};\nconst state = {{ payload }};\n{}\n{}",
            payload,
            app_server_request_normalization_core_script(),
            probe
        );

        context.with(|ctx| {
            let json_text: String = ctx
                .eval(script)
                .expect("execute app-server request normalization core");
            serde_json::from_str(&json_text).expect("parse app-server request normalization result")
        })
    }

    fn run_history_lineage_hydration_probe(probe: &str) -> serde_json::Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!("{}\n{}", history_lineage_hydration_core_script(), probe);

        context.with(|ctx| {
            let json_text: String = ctx
                .eval(script)
                .expect("execute history lineage hydration core");
            serde_json::from_str(&json_text).expect("parse history lineage hydration result")
        })
    }

    fn run_async_history_lineage_hydration_probe(probe: &str) -> serde_json::Value {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!("{}\n{}", history_lineage_hydration_core_script(), probe);
        context.with(|ctx| {
            ctx.eval::<(), _>(script)
                .expect("start async history lineage hydration probe");
        });
        while runtime
            .execute_pending_job()
            .expect("execute history lineage hydration job")
        {}
        context.with(|ctx| {
            let json_text: String = ctx
                .eval("globalThis.__historyLineageResult")
                .expect("read async history lineage hydration result");
            serde_json::from_str(&json_text).expect("parse async history lineage hydration result")
        })
    }

    #[test]
    fn history_lineage_hydration_repairs_only_idle_conversations_with_a_changed_rollout_path() {
        let result = run_history_lineage_hydration_probe(
            r#"
const conversations = [
  {id: "idle-stale", rolloutPath: "root.jsonl", turns: []},
  {id: "running-stale", rolloutPath: "root.jsonl", turns: [{status: "inProgress"}]},
  {id: "current", rolloutPath: "active.jsonl", turns: []},
];
const manager = {
  getCachedConversations: () => conversations,
  isConversationStreaming: (id) => id === "running-stale",
  threadStore: {threadsById: new Map([
    ["idle-stale", {path: "branch.jsonl"}],
    ["running-stale", {path: "branch.jsonl"}],
    ["current", {path: "active.jsonl"}],
  ])},
};
JSON.stringify(planHistoryLineageRehydration(manager));
"#,
        );

        assert_eq!(result["repair"], json!(["idle-stale"]));
        assert_eq!(result["deferred"], json!(["running-stale"]));
    }

    #[test]
    fn history_lineage_hydration_repairs_fragmented_canonical_history_without_path_drift() {
        let result = run_history_lineage_hydration_probe(
            r#"
const fragmentedHistory = {
  kind: "canonical",
  history: {
    isComplete: false,
    islands: [
      {id: "old", entries: []},
      {id: "tail", entries: []},
    ],
    entitiesByKey: {},
  },
};
const normalTail = {
  kind: "canonical",
  history: {
    isComplete: false,
    islands: [{id: "tail", entries: []}],
    entitiesByKey: {},
  },
};
const activeFragmentedHistory = {
  kind: "canonical",
  history: {
    isComplete: false,
    islands: [{id: "old", entries: []}, {id: "tail", entries: []}],
    entitiesByKey: {running: {status: "inProgress"}},
  },
};
const conversations = [
  {id: "fragmented", rolloutPath: "active.jsonl", turns: [], turnHistory: fragmentedHistory},
  {id: "normal-tail", rolloutPath: "active.jsonl", turns: [], turnHistory: normalTail},
  {id: "active-fragmented", rolloutPath: "active.jsonl", turns: [], turnHistory: activeFragmentedHistory},
];
const manager = {
  getCachedConversations: () => conversations,
  isConversationStreaming: () => false,
  threadStore: {threadsById: new Map([
    ["fragmented", {path: "active.jsonl"}],
    ["normal-tail", {path: "active.jsonl"}],
    ["active-fragmented", {path: "active.jsonl"}],
  ])},
};
JSON.stringify(planHistoryLineageRehydration(manager));
"#,
        );

        assert_eq!(result["repair"], json!(["fragmented"]));
        assert_eq!(result["deferred"], json!(["active-fragmented"]));
    }

    #[test]
    fn history_lineage_hydration_discards_canonical_cache_and_loads_complete_history() {
        let result = run_async_history_lineage_hydration_probe(
            r#"
let conversation = {
  id: "fragmented",
  cwd: "C:/workspace",
  rolloutPath: "active.jsonl",
  turns: [{status: "completed", id: "legacy-overlay"}],
  turnHistory: {
    kind: "canonical",
    history: {
      isComplete: false,
      islands: [{id: "old", entries: []}, {id: "tail", entries: []}],
      entitiesByKey: {},
    },
  },
  resumeState: "resumed",
};
let turnHistoryPresentDuringResume = null;
let completeHistoryLoads = 0;
const manager = {
  getCachedConversations: () => [conversation],
  getConversation: () => conversation,
  isConversationStreaming: () => false,
  updateConversationState: (_id, update) => {
    const next = JSON.parse(JSON.stringify(conversation));
    update(next);
    conversation = next;
  },
  setConversation: (value) => { conversation = value; },
  resumeConversationForUnavailableOwner: async () => {
    turnHistoryPresentDuringResume = conversation.turnHistory != null;
    conversation.resumeState = "resumed";
    return {status: "ready"};
  },
  getCompleteConversationTurns: async () => { completeHistoryLoads += 1; return []; },
  threadStore: {threadsById: new Map([["fragmented", {path: "active.jsonl"}]])},
};
rehydrateStaleHistoryLineages(manager).then((outcome) => {
  globalThis.__historyLineageResult = JSON.stringify({
    outcome,
    turnHistoryPresentDuringResume,
    completeHistoryLoads,
  });
});
"#,
        );

        assert_eq!(result["outcome"]["repaired"], json!(["fragmented"]));
        assert_eq!(result["turnHistoryPresentDuringResume"], false);
        assert_eq!(result["completeHistoryLoads"], 1);
    }

    #[test]
    fn history_lineage_hydration_uses_codex_resume_without_evicting_the_conversation() {
        let result = run_async_history_lineage_hydration_probe(
            r#"
let conversation = {
  id: "idle-stale",
  cwd: "C:/workspace",
  rolloutPath: "root.jsonl",
  turns: [{status: "completed"}],
  latestCollaborationMode: {mode: "default"},
  resumeState: "resumed",
};
let resumeParams = null;
let evictionCalled = false;
const manager = {
  getCachedConversations: () => [conversation],
  getConversation: () => conversation,
  isConversationStreaming: () => false,
  updateConversationState: (_id, update) => {
    const next = JSON.parse(JSON.stringify(conversation));
    update(next);
    conversation = next;
  },
  setConversation: (value) => { conversation = value; },
  resumeConversationForUnavailableOwner: async (params) => { resumeParams = params; },
  threadStore: {
    threadsById: new Map([["idle-stale", {path: "branch.jsonl"}]]),
    removeConversationFromCache: () => { evictionCalled = true; },
  },
};
rehydrateStaleHistoryLineages(manager).then((outcome) => {
  globalThis.__historyLineageResult = JSON.stringify({
    outcome,
    conversation,
    resumeParams,
    evictionCalled,
  });
});
"#,
        );

        assert_eq!(result["outcome"]["repaired"], json!(["idle-stale"]));
        assert_eq!(result["conversation"]["rolloutPath"], "branch.jsonl");
        assert_eq!(result["conversation"]["turns"], json!([]));
        assert_eq!(result["conversation"]["resumeState"], "needs_resume");
        assert_eq!(
            result["resumeParams"]["workspaceRoots"],
            json!(["C:/workspace"])
        );
        assert_eq!(result["evictionCalled"], false);
    }

    #[test]
    fn history_lineage_hydration_restores_the_cached_transcript_when_resume_fails() {
        let result = run_async_history_lineage_hydration_probe(
            r#"
let conversation = {
  id: "idle-stale",
  cwd: "C:/workspace",
  rolloutPath: "root.jsonl",
  turns: [{status: "completed", id: "old-turn"}],
  latestCollaborationMode: {mode: "default"},
  resumeState: "resumed",
};
const manager = {
  getCachedConversations: () => [conversation],
  getConversation: () => conversation,
  isConversationStreaming: () => false,
  updateConversationState: (_id, update) => {
    const next = JSON.parse(JSON.stringify(conversation));
    update(next);
    conversation = next;
  },
  setConversation: (value) => { conversation = value; },
  resumeConversationForUnavailableOwner: async () => { throw new Error("resume failed"); },
  threadStore: {threadsById: new Map([["idle-stale", {path: "branch.jsonl"}]])},
};
rehydrateStaleHistoryLineages(manager).then((outcome) => {
  globalThis.__historyLineageResult = JSON.stringify({outcome, conversation});
});
"#,
        );

        assert_eq!(result["outcome"]["repaired"], json!([]));
        assert_eq!(result["outcome"]["failed"][0]["id"], "idle-stale");
        assert_eq!(result["conversation"]["rolloutPath"], "root.jsonl");
        assert_eq!(
            result["conversation"]["turns"],
            json!([{ "status": "completed", "id": "old-turn" }])
        );
        assert_eq!(result["conversation"]["resumeState"], "resumed");
    }

    #[test]
    fn codex_app_request_normalization_routes_resumed_threads_through_active_provider() {
        let result = run_app_server_request_normalization_probe_with_payload(
            json!({ "modelProvider": "codex_model_router_v2" }),
            r#"
const directOriginal = { threadId: "thread-1", model: "gpt-5.6-sol", modelProvider: "openai" };
const wrappedOriginal = {
  method: "thread/resume",
  params: { threadId: "thread-2", model: "qwen3.8", modelProvider: "openai" },
  hostId: "local",
};
const unrelatedOriginal = { threadId: "thread-3", modelProvider: "openai" };
const direct = normalizeAppServerRequestParams("thread/resume", directOriginal);
const wrapped = normalizeAppServerRequestParams("send-cli-request-for-host", wrappedOriginal);
const unrelated = normalizeAppServerRequestParams("thread/read", unrelatedOriginal);
JSON.stringify({
  direct,
  wrapped,
  unrelated,
  directOriginal,
  wrappedOriginal,
  unrelatedIdentityPreserved: unrelated === unrelatedOriginal,
});
"#,
        );

        assert_eq!(
            result["direct"],
            json!({
                "threadId": "thread-1",
                "model": "gpt-5.6-sol",
                "modelProvider": "codex_model_router_v2"
            })
        );
        assert_eq!(
            result["wrapped"],
            json!({
                "method": "thread/resume",
                "params": {
                    "threadId": "thread-2",
                    "model": "qwen3.8",
                    "modelProvider": "codex_model_router_v2"
                },
                "hostId": "local"
            })
        );
        assert_eq!(
            result["unrelated"],
            json!({
                "threadId": "thread-3",
                "modelProvider": "openai"
            })
        );
        assert_eq!(result["directOriginal"]["modelProvider"], "openai");
        assert_eq!(
            result["wrappedOriginal"]["params"]["modelProvider"],
            "openai"
        );
        assert_eq!(result["unrelatedIdentityPreserved"], true);
    }

    #[test]
    fn codex_app_request_normalization_does_not_invent_a_resume_provider() {
        let result = run_app_server_request_normalization_probe_with_payload(
            json!({ "modelProvider": null }),
            r#"
const directOriginal = { threadId: "thread-1", modelProvider: "openai" };
const direct = normalizeAppServerRequestParams("thread/resume", directOriginal);
const list = normalizeAppServerRequestParams("thread/list", { modelProviders: null, limit: 20 });
const wrappedList = normalizeAppServerRequestParams("send-cli-request-for-host", {
  method: "thread/list",
  params: { modelProviders: null, limit: 50 },
});
JSON.stringify({
  direct,
  directIdentityPreserved: direct === directOriginal,
  list,
  wrappedList,
});
"#,
        );

        assert_eq!(
            result["direct"],
            json!({
                "threadId": "thread-1",
                "modelProvider": "openai"
            })
        );
        assert_eq!(result["directIdentityPreserved"], true);
        assert_eq!(result["list"], json!({ "modelProviders": [], "limit": 20 }));
        assert_eq!(
            result["wrappedList"],
            json!({
                "method": "thread/list",
                "params": { "modelProviders": [], "limit": 50 }
            })
        );
    }

    #[test]
    fn codex_app_request_normalization_replaces_the_previous_request_wrapper() {
        let result = run_app_server_request_normalization_probe_with_payload(
            json!({ "modelProvider": "codex_model_router_v2" }),
            r#"
JSON.stringify({
  v5NeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "5" }),
  v6NeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "6" }),
  v7NeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "7" }),
  v8NeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "8" }),
  v9NeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "9" }),
  currentNeedsPatch: appServerRequestClientNeedsPatch({ __ccSwitchModelRequestPatch: "10" }),
  missingNeedsPatch: appServerRequestClientNeedsPatch({}),
  patchVersion: appServerRequestPatchVersion,
});
"#,
        );

        assert_eq!(result["v5NeedsPatch"], true);
        assert_eq!(result["v6NeedsPatch"], true);
        assert_eq!(result["v7NeedsPatch"], true);
        assert_eq!(result["v8NeedsPatch"], true);
        assert_eq!(result["v9NeedsPatch"], true);
        assert_eq!(result["currentNeedsPatch"], false);
        assert_eq!(result["missingNeedsPatch"], true);
        assert_eq!(result["patchVersion"], "10");
    }

    #[test]
    fn guardian_v2_compatibility_downgrades_only_the_rejected_request_override() {
        let result = run_guardian_v2_compatibility_probe(
            r#"
const direct = {
  threadId: "thread-1",
  config: {
    "features.guardianv2": { enabled: true, future_field: "unsupported" },
    "features.memories": true,
  },
};
const wrapped = {
  method: "thread/resume",
  params: {
    threadId: "thread-2",
    config: { features: { guardianv2: { enabled: false, future_field: 1 }, goals: true } },
  },
};
const directFallback = guardianV2FallbackRequestParams("thread/resume", direct);
const wrappedFallback = guardianV2FallbackRequestParams("send-cli-request-for-host", wrapped);
JSON.stringify({
  directValue: directFallback.config["features.guardianv2"],
  directMemories: directFallback.config["features.memories"],
  directOriginalStillObject: typeof direct.config["features.guardianv2"] === "object",
  wrappedValue: wrappedFallback.params.config.features.guardianv2,
  wrappedGoals: wrappedFallback.params.config.features.goals,
  wrappedOriginalStillObject: typeof wrapped.params.config.features.guardianv2 === "object",
  unrelatedIdentityPreserved: guardianV2FallbackRequestParams("thread/resume", {config: {"features.memories": true}}).config["features.memories"],
});
"#,
        );

        assert_eq!(result["directValue"], true);
        assert_eq!(result["directMemories"], true);
        assert_eq!(result["directOriginalStillObject"], true);
        assert_eq!(result["wrappedValue"], false);
        assert_eq!(result["wrappedGoals"], true);
        assert_eq!(result["wrappedOriginalStillObject"], true);
        assert_eq!(result["unrelatedIdentityPreserved"], true);
    }

    #[test]
    fn guardian_v2_compatibility_retries_only_the_exact_feature_schema_failure() {
        let result = run_guardian_v2_compatibility_probe(
            r#"
JSON.stringify({
  exact: isGuardianV2FeatureTomlError({message: "failed to load configuration: data did not match any variant of untagged enum FeatureToml\nin `features.guardianv2`"}),
  otherFeature: isGuardianV2FeatureTomlError({message: "FeatureToml in `features.memories`"}),
  network: isGuardianV2FeatureTomlError({message: "connection reset"}),
});
"#,
        );

        assert_eq!(result["exact"], true);
        assert_eq!(result["otherFeature"], false);
        assert_eq!(result["network"], false);
    }

    #[test]
    fn guardian_v2_statsig_config_removes_only_the_legacy_model_whitelist_pollution() {
        let result = run_guardian_v2_compatibility_probe(
            r#"
const source = {value: {
  enabled: true,
  mode: "future",
  nested: {unknown: true},
  available_models: ["qwen3.8"],
  use_hidden_models: false,
  default_model: "qwen3.8",
}};
const patched = patchGuardianV2DynamicConfig("2553103476", source);
const unrelated = {value: {enabled: true, mode: "keep"}};
JSON.stringify({
  patchedValue: patched.value,
  sourceValue: source.value,
  unrelatedIdentity: patchGuardianV2DynamicConfig("other", unrelated) === unrelated,
});
"#,
        );

        assert_eq!(
            result["patchedValue"],
            json!({ "enabled": true, "mode": "future", "nested": { "unknown": true } })
        );
        assert_eq!(
            result["sourceValue"],
            json!({
                "enabled": true,
                "mode": "future",
                "nested": { "unknown": true },
                "available_models": ["qwen3.8"],
                "use_hidden_models": false,
                "default_model": "qwen3.8"
            })
        );
        assert_eq!(result["unrelatedIdentity"], true);
    }

    #[test]
    fn statsig_model_whitelist_fields_are_scoped_to_their_own_dynamic_config() {
        let result = run_guardian_v2_compatibility_probe(
            r#"
const model = {value: {available_models: []}};
const unrelated = {value: {enabled: true}};
const legacy = {value: {enabled: true, available_models: ["qwen3.8"], default_model: "qwen3.8", use_hidden_models: false}};
const modelPrepared = prepareStatsigDynamicConfig("107580212", model, false);
const unrelatedPrepared = prepareStatsigDynamicConfig("unrelated", unrelated, false);
const repairedPrepared = prepareStatsigDynamicConfig("unrelated", legacy, true);
JSON.stringify({
  modelPatch: modelPrepared.patchModelWhitelist,
  unrelatedPatch: unrelatedPrepared.patchModelWhitelist,
  unrelatedIdentity: unrelatedPrepared.config === unrelated,
  repairedPatch: repairedPrepared.patchModelWhitelist,
  repairedValue: repairedPrepared.config.value,
  legacyOriginalKeys: Object.keys(legacy.value).sort(),
});
"#,
        );
        assert_eq!(result["modelPatch"], true);
        assert_eq!(result["unrelatedPatch"], false);
        assert_eq!(result["unrelatedIdentity"], true);
        assert_eq!(result["repairedPatch"], false);
        assert_eq!(result["repairedValue"], json!({ "enabled": true }));
        assert_eq!(
            result["legacyOriginalKeys"],
            json!([
                "available_models",
                "default_model",
                "enabled",
                "use_hidden_models"
            ])
        );

        let script = build_model_picker_unlock_script(&CodexModelCatalogProjection::empty());

        assert!(script.contains("const prepared = prepareStatsigDynamicConfig"));
        assert!(script.contains("client.__ccSwitchStatsigPatchVersion !== \"2\""));
        assert!(script.contains("client.__ccSwitchModelWhitelistPatched === true"));
    }

    #[test]
    fn model_picker_fallback_descriptor_does_not_invent_reasoning_capability() {
        let result = run_model_picker_patch_core_probe_with_payload(
            json!({
                "defaultModel": "unknown-third-party",
                "modelNames": ["unknown-third-party"],
                "models": []
            }),
            r#"
const descriptor = descriptorFor("unknown-third-party");
JSON.stringify({
  hasDefault: Object.prototype.hasOwnProperty.call(descriptor, "defaultReasoningEffort"),
  hasEfforts: Object.prototype.hasOwnProperty.call(descriptor, "supportedReasoningEfforts"),
});
"#,
        );

        assert_eq!(result["hasDefault"], false);
        assert_eq!(result["hasEfforts"], false);
    }

    /// 回归：模型解锁器不能再给 React Intl 的 formats/defaultFormats/formatters
    /// 写入 defaultModel/useHiddenModels，否则 MessageFormat 缓存会因循环引用失败并回退
    /// 为未插值的 ICU source。
    #[test]
    fn model_picker_patch_core_does_not_pollute_react_intl_objects() {
        let result = run_model_picker_patch_core_probe(
            r#"
const intl = {
  formats: {number: {compact: {notation: "compact"}}},
  defaultFormats: {date: {short: {year: "numeric"}}},
  formatters: {cache: {message: {}}},
};
const before = JSON.stringify(intl);
const changed = [intl, intl.formats, intl.defaultFormats, intl.formatters]
  .map((value) => patchModelContainer(value));
JSON.stringify({before, after: JSON.stringify(intl), changed});
"#,
        );

        assert_eq!(result["before"], result["after"]);
        assert_eq!(result["changed"], json!([false, false, false, false]));
    }

    /// 已验证的模型门仍应补齐目录、解除隐藏并设置已有的默认模型字段；同时不能
    /// 凭空向 camelCase 容器追加 snake_case 控制字段。
    #[test]
    fn model_picker_patch_core_still_unlocks_explicit_model_gate() {
        let result = run_model_picker_patch_core_probe(
            r#"
const gate = {availableModels: ["gpt-5.6-sol"], useHiddenModels: true, defaultModel: null};
const changed = patchModelContainer(gate);
JSON.stringify({
  changed,
  availableModels: gate.availableModels,
  useHiddenModels: gate.useHiddenModels,
  defaultModel: gate.defaultModel?.model,
  hasSnakeCaseFlag: Object.prototype.hasOwnProperty.call(gate, "use_hidden_models"),
});
"#,
        );

        assert_eq!(result["changed"], true);
        assert_eq!(
            result["availableModels"],
            json!(["gpt-5.6-sol", "qwen3.6", "deepseek-v4-flash"])
        );
        assert_eq!(result["useHiddenModels"], false);
        assert_eq!(result["defaultModel"], "qwen3.6");
        assert_eq!(result["hasSnakeCaseFlag"], false);
    }

    #[test]
    fn model_picker_patch_core_uses_latest_shared_catalog_payload() {
        let result = run_model_picker_patch_core_probe_with_payload(
            json!({
                "defaultModel": "qwen3.8",
                "modelNames": ["qwen3.8"],
                "models": [{ "model": "qwen3.8", "hidden": false }]
            }),
            r#"
const first = [];
patchModelArray(first, true);
state.payload = {
  defaultModel: "deepseek-v4-flash",
  modelNames: ["deepseek-v4-flash", "deepseek-v4-vision"],
  models: [
    {model: "deepseek-v4-flash", hidden: false},
    {model: "deepseek-v4-vision", inputModalities: ["text", "image"], hidden: false},
  ],
};
const second = [];
patchModelArray(second, true);
JSON.stringify({
  first: first.map((model) => model.model),
  second: second.map((model) => model.model),
  modalities: second.find((model) => model.model === "deepseek-v4-vision")?.inputModalities,
});
"#,
        );

        assert_eq!(result["first"], json!(["qwen3.8"]));
        assert_eq!(
            result["second"],
            json!(["deepseek-v4-flash", "deepseek-v4-vision"])
        );
        assert_eq!(result["modalities"], json!(["text", "image"]));
    }

    #[test]
    fn model_picker_unlock_script_patches_renderer_whitelists() {
        let catalog = CodexModelCatalogProjection {
            default_model: Some("qwen3.6".to_string()),
            model_provider: Some("codex_model_router_v2".to_string()),
            model_names: vec!["qwen3.6".to_string(), "deepseek-v4-flash".to_string()],
            models: vec![json!({ "model": "qwen3.6", "hidden": false })],
        };
        let script = build_model_picker_unlock_script(&catalog);
        assert!(script.contains("qwen3.6"));
        assert!(script.contains("deepseek-v4-flash"));
        assert!(script.contains("available_models"));
        assert!(script.contains("use_hidden_models: false"));
        assert!(script.contains("107580212"));
        assert!(script.contains("list-models-for-host"));
        assert!(script.contains("model/list"));
        assert!(script.contains("patchModelArray(result.data, true)"));
        assert!(script.contains("isModelListMethod(method)"));
        assert!(script.contains("await installAppServerPatch()"));
        assert!(script.contains("!state.requestIds.has(requestId)"));
        assert!(script.contains("__ccSwitchCodexAppCompatibilityV10"));
        assert!(script.contains("appServerRequestPatchVersion = \"10\""));
        assert!(script.contains("rehydrateStaleHistoryLineages(manager)"));
        assert!(
            script.contains("state.reactRequestClientPatchVersion = appServerRequestPatchVersion")
        );
        assert!(
            !script.contains("if (state.appServerPatchVersion !== appServerRequestPatchVersion)")
        );
        assert!(!script.contains(
            "if (state.reactRequestClientPatchVersion === appServerRequestPatchVersion) return"
        ));
        assert!(!script
            .contains("if (state.appServerPatchVersion === appServerRequestPatchVersion) return"));
        assert!(script.contains("2553103476"));
        assert!(script.contains("isGuardianV2FeatureTomlError(error)"));
        assert!(script.contains("guardianV2FallbackRequestParams"));
        assert!(script.contains("auth.setAuthMethod(\"chatgpt\")"));
    }

    /// 验证新版历史兼容层同时恢复官方目录同步，并把 Desktop 的全量历史查询
    /// 明确投影为 `modelProviders: []`，不再受活动 Provider 桶影响。
    #[test]
    fn codex_app_compatibility_script_repairs_native_history_catalog() {
        let script = build_model_picker_unlock_script(&CodexModelCatalogProjection::empty());

        assert!(script.contains("localThreadCatalog"));
        assert!(script.contains("requestStartupSync"));
        assert!(script.contains("readStatus"));
        assert!(script.contains("const historySync = await triggerLocalThreadCatalogSync()"));
        assert!(script.contains("params?.modelProviders == null"));
        assert!(script.contains("if (params == null) return { modelProviders: [] }"));
        assert!(script.contains("modelProviders: []"));
        assert!(script.contains("getCompatibleThreadSortKey"));
        assert!(script.contains("document.getElementById(\"root\")"));
        assert!(!script.contains("document.querySelectorAll(\"*\")"));
        assert!(script.contains("module?.appServices?.localThreadCatalog"));
        assert!(script.contains("fetch(url)"));
        assert!(script.contains("await state.historyQueryRefreshPromise"));
        assert!(script.contains("state.historyQueryRefreshCompleted = true"));
    }

    /// The renderer heartbeat must not synchronously walk every DOM node and the
    /// complete React object graph. New app-server clients are discovered from
    /// the cached module on every heartbeat; React traversal is an incremental,
    /// idle-time fallback with one in-flight job and an observer-backed dirty set.
    #[test]
    fn codex_app_compatibility_heartbeat_does_not_repeat_full_react_scan() {
        let script = build_model_picker_unlock_script(&CodexModelCatalogProjection::empty());

        assert!(script.contains("const rendererSchedulerVersion = \"2\""));
        assert!(script.contains("clearInterval(state.interval)"));
        assert!(script.contains("const patchAppServerModuleClients"));
        assert!(script.contains("state.reactDiscoveryPromise"));
        assert!(script.contains("requestIdleCallback"));
        assert!(script.contains("const sliceDeadline = monotonicNow() + 4"));
        assert!(script.contains("sliceCount < 800"));
        assert!(script.contains("state.lastReactFullScanAt || 0) >= 30000"));
        assert!(script.contains("state.lastReactIncrementalScanAt || 0) >= 5000"));
        assert!(script.contains("new MutationObserver"));
        assert!(script.contains("modulePatched = patchAppServerModuleClients(module)"));
        assert!(!script.contains("document.querySelectorAll(\"*\")"));
        assert!(
            !script.contains("if (state.appServerPatchVersion !== appServerRequestPatchVersion)")
        );
        assert!(!script.contains("patched += patchReactAppServerClients();"));
        assert!(!script.contains(
            "if (state.historyQueryRefreshCompleted) patched += patchReactAppServerClients()"
        ));
    }

    #[test]
    fn app_server_module_discovery_patches_a_replaced_signal_client() {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = format!(
            r#"
const patched = [];
const patchRequestClient = (client) => {{
  if (!client || typeof client.sendRequest !== "function") return false;
  patched.push(client.id);
  return true;
}};
{}
let current = {{ id: "first", sendRequest() {{}} }};
const module = {{ signal: {{ get: () => current }} }};
const firstCount = patchAppServerModuleClients(module);
current = {{ id: "second", sendRequest() {{}} }};
const secondCount = patchAppServerModuleClients(module);
JSON.stringify({{ firstCount, secondCount, patched }});
"#,
            app_server_module_client_discovery_core_script()
        );

        context.with(|ctx| {
            let json_text: String = ctx.eval(script).expect("execute module discovery core");
            let result: Value =
                serde_json::from_str(&json_text).expect("parse module discovery result");
            assert_eq!(result["firstCount"], 1);
            assert_eq!(result["secondCount"], 1);
            assert_eq!(result["patched"], json!(["first", "second"]));
        });
    }

    #[test]
    fn codex_app_compatibility_script_is_valid_javascript() {
        let runtime = rquickjs::Runtime::new().expect("create JavaScript runtime");
        let context = rquickjs::Context::full(&runtime).expect("create JavaScript context");
        let script = build_model_picker_unlock_script(&CodexModelCatalogProjection::empty());
        let encoded = serde_json::to_string(&script).expect("encode renderer script");

        context.with(|ctx| {
            let parsed: bool = ctx
                .eval(format!("new Function({encoded}); true"))
                .expect("parse renderer compatibility script");
            assert!(parsed);
        });
    }

    /// 验证 CDP 返回值能准确区分“脚本已安装”和“原生历史同步已请求”。
    #[test]
    fn compatibility_evidence_reads_native_history_sync_result() {
        let evaluation = json!({
            "result": {
                "result": {
                    "value": {
                        "historySync": {
                            "requested": true,
                            "complete": false,
                            "afterCount": 17
                        },
                        "historyQueryPatch": {
                            "patched": true,
                            "refreshRequested": true
                        }
                    }
                }
            }
        });

        let evidence = codex_app_compatibility_evidence_from_evaluation(&evaluation)
            .expect("compatibility evidence");
        assert!(evidence.history_sync_requested);
        assert_eq!(evidence.history_catalog_complete, Some(false));
        assert_eq!(evidence.history_catalog_count, Some(17));
        assert!(evidence.all_provider_history_patched);
        assert!(evidence.history_refresh_requested);
    }

    #[test]
    fn compatibility_evidence_does_not_report_history_ready_after_hydration_failure() {
        let evaluation = json!({
            "result": {
                "result": {
                    "value": {
                        "historySync": {"requested": true, "complete": true},
                        "historyQueryPatch": {
                            "patched": true,
                            "refreshRequested": true,
                            "lineageHydration": {
                                "repaired": [],
                                "deferred": [],
                                "failed": [{"id": "thread-1", "error": "resume failed"}]
                            }
                        }
                    }
                }
            }
        });

        let evidence = codex_app_compatibility_evidence_from_evaluation(&evaluation)
            .expect("compatibility evidence");
        assert!(evidence.all_provider_history_patched);
        assert!(!evidence.history_refresh_requested);
    }

    /// 验证 Desktop 可执行文件缺失时，错误信息不会被误读成不支持 CLI/app-server。
    #[test]
    fn desktop_executable_missing_message_preserves_cli_support_boundary() {
        let message = codex_desktop_executable_not_found_message();
        assert!(message.contains("Desktop menu unlock flow"));
        assert!(message.contains("will not launch CLI/app-server codex as an Electron shell"));
        assert!(message.contains("Codex CLI/app-server is still supported"));
        assert!(message.contains("model_catalog_json"));
        assert!(message.contains("/v1/models"));
    }

    #[test]
    fn shared_cdp_port_requires_explicit_codex_target() {
        let targets = vec![CdpTarget {
            id: "chrome-page".to_string(),
            target_type: "page".to_string(),
            title: "Chrome".to_string(),
            url: "https://example.com".to_string(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/page/1".to_string()),
        }];

        assert!(pick_codex_page_targets(&targets, 9222).is_err());
    }

    #[test]
    fn windows_codex_package_candidates_cover_known_desktop_layouts() {
        let root = PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex_26.608.1.0_x64__id");
        let candidates = windows_codex_package_executable_candidates(&root);

        assert!(candidates.contains(&root.join("app").join("ChatGPT.exe")));
        assert!(candidates.contains(&root.join("app").join("Codex.exe")));
        assert!(candidates.contains(&root.join("app").join("resources").join("Codex.exe")));
        assert!(!candidates.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "codex.exe")
        }));
    }

    /// 验证 Appx manifest 里的新版 `ChatGPT.exe` 与旧版 `Codex.exe` 都会进入候选。
    #[test]
    fn appx_manifest_candidates_use_declared_desktop_executable() {
        let package_dir = tempfile::tempdir().expect("create appx package temp dir");
        std::fs::write(
            package_dir.path().join("AppxManifest.xml"),
            r#"
<Package>
  <Applications>
    <Application Id="UnifiedApp" Executable="app/ChatGPT.exe" EntryPoint="Windows.FullTrustApplication" />
    <Application Id="App" Executable="app/bin/Codex.exe" EntryPoint="Windows.FullTrustApplication" />
    <Application Id="Cli" Executable="app/resources/codex.exe" EntryPoint="Windows.FullTrustApplication" />
  </Applications>
</Package>
"#,
        )
        .expect("write appx manifest");

        let candidates = windows_codex_package_executable_candidates(package_dir.path());

        assert_eq!(candidates[0], package_dir.path().join(r"app\ChatGPT.exe"));
        assert!(candidates.contains(&package_dir.path().join(r"app\bin\Codex.exe")));
        assert!(!candidates.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "codex.exe")
        }));
    }

    /// 验证 MSIX/Appx 包名版本解析兼容稳定版和预览版包名。
    #[test]
    fn codex_desktop_version_sort_keys_parse_package_and_appx_versions() {
        assert_eq!(
            version_tuple_from_package_name("OpenAI.Codex_26.623.141536.0_x64__2p2nqsd0c76g0"),
            vec![26, 623, 141536, 0]
        );
        assert_eq!(
            version_tuple_from_package_name("OpenAI.Codex.Preview_27.1.2.3_x64__2p2nqsd0c76g0"),
            vec![27, 1, 2, 3]
        );
        assert_eq!(
            version_tuple_from_text("26.623.141536.0"),
            vec![26, 623, 141536, 0]
        );
        assert!(version_tuple_from_package_name("Other.Package.Without.Version").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn powershell_timeout_terminates_and_reaps_stalled_child() {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut command = Command::new("powershell");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .creation_flags(CREATE_NO_WINDOW);

        let started = Instant::now();
        let output = command_output_with_timeout(&mut command, Duration::from_secs(1));

        assert!(output.is_none(), "stalled PowerShell should time out");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed-out PowerShell child must be terminated and reaped promptly"
        );
    }

    /// 验证只接受平台 Desktop shell，避免把 CLI/app-server `codex` 用于 renderer 解锁。
    #[test]
    fn codex_desktop_executable_validation_rejects_cli_launcher() {
        let desktop_dir = tempfile::tempdir().expect("create desktop temp dir");
        let desktop = write_desktop_test_executable(desktop_dir.path());
        let resolved = canonical_codex_desktop_executable_path(&desktop)
            .expect("platform Desktop executable should be accepted");
        assert!(resolved.ends_with(desktop_test_executable_name()));

        let cli_dir = tempfile::tempdir().expect("create cli temp dir");
        let cli = cli_dir.path().join(cli_test_executable_name());
        std::fs::write(&cli, "").expect("write cli exe");
        let error = canonical_codex_desktop_executable_path(&cli)
            .expect_err("lowercase CLI codex should be rejected");
        assert!(error.contains("not Codex Desktop"));
    }

    /// 验证新版通用壳只有位于 OpenAI.Codex 包目录时才会被接入 Codex 修复链路。
    #[cfg(target_os = "windows")]
    #[test]
    fn unified_chatgpt_shell_requires_codex_msix_ancestor() {
        let package_dir = tempfile::tempdir().expect("create windows apps temp dir");
        let package_root = package_dir
            .path()
            .join("OpenAI.Codex_26.707.3748.0_x64__2p2nqsd0c76g0");
        let shell = package_root.join("app").join("ChatGPT.exe");
        std::fs::create_dir_all(shell.parent().expect("unified shell parent"))
            .expect("create unified shell parent");
        std::fs::write(&shell, "").expect("write unified shell");

        let resolved = canonical_codex_desktop_executable_path(&shell)
            .expect("OpenAI.Codex unified shell should be accepted");
        assert!(resolved.ends_with(Path::new("app").join("ChatGPT.exe")));

        let unrelated = package_dir.path().join("ChatGPT.exe");
        std::fs::write(&unrelated, "").expect("write unrelated ChatGPT shell");
        assert!(canonical_codex_desktop_executable_path(&unrelated).is_err());
    }

    /// 验证运行中进程探测也按可执行文件名精确区分 Desktop 和小写 app-server。
    #[cfg(target_os = "windows")]
    #[test]
    fn codex_main_process_probe_filters_cli_launcher_name_case() {
        assert!(DETECT_CODEX_MAIN_PROCESS_SCRIPT.contains("$legacyCodex = $leaf -ceq 'Codex.exe'"));
        assert!(DETECT_CODEX_MAIN_PROCESS_SCRIPT.contains("Name = 'ChatGPT.exe'"));
        assert!(DETECT_CODEX_MAIN_PROCESS_SCRIPT.contains("OpenAI\\.Codex"));
        assert!(!DETECT_CODEX_MAIN_PROCESS_SCRIPT.contains(" -ieq 'Codex.exe'"));
    }

    /// 验证已确认的 Desktop 路径能写入状态文件并再次读回。
    #[test]
    fn remembered_codex_desktop_executable_round_trips_confirmed_path() {
        let desktop_dir = tempfile::tempdir().expect("create confirmed desktop temp dir");
        let desktop = write_desktop_test_executable(desktop_dir.path());
        let state_dir = tempfile::tempdir().expect("create state temp dir");
        let state_path = state_dir
            .path()
            .join(REMEMBERED_CODEX_DESKTOP_EXECUTABLE_FILENAME);

        let remembered = remember_codex_desktop_executable_at(&state_path, &desktop)
            .expect("remember confirmed desktop path");
        let loaded = read_remembered_codex_desktop_executable_from(&state_path)
            .expect("read remembered desktop path");

        assert_eq!(loaded, remembered);
        assert!(loaded.ends_with(desktop_test_executable_name()));
    }

    /// 旧 Codex.app 和统一 ChatGPT.app 都必须按 bundle 元数据解析主程序，不能硬编码壳名称。
    #[test]
    fn macos_codex_bundle_metadata_accepts_legacy_and_unified_shells() {
        assert_eq!(
            macos_codex_bundle_executable_from_metadata(
                Path::new("/Applications/Codex.app"),
                "com.openai.codex",
                "Codex",
            ),
            Some(PathBuf::from(
                "/Applications/Codex.app/Contents/MacOS/Codex"
            ))
        );
        assert_eq!(
            macos_codex_bundle_executable_from_metadata(
                Path::new("/Applications/ChatGPT.app"),
                "com.openai.codex",
                "ChatGPT",
            ),
            Some(PathBuf::from(
                "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"
            ))
        );
    }

    /// 独立 ChatGPT 即使目录和主程序同名，也不能进入 Codex Desktop/CDP 链路。
    #[test]
    fn macos_chatgpt_bundle_rejects_non_codex_identity_and_unsafe_executable_names() {
        assert_eq!(
            macos_codex_bundle_executable_from_metadata(
                Path::new("/Applications/ChatGPT.app"),
                "com.openai.chat",
                "ChatGPT",
            ),
            None
        );
        assert_eq!(
            macos_codex_bundle_executable_from_metadata(
                Path::new("/Applications/ChatGPT.app"),
                "com.openai.codex",
                "../Codex",
            ),
            None
        );
    }

    /// 反查 bundle 时兼容两代包装，但拒绝 Framework 内部 Service.app 和无关应用。
    #[test]
    fn macos_codex_bundle_ancestor_accepts_only_supported_top_level_shells() {
        assert_eq!(
            macos_codex_bundle_ancestor(Path::new("/Applications/Codex.app/Contents/MacOS/Codex")),
            Some(PathBuf::from("/Applications/Codex.app"))
        );
        assert_eq!(
            macos_codex_bundle_ancestor(Path::new(
                "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"
            )),
            Some(PathBuf::from("/Applications/ChatGPT.app"))
        );
        assert_eq!(
            macos_codex_bundle_ancestor(Path::new("/Applications/Other.app/Contents/MacOS/Other")),
            None
        );
    }

    /// Spotlight 不可用时也必须从系统和用户 Applications 目录发现统一壳。
    #[test]
    fn macos_common_candidates_include_legacy_and_unified_bundles() {
        let candidates = macos_codex_common_bundle_candidates();
        assert!(candidates.contains(&PathBuf::from("/Applications/Codex.app")));
        assert!(candidates.contains(&PathBuf::from("/Applications/ChatGPT.app")));
        if let Some(home) = std::env::var_os("HOME") {
            let applications = PathBuf::from(home).join("Applications");
            assert!(candidates.contains(&applications.join("Codex.app")));
            assert!(candidates.contains(&applications.join("ChatGPT.app")));
        }
    }

    /// 验证 Linux desktop entry 只提取绝对路径 Exec，忽略包装命令。
    #[test]
    fn linux_desktop_entry_exec_parser_keeps_absolute_exec_paths() {
        let values = linux_desktop_entry_exec_values(
            "Name=Codex\nExec=/opt/Codex/Codex --no-sandbox %U\nExec=flatpak run com.openai.Codex\n",
        );
        assert_eq!(values, vec!["/opt/Codex/Codex"]);
    }

    #[test]
    fn default_cdp_port_allows_initializing_codex_page() {
        let targets = vec![CdpTarget {
            id: "codex-page".to_string(),
            target_type: "page".to_string(),
            title: String::new(),
            url: String::new(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9229/devtools/page/1".to_string()),
        }];

        let targets = pick_codex_page_targets(&targets, DEFAULT_CODEX_DEBUG_PORT)
            .expect("default Codex CDP port should allow early blank targets");
        assert_eq!(targets[0].id, "codex-page");
    }

    #[test]
    fn codex_cdp_port_injects_all_matching_pages() {
        let targets = vec![
            CdpTarget {
                id: "codex-main".to_string(),
                target_type: "page".to_string(),
                title: "Codex".to_string(),
                url: "app://codex/index.html".to_string(),
                web_socket_debugger_url: Some("ws://127.0.0.1:9229/devtools/page/1".to_string()),
            },
            CdpTarget {
                id: "codex-dialog".to_string(),
                target_type: "page".to_string(),
                title: "Codex".to_string(),
                url: "app://codex/dialog.html".to_string(),
                web_socket_debugger_url: Some("ws://127.0.0.1:9229/devtools/page/2".to_string()),
            },
        ];

        let picked = pick_codex_page_targets(&targets, DEFAULT_CODEX_DEBUG_PORT)
            .expect("all Codex targets should be injectable");
        assert_eq!(
            picked
                .iter()
                .map(|target| target.id.as_str())
                .collect::<Vec<_>>(),
            vec!["codex-main", "codex-dialog"]
        );
    }
}
