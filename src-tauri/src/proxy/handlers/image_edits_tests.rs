//! Regression tests for the raw `/v1/images/edits` path.
//!
//! Codex Desktop calls the Images API directly (`POST /v1/images/edits`) instead of
//! going through `/v1/responses`. The endpoint is not registered as a dedicated Axum
//! route, so it is handled by `handle_raw_openai_passthrough`, which forwards the
//! original bytes (multipart) and only parses routing fields. These tests pin down:
//!
//! 1. the endpoint gate for image requests (including the `/codex/v1` alias),
//! 2. multipart routing-field extraction that neither rewrites nor decodes the
//!    uploaded image/mask bytes and that rejects ambiguous `model`/`stream` fields,
//! 3. the endpoint-specific official fallback resolver on schema-v2 MultiRouter
//!    configs: text-only official routes must still carry image edits to managed
//!    OAuth without inheriting the text model override, explicit non-official image
//!    routes must stay untouched, and a disabled/absent official route must not
//!    fall back at all.

use super::*;

use crate::{
    database::Database,
    provider::{CodexOfficialAuthConfig, CodexOfficialAuthMode, ProviderMeta},
    proxy::{
        failover_switch::FailoverSwitchManager,
        provider_router::ProviderRouter,
        providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
        server::ProxyState,
        types::{ProxyConfig, ProxyStatus},
    },
};
use axum::http::{HeaderMap, HeaderValue};
use bytes::Bytes;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

/// Mirrors `tests::build_state` for this child module; the sibling helper is private.
fn build_state(db: Arc<Database>) -> ProxyState {
    ProxyState {
        db: db.clone(),
        config: Arc::new(RwLock::new(ProxyConfig::default())),
        status: Arc::new(RwLock::new(ProxyStatus::default())),
        start_time: Arc::new(RwLock::new(None)),
        current_providers: Arc::new(RwLock::new(HashMap::new())),
        provider_router: Arc::new(ProviderRouter::new(db.clone())),
        gemini_shadow: Arc::new(GeminiShadowStore::default()),
        codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
        app_handle: None,
        failover_manager: Arc::new(FailoverSwitchManager::new(db)),
    }
}

fn multipart_headers(boundary: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
            .expect("multipart content type"),
    );
    headers
}

fn push_text_part(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        )
        .as_bytes(),
    );
}

fn push_binary_part(body: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

fn push_file_part(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    file_name: &str,
    content_type: &str,
    value: &[u8],
) {
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

fn finish_multipart(body: &mut Vec<u8>, boundary: &str) {
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
}

fn multipart_body_with(parts: impl FnOnce(&mut Vec<u8>, &str), boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();
    parts(&mut body, boundary);
    finish_multipart(&mut body, boundary);
    body
}

fn body_contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn effective_provider_points_at_local_proxy(provider: &crate::provider::Provider) -> bool {
    provider
        .settings_config
        .get("base_url")
        .and_then(Value::as_str)
        .is_some_and(|base_url| {
            let base_url = base_url.to_ascii_lowercase();
            base_url.contains("127.0.0.1") || base_url.contains("localhost")
        })
}

fn v2_router(routes: serde_json::Value, default_route_id: &str) -> crate::provider::Provider {
    crate::provider::Provider::with_id(
        "codex-router".to_string(),
        "OpenAI Multi-Model Router".to_string(),
        json!({
            "codexRouting": {
                "schemaVersion": 2,
                "enabled": true,
                "defaultRouteId": default_route_id,
                "routes": routes
            }
        }),
        None,
    )
}

fn v2_route(
    id: &str,
    target_provider_id: &str,
    selection: serde_json::Value,
    auth_source: &str,
) -> serde_json::Value {
    json!({
        "id": id,
        "label": id,
        "enabled": true,
        "targetProviderId": target_provider_id,
        "modelSelection": selection,
        "authPolicy": {"source": auth_source}
    })
}

fn v2_target_provider(
    id: &str,
    api_format: &str,
    models: serde_json::Value,
) -> crate::provider::Provider {
    let mut provider = crate::provider::Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({
            "base_url": format!("https://{id}.example/v1"),
            "auth": {"OPENAI_API_KEY": format!("secret-{id}")},
            "modelCatalog": {"models": models}
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some(api_format.to_string()),
        ..Default::default()
    });
    provider
}

fn managed_oauth_official_provider(
    id: &str,
    models: serde_json::Value,
) -> crate::provider::Provider {
    let mut provider = v2_target_provider(id, "openai_responses", models);
    provider.name = "OpenAI Official".to_string();
    provider.category = Some("official".to_string());
    provider.meta = Some(ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        codex_official_auth: Some(CodexOfficialAuthConfig {
            mode: CodexOfficialAuthMode::ManagedOauth,
            account_id: Some("account-1".to_string()),
        }),
        ..Default::default()
    });
    provider
}

/// `/v1/images/edits` must reach the image branch no matter which local alias the
/// Codex client used; every other raw endpoint must keep the normal raw routing.
#[test]
fn image_edits_endpoint_gate_covers_codex_alias_and_rejects_other_raw_paths() {
    assert!(codex_image_edit_endpoint("/v1/images/edits"));
    assert!(codex_image_edit_endpoint("/v1/images/edits?quality=high"));
    for path in [
        "/v1/responses",
        "/v1/embeddings",
        "/v1/vendor/images/edits",
        "/v1/foo/images/generations",
    ] {
        assert!(!codex_image_edit_endpoint(path));
    }
    for alias in [
        "/codex/v1/images/edits?quality=high",
        "/v1/v1/images/edits?quality=high",
    ] {
        let uri = alias.parse().expect("alias uri");
        let endpoint = raw_openai_passthrough_endpoint_with_query(&uri);
        assert_eq!(endpoint, "/v1/images/edits?quality=high");
        assert!(codex_image_edit_endpoint(&endpoint));
    }
}

/// Binary `image`/`mask` parts must be ignored while `model`/`stream` are read,
/// including when `model` is sent after the image, and the payload must stay identical.
#[tokio::test]
async fn multipart_image_edits_route_body_reads_fields_around_binary_parts() {
    const BOUNDARY: &str = "ccsm-image-edits-boundary";
    // Deliberately invalid UTF-8 so a naive `String::from_utf8` parse of the whole
    // body (or of the image parts) would fail or corrupt the payload.
    const IMAGE: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0xC3, 0x28,
    ];
    const MASK: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x80, 0xFE, 0xFF,
    ];

    let body = multipart_body_with(
        |body, boundary| {
            push_file_part(body, boundary, "image", "edit.png", "image/png", IMAGE);
            push_text_part(body, boundary, "prompt", "make the sky teal");
            push_file_part(body, boundary, "mask", "mask.png", "image/png", MASK);
            push_text_part(body, boundary, "model", "gpt-image-2");
            push_text_part(body, boundary, "stream", "true");
        },
        BOUNDARY,
    );

    let raw_body = Bytes::from(body.clone());
    let route_body =
        parse_image_passthrough_route_body(&multipart_headers(BOUNDARY), raw_body.clone())
            .await
            .expect("multipart routing fields");

    assert_eq!(
        route_body,
        json!({"model": "gpt-image-2", "stream": true}),
        "routing must read model/stream without turning the upload into JSON"
    );
    assert_eq!(
        raw_body.to_vec(),
        body,
        "routing extraction must not rewrite the forwarded bytes"
    );
    assert!(body_contains(&raw_body, IMAGE));
    assert!(body_contains(&raw_body, MASK));
    assert!(body_contains(&raw_body, b"make the sky teal"));
}

/// Ambiguous or malformed routing fields must fail closed instead of silently
/// routing an explicitly addressed image to the official fallback.
#[tokio::test]
async fn multipart_image_edits_route_body_rejects_duplicate_and_malformed_fields() {
    const BOUNDARY: &str = "ccsm-image-edits-invalid";

    let duplicate_model = multipart_body_with(
        |body, boundary| {
            push_text_part(body, boundary, "model", "gpt-image-2");
            push_text_part(body, boundary, "model", "gpt-image-1");
        },
        BOUNDARY,
    );
    let duplicate_stream = multipart_body_with(
        |body, boundary| {
            push_text_part(body, boundary, "model", "gpt-image-2");
            push_text_part(body, boundary, "stream", "true");
            push_text_part(body, boundary, "stream", "false");
        },
        BOUNDARY,
    );
    let model_file_part = multipart_body_with(
        |body, boundary| {
            push_file_part(
                body,
                boundary,
                "model",
                "model.txt",
                "text/plain",
                b"gpt-image-2",
            );
        },
        BOUNDARY,
    );
    let model_binary = multipart_body_with(
        |body, boundary| {
            push_binary_part(body, boundary, "model", &[0xFF, 0xFE, 0x00]);
        },
        BOUNDARY,
    );
    let model_empty = multipart_body_with(
        |body, boundary| {
            push_text_part(body, boundary, "model", "   ");
        },
        BOUNDARY,
    );
    let stream_not_bool = multipart_body_with(
        |body, boundary| {
            push_text_part(body, boundary, "model", "gpt-image-2");
            push_text_part(body, boundary, "stream", "yes");
        },
        BOUNDARY,
    );

    let cases: Vec<(&str, Vec<u8>, HeaderMap)> = vec![
        (
            "duplicate model",
            duplicate_model,
            multipart_headers(BOUNDARY),
        ),
        (
            "duplicate stream",
            duplicate_stream,
            multipart_headers(BOUNDARY),
        ),
        (
            "model sent as file part",
            model_file_part,
            multipart_headers(BOUNDARY),
        ),
        ("non-utf8 model", model_binary, multipart_headers(BOUNDARY)),
        ("empty model", model_empty, multipart_headers(BOUNDARY)),
        (
            "non-boolean stream",
            stream_not_bool,
            multipart_headers(BOUNDARY),
        ),
        (
            "missing boundary parameter",
            multipart_body_with(
                |body, boundary| {
                    push_text_part(body, boundary, "model", "gpt-image-2");
                },
                BOUNDARY,
            ),
            {
                let mut headers = HeaderMap::new();
                headers.insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("multipart/form-data"),
                );
                headers
            },
        ),
        (
            "body boundary mismatch",
            multipart_body_with(
                |body, boundary| {
                    push_text_part(body, boundary, "model", "gpt-image-2");
                },
                "other-boundary",
            ),
            multipart_headers(BOUNDARY),
        ),
    ];

    for (label, body, headers) in cases {
        let error = parse_image_passthrough_route_body(&headers, Bytes::from(body))
            .await
            .expect_err(label);
        assert!(
            matches!(error, ProxyError::InvalidRequest(_)),
            "{label} must be rejected as an invalid request, got {error:?}"
        );
    }
}

/// JSON image edits (and non-JSON garbage) must keep the pre-existing behaviour.
#[tokio::test]
async fn json_image_edits_route_body_parses_and_stays_tolerant() {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    let route_body = parse_image_passthrough_route_body(
        &headers,
        Bytes::from_static(br#"{"model":"gpt-image-2","prompt":"draw","stream":true}"#),
    )
    .await
    .expect("json routing body");
    assert_eq!(
        route_body,
        json!({"model": "gpt-image-2", "prompt": "draw", "stream": true})
    );

    let tolerant = parse_image_passthrough_route_body(&headers, Bytes::from_static(b"not-json"))
        .await
        .expect("non-json body must not fail the forward");
    assert_eq!(tolerant, json!({}));

    let empty = parse_image_passthrough_route_body(&headers, Bytes::new())
        .await
        .expect("empty body");
    assert_eq!(empty, json!({}));
}

/// The reported production bug: a v2 MultiRouter whose official route only lists
/// text models must still serve `gpt-image-2` from managed OAuth, must not inherit
/// the text `upstreamModel`, and must not fall back to the router's local proxy URL.
#[test]
fn image_edits_v2_text_only_official_route_falls_back_without_text_model_override() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let router = v2_router(
        json!([v2_route(
            "official",
            "codex-official",
            json!({"mode": "include", "models": ["gpt-5.5", "gpt-5.4"]}),
            "managed_codex_oauth"
        )]),
        "official",
    );
    let official = managed_oauth_official_provider(
        "codex-official",
        json!([{"model": "gpt-5.5"}, {"model": "gpt-5.4"}]),
    );
    db.save_provider("codex", &router).expect("save router");
    db.save_provider("codex", &official).expect("save official");
    let state = build_state(db);

    let resolved = resolve_codex_image_generation_provider(
        &state,
        &router,
        &json!({"model": "gpt-image-2", "prompt": "edit the uploaded image", "stream": true}),
    )
    .expect("resolve image provider")
    .expect("image edits must fall back to the managed official OAuth route");

    assert_eq!(
        resolved
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref()),
        Some("codex_oauth")
    );
    assert_eq!(
        resolved
            .settings_config
            .get("codexResolvedRouteId")
            .and_then(Value::as_str),
        Some("official")
    );
    assert!(
        resolved
            .settings_config
            .get("codexResolvedUpstreamModelOverride")
            .is_none(),
        "image edits must keep gpt-image-2 instead of inheriting the text route model"
    );
    assert!(
        !effective_provider_points_at_local_proxy(&resolved),
        "the official fallback must not re-enter the local proxy (self loop)"
    );
}

/// A user who explicitly routes `gpt-image-2` to a third-party Images API keeps
/// that route: the endpoint-specific official fallback must step aside.
#[test]
fn image_edits_v2_explicit_third_party_image_route_is_not_hijacked() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let router = v2_router(
        json!([
            v2_route(
                "image-api",
                "image-target",
                json!({"mode": "include", "models": ["gpt-image-2"]}),
                "provider_config"
            ),
            v2_route(
                "official",
                "codex-official",
                json!({"mode": "include", "models": ["gpt-5.5"]}),
                "managed_codex_oauth"
            )
        ]),
        "image-api",
    );
    let image_target = v2_target_provider(
        "image-target",
        "openai_responses",
        json!([{"model": "gpt-image-2"}]),
    );
    let official = managed_oauth_official_provider("codex-official", json!([{"model": "gpt-5.5"}]));
    db.save_provider("codex", &router).expect("save router");
    db.save_provider("codex", &image_target)
        .expect("save image target");
    db.save_provider("codex", &official).expect("save official");
    let state = build_state(db);

    let resolved = resolve_codex_image_generation_provider(
        &state,
        &router,
        &json!({"model": "gpt-image-2", "prompt": "edit the uploaded image"}),
    )
    .expect("resolve image provider");

    assert!(
        resolved.is_none(),
        "an explicit third-party image route must stay owned by the normal router"
    );
}

/// Without an enabled official route there is nothing to fall back to: the
/// resolver must return `None` so the normal fail-closed routing decides.
#[test]
fn image_edits_v2_disabled_or_missing_official_route_does_not_fallback() {
    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("disabled official route", {
            let mut disabled = v2_route(
                "official",
                "codex-official",
                json!({"mode": "include", "models": ["gpt-5.5"]}),
                "managed_codex_oauth",
            );
            disabled["enabled"] = json!(false);
            json!([
                v2_route(
                    "text",
                    "deepseek-target",
                    json!({"mode": "include", "models": ["deepseek-v4-flash"]}),
                    "provider_config"
                ),
                disabled
            ])
        }),
        (
            "no official route at all",
            json!([v2_route(
                "text",
                "deepseek-target",
                json!({"mode": "include", "models": ["deepseek-v4-flash"]}),
                "provider_config"
            )]),
        ),
    ];

    for (label, routes) in cases {
        let db = Arc::new(Database::memory().expect("memory db"));
        let router = v2_router(routes, "text");
        let text_target = v2_target_provider(
            "deepseek-target",
            "openai_chat",
            json!([{"model": "deepseek-v4-flash"}]),
        );
        let official =
            managed_oauth_official_provider("codex-official", json!([{"model": "gpt-5.5"}]));
        db.save_provider("codex", &router).expect("save router");
        db.save_provider("codex", &text_target)
            .expect("save text target");
        db.save_provider("codex", &official).expect("save official");
        let state = build_state(db);

        let resolved = resolve_codex_image_generation_provider(
            &state,
            &router,
            &json!({"model": "gpt-image-2", "prompt": "edit the uploaded image"}),
        )
        .unwrap_or_else(|error| panic!("{label}: resolve failed: {error:?}"));

        assert!(
            resolved.is_none(),
            "{label} must not produce an official image fallback"
        );
    }
}

struct IsolatedHome {
    _dir: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
}
impl IsolatedHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("test home");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings().expect("isolated settings");
        Self {
            _dir: dir,
            previous,
        }
    }
}
impl Drop for IsolatedHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        let _ = crate::settings::reload_settings();
    }
}

#[tokio::test]
#[serial_test::serial]
async fn image_edits_handler_reaches_official_auth_instead_of_router_self_loop() {
    let _home = IsolatedHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let mut router = v2_router(
        json!([v2_route(
            "official",
            "codex-official",
            json!({"mode": "include", "models": ["gpt-5.5"]}),
            "managed_codex_oauth"
        )]),
        "official",
    );
    router.settings_config["base_url"] = json!("http://127.0.0.1:15721/v1");
    db.save_provider("codex", &router).unwrap();
    db.save_provider(
        "codex",
        &managed_oauth_official_provider("codex-official", json!([{"model": "gpt-5.5"}])),
    )
    .unwrap();
    db.set_current_provider("codex", &router.id).unwrap();
    let state = build_state(db);
    for endpoint in [
        "/v1/images/edits",
        "/codex/v1/images/edits",
        "/v1/v1/images/edits",
    ] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(endpoint)
            .header("originator", "codex_cli_rs")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"model":"gpt-image-2","prompt":"test edit"}"#,
            ))
            .unwrap();
        let response = handle_raw_openai_passthrough(State(state.clone()), request)
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let message = body["error"]["message"].as_str().unwrap();
        // No AppHandle is deliberately supplied: reaching official authentication
        // proves the new handler branch ran, without reading credentials or networking.
        assert!(message.contains("Codex OAuth"), "{endpoint}: {body}");
        assert!(message.contains("AppHandle"), "{endpoint}: {body}");
        assert!(
            !message.contains("递归") && !message.contains("self_loop"),
            "{body}"
        );
        assert_eq!(body["error"]["model"], "gpt-image-2");
    }
}

#[test]
fn image_edits_does_not_treat_other_managed_accounts_as_official() {
    let state = build_state(Arc::new(Database::memory().unwrap()));
    for provider_type in ["xai_oauth", "github_copilot"] {
        let mut target = v2_target_provider("third-party", "openai_responses", json!([]));
        target.meta.as_mut().unwrap().provider_type = Some(provider_type.to_string());
        assert!(target.uses_managed_account_auth());
        assert!(resolve_codex_image_generation_provider(
            &state,
            &target,
            &json!({"model":"gpt-image-2"})
        )
        .unwrap()
        .is_none());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn image_edits_handler_preserves_multipart_on_explicit_third_party_route() {
    let _home = IsolatedHome::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let app = axum::Router::new().route(
        "/v1/images/edits",
        axum::routing::post(move |request: axum::extract::Request| {
            let tx = tx.clone();
            async move {
                let (parts, body) = request.into_parts();
                let bytes = body.collect().await.unwrap().to_bytes();
                tx.send((parts.uri, parts.headers, bytes)).await.unwrap();
                axum::Json(json!({"created":1,"data":[{"b64_json":"synthetic"}]}))
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let db = Arc::new(Database::memory().unwrap());
    let mut router = v2_router(
        json!([
            v2_route("images", "images", json!({"mode":"all"}), "provider_config"),
            v2_route(
                "official",
                "codex-official",
                json!({"mode":"all"}),
                "managed_codex_oauth"
            )
        ]),
        "official",
    );
    router.settings_config["base_url"] = json!("http://127.0.0.1:15721/v1");
    let mut target = v2_target_provider(
        "images",
        "openai_responses",
        json!([{"model":"gpt-image-2"}]),
    );
    target.settings_config["base_url"] = json!(format!("http://{addr}/v1"));
    db.save_provider("codex", &router).unwrap();
    db.save_provider("codex", &target).unwrap();
    db.save_provider(
        "codex",
        &managed_oauth_official_provider("codex-official", json!([{"model":"gpt-5.5"}])),
    )
    .unwrap();
    db.set_current_provider("codex", &router.id).unwrap();
    let raw = multipart_body_with(
        |body, boundary| {
            push_file_part(
                body,
                boundary,
                "image[]",
                "source.png",
                "image/png",
                b"\x89PNG\xff\x00",
            );
            push_file_part(
                body,
                boundary,
                "mask",
                "mask.png",
                "image/png",
                b"\x00\xfeMASK",
            );
            push_text_part(body, boundary, "model", "gpt-image-2");
            push_text_part(body, boundary, "prompt", "synthetic test edit");
        },
        "image-boundary",
    );
    let content_type = "multipart/form-data; boundary=image-boundary";
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/images/edits?quality=high")
        .header("originator", "codex_cli_rs")
        .header("content-type", content_type)
        .body(axum::body::Body::from(raw.clone()))
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        handle_raw_openai_passthrough(State(build_state(db)), request),
    )
    .await;
    server.abort();
    let response = response
        .expect("local request timeout")
        .expect("handler response");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let (uri, headers, actual) = rx.recv().await.unwrap();
    assert_eq!(uri.to_string(), "/v1/images/edits?quality=high");
    assert_eq!(headers["content-type"], content_type);
    assert_eq!(actual.as_ref(), raw.as_slice());
}
