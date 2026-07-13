use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_CODEX_DEBUG_PORT: u16 = 9229;
const CDP_HTTP_TIMEOUT: Duration = Duration::from_secs(2);
const CDP_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const MODEL_PICKER_PATCH_KEY: &str = "__ccSwitchCodexAppCompatibilityV4";
const REMEMBERED_CODEX_DESKTOP_EXECUTABLE_FILENAME: &str = "codex-desktop-executable.json";
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
    pub message: String,
}

/// renderer 脚本返回的新版历史目录同步证据。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CodexAppCompatibilityEvidence {
    history_sync_requested: bool,
    history_catalog_complete: Option<bool>,
    history_catalog_count: Option<usize>,
}

/// 注入脚本需要的模型目录投影，避免把整个 catalog 私有字段泄漏到 renderer。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexModelCatalogProjection {
    default_model: Option<String>,
    model_names: Vec<String>,
    models: Vec<Value>,
}

impl CodexModelCatalogProjection {
    /// 构造不含路由模型的兼容投影，使历史目录修复不依赖 CCSM 模型目录是否已生成。
    fn empty() -> Self {
        Self {
            default_model: None,
            model_names: Vec::new(),
            models: Vec::new(),
        }
    }
}

/// Chrome DevTools Protocol `/json` 返回的页面 target 摘要。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct CdpTarget {
    id: String,
    #[serde(rename = "type")]
    target_type: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
}

/// 尝试为当前或新启动的 Codex App 安装 renderer 兼容层。
///
/// 副作用：
/// - 若发现已开放 CDP 的 Codex renderer，会触发新版历史目录同步并修复模型菜单；
/// - 若未发现 CDP 且 Codex 未运行，会以 remote debugging 参数启动 Codex。
pub async fn unlock_codex_model_picker() -> Result<CodexModelPickerUnlockResult, String> {
    let catalog = load_cc_switch_model_catalog_projection()
        .unwrap_or_else(|_| CodexModelCatalogProjection::empty());
    let attempted_ports = candidate_debug_ports(DEFAULT_CODEX_DEBUG_PORT);

    if let Some(result) = try_inject_on_candidate_ports(&catalog, &attempted_ports).await {
        return Ok(result);
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
            message: "Codex App is already running without an injectable CDP port. Fully quit Codex, then launch it from CCSwitchMulti so the history catalog and model compatibility layer can be installed.".to_string(),
        });
    }

    let executable =
        resolve_codex_executable().ok_or_else(codex_desktop_executable_not_found_message)?;
    launch_codex_with_debug_port(&executable, DEFAULT_CODEX_DEBUG_PORT)?;

    let mut last_result = None;
    for _ in 0..30 {
        if let Some(result) =
            try_inject_on_candidate_ports(&catalog, &[DEFAULT_CODEX_DEBUG_PORT]).await
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
            message: "Codex was launched with remote debugging; waiting for the renderer target."
                .to_string(),
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
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
        message: "Codex was launched, but no injectable renderer target appeared before timeout."
            .to_string(),
    }))
}

/// 返回当前平台的 Desktop 可执行文件发现失败说明，避免 Windows-only 文案误导 macOS/Linux 用户。
fn codex_desktop_executable_not_found_message() -> String {
    let platform_sources = if cfg!(target_os = "windows") {
        "running Desktop process, remembered Desktop path, MSIX/Appx package metadata and manifest, App Paths registry entries, PATH commands, and common local install folders"
    } else if cfg!(target_os = "macos") {
        "running Desktop process, remembered Desktop path, /Applications, ~/Applications, and Spotlight-discovered Codex.app bundles"
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
fn load_cc_switch_model_catalog_projection() -> Result<CodexModelCatalogProjection, String> {
    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    let catalog = crate::config::read_json_file(&catalog_path)
        .map_err(|error| format!("Failed to read {}: {error}", catalog_path.display()))?;
    let default_model = read_current_codex_default_model();
    let (model_names, models) = codex_model_entries_from_catalog_value(&catalog);
    if model_names.is_empty() {
        return Err(format!(
            "No models were found in {}",
            catalog_path.display()
        ));
    }
    Ok(CodexModelCatalogProjection {
        default_model: default_model.or_else(|| model_names.first().cloned()),
        model_names,
        models,
    })
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
    object
        .entry("defaultReasoningEffort".to_string())
        .or_insert_with(|| Value::String("medium".to_string()));
    Value::Object(object)
}

/// 读取当前 Codex 默认模型，用于 renderer 动态配置的 default_model。
fn read_current_codex_default_model() -> Option<String> {
    let text = crate::codex_config::read_codex_config_text().ok()?;
    let parsed = text.parse::<toml::Value>().ok()?;
    parsed
        .get("model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 在候选 CDP 端口中寻找 Codex renderer 并安装模型白名单补丁。
async fn try_inject_on_candidate_ports(
    catalog: &CodexModelCatalogProjection,
    ports: &[u16],
) -> Option<CodexModelPickerUnlockResult> {
    for port in ports {
        let targets = match list_cdp_targets(*port).await {
            Ok(targets) => targets,
            Err(_) => continue,
        };
        let targets = match pick_codex_page_targets(&targets, *port) {
            Ok(targets) => targets,
            Err(_) => continue,
        };
        let script = build_model_picker_unlock_script(catalog);
        let mut injected_target = None;
        let mut compatibility_evidence = CodexAppCompatibilityEvidence::default();
        for target in targets {
            let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
                continue;
            };
            if let Ok(evidence) = install_script(websocket_url, &script).await {
                if evidence.history_sync_requested {
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
            target_id: Some(target.id),
            target_title: Some(target.title),
            target_url: Some(target.url),
            model_count: catalog.model_names.len(),
            model_names: catalog.model_names.clone(),
            injected: true,
            launched: false,
            codex_executable: detect_running_codex_main_process()
                .map(|path| path.display().to_string()),
            history_sync_requested: compatibility_evidence.history_sync_requested,
            history_catalog_complete: compatibility_evidence.history_catalog_complete,
            history_catalog_count: compatibility_evidence.history_catalog_count,
            message: if compatibility_evidence.history_sync_requested {
                "Codex App compatibility layer was injected; the native history catalog sync was requested and model visibility was refreshed.".to_string()
            } else {
                "Codex App compatibility layer was injected, but the native localThreadCatalog service was not available in this renderer; model visibility was refreshed without rewriting history metadata.".to_string()
            },
        });
    }
    None
}

/// 生成去重后的 CDP 端口探测列表。
fn candidate_debug_ports(preferred: u16) -> Vec<u16> {
    let mut ports = vec![preferred, DEFAULT_CODEX_DEBUG_PORT, 9222, 9223, 9230, 9231];
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// 查询 CDP 页面 target。
async fn list_cdp_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
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
    for url in urls {
        match client.get(&url).send().await {
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

/// 选择最可能属于 Codex Desktop 的 renderer 页面。
///
/// 共享调试端口如 9222 可能属于 Chrome 或其它 Electron 应用，必须看到 Codex
/// 标识才注入；CCSwitchMulti 自己启动的默认 9229 允许在标题还没初始化时退回
/// 到第一个 page target。
fn pick_codex_page_targets(
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
async fn install_script(
    websocket_url: &str,
    script: &str,
) -> Result<CodexAppCompatibilityEvidence, String> {
    let (socket, _) = tokio::time::timeout(CDP_CONNECT_TIMEOUT, connect_async(websocket_url))
        .await
        .map_err(|_| "timed out connecting CDP websocket".to_string())?
        .map_err(|error| format!("failed to connect CDP websocket: {error}"))?;
    let mut session = CdpSession::new(socket);
    session.send_command(1, "Runtime.enable", json!({})).await?;
    session.send_command(2, "Page.enable", json!({})).await?;
    session
        .send_command(
            3,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": script }),
        )
        .await?;
    // 只有 Runtime.evaluate 会返回当前 renderer 的执行结果；新文档脚本注册命令只返回
    // identifier，不能用来判断 localThreadCatalog 是否真正触发。
    let evaluation = session
        .send_command(
            4,
            "Runtime.evaluate",
            json!({
                "expression": script,
                "awaitPromise": true,
                "returnByValue": true,
                "allowUnsafeEvalBlockedByCSP": true
            }),
        )
        .await?;
    codex_app_compatibility_evidence_from_evaluation(&evaluation)
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
    })
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
                json!({ "id": id, "method": method, "params": params }).to_string(),
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

/// 构造 renderer 注入脚本：触发新版本地历史目录同步，并修复模型白名单和缓存。
fn build_model_picker_unlock_script(catalog: &CodexModelCatalogProjection) -> String {
    let payload = serde_json::to_string(catalog).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"
(async () => {{
  const payload = {payload};
  const patchKey = "{MODEL_PICKER_PATCH_KEY}";
  const state = window[patchKey] || {{}};
  state.payload = payload;
  state.requestIds = state.requestIds || new Set();
  state.modulePromises = state.modulePromises || new Map();
  state.failures = state.failures || [];
  window[patchKey] = state;

  const reasoningEfforts = () => ["low", "medium", "high", "xhigh"].map((reasoningEffort) => ({{ reasoningEffort, description: `${{reasoningEffort}} effort` }}));
  const modelNames = () => Array.from(new Set([payload.defaultModel, ...(payload.modelNames || [])].filter((name) => typeof name === "string" && name.trim()).map((name) => name.trim())));
  const descriptorFor = (name) => {{
    const existing = (payload.models || []).find((model) => model && model.model === name);
    return {{
      model: name,
      id: name,
      slug: name,
      name,
      displayName: name,
      hidden: false,
      defaultReasoningEffort: "medium",
      supportedReasoningEfforts: reasoningEfforts(),
      ...(existing || {{}}),
      hidden: false,
    }};
  }};
  const stringArray = (value) => Array.isArray(value) && value.every((item) => typeof item === "string");
  const modelArray = (value, allowEmpty = false) => Array.isArray(value) && (allowEmpty || value.length > 0) && value.every((item) => item && typeof item === "object" && typeof item.model === "string");
  const patchModelNameArray = (models) => {{
    if (!stringArray(models)) return false;
    let changed = false;
    for (const name of modelNames()) {{
      if (!models.includes(name)) {{
        models.push(name);
        changed = true;
      }}
    }}
    return changed;
  }};
  const patchModelArray = (models, allowEmpty = false) => {{
    if (!modelArray(models, allowEmpty)) return false;
    const names = modelNames();
    const existing = new Map(models.map((model) => [model.model, model]));
    let changed = false;
    for (const model of models) {{
      if (names.includes(model.model) && model.hidden !== false) {{
        model.hidden = false;
        changed = true;
      }}
    }}
    for (const name of names) {{
      if (!existing.has(name)) {{
        models.push(descriptorFor(name));
        changed = true;
      }}
    }}
    return changed;
  }};
  const removeHiddenNames = (container, key) => {{
    if (!Array.isArray(container?.[key])) return false;
    const names = new Set(modelNames());
    const before = container[key].length;
    container[key] = container[key].filter((name) => !names.has(name));
    return before !== container[key].length;
  }};
  const patchNameSet = (setLike) => {{
    if (!(setLike instanceof Set)) return false;
    let changed = false;
    for (const name of modelNames()) {{
      if (!setLike.has(name)) {{
        setLike.add(name);
        changed = true;
      }}
    }}
    return changed;
  }};
  const patchModelContainer = (value) => {{
    if (!value || typeof value !== "object") return false;
    let changed = false;
    const looksLikeModelGate = "availableModels" in value || "available_models" in value || "useHiddenModels" in value || "use_hidden_models" in value || "defaultModel" in value || "default_model" in value;
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
    if (looksLikeModelGate && value.useHiddenModels !== false) {{
      value.useHiddenModels = false;
      changed = true;
    }}
    if (looksLikeModelGate && value.use_hidden_models !== false) {{
      value.use_hidden_models = false;
      changed = true;
    }}
    if (typeof value.default_model === "string" && modelNames().length && !modelNames().includes(value.default_model)) {{
      value.default_model = modelNames()[0];
      changed = true;
    }}
    if (value.defaultModel == null && modelNames().length > 0) {{
      value.defaultModel = descriptorFor(modelNames()[0]);
      changed = true;
    }}
    return changed;
  }};
  const patchObjectGraph = (root, visited = new WeakSet(), depth = 0) => {{
    if (!root || typeof root !== "object" || visited.has(root) || depth > 5) return false;
    visited.add(root);
    let changed = patchModelContainer(root);
    if (root instanceof Element || root === window || root === document || root === document.body || root === document.documentElement) return changed;
    for (const key of Object.keys(root)) {{
      if (["ownerDocument", "parentElement", "parentNode", "children", "childNodes"].includes(key)) continue;
      try {{
        if (patchObjectGraph(root[key], visited, depth + 1)) changed = true;
      }} catch {{}}
    }}
    return changed;
  }};
  const patchStatsigConfig = (config) => {{
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
        config.value = nextValue;
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
    for (const client of statsigClients()) {{
      if (typeof client.getDynamicConfig !== "function") continue;
      if (!client.__ccSwitchModelWhitelistPatched) {{
        const original = client.getDynamicConfig.bind(client);
        client.getDynamicConfig = (name, options) => patchStatsigConfig(original(name, options));
        client.__ccSwitchModelWhitelistPatched = true;
      }}
      try {{ patchStatsigConfig(client.getDynamicConfig("107580212", {{ disableExposureLog: true }})); }} catch {{}}
    }}
  }};
  const assetUrl = (namePart) => {{
    const urls = [
      ...Array.from(document.scripts || []).map((script) => script.src),
      ...Array.from(document.querySelectorAll("link[href]") || []).map((link) => link.href),
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
    ].filter(Boolean);
    return urls.find((url) => url.includes("/assets/") && url.includes(namePart) && url.split("?")[0].endsWith(".js")) || "";
  }};
  const loadAppModule = async (namePart) => {{
    if (!state.modulePromises.has(namePart)) {{
      state.modulePromises.set(namePart, Promise.resolve().then(async () => {{
        const url = assetUrl(namePart);
        if (!url) throw new Error(`Codex App asset not found: ${{namePart}}`);
        return await import(url);
      }}).catch((error) => {{
        state.modulePromises.delete(namePart);
        throw error;
      }}));
    }}
    return await state.modulePromises.get(namePart);
  }};
  // 新版 Codex/ChatGPT App 用 localThreadCatalog 保存统一侧边栏目录。这里只调用
  // App 自己的 RPC 同步服务，不直接改 codex-dev.db 或历史 provider 元数据。
  const triggerLocalThreadCatalogSync = async () => {{
    if (state.historySyncPromise) return await state.historySyncPromise;
    state.historySyncPromise = Promise.resolve().then(async () => {{
      const module = await loadAppModule("rpc-");
      const roots = Object.values(module).filter((item) => item && (typeof item === "object" || typeof item === "function"));
      for (const root of roots) {{
        try {{
          const catalog = root.localThreadCatalog;
          if (!catalog || typeof catalog.requestStartupSync !== "function") continue;
          const before = typeof catalog.readSnapshot === "function" ? await catalog.readSnapshot() : null;
          await catalog.requestStartupSync();
          const after = typeof catalog.readSnapshot === "function" ? await catalog.readSnapshot() : null;
          state.historySync = {{
            requested: true,
            beforeCount: Array.isArray(before?.entries) ? before.entries.length : null,
            afterCount: Array.isArray(after?.entries) ? after.entries.length : null,
            complete: after?.isComplete ?? null,
          }};
          if (after?.isComplete !== true) {{
            setTimeout(() => {{ state.historySyncPromise = null; }}, 10000);
          }}
          return state.historySync;
        }} catch (error) {{
          state.failures.push(String(error?.message || error));
        }}
      }}
      throw new Error("Codex App localThreadCatalog RPC service was not found");
    }}).catch((error) => {{
      state.historySync = {{ requested: false, error: String(error?.message || error) }};
      state.failures.push(state.historySync.error);
      state.historySyncPromise = null;
      return state.historySync;
    }});
    return await state.historySyncPromise;
  }};
  const appServerMethod = (method, params) => method === "send-cli-request-for-host" && params?.method ? String(params.method) : String(method || "");
  const patchAppServerResult = (method, result) => {{
    if (method !== "list-models-for-host") return result;
    if (Array.isArray(result)) patchModelArray(result, true);
    if (Array.isArray(result?.data)) patchModelArray(result.data, true);
    if (Array.isArray(result?.models)) patchModelArray(result.models, true);
    patchModelContainer(result);
    patchObjectGraph(result);
    return result;
  }};
  const patchRequestClient = (client) => {{
    if (!client || typeof client.sendRequest !== "function") return false;
    if (client.__ccSwitchModelRequestPatch === "1") return true;
    const original = client.__ccSwitchOriginalSendRequest || client.sendRequest.bind(client);
    client.__ccSwitchOriginalSendRequest = original;
    client.sendRequest = async function ccSwitchPatchedSendRequest(method, params, options) {{
      const result = await original(method, params, options);
      return patchAppServerResult(appServerMethod(method, params), result);
    }};
    client.__ccSwitchModelRequestPatch = "1";
    return true;
  }};
  const installAppServerPatch = async () => {{
    try {{
      const module = await loadAppModule("app-server-manager-signals-");
      for (const candidate of Object.values(module).filter((item) => item && typeof item === "object")) {{
        patchRequestClient(candidate);
        if (typeof candidate.sendRequest !== "function" && typeof candidate.get === "function") {{
          try {{ patchRequestClient(candidate.get()); }} catch {{}}
        }}
      }}
    }} catch (error) {{
      state.failures.push(String(error?.message || error));
    }}
  }};
  const patchMcpModelResponseData = (data) => {{
    if (data?.type !== "mcp-response") return false;
    const message = data.message || data.response;
    const requestId = message?.id != null ? String(message.id) : "";
    if (state.requestIds.size > 0 && !state.requestIds.has(requestId)) return false;
    state.requestIds.delete(requestId);
    return patchModelContainer(data) || patchModelContainer(message) || patchModelContainer(message?.result) || patchModelContainer(message?.result?.data);
  }};
  const installMessagePatch = () => {{
    if (state.messagePatchInstalled) return;
    state.messagePatchInstalled = true;
    const originalDispatchEvent = window.dispatchEvent;
    window.dispatchEvent = function ccSwitchPatchedDispatchEvent(event) {{
      try {{
        const detail = event?.detail;
        const request = detail?.request;
        if (event?.type === "codex-message-from-view" && detail?.type === "mcp-request" && request?.method === "model/list") {{
          request.params = {{ ...(request.params || {{}}), includeHidden: true }};
          if (request.id != null) state.requestIds.add(String(request.id));
        }}
        if (event?.type === "message") patchMcpModelResponseData(event.data);
      }} catch (error) {{
        state.failures.push(String(error?.message || error));
      }}
      return originalDispatchEvent.call(this, event);
    }};
    window.addEventListener("message", (event) => {{
      try {{ patchMcpModelResponseData(event?.data); }} catch (error) {{ state.failures.push(String(error?.message || error)); }}
    }}, true);
  }};
  const installResponsePatch = () => {{
    if (state.responsePatchInstalled || typeof Response === "undefined") return;
    state.responsePatchInstalled = true;
    const originalJson = Response.prototype.json;
    Response.prototype.json = async function ccSwitchPatchedResponseJson(...args) {{
      const data = await originalJson.apply(this, args);
      try {{ patchModelContainer(data); patchObjectGraph(data); }} catch (error) {{ state.failures.push(String(error?.message || error)); }}
      return data;
    }};
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
    const auth = authContextValueFrom(element);
    if (!auth || auth.authMethod === "chatgpt") return false;
    try {{
      auth.setAuthMethod("chatgpt");
      return true;
    }} catch (error) {{
      state.failures.push(String(error?.message || error));
      return false;
    }}
  }};
  const patchReactState = () => {{
    const visited = new WeakSet();
    const nodes = [document.body, ...document.querySelectorAll("button, [role='menu'], [role='dialog'], [data-radix-popper-content-wrapper]")].filter(Boolean);
    for (const node of nodes.slice(0, 220)) {{
      spoofChatGPTAuthMethod(node);
      for (const key of reactFiberKeys(node)) patchObjectGraph(node[key], visited);
    }}
  }};
  const run = () => {{
    installResponsePatch();
    installMessagePatch();
    void installAppServerPatch();
    void triggerLocalThreadCatalogSync();
    patchStatsig();
    patchReactState();
  }};
  run();
  if (!state.interval) state.interval = setInterval(run, 1500);
  const historySync = await triggerLocalThreadCatalogSync();
  return {{ status: "ok", modelCount: modelNames().length, available_models: modelNames(), historySync, patchKey }};
}})()
"#
    )
}

/// 启动 Codex Desktop，并传入 remote debugging 参数。
fn launch_codex_with_debug_port(executable: &Path, debug_port: u16) -> Result<(), String> {
    if let Some(running) = detect_running_codex_main_process() {
        return Err(format!(
            "Codex Desktop is already running at {}. Fully quit Codex, then launch it from CCSwitchMulti so the model picker patch can be installed.",
            running.display()
        ));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(bundle) = macos_codex_bundle_for_executable(executable) {
            let mut command = Command::new("open");
            command.arg(bundle).arg("--args");
            append_codex_debug_args(&mut command, debug_port);
            return command
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("failed to launch {}: {error}", executable.display()));
        }
    }
    let mut command = Command::new(executable);
    append_codex_debug_args(&mut command, debug_port);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to launch {}: {error}", executable.display()))
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
fn detect_running_codex_main_process() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let script = DETECT_CODEX_MAIN_PROCESS_SCRIPT;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
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
fn resolve_codex_executable() -> Option<PathBuf> {
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

/// 非 Windows 平台沿用文件名级别的 Desktop shell 校验。
#[cfg(not(target_os = "windows"))]
fn is_codex_desktop_executable_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_codex_desktop_executable_name)
}

/// 判断路径文件名是否是当前平台的 Desktop shell，而不是小写 CLI/app-server。
#[cfg(target_os = "macos")]
fn is_codex_desktop_executable_name(name: &str) -> bool {
    name == "Codex"
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

/// macOS 上通过 System Events 找到运行中的 Codex.app，再解析 bundle 内部二进制。
#[cfg(target_os = "macos")]
fn detect_running_macos_codex_main_process() -> Option<PathBuf> {
    let script = r#"
tell application "System Events"
  set matches to application processes whose name is "Codex"
  if (count of matches) is 0 then return ""
  try
    return POSIX path of (application file of item 1 of matches)
  end try
end tell
"#;
    if let Some(bundle) = command_stdout_trimmed(Command::new("osascript").arg("-e").arg(script))
        .filter(|path| !path.is_empty())
    {
        let executable = macos_codex_bundle_executable(Path::new(&bundle));
        if let Ok(executable) = canonical_codex_desktop_executable_path(&executable) {
            return Some(executable);
        }
    }

    let mut saw_codex_process = false;
    for pid in command_stdout_lines(Command::new("pgrep").args(["-x", "Codex"])) {
        saw_codex_process = true;
        let path = command_stdout_trimmed(Command::new("ps").args(["-p", &pid, "-o", "comm="]));
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            continue;
        };
        if let Ok(executable) = canonical_codex_desktop_executable_path(Path::new(&path)) {
            return Some(executable);
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
        push_codex_desktop_executable_candidate(
            &mut candidates,
            Vec::new(),
            macos_codex_bundle_executable(&bundle),
        );
    }
    for bundle in command_stdout_lines(
        Command::new("mdfind")
            .arg("kMDItemFSName == 'Codex.app' || kMDItemCFBundleIdentifier == 'com.openai.codex'"),
    ) {
        push_codex_desktop_executable_candidate(
            &mut candidates,
            Vec::new(),
            macos_codex_bundle_executable(Path::new(&bundle)),
        );
    }

    candidates.pop().map(|(_, executable)| executable)
}

/// macOS 常见应用目录候选，覆盖系统级和用户级安装。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_codex_common_bundle_candidates() -> Vec<PathBuf> {
    let mut bundles = vec![PathBuf::from("/Applications/Codex.app")];
    if let Some(home) = std::env::var_os("HOME") {
        bundles.push(PathBuf::from(home).join("Applications").join("Codex.app"));
    }
    bundles
}

/// 从 macOS `.app` bundle 路径推导 Desktop 主二进制路径。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_codex_bundle_executable(bundle: &Path) -> PathBuf {
    bundle.join("Contents").join("MacOS").join("Codex")
}

/// 如果路径位于 Codex.app 内部，返回对应 bundle 路径，便于使用 macOS `open` 启动。
#[cfg(target_os = "macos")]
fn macos_codex_bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let mut current = executable.parent();
    while let Some(path) = current {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "Codex.app")
        {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
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
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&stdout).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn model_picker_unlock_script_patches_renderer_whitelists() {
        let catalog = CodexModelCatalogProjection {
            default_model: Some("qwen3.6".to_string()),
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
        assert!(script.contains("__ccSwitchCodexAppCompatibilityV4"));
        assert!(script.contains("auth.setAuthMethod(\"chatgpt\")"));
    }

    /// 验证新版历史修复只调用 App 官方目录同步，不改写 thread/list 的查询语义。
    #[test]
    fn codex_app_compatibility_script_repairs_native_history_catalog() {
        let script = build_model_picker_unlock_script(&CodexModelCatalogProjection::empty());

        assert!(script.contains("localThreadCatalog"));
        assert!(script.contains("requestStartupSync"));
        assert!(script.contains("readSnapshot"));
        assert!(script.contains("const historySync = await triggerLocalThreadCatalogSync()"));
        assert!(!script.contains("modelProviders: []"));
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

    /// 验证只接受平台 Desktop shell，避免把 CLI/app-server `codex` 用于 renderer 解锁。
    #[test]
    fn codex_desktop_executable_validation_rejects_cli_launcher() {
        let desktop_dir = tempfile::tempdir().expect("create desktop temp dir");
        let desktop = desktop_dir.path().join(desktop_test_executable_name());
        std::fs::write(&desktop, "").expect("write desktop exe");
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
        let desktop = desktop_dir.path().join(desktop_test_executable_name());
        std::fs::write(&desktop, "").expect("write desktop exe");
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

    /// 验证 macOS `.app` bundle 会解析到内部 Desktop 主二进制。
    #[test]
    fn macos_codex_bundle_candidate_points_to_internal_binary() {
        assert_eq!(
            macos_codex_bundle_executable(Path::new("/Applications/Codex.app")),
            PathBuf::from("/Applications/Codex.app/Contents/MacOS/Codex")
        );
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
