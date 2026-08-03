//! HTTP代理服务器
//!
//! 基于Axum的HTTP服务器，处理代理请求
//!
//! Uses a manual hyper HTTP/1.1 accept loop with `preserve_header_case(true)` so
//! that the original header-name casing from the CLI client is captured in a
//! `HeaderCaseMap` extension.  This map is later forwarded to the upstream via
//! the hyper-based HTTP client, producing wire-level header casing identical to
//! a direct (non-proxied) CLI request.

use super::{
    failover_switch::FailoverSwitchManager,
    handlers,
    log_codes::srv as log_srv,
    provider_router::ProviderRouter,
    providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
    types::*,
    ProxyError,
};
use crate::database::Database;
use axum::{
    extract::DefaultBodyLimit,
    routing::{any, get, post},
    Router,
};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;

/// 代理服务器状态（共享）
#[derive(Clone)]
pub struct ProxyState {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<ProxyConfig>>,
    pub status: Arc<RwLock<ProxyStatus>>,
    pub start_time: Arc<RwLock<Option<std::time::Instant>>>,
    /// 每个应用类型当前使用的 provider (app_type -> (provider_id, provider_name))
    pub current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    /// 共享的 ProviderRouter（持有熔断器状态，跨请求保持）
    pub provider_router: Arc<ProviderRouter>,
    /// Gemini Native shadow state，用于 thoughtSignature / tool call 回放
    pub gemini_shadow: Arc<GeminiShadowStore>,
    /// Codex Chat bridge history，用于恢复 previous_response_id 指向的 tool call
    pub codex_chat_history: Arc<CodexChatHistoryStore>,
    /// AppHandle，用于发射事件和更新托盘菜单
    pub app_handle: Option<tauri::AppHandle>,
    /// 故障转移切换管理器
    pub failover_manager: Arc<FailoverSwitchManager>,
}

/// 代理HTTP服务器
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyServerMode {
    FullProxy,
    ExternalOpenAiApiOnly,
}

pub struct ProxyServer {
    config: ProxyConfig,
    mode: ProxyServerMode,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    /// 服务器任务句柄，用于等待服务器实际关闭
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ProxyServer {
    pub fn new(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        Self::new_with_mode(config, db, app_handle, ProxyServerMode::FullProxy)
    }

    pub fn new_external_openai_api(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        Self::new_with_mode(
            config,
            db,
            app_handle,
            ProxyServerMode::ExternalOpenAiApiOnly,
        )
    }

    fn new_with_mode(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
        mode: ProxyServerMode,
    ) -> Self {
        // 创建共享的 ProviderRouter（熔断器状态将跨所有请求保持）
        let provider_router = Arc::new(ProviderRouter::new(db.clone()));
        // 创建故障转移切换管理器
        let failover_manager = Arc::new(FailoverSwitchManager::new(db.clone()));

        let state = ProxyState {
            db,
            config: Arc::new(RwLock::new(config.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            provider_router,
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle,
            failover_manager,
        };

        Self {
            config,
            mode,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start(&self) -> Result<ProxyServerInfo, ProxyError> {
        // 检查是否已在运行
        if self.shutdown_tx.read().await.is_some() {
            return Err(ProxyError::AlreadyRunning);
        }

        let addr: SocketAddr =
            format!("{}:{}", self.config.listen_address, self.config.listen_port)
                .parse()
                .map_err(|e| ProxyError::BindFailed(format!("无效的地址: {e}")))?;

        // 创建关闭通道
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // 构建路由
        let app = self.build_router();

        // 绑定监听器
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::BindFailed(format_bind_error(&addr, e)))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| ProxyError::BindFailed(format_bind_error(&addr, e)))?;
        let actual_port = local_addr.port();

        log::info!("[{}] 代理服务器启动于 {local_addr}", log_srv::STARTED);

        // 更新全局代理端口，用于系统代理检测
        crate::proxy::http_client::set_proxy_port(actual_port);

        // 保存关闭句柄
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // 更新状态
        let mut status = self.state.status.write().await;
        status.running = true;
        status.address = self.config.listen_address.clone();
        status.port = actual_port;
        drop(status);

        // 记录启动时间
        *self.state.start_time.write().await = Some(std::time::Instant::now());

        // 启动服务器 — 使用手动 hyper HTTP/1.1 accept loop
        // 开启 preserve_header_case 以捕获客户端请求头的原始大小写
        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, _remote_addr) = match result {
                            Ok(v) => v,
                            Err(e) => {
                                log::error!("[{SRV}] accept 失败: {e}", SRV = log_srv::ACCEPT_ERR);
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                continue;
                            }
                        };

                        let app = app.clone();
                        tokio::spawn(async move {
                            // Peek raw TCP bytes to capture original header casing
                            // before hyper parses (and lowercases) the header names.
                            let original_cases = {
                                let mut peek_buf = vec![0u8; 8192];
                                match stream.peek(&mut peek_buf).await {
                                    Ok(n) => {
                                        let cases = super::hyper_client::OriginalHeaderCases::from_raw_bytes(&peek_buf[..n]);
                                        log::debug!(
                                            "[ProxyServer] Peeked {} bytes, captured {} header casings",
                                            n, cases.cases.len()
                                        );
                                        cases
                                    }
                                    Err(e) => {
                                        log::debug!("[ProxyServer] peek failed (non-fatal): {e}");
                                        super::hyper_client::OriginalHeaderCases::default()
                                    }
                                }
                            };

                            // service_fn 将 axum Router（tower::Service）桥接到 hyper
                            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut router = app.clone();
                                let cases = original_cases.clone();
                                async move {
                                    // 将 hyper::body::Incoming 转为 axum::body::Body，保留 extensions
                                    let (mut parts, body) = req.into_parts();

                                    // Insert our own header case map alongside hyper's internal one
                                    parts.extensions.insert(cases);

                                    let body = axum::body::Body::new(body);
                                    let axum_req = http::Request::from_parts(parts, body);
                                    <Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut router, axum_req).await
                                }
                            });

                            if let Err(e) = hyper::server::conn::http1::Builder::new()
                                .preserve_header_case(true)
                                .serve_connection(TokioIo::new(stream), service)
                                .await
                            {
                                // Connection reset / broken pipe 等在代理场景下很常见，debug 级别
                                log::debug!("[{SRV}] connection error: {e}", SRV = log_srv::CONN_ERR);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }

            // 服务器停止后更新状态
            state.status.write().await.running = false;
            *state.start_time.write().await = None;
        });

        // 保存服务器任务句柄
        *self.server_handle.write().await = Some(handle);

        Ok(ProxyServerInfo {
            address: self.config.listen_address.clone(),
            port: actual_port,
            started_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub async fn stop(&self) -> Result<(), ProxyError> {
        // 1. 发送关闭信号
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        } else {
            return Err(ProxyError::NotRunning);
        }

        // 2. 等待服务器任务结束（带 5 秒超时保护）
        if let Some(handle) = self.server_handle.write().await.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {
                    log::info!("[{}] 代理服务器已完全停止", log_srv::STOPPED);
                    Ok(())
                }
                Ok(Err(e)) => {
                    log::warn!("[{}] 代理服务器任务异常终止: {e}", log_srv::TASK_ERROR);
                    Err(ProxyError::StopFailed(e.to_string()))
                }
                Err(_) => {
                    log::warn!(
                        "[{}] 代理服务器停止超时（5秒），强制继续",
                        log_srv::STOP_TIMEOUT
                    );
                    Err(ProxyError::StopTimeout)
                }
            }
        } else {
            Ok(())
        }
    }

    pub async fn get_status(&self) -> ProxyStatus {
        let mut status = self.state.status.read().await.clone();

        // 计算运行时间
        if let Some(start) = *self.state.start_time.read().await {
            status.uptime_seconds = start.elapsed().as_secs();
        }

        // 从 current_providers HashMap 获取每个应用类型当前正在使用的 provider
        let current_providers = self.state.current_providers.read().await;
        status.active_targets = current_providers
            .iter()
            .map(|(app_type, (provider_id, provider_name))| ActiveTarget {
                app_type: app_type.clone(),
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
            })
            .collect();

        status
    }

    /// 更新某个应用类型当前“目标供应商”（用于 UI 展示 active_targets）
    ///
    /// 注意：这不代表该供应商一定已经处理过请求，而是用于“热切换/启用故障转移立即切 P1”
    /// 等场景下，让 UI 能立刻反映最新目标。
    pub async fn set_active_target(&self, app_type: &str, provider_id: &str, provider_name: &str) {
        let mut current_providers = self.state.current_providers.write().await;
        current_providers.insert(
            app_type.to_string(),
            (provider_id.to_string(), provider_name.to_string()),
        );
    }

    fn build_router(&self) -> Router {
        if self.mode == ProxyServerMode::ExternalOpenAiApiOnly {
            return self.build_external_openai_api_router();
        }

        Router::new()
            // 健康检查
            .route("/health", get(handlers::health_check))
            .route("/status", get(handlers::get_status))
            // Claude API (支持带前缀和不带前缀两种格式)
            .route("/v1/messages", post(handlers::handle_messages))
            .route("/claude/v1/messages", post(handlers::handle_messages))
            // Claude Desktop 3P 本地 gateway（独立 provider namespace）
            .route(
                "/claude-desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            // OpenAI Chat Completions API (Codex CLI，支持带前缀和不带前缀)
            .route("/chat/completions", post(handlers::handle_chat_completions))
            .route(
                "/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/v1/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/codex/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            // OpenAI Models API (Codex CLI reachability check)
            .route("/models", get(handlers::handle_models))
            .route("/v1/models", get(handlers::handle_models))
            // OpenAI Images API (Codex Desktop 内置 Image Gen)
            .route(
                "/images/generations",
                post(handlers::handle_image_generations),
            )
            .route(
                "/v1/images/generations",
                post(handlers::handle_image_generations),
            )
            .route(
                "/v1/v1/images/generations",
                post(handlers::handle_image_generations),
            )
            .route(
                "/codex/v1/images/generations",
                post(handlers::handle_image_generations),
            )
            // OpenAI Responses API (Codex CLI，支持带前缀和不带前缀)
            .route(
                "/responses",
                get(handlers::handle_responses_websocket).post(handlers::handle_responses),
            )
            .route(
                "/v1/responses",
                get(handlers::handle_responses_websocket).post(handlers::handle_responses),
            )
            .route(
                "/v1/v1/responses",
                get(handlers::handle_responses_websocket).post(handlers::handle_responses),
            )
            .route(
                "/codex/v1/responses",
                get(handlers::handle_responses_websocket).post(handlers::handle_responses),
            )
            // Codex GPT-Live / Realtime Voice：call-create 是 HTTP POST，会话是
            // WebSocket Upgrade。必须显式接管，不能落到普通 raw passthrough。
            .route(
                "/live",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/v1/live",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/v1/v1/live",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/codex/v1/live",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/live/*call_id",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/v1/live/*call_id",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/v1/v1/live/*call_id",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            .route(
                "/codex/v1/live/*call_id",
                get(handlers::handle_codex_realtime_websocket)
                    .post(handlers::handle_codex_realtime_http),
            )
            // Grok Build uses the Responses protocol but has an independent
            // provider namespace and failover queue.
            .route(
                "/grokbuild/v1/responses",
                post(handlers::handle_grokbuild_responses),
            )
            // OpenAI Responses Compact API (Codex CLI 远程压缩，透传)
            .route(
                "/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/codex/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/grokbuild/v1/responses/compact",
                post(handlers::handle_grokbuild_responses_compact),
            )
            // Gemini API (支持带前缀和不带前缀)
            //
            // 用 `any(..)` 覆盖所有 HTTP 方法：除了 POST `:generateContent` /
            // `:streamGenerateContent` / `:countTokens` 之外，Gemini SDK / CLI 还会发
            // GET `/models`、GET `/models/<id>` 等只读端点。如果只挂 POST，这些 GET
            // 请求会在路由层 404，绕过本地代理的统计、整流和故障转移。
            .route("/v1beta/*path", any(handlers::handle_gemini))
            .route("/gemini/v1beta/*path", any(handlers::handle_gemini))
            // Gemini 的 GA 版本也叫 /v1，给原 SDK 留一条出口
            .route("/gemini/v1/*path", any(handlers::handle_gemini))
            // 提高默认请求体大小限制（避免 413 Payload Too Large）
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .fallback(handlers::handle_unregistered_proxy_endpoint)
            .with_state(self.state.clone())
    }

    fn build_external_openai_api_router(&self) -> Router {
        Router::new()
            .route("/health", get(handlers::health_check))
            .route("/v1/models", get(handlers::handle_external_models))
            .route(
                "/v1/chat/completions",
                post(handlers::handle_external_chat_completions),
            )
            .route(
                "/v1/responses",
                get(handlers::handle_responses_websocket).post(handlers::handle_external_responses),
            )
            .route(
                "/v1/images/generations",
                post(handlers::handle_external_image_generations),
            )
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .fallback(handlers::handle_unregistered_proxy_endpoint)
            .with_state(self.state.clone())
    }

    /// 在不重启服务的情况下更新运行时配置
    pub async fn apply_runtime_config(&self, config: &ProxyConfig) {
        *self.state.config.write().await = config.clone();
    }

    /// 热更新熔断器配置
    ///
    /// 将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state.provider_router.update_all_configs(config).await;
    }

    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state
            .provider_router
            .update_app_configs(app_type, config)
            .await;
    }

    /// 重置指定 Provider 的熔断器
    pub async fn reset_provider_circuit_breaker(&self, provider_id: &str, app_type: &str) {
        self.state
            .provider_router
            .reset_provider_breaker(provider_id, app_type)
            .await;
    }
}

/// 把底层 bind 错误转成用户可操作的诊断文案。
///
/// 端口被另一 CCSwitchMulti、旧进程或其它本地服务占用时，原始 OS 错误通常只说
/// `Address already in use`，用户很容易误判为 provider 配置损坏。
fn format_bind_error(addr: &SocketAddr, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::AddrInUse {
        return format!(
            "{} 已被占用（可能是另一个 CCSwitchMulti/CCSwitch 实例、旧进程残留或其它本地服务）。请结束占用该端口的进程，或在代理设置里改用其它监听端口。原始错误: {error}",
            addr
        );
    }
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::Provider,
        proxy::external_openai_api::{
            self, ExternalOpenAiApiBackendType, ExternalOpenAiApiProfileUpdate,
        },
    };
    use axum::{body::Body, response::IntoResponse, Json};
    use bytes::Bytes;
    use http::{header, HeaderMap, Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    /// 测试专用 home，用于隔离 Codex live config/catalog 文件。
    struct TestHomeGuard {
        _dir: tempfile::TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TestHomeGuard {
        /// 创建临时 home 并覆盖环境变量，避免 endpoint 测试读取真实用户 `.codex`。
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create temp home");
            let original_home = std::env::var("HOME").ok();
            let original_userprofile = std::env::var("USERPROFILE").ok();
            let original_test_home = std::env::var("CC_SWITCH_TEST_HOME").ok();

            std::env::set_var("HOME", dir.path());
            std::env::set_var("USERPROFILE", dir.path());
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                _dir: dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TestHomeGuard {
        /// 测试结束后恢复调用方环境变量，避免影响后续用例。
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => std::env::set_var("USERPROFILE", value),
                None => std::env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    /// 构造只用于 router 测试的内存数据库和 proxy server。
    fn build_test_server() -> (ProxyServer, Arc<Database>) {
        let db = Arc::new(Database::memory().expect("memory db"));
        let config = ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 15721,
            ..ProxyConfig::default()
        };
        (ProxyServer::new(config, db.clone(), None), db)
    }

    fn build_external_test_server() -> (ProxyServer, Arc<Database>) {
        let db = Arc::new(Database::memory().expect("memory db"));
        let config = ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 15722,
            ..ProxyConfig::default()
        };
        (
            ProxyServer::new_external_openai_api(config, db.clone(), None),
            db,
        )
    }

    /// 把 Codex MultiRouter 配置成官方 OAuth 路由，但 base_url 指向本地 mock。
    async fn save_codex_official_mock_router(db: &Database, official_base_url: &str) {
        let mut official_provider = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "base_url": format!("{official_base_url}/backend-api/codex"),
                "api_key": "sk-official"
            }),
            None,
        );
        official_provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &official_provider)
            .expect("save official provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "official",
                        "routes": [
                            {
                                "id": "official",
                                "label": "OpenAI Official",
                                "enabled": true,
                                "targetProviderId": "codex-official",
                                "match": { "models": ["gpt-5.6-luna"] },
                                "upstream": { "apiFormat": "openai_responses" }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex proxy config");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");
    }

    /// 把 Codex MultiRouter 配置成 Chat 上游，base_url 指向本地 mock。
    async fn save_codex_chat_mock_router(db: &Database, chat_base_url: &str) {
        let mut chat_provider = Provider::with_id(
            "k3-chat".to_string(),
            "Kimi K3".to_string(),
            json!({
                "base_url": chat_base_url,
                "api_key": "sk-k3"
            }),
            None,
        );
        chat_provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &chat_provider)
            .expect("save chat provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "k3",
                        "routes": [
                            {
                                "id": "k3",
                                "label": "Kimi K3",
                                "enabled": true,
                                "targetProviderId": "k3-chat",
                                "match": { "models": ["k3"] },
                                "upstream": { "apiFormat": "openai_chat" }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex proxy config");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");
    }

    #[test]
    fn bind_error_for_addr_in_use_includes_actionable_port_diagnostic() {
        let addr: SocketAddr = "127.0.0.1:15721".parse().expect("socket addr");
        let message = format_bind_error(
            &addr,
            std::io::Error::new(std::io::ErrorKind::AddrInUse, "Address already in use"),
        );

        assert!(message.contains("127.0.0.1:15721"));
        assert!(message.contains("已被占用"));
        assert!(message.contains("CCSwitchMulti"));
        assert!(message.contains("改用其它监听端口"));
    }

    /// 读取 Axum 响应体为 JSON，方便断言 OpenAI-compatible 响应结构。
    async fn response_json(response: axum::response::Response) -> Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        serde_json::from_slice(&body).expect("json body")
    }

    #[tokio::test]
    async fn v1_models_requires_external_api_key_for_non_codex_clients() {
        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["code"], "external_openai_api_disabled");
    }

    #[tokio::test]
    async fn v1_image_generations_is_routed_before_external_auth() {
        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/images/generations")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"model":"gpt-image-1","prompt":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Images API must enter the handler instead of falling through to Axum 404"
        );
        let body = response_json(response).await;
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["code"], "external_openai_api_disabled");
    }

    #[tokio::test]
    async fn unknown_v1_endpoint_raw_passthrough_forwards_to_profile_backend() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_openai_raw_passthrough_mock(captured_request.clone()).await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["text-embedding-3-small"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("text-embedding-3-small".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/embeddings?encoding_format=float")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"model":"text-embedding-3-small","input":"ping"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["object"], "list");
        assert_eq!(body["model"], "text-embedding-3-small");

        let captured = captured_request
            .lock()
            .expect("captured raw request lock")
            .clone()
            .expect("captured raw request");
        assert_eq!(
            captured["path_and_query"],
            "/v1/embeddings?encoding_format=float"
        );
        assert_eq!(captured["authorization"], "Bearer sk-selected");
        assert_eq!(captured["content_type"], "application/json");
        assert_eq!(captured["body"]["model"], "text-embedding-3-small");
        assert_eq!(captured["body"]["input"], "ping");
    }

    #[tokio::test]
    async fn unknown_codex_v1_alias_raw_passthrough_normalizes_upstream_path() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_openai_raw_passthrough_mock(captured_request.clone()).await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["text-embedding-3-small"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("text-embedding-3-small".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/codex/v1/embeddings?encoding_format=float")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"model":"text-embedding-3-small","input":"ping"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let captured = captured_request
            .lock()
            .expect("captured raw alias request lock")
            .clone()
            .expect("captured raw alias request");
        assert_eq!(
            captured["path_and_query"],
            "/v1/embeddings?encoding_format=float"
        );
    }

    #[tokio::test]
    async fn codex_realtime_live_call_routes_to_official_backend_not_nonofficial_route() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_realtime_backend_mock(captured_request.clone()).await;
        let (server, db) = build_test_server();
        db.save_provider(
            "codex",
            &Provider::with_id(
                "codex-official".to_string(),
                "OpenAI Official".to_string(),
                json!({
                    "base_url": format!("{upstream_base_url}/backend-api/codex"),
                    "api_key": "sk-official"
                }),
                None,
            ),
        )
        .expect("save official provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "deepseek",
                        "routes": [
                            {
                                "id": "official",
                                "label": "OpenAI Official",
                                "enabled": true,
                                "targetProviderId": "codex-official",
                                "match": { "models": ["gpt-5.6-sol"] },
                                "upstream": { "apiFormat": "openai_responses" }
                            },
                            {
                                "id": "deepseek",
                                "label": "DeepSeek",
                                "enabled": true,
                                "match": { "models": ["deepseek-v4-flash"] },
                                "upstream": {
                                    "baseUrl": "https://api.deepseek.com",
                                    "apiKey": "sk-deepseek"
                                }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex failover queue");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");

        let boundary = "codex-realtime-call-boundary";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"sdp\"\r\n\
             Content-Type: application/sdp\r\n\
             \r\n\
             v=offer\r\n\
             o=- 1 1 IN IP4 127.0.0.1\r\n\
             s=codex\r\n\
             \r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"session\"\r\n\
             Content-Type: application/json\r\n\
             \r\n\
             {{\"model\":\"gpt-live-1-codex\",\"delegation\":{{\"type\":\"client\"}}}}\r\n\
             --{boundary}--\r\n"
        );
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/live")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/v1/live/rtc_123"
        );
        let captured = captured_request
            .lock()
            .expect("captured realtime request lock")
            .clone()
            .expect("captured realtime request");
        assert_eq!(
            captured["path_and_query"],
            "/backend-api/codex/realtime/calls?intent=quicksilver&architecture=avas"
        );
        assert_eq!(captured["authorization"], "Bearer sk-official");
        assert_eq!(captured["content_type"], "application/json");
        assert!(captured["body"]["sdp"]
            .as_str()
            .unwrap()
            .starts_with("v=offer"));
        assert_eq!(captured["body"]["session"]["model"], "gpt-live-1-codex");
    }

    #[tokio::test]
    async fn unknown_non_v1_endpoint_returns_structured_not_found() {
        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/not-a-proxy-endpoint")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["code"], "ccswitch_route_not_found");
    }

    #[tokio::test]
    #[serial]
    async fn v1_models_for_codex_client_returns_catalog_and_openai_data() {
        let _home = TestHomeGuard::new();
        std::fs::create_dir_all(crate::codex_config::get_codex_config_dir())
            .expect("create codex dir");
        std::fs::write(
            crate::codex_config::get_codex_config_path(),
            r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"

[model_providers.custom]
base_url = "http://127.0.0.1:15721/v1"
"#,
        )
        .expect("write config");
        std::fs::write(
            crate::codex_config::get_codex_model_catalog_path(),
            serde_json::to_string_pretty(&json!({
                "models": [
                    { "slug": "qwen3.6", "model": "qwen3.6", "display_name": "Qwen 3.6" },
                    {
                        "slug": "deepseek-v4-flash",
                        "model": "deepseek-v4-flash",
                        "display_name": "DeepSeek V4 Flash"
                    }
                ]
            }))
            .expect("serialize catalog"),
        )
        .expect("write catalog");

        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/models")
                    .header(header::USER_AGENT, "codex-cli/0.140.0")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["object"], "list");
        assert!(
            body["models"].as_array().is_some(),
            "Codex CLI raw catalog shape should remain available"
        );
        let ids: Vec<_> = body["data"]
            .as_array()
            .expect("OpenAI data array")
            .iter()
            .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
            .collect();
        assert!(ids.contains(&"qwen3.6"));
        assert!(ids.contains(&"deepseek-v4-flash"));
    }

    #[tokio::test]
    async fn external_only_v1_models_never_serves_codex_catalog_by_user_agent() {
        let (server, _db) = build_external_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/models")
                    .header(header::USER_AGENT, "codex-cli-test")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["code"], "external_openai_api_disabled");
    }

    #[tokio::test]
    async fn v1_models_returns_profile_backend_models_with_valid_key() {
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": "https://selected.example/v1",
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("default-visible".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/models")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let ids: Vec<_> = body["data"]
            .as_array()
            .expect("data array")
            .iter()
            .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
            .collect();

        assert!(ids.contains(&"visible-model"));
        assert!(ids.contains(&"default-visible"));
        assert_eq!(body["object"], "list");
    }

    #[tokio::test]
    async fn v1_chat_completions_forwards_to_profile_backend() {
        let (upstream_base_url, _upstream_task) = spawn_openai_chat_mock().await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("visible-model".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "model": "visible-model",
                            "messages": [{ "role": "user", "content": "ping" }]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        let status = response.status();
        let body = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "unexpected response body: {body}");
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["choices"][0]["message"]["content"], "pong");
    }

    #[tokio::test]
    async fn v1_chat_completions_preserves_chinese_for_profile_backend() {
        let captured_body = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_openai_chat_mock_with_capture(captured_body.clone()).await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("visible-model".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let user_text = "当前页是教材与参考资料页，请用两句话说明应该学什么。Bondy-Murty、West";
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "model": "visible-model",
                            "messages": [{ "role": "user", "content": user_text }]
                        }))
                        .expect("serialize request"),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        let captured = captured_body
            .lock()
            .expect("captured body lock")
            .clone()
            .expect("mock upstream body");
        assert_eq!(captured["messages"][0]["content"], user_text);
        assert!(!captured["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains('?'));

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["choices"][0]["message"]["content"], "pong");
    }

    #[tokio::test]
    async fn v1_chat_completions_stream_forwards_sse_chunks() {
        let (upstream_base_url, _upstream_task) = spawn_openai_chat_mock().await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("visible-model".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "model": "visible-model",
                            "stream": true,
                            "messages": [{ "role": "user", "content": "ping" }]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&http::HeaderValue::from_static("text/event-stream"))
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect stream")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 stream");

        assert!(text.contains("\"object\":\"chat.completion.chunk\""));
        assert!(text.contains("\"content\":\"pong\""));
        assert!(text.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn v1_responses_requires_external_api_key_for_non_codex_clients() {
        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "model": "visible-model",
                            "input": "ping"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["error"]["type"], "authentication_error");
        assert_eq!(body["error"]["code"], "external_openai_api_disabled");
    }

    #[tokio::test]
    async fn v1_responses_websocket_probe_returns_http_426() {
        let (server, _db) = build_test_server();
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/responses")
                    .header(header::CONNECTION, "Upgrade")
                    .header(header::UPGRADE, "websocket")
                    .header(header::SEC_WEBSOCKET_VERSION, "13")
                    .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], "responses_websocket_not_supported");
    }

    #[tokio::test]
    async fn v1_responses_converts_to_chat_only_backend() {
        let (upstream_base_url, _upstream_task) = spawn_openai_chat_mock().await;
        let (server, db) = build_test_server();
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": upstream_base_url,
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save provider");
        let generated = external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("visible-model".to_string()),
                listen_address: None,
                listen_port: None,
            },
        )
        .expect("enable profile");

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", generated.api_key),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "model": "visible-model",
                            "input": "ping"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["object"], "response");
        assert_eq!(body["output"][0]["type"], "message");
        assert_eq!(body["output"][0]["content"][0]["text"], "pong");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_strips_replayed_item_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "type": "compaction_trigger" },
                {
                    "type": "message",
                    "id": "resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "old turn" }]
                },
                {
                    "type": "reasoning",
                    "id": "rs_resp_chatcmpl-L2v9jyTIwSBd0avrEPO8umbl",
                    "summary": [{ "type": "summary_text", "text": "thinking" }]
                },
                {
                    "type": "agent_message",
                    "id": "msg_amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent done" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "done"
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        let status = response.status();
        let response_body = response
            .into_body()
            .collect()
            .await
            .expect("collect proxy response")
            .to_bytes();
        let captured_pre = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone();
        assert_eq!(
            status,
            StatusCode::OK,
            "proxy returned {status}: {}; captured={}",
            String::from_utf8_lossy(&response_body),
            captured_pre
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        assert_eq!(captured["path"], "/backend-api/codex/responses");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert_eq!(input[0]["type"], "compaction_trigger");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official mock must not receive replayed ids: {item}"
            );
        }
        assert_eq!(input[1]["content"][0]["text"], "old turn");
        assert_eq!(input[2]["summary"][0]["text"], "thinking");
        assert_eq!(input[3]["content"][0]["text"], "subagent done");
        assert_eq!(input[4]["call_id"], "call_1");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_streaming_strips_replayed_item_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_sse_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "stream": true,
            "input": [
                {
                    "type": "message",
                    "id": "resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "old turn" }]
                },
                {
                    "type": "reasoning",
                    "id": "rs_resp_chatcmpl-L2v9jyTIwSBd0avrEPO8umbl",
                    "summary": [{ "type": "summary_text", "text": "thinking" }]
                },
                {
                    "type": "agent_message",
                    "id": "msg_amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent done" }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(response.status(), StatusCode::OK);
        let response_text = String::from_utf8_lossy(
            &response
                .into_body()
                .collect()
                .await
                .expect("collect stream")
                .to_bytes(),
        )
        .to_string();
        let created_pos = response_text
            .find("event: response.created")
            .expect("response.created event");
        let delta_pos = response_text
            .find("event: response.output_text.delta")
            .expect("output_text.delta event");
        let completed_pos = response_text
            .find("event: response.completed")
            .expect("response.completed event");
        assert!(
            created_pos < delta_pos && delta_pos < completed_pos,
            "Codex needs the full Responses lifecycle, otherwise it reconnects: {response_text}"
        );

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official streaming mock must not receive replayed ids: {item}"
            );
        }
        assert_eq!(input[0]["content"][0]["text"], "old turn");
        assert_eq!(input[1]["summary"][0]["text"], "thinking");
        assert_eq!(input[2]["content"][0]["text"], "subagent done");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_compact_official_mock_strips_replayed_item_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "type": "compaction_trigger" },
                {
                    "type": "message",
                    "id": "resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "old turn" }]
                },
                {
                    "type": "reasoning",
                    "id": "rs_resp_chatcmpl-L2v9jyTIwSBd0avrEPO8umbl",
                    "summary": [{ "type": "summary_text", "text": "thinking" }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses/compact")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        let status = response.status();
        let response_body = response
            .into_body()
            .collect()
            .await
            .expect("collect compact response")
            .to_bytes();
        assert_eq!(
            status,
            StatusCode::OK,
            "compact proxy returned {status}: {}",
            String::from_utf8_lossy(&response_body)
        );

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert_eq!(input[0]["type"], "compaction_trigger");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official compact mock must not receive replayed ids: {item}"
            );
        }
        assert_eq!(input[1]["content"][0]["text"], "old turn");
        assert_eq!(input[2]["summary"][0]["text"], "thinking");
    }

    #[tokio::test]
    #[serial]
    async fn codex_cross_provider_chat_then_official_mock_drops_replayed_history_ids() {
        let chat_captured = Arc::new(Mutex::new(None));
        let (chat_base_url, _chat_task) =
            spawn_openai_chat_mock_with_capture(chat_captured.clone()).await;
        let official_captured = Arc::new(Mutex::new(None));
        let (official_base_url, _official_task) =
            spawn_codex_responses_mock(official_captured.clone()).await;
        let mock_official_url = format!("{official_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();

        let mut chat_provider = Provider::with_id(
            "k3-chat".to_string(),
            "Kimi K3".to_string(),
            json!({
                "base_url": chat_base_url,
                "api_key": "sk-k3"
            }),
            None,
        );
        chat_provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &chat_provider)
            .expect("save chat provider");

        let mut official_provider = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "base_url": format!("{official_base_url}/backend-api/codex"),
                "api_key": "sk-official"
            }),
            None,
        );
        official_provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &official_provider)
            .expect("save official provider");

        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "k3",
                        "routes": [
                            {
                                "id": "official",
                                "label": "OpenAI Official",
                                "enabled": true,
                                "targetProviderId": "codex-official",
                                "match": { "models": ["gpt-5.6-luna"] },
                                "upstream": { "apiFormat": "openai_responses" }
                            },
                            {
                                "id": "k3",
                                "label": "Kimi K3",
                                "enabled": true,
                                "targetProviderId": "k3-chat",
                                "match": { "models": ["k3"] },
                                "upstream": { "apiFormat": "openai_chat" }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex proxy config");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");

        let first_body = json!({
            "model": "k3",
            "stream": true,
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "ping" }]
                }
            ]
        });
        let first_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(first_body.to_string()))
                    .expect("first request"),
            )
            .await
            .expect("first response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_text = String::from_utf8_lossy(
            &first_response
                .into_body()
                .collect()
                .await
                .expect("collect first response")
                .to_bytes(),
        )
        .to_string();
        assert!(
            first_text.contains("\"id\":\"msg_resp_chatcmpl_mock\""),
            "chat-sourced message item id should use msg_ prefix: {first_text}"
        );

        let second_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "message",
                    "id": "resp_chatcmpl_mock_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "pong" }]
                },
                {
                    "type": "reasoning",
                    "id": "rs_resp_chatcmpl_mock",
                    "summary": [{ "type": "summary_text", "text": "thinking" }]
                },
                {
                    "type": "agent_message",
                    "id": "msg_amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent done" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "done"
                }
            ]
        });
        let second_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(second_body.to_string()))
                    .expect("second request"),
            )
            .await
            .expect("second response");
        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(second_response.status(), StatusCode::OK);

        let captured = official_captured
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured official request");
        let input = captured["body"]["input"].as_array().expect("input array");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official mock must not receive replayed history ids: {item}"
            );
        }
        assert_eq!(input[0]["content"][0]["text"], "pong");
        assert_eq!(input[1]["summary"][0]["text"], "thinking");
        assert_eq!(input[2]["content"][0]["text"], "subagent done");
        assert_eq!(input[3]["call_id"], "call_1");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_subagent_tool_history_strips_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "spawn subagent" }]
                },
                {
                    "type": "function_call",
                    "id": "fc_spawn_1",
                    "call_id": "call_spawn",
                    "name": "spawn_agent",
                    "arguments": r#"{"task":"check"}"#
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_spawn",
                    "output": "ok"
                },
                {
                    "type": "agent_message",
                    "id": "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent running" }]
                },
                {
                    "type": "function_call",
                    "id": "fc_read_1",
                    "call_id": "call_read",
                    "name": "read_file",
                    "arguments": r#"{"path":"README.md"}"#
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_read",
                    "output": "content"
                },
                {
                    "type": "agent_message",
                    "id": "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent done" }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official mock must not receive subagent/tool ids: {item}"
            );
        }
        assert_eq!(input[0]["content"][0]["text"], "spawn subagent");
        assert_eq!(input[1]["call_id"], "call_spawn");
        assert_eq!(input[1]["name"], "spawn_agent");
        assert_eq!(input[1]["arguments"], r#"{"task":"check"}"#);
        assert_eq!(input[2]["call_id"], "call_spawn");
        assert_eq!(input[2]["output"], "ok");
        assert_eq!(input[3]["content"][0]["text"], "subagent running");
        assert_eq!(input[4]["call_id"], "call_read");
        assert_eq!(input[4]["name"], "read_file");
        assert_eq!(input[4]["arguments"], r#"{"path":"README.md"}"#);
        assert_eq!(input[5]["call_id"], "call_read");
        assert_eq!(input[5]["output"], "content");
        assert_eq!(input[6]["content"][0]["text"], "subagent done");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_recovers_plaintext_encrypted_agent_message() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "agent_message",
                    "id": "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{
                        "type": "encrypted_content",
                        "encrypted_content": "subagent plaintext reply"
                    }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert!(input[0].get("id").is_none());
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "subagent plaintext reply");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_local_tool_round_trip_preserves_call_chain() {
        let captured_request = Arc::new(Mutex::new(None));
        let tool_output = json!([{
            "type": "function_call",
            "id": "fc_call_write",
            "call_id": "call_write",
            "name": "apply_patch",
            "arguments": r#"{"patch":"local"}"#,
            "status": "completed"
        }]);
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock_with_output(captured_request.clone(), tool_output).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let first_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "read file" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_read",
                    "output": "file content"
                }
            ]
        });
        let first_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(first_body.to_string()))
                    .expect("first request"),
            )
            .await
            .expect("first response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_json = response_json(first_response).await;
        assert_eq!(first_json["output"][0]["call_id"], "call_write");
        assert_eq!(first_json["output"][0]["name"], "apply_patch");
        assert_eq!(first_json["output"][0]["arguments"], r#"{"patch":"local"}"#);

        let second_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "function_call",
                    "id": "fc_call_write",
                    "call_id": "call_write",
                    "name": "apply_patch",
                    "arguments": r#"{"patch":"local"}"#
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_write",
                    "output": "done"
                }
            ]
        });
        let second_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(second_body.to_string()))
                    .expect("second request"),
            )
            .await
            .expect("second response");
        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(second_response.status(), StatusCode::OK);

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        for item in input {
            assert!(
                item.get("id").is_none(),
                "official mock must not receive tool ids: {item}"
            );
        }
        assert_eq!(input[0]["call_id"], "call_write");
        assert_eq!(input[0]["name"], "apply_patch");
        assert_eq!(input[0]["arguments"], r#"{"patch":"local"}"#);
        assert_eq!(input[1]["call_id"], "call_write");
        assert_eq!(input[1]["output"], "done");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_preserves_real_agent_message_ciphertext() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex/responses");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let ciphertext = "AAAA".repeat(32);
        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "agent_message",
                    "id": "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{
                        "type": "encrypted_content",
                        "encrypted_content": ciphertext
                    }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert!(input[0].get("id").is_none());
        assert_eq!(input[0]["content"][0]["type"], "encrypted_content");
        assert_eq!(input[0]["content"][0]["encrypted_content"], ciphertext);
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_official_mock_502_preserves_upstream_status_and_allows_reconnect() {
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_502_then_success_mock(captured_requests.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "ping" }]
                }
            ]
        });

        let first_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("first request"),
            )
            .await
            .expect("first response");
        assert_eq!(first_response.status(), StatusCode::BAD_GATEWAY);
        let first_json = response_json(first_response).await;
        assert_eq!(first_json["error"]["code"], "cc_switch_upstream_error");
        assert_eq!(first_json["error"]["upstream_status"], 502);
        assert!(first_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("upstream_status: HTTP 502"));

        let second_response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("second request"),
            )
            .await
            .expect("second response");
        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(
            second_response.status(),
            StatusCode::OK,
            "Codex reconnect after official 502 must not be killed by local policy"
        );

        let requests = captured_requests
            .lock()
            .expect("captured requests lock")
            .clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["path"], "/backend-api/codex/responses");
        assert_eq!(requests[1]["path"], "/backend-api/codex/responses");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_chat_mock_hosted_web_search_replays_full_lifecycle() {
        let chat_captured = Arc::new(Mutex::new(None));
        let (chat_base_url, _chat_task) =
            spawn_openai_chat_mock_with_capture(chat_captured.clone()).await;
        let (server, db) = build_test_server();
        save_codex_chat_mock_router(&db, &chat_base_url).await;

        let request_body = json!({
            "model": "k3",
            "stream": true,
            "tools": [{
                "type": "web_search",
                "external_web_access": true,
                "search_content_types": ["text", "image"]
            }],
            "tool_choice": { "type": "web_search" },
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "search" }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let response_text = String::from_utf8_lossy(
            &response
                .into_body()
                .collect()
                .await
                .expect("collect hosted tool response")
                .to_bytes(),
        )
        .to_string();
        let created_pos = response_text
            .find("event: response.created")
            .expect("response.created event");
        let delta_pos = response_text
            .find("event: response.output_text.delta")
            .expect("output_text.delta event");
        let completed_pos = response_text
            .find("event: response.completed")
            .expect("response.completed event");
        assert!(
            created_pos < delta_pos && delta_pos < completed_pos,
            "hosted web_search must replay the full lifecycle: {response_text}"
        );
        assert!(
            response_text.contains("\"id\":\"msg_resp_chatcmpl_mock\""),
            "chat-sourced message id should use msg_ prefix: {response_text}"
        );

        let chat_body = chat_captured
            .lock()
            .expect("captured chat request lock")
            .clone()
            .expect("captured chat request");
        assert_eq!(chat_body["stream"], false);
        assert_eq!(chat_body["tools"][0]["function"]["name"], "web_search");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_compaction_chat_mock_overflow_trims_history_and_retries() {
        // 端到端验证 Kimi 256K 溢出修复：v2 compaction 首次 401
        // "supports only 256K context" 后，代理必须用裁剪后的历史对同一供应商
        // 重试，而不是直接把 401 抛回客户端。第二次出站请求的 messages 必须
        // 少于第一次，且保留压缩指令。
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let (chat_base_url, _chat_task) =
            spawn_chat_overflow_then_success_mock(captured_requests.clone()).await;
        let (server, db) = build_test_server();
        save_codex_chat_mock_router(&db, &chat_base_url).await;

        let request_body = json!({
            "model": "k3",
            "input": [
                { "type": "compaction_trigger" },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "old question" }] },
                { "type": "function_call", "call_id": "call_1", "name": "read", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "old result" },
                { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "mid answer" }] },
                { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "thinking" }] },
                { "type": "function_call", "call_id": "call_2", "name": "write", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_2", "output": "new result" },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "compact now" }] }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        "x-codex-turn-metadata",
                        r#"{"request_kind":"compaction","compaction":{"trigger":"auto","implementation":"responses_compaction_v2","phase":"pre_turn"}}"#,
                    )
                    .body(Body::from(request_body.to_string()))
                    .expect("compaction request"),
            )
            .await
            .expect("compaction response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "overflowed v2 compaction must be retried with trimmed history, not surfaced as 401"
        );

        let requests = captured_requests
            .lock()
            .expect("captured requests lock")
            .clone();
        assert_eq!(
            requests.len(),
            2,
            "expected original attempt + trimmed retry"
        );
        let first_messages = requests[0]["body"]["messages"]
            .as_array()
            .expect("first upstream messages");
        let retry_messages = requests[1]["body"]["messages"]
            .as_array()
            .expect("retry upstream messages");
        assert!(
            retry_messages.len() < first_messages.len(),
            "trimmed retry must carry fewer messages: first={} retry={}",
            first_messages.len(),
            retry_messages.len()
        );

        let retry_text = requests[1]["body"].to_string();
        assert!(
            retry_text.contains("compact now"),
            "compaction instruction must survive trimming: {retry_text}"
        );
        assert!(
            !retry_text.contains("old question"),
            "oldest history must be trimmed from the retry: {retry_text}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_anthropic_mock_round_trip_uses_msg_prefix() {
        let anthropic_captured = Arc::new(Mutex::new(None));
        let (anthropic_base_url, _anthropic_task) =
            spawn_anthropic_messages_mock(anthropic_captured.clone()).await;
        let (server, db) = build_test_server();

        let mut anthropic_provider = Provider::with_id(
            "claude-anthropic".to_string(),
            "Claude Anthropic".to_string(),
            json!({
                "base_url": anthropic_base_url,
                "api_key": "sk-anthropic"
            }),
            None,
        );
        anthropic_provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic_messages".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &anthropic_provider)
            .expect("save anthropic provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "claude",
                        "routes": [
                            {
                                "id": "claude",
                                "label": "Claude Anthropic",
                                "enabled": true,
                                "targetProviderId": "claude-anthropic",
                                "match": { "models": ["claude"] },
                                "upstream": { "apiFormat": "anthropic_messages" }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex proxy config");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");

        let request_body = json!({
            "model": "claude",
            "stream": true,
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "ping" }]
                }
            ]
        });
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let response_text = String::from_utf8_lossy(
            &response
                .into_body()
                .collect()
                .await
                .expect("collect anthropic response")
                .to_bytes(),
        )
        .to_string();
        assert!(
            response_text.contains("\"id\":\"msg_resp_msg_1_0\""),
            "Anthropic-derived message id should use msg_ prefix: {response_text}"
        );
        assert!(response_text.contains("event: response.completed"));

        let anthropic_body = anthropic_captured
            .lock()
            .expect("captured anthropic request lock")
            .clone()
            .expect("captured anthropic request");
        assert!(
            anthropic_body["body"]["messages"].is_array(),
            "Anthropic mock should receive messages array: {anthropic_body}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_lite_official_mock_strips_replayed_item_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;
        let mock_official_url = format!("{upstream_base_url}/backend-api/codex");
        std::env::set_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL", &mock_official_url);

        let (server, db) = build_test_server();
        save_codex_official_mock_router(&db, &upstream_base_url).await;

        let request_body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                {
                    "type": "message",
                    "id": "resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "old turn" }]
                }
            ]
        });
        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        http::header::HeaderName::from_static(
                            "x-openai-internal-codex-responses-lite",
                        ),
                        "1",
                    )
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        std::env::remove_var("CC_SWITCH_TEST_CODEX_OFFICIAL_MOCK_URL");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert!(
            input[0].get("id").is_none(),
            "responses-lite official mock must not receive replayed ids"
        );
        assert_eq!(input[0]["content"][0]["text"], "old turn");
    }

    #[tokio::test]
    #[serial]
    async fn codex_v1_responses_third_party_mock_preserves_native_agent_message_ids() {
        let captured_request = Arc::new(Mutex::new(None));
        let (upstream_base_url, _upstream_task) =
            spawn_codex_responses_mock(captured_request.clone()).await;

        let (server, db) = build_test_server();
        let mut responses_provider = Provider::with_id(
            "responses-third".to_string(),
            "Responses Third".to_string(),
            json!({
                "base_url": format!("{upstream_base_url}/backend-api/codex"),
                "api_key": "sk-third"
            }),
            None,
        );
        responses_provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &responses_provider)
            .expect("save responses provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "responses",
                        "routes": [
                            {
                                "id": "responses",
                                "label": "Responses Third",
                                "enabled": true,
                                "targetProviderId": "responses-third",
                                "match": { "models": ["k3"] },
                                "upstream": { "apiFormat": "openai_responses" }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex proxy config");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");

        let request_body = json!({
            "model": "k3",
            "input": [
                {
                    "type": "agent_message",
                    "id": "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "subagent done" }]
                },
                {
                    "type": "message",
                    "id": "resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "old turn" }]
                }
            ]
        });

        let response = server
            .build_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/responses")
                    .header(header::AUTHORIZATION, "Bearer PROXY_MANAGED")
                    .header(header::USER_AGENT, "codex/0.146.0-test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let _ = response
            .into_body()
            .collect()
            .await
            .expect("collect third-party response");

        let captured = captured_request
            .lock()
            .expect("captured responses request lock")
            .clone()
            .expect("captured responses request");
        let input = captured["body"]["input"].as_array().expect("input array");
        assert_eq!(input[0]["id"], "amsg_019fc3fa-9b0a-7db1-9ca8-131d56d047ac");
        assert_eq!(
            input[1]["id"],
            "msg_resp_chatcmpl-2gyygAFeaDX2rFNtuG7mOhf9_msg"
        );
    }

    /// 启动一个只服务 OpenAI Chat Completions 的本地 mock upstream。
    async fn spawn_openai_chat_mock() -> (String, tokio::task::JoinHandle<()>) {
        spawn_openai_chat_mock_with_capture(Arc::new(Mutex::new(None))).await
    }

    /// 启动 Codex /responses 官方链路 mock，捕获真实出站 JSON。
    async fn spawn_codex_responses_mock(
        captured_request: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        spawn_codex_responses_mock_with_output(captured_request, json!([])).await
    }

    /// 启动 Codex /responses 官方链路 mock，允许自定义输出项。
    async fn spawn_codex_responses_mock_with_output(
        captured_request: Arc<Mutex<Option<Value>>>,
        output: Value,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().fallback(post(
            move |uri: axum::http::Uri, headers: HeaderMap, body: Bytes| {
                let captured_request = captured_request.clone();
                let output = output.clone();
                async move {
                    let parsed_body =
                        serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                    *captured_request.lock().expect("capture responses request") = Some(json!({
                        "path": uri.path(),
                        "authorization": headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("")
                            .to_string(),
                        "body": parsed_body
                    }));
                    Json(json!({
                        "id": "resp_mock",
                        "object": "response",
                        "created_at": 0,
                        "status": "completed",
                        "model": "gpt-5.6-luna",
                        "output": output,
                        "usage": {
                            "input_tokens": 0,
                            "output_tokens": 0,
                            "total_tokens": 0
                        }
                    }))
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind responses mock upstream");
        let addr = listener.local_addr().expect("responses mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("responses mock upstream serve");
        });
        (format!("http://{addr}"), task)
    }

    /// 启动官方链路的 502→200 mock：第一次回上游 502，之后回成功，
    /// 用于验证 Codex 在官方网络抖动后的重连不会被本地策略永久吃掉。
    async fn spawn_codex_responses_502_then_success_mock(
        captured_requests: Arc<Mutex<Vec<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = Router::new().fallback(post(
            move |uri: axum::http::Uri, headers: HeaderMap, body: Bytes| {
                let captured_requests = captured_requests.clone();
                let attempts = attempts.clone();
                async move {
                    let parsed_body =
                        serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                    captured_requests
                        .lock()
                        .expect("capture requests lock")
                        .push(json!({
                            "path": uri.path(),
                            "authorization": headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("")
                                .to_string(),
                            "body": parsed_body
                        }));
                    if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(json!({
                                "error": {
                                    "message": "upstream gateway timeout",
                                    "type": "server_error"
                                }
                            })),
                        )
                            .into_response();
                    }
                    Json(json!({
                        "id": "resp_mock",
                        "object": "response",
                        "created_at": 0,
                        "status": "completed",
                        "model": "gpt-5.6-luna",
                        "output": [],
                        "usage": {
                            "input_tokens": 0,
                            "output_tokens": 0,
                            "total_tokens": 0
                        }
                    }))
                    .into_response()
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind 502 mock upstream");
        let addr = listener.local_addr().expect("502 mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("502 mock upstream serve");
        });
        (format!("http://{addr}"), task)
    }

    /// 启动 Anthropic Messages mock，捕获出站请求并返回一条文本消息。
    async fn spawn_anthropic_messages_mock(
        captured_request: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/v1/messages",
            post(move |headers: HeaderMap, body: Bytes| {
                let captured_request = captured_request.clone();
                async move {
                    let parsed_body =
                        serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                    *captured_request.lock().expect("capture anthropic request") = Some(json!({
                        "authorization": headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("")
                            .to_string(),
                        "body": parsed_body
                    }));
                    Json(json!({
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "model": "claude",
                        "content": [{ "type": "text", "text": "pong" }],
                        "stop_reason": "end_turn",
                        "usage": {
                            "input_tokens": 1,
                            "output_tokens": 1
                        }
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind anthropic mock upstream");
        let addr = listener.local_addr().expect("anthropic mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("anthropic mock upstream serve");
        });
        (format!("http://{addr}"), task)
    }

    /// 启动 Codex /responses 官方链路的 streaming mock，捕获出站 JSON 并回 SSE。
    async fn spawn_codex_responses_sse_mock(
        captured_request: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().fallback(
            post(
                move |uri: axum::http::Uri, headers: HeaderMap, body: Bytes| {
                    let captured_request = captured_request.clone();
                    async move {
                        let parsed_body =
                            serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                        *captured_request.lock().expect("capture responses request") =
                            Some(json!({
                                "path": uri.path(),
                                "authorization": headers
                                    .get(header::AUTHORIZATION)
                                    .and_then(|value| value.to_str().ok())
                                    .unwrap_or("")
                                    .to_string(),
                                "body": parsed_body
                            }));
                        let sse = concat!(
                            "event: response.created\n",
                            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_mock\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-luna\",\"output\":[]}}\n\n",
                            "event: response.output_text.delta\n",
                            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_mock\",\"output_index\":0,\"delta\":\"pong\"}\n\n",
                            "event: response.completed\n",
                            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_mock\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"gpt-5.6-luna\",\"output\":[{\"type\":\"message\",\"id\":\"msg_mock\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"pong\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":0,\"output_tokens\":1,\"total_tokens\":1}}}\n\n"
                        );
                        ([(header::CONTENT_TYPE, "text/event-stream")], sse)
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind responses sse mock upstream");
        let addr = listener
            .local_addr()
            .expect("responses sse mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("responses sse mock upstream serve");
        });
        (format!("http://{addr}"), task)
    }

    /// 启动一个会保存最近一次请求体的 OpenAI Chat mock，用于断言代理转发前后文本不变。
    async fn spawn_openai_chat_mock_with_capture(
        captured_body: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let captured_body = captured_body.clone();
                async move {
                    *captured_body.lock().expect("capture upstream body") = Some(body.clone());
                    if body.get("stream").and_then(|value| value.as_bool()) == Some(true) {
                        return (
                            [(header::CONTENT_TYPE, "text/event-stream")],
                            "data: {\"id\":\"chatcmpl_mock\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"visible-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"pong\"},\"finish_reason\":null}]}\n\n\
                             data: [DONE]\n\n",
                        )
                            .into_response();
                    }
                    Json(json!({
                        "id": "chatcmpl_mock",
                        "object": "chat.completion",
                        "created": 0,
                        "model": "visible-model",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "pong" },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1,
                            "total_tokens": 2
                        }
                    }))
                    .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock upstream serve");
        });
        (format!("http://{addr}/v1"), task)
    }

    /// 启动 Chat 上游 mock：第一次回 401 Kimi 上下文溢出，之后回正常 chat completion，
    /// 并捕获每次出站请求体供断言裁剪行为。
    async fn spawn_chat_overflow_then_success_mock(
        captured_requests: Arc<Mutex<Vec<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let captured_requests = captured_requests.clone();
                let attempts = attempts.clone();
                async move {
                    captured_requests
                        .lock()
                        .expect("capture requests lock")
                        .push(json!({ "body": body }));
                    if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "error": {
                                    "message": "k3-256k supports only 256K context.",
                                    "type": "authentication_error"
                                }
                            })),
                        )
                            .into_response();
                    }
                    Json(json!({
                        "id": "chatcmpl_compact",
                        "object": "chat.completion",
                        "created": 0,
                        "model": "k3",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "compact summary" },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1,
                            "total_tokens": 2
                        }
                    }))
                    .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind overflow mock upstream");
        let addr = listener.local_addr().expect("overflow mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("overflow mock upstream serve");
        });
        (format!("http://{addr}/v1"), task)
    }

    /// 启动 Codex GPT-Live backend call-create mock，捕获真实 JSON 形态。
    async fn spawn_codex_realtime_backend_mock(
        captured_request: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().fallback(post(
            move |uri: axum::http::Uri, headers: HeaderMap, body: Bytes| {
                let captured_request = captured_request.clone();
                async move {
                    let parsed_body =
                        serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                    let authorization = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let content_type = headers
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    *captured_request.lock().expect("capture realtime request") = Some(json!({
                        "path_and_query": uri
                            .path_and_query()
                            .map(|value| value.as_str())
                            .unwrap_or_else(|| uri.path()),
                        "authorization": authorization,
                        "content_type": content_type,
                        "body": parsed_body
                    }));
                    (
                        [
                            (header::LOCATION, "/v1/live/rtc_123"),
                            (header::CONTENT_TYPE, "application/sdp"),
                        ],
                        "v=answer\r\n",
                    )
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind realtime backend mock upstream");
        let addr = listener.local_addr().expect("realtime mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("realtime backend mock serve");
        });
        (format!("http://{addr}"), task)
    }

    /// 启动一个 raw OpenAI-compatible mock，用于验证 fallback 真实转发未知 `/v1/*`。
    async fn spawn_openai_raw_passthrough_mock(
        captured_request: Arc<Mutex<Option<Value>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/v1/embeddings",
            post(
                move |uri: axum::http::Uri, headers: HeaderMap, body: Bytes| {
                    let captured_request = captured_request.clone();
                    async move {
                        let parsed_body =
                            serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
                        let authorization = headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("")
                            .to_string();
                        let content_type = headers
                            .get(header::CONTENT_TYPE)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("")
                            .to_string();
                        *captured_request.lock().expect("capture raw request") = Some(json!({
                            "path_and_query": uri
                                .path_and_query()
                                .map(|value| value.as_str())
                                .unwrap_or_else(|| uri.path()),
                            "authorization": authorization,
                            "content_type": content_type,
                            "body": parsed_body
                        }));
                        Json(json!({
                            "object": "list",
                            "model": "text-embedding-3-small",
                            "data": [{
                                "object": "embedding",
                                "embedding": [0.0, 1.0],
                                "index": 0
                            }],
                            "usage": {
                                "prompt_tokens": 1,
                                "total_tokens": 1
                            }
                        }))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind raw mock upstream");
        let addr = listener.local_addr().expect("raw mock upstream addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("raw mock upstream serve");
        });
        (format!("http://{addr}/v1"), task)
    }
}
