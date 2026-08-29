use std::{collections::BTreeSet, fmt};

use http::{header, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    protocol_compatibility::{HistoryReplay, ToolSchemaDialect},
    provider::{LocalProxyRequestOverrides, Provider, ProviderMeta},
    proxy::{
        body_filter::filter_private_params_with_whitelist,
        error::ProxyError,
        json_canonical::{canonical_json_string, canonicalize_value},
    },
};

use super::{
    apply_codex_upstream_model, codex_provider_text_only_input, inject_codex_chat_prompt_cache_key,
    prepare_codex_native_responses_model, provider_needs_responses_namespace_flatten,
    resolve_codex_cache_config, resolve_codex_chat_reasoning_config,
    transform_codex_chat::{
        apply_hosted_tool_switches_to_chat_body, build_codex_tool_context_from_request,
        responses_to_chat_completions_with_reasoning_text_only_and_cache,
    },
    CodexAdapter, ProviderAdapter,
};

pub(crate) const CODEX_REQUEST_PREPARER_VERSION: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexRequestTransport {
    Responses,
    ChatCompletions,
}

impl CodexRequestTransport {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Responses => "/v1/responses",
            Self::ChatCompletions => "/v1/chat/completions",
        }
    }

    fn endpoint_suffix(self) -> &'static str {
        match self {
            Self::Responses => "/responses",
            Self::ChatCompletions => "/chat/completions",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodexRequestOptions {
    pub hosted_web_search_enabled: bool,
    pub hosted_image_generation_enabled: bool,
    pub prompt_cache_session_id: Option<String>,
    pub tool_schema_dialect: Option<ToolSchemaDialect>,
    pub history_replay: Option<HistoryReplay>,
}

#[derive(Clone)]
pub(crate) struct PreparedCodexRequest {
    pub url: String,
    pub headers: HeaderMap,
    pub body: Value,
}

impl fmt::Debug for PreparedCodexRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names = self
            .headers
            .keys()
            .map(HeaderName::as_str)
            .collect::<BTreeSet<_>>();
        formatter
            .debug_struct("PreparedCodexRequest")
            .field("url", &redacted_url(&self.url))
            .field("header_names", &header_names)
            .field(
                "body_keys",
                &self
                    .body
                    .as_object()
                    .map(|body| body.keys().cloned().collect::<BTreeSet<_>>()),
            )
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct CodexThirdPartyRequestPolicy {
    provider: Provider,
    base_url: String,
    auth_headers: HeaderMap,
    authentication_kind: String,
    credential_fingerprint: String,
    fingerprint: String,
    is_full_url: bool,
}

impl CodexThirdPartyRequestPolicy {
    pub(crate) fn compile(provider: &Provider) -> Result<Self, ProxyError> {
        if provider.uses_managed_account_auth()
            || provider.category.as_deref() == Some("official")
            || super::is_codex_official_provider(provider)
        {
            return Err(ProxyError::ConfigError(
                "managed or official providers do not use third-party request preparation"
                    .to_string(),
            ));
        }

        let adapter = CodexAdapter::new();
        let base_url = adapter.extract_base_url(provider)?;
        Url::parse(&base_url).map_err(|_| {
            ProxyError::ConfigError("Codex Provider base_url must be an absolute URL".to_string())
        })?;
        let auth = adapter.extract_auth(provider).ok_or_else(|| {
            ProxyError::ConfigError("Codex Provider is missing third-party authentication".into())
        })?;
        let authentication_kind = format!("{:?}", auth.strategy).to_ascii_lowercase();
        let credential_fingerprint =
            sha256_hex(format!("{}:{}", authentication_kind, auth.api_key.trim()).as_bytes());
        let mut auth_headers = HeaderMap::new();
        for (name, value) in adapter.get_auth_headers(&auth)? {
            auth_headers.append(name, value);
        }

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);
        let provider_value = serde_json::to_value(provider).map_err(|error| {
            ProxyError::Internal(format!(
                "Failed to fingerprint Codex Provider policy: {error}"
            ))
        })?;
        let fingerprint_material = serde_json::json!({
            "preparerVersion": CODEX_REQUEST_PREPARER_VERSION,
            "provider": provider_value,
            "credentialFingerprint": credential_fingerprint,
            "probeBudget": crate::protocol_compatibility::PROBE_MAX_OUTPUT_TOKENS,
            "probeCorpusVersion": 4,
        });
        let fingerprint = sha256_hex(canonical_json_string(&fingerprint_material).as_bytes());

        Ok(Self {
            provider: provider.clone(),
            base_url,
            auth_headers,
            authentication_kind,
            credential_fingerprint,
            fingerprint,
            is_full_url,
        })
    }

    pub(crate) fn with_full_url(mut self, is_full_url: bool) -> Result<Self, ProxyError> {
        self.provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .is_full_url = Some(is_full_url);
        Self::compile(&self.provider)
    }

    pub(crate) fn prepare(
        &self,
        transport: CodexRequestTransport,
        logical_body: Value,
        options: CodexRequestOptions,
    ) -> Result<PreparedCodexRequest, ProxyError> {
        let url = self.prepare_url(transport)?;
        let body = self.prepare_body(transport, logical_body, &options)?;
        let headers = self.prepare_headers();
        Ok(PreparedCodexRequest { url, headers, body })
    }

    pub(crate) fn prepare_url(
        &self,
        transport: CodexRequestTransport,
    ) -> Result<String, ProxyError> {
        self.prepare_url_with_query(transport, None)
    }

    pub(crate) fn prepare_url_with_query(
        &self,
        transport: CodexRequestTransport,
        passthrough_query: Option<&str>,
    ) -> Result<String, ProxyError> {
        if self.is_full_url {
            return derive_full_endpoint(&self.base_url, transport)
                .map(|url| append_query(&url, passthrough_query));
        }
        Ok(append_query(
            &CodexAdapter::new().build_url(&self.base_url, transport.endpoint()),
            passthrough_query,
        ))
    }

    pub(crate) fn prepare_body(
        &self,
        transport: CodexRequestTransport,
        logical_body: Value,
        options: &CodexRequestOptions,
    ) -> Result<Value, ProxyError> {
        let body = self.prepare_protocol_body(transport, logical_body, options)?;
        self.finalize_body(transport, body, options)
    }

    pub(crate) fn prepare_protocol_body(
        &self,
        transport: CodexRequestTransport,
        logical_body: Value,
        options: &CodexRequestOptions,
    ) -> Result<Value, ProxyError> {
        let body = match transport {
            CodexRequestTransport::ChatCompletions => {
                self.prepare_chat_body(logical_body, options)?
            }
            CodexRequestTransport::Responses => self.prepare_responses_body(logical_body)?,
        };
        Ok(body)
    }

    pub(crate) fn apply_body_policy(&self, body: Value) -> Value {
        apply_provider_body_policy(&self.provider, body)
    }

    pub(crate) fn finalize_body(
        &self,
        transport: CodexRequestTransport,
        body: Value,
        options: &CodexRequestOptions,
    ) -> Result<Value, ProxyError> {
        let mut body = self.apply_body_policy(body);
        if transport == CodexRequestTransport::Responses {
            body = match options.history_replay.unwrap_or(HistoryReplay::NativeOnly) {
                HistoryReplay::ResponsesReasoningTextContent => {
                    super::openai_compat::normalize_third_party_responses_reasoning_items(body)
                }
                HistoryReplay::Omit => omit_responses_reasoning_items(body),
                HistoryReplay::NativeOnly | HistoryReplay::ChatReasoningContent => body,
            };
        }
        super::codex_tool_schema::compile_tool_schemas(
            &mut body,
            options
                .tool_schema_dialect
                .unwrap_or(ToolSchemaDialect::OpenAi),
        )?;
        if transport == CodexRequestTransport::Responses
            && provider_needs_responses_namespace_flatten(&self.provider)
        {
            super::transform_codex_responses_xai_sanitize::sanitize_xai_responses_request(
                &mut body,
            );
        }
        Ok(body)
    }

    pub(crate) fn prepare_headers(&self) -> HeaderMap {
        let mut headers = self.auth_headers.clone();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        if let Some(user_agent) = self
            .provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
        {
            headers.insert(header::USER_AGENT, user_agent);
        }
        apply_provider_header_overrides(
            &mut headers,
            self.provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
        );
        headers
    }

    pub(crate) fn auth_header_pairs(&self) -> Vec<(HeaderName, HeaderValue)> {
        self.auth_headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn credential_fingerprint(&self) -> &str {
        &self.credential_fingerprint
    }

    pub(crate) fn authentication_kind(&self) -> &str {
        &self.authentication_kind
    }

    pub(crate) fn canonical_base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn is_full_url(&self) -> bool {
        self.is_full_url
    }

    fn prepare_chat_body(
        &self,
        mut logical_body: Value,
        options: &CodexRequestOptions,
    ) -> Result<Value, ProxyError> {
        let explicit_prompt_cache_key = logical_body
            .get("prompt_cache_key")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        apply_codex_upstream_model(&self.provider, &mut logical_body);
        let reasoning_config = resolve_codex_chat_reasoning_config(&self.provider, &logical_body);
        let text_only_override = codex_provider_text_only_input(&self.provider);
        let cache_config = resolve_codex_cache_config(&self.provider, &logical_body);
        let mut tool_context = build_codex_tool_context_from_request(&logical_body);
        tool_context.apply_hosted_tool_switches(
            options.hosted_web_search_enabled,
            options.hosted_image_generation_enabled,
        );
        let mut body = responses_to_chat_completions_with_reasoning_text_only_and_cache(
            logical_body,
            reasoning_config.as_ref(),
            text_only_override,
            Some(&cache_config),
        )?;
        apply_hosted_tool_switches_to_chat_body(&mut body, &tool_context);
        inject_codex_chat_prompt_cache_key(
            &self.provider,
            &mut body,
            explicit_prompt_cache_key.as_deref(),
            options.prompt_cache_session_id.as_deref(),
        );
        Ok(body)
    }

    fn prepare_responses_body(&self, mut logical_body: Value) -> Result<Value, ProxyError> {
        prepare_codex_native_responses_model(&self.provider, &mut logical_body)?;
        Ok(logical_body)
    }
}

fn omit_responses_reasoning_items(mut body: Value) -> Value {
    if let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) {
        input.retain(|item| item.get("type").and_then(Value::as_str) != Some("reasoning"));
    }
    body
}

impl fmt::Debug for CodexThirdPartyRequestPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexThirdPartyRequestPolicy")
            .field("provider_id", &self.provider.id)
            .field("base_url", &redacted_url(&self.base_url))
            .field("authentication_kind", &self.authentication_kind)
            .field("has_authentication", &!self.auth_headers.is_empty())
            .field("is_full_url", &self.is_full_url)
            .field("fingerprint", &self.fingerprint)
            .finish()
    }
}

impl PartialEq for CodexThirdPartyRequestPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
    }
}

impl Eq for CodexThirdPartyRequestPolicy {}

pub(crate) fn apply_provider_body_policy(provider: &Provider, body: Value) -> Value {
    let mut body = canonicalize_value(filter_private_params_with_whitelist(body, &[]));
    if let Some(overrides) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
    {
        if apply_provider_body_overrides(&mut body, overrides) {
            body = canonicalize_value(filter_private_params_with_whitelist(body, &[]));
        }
    }
    body
}

pub(crate) fn apply_provider_header_policy(provider: &Provider, headers: &mut HeaderMap) {
    if let Ok(Some(user_agent)) = provider
        .meta
        .as_ref()
        .map(|meta| meta.custom_user_agent_header())
        .unwrap_or(Ok(None))
    {
        headers.insert(header::USER_AGENT, user_agent);
    }
    apply_provider_header_overrides(
        headers,
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
    );
}

fn apply_provider_body_overrides(body: &mut Value, overrides: &LocalProxyRequestOverrides) -> bool {
    let Some(override_body) = overrides.body.as_ref() else {
        return false;
    };
    if !override_body.is_object() {
        log::warn!("[LocalProxyOverrides] Ignoring body override because it is not an object");
        return false;
    }
    merge_json_override_inner(body, override_body, true)
}

fn merge_json_override_inner(target: &mut Value, patch: &Value, is_top_level: bool) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    log::warn!(
                        "[LocalProxyOverrides] Ignoring body override for protected field: stream"
                    );
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override_inner(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

fn apply_provider_header_overrides(
    headers: &mut HeaderMap,
    overrides: Option<&LocalProxyRequestOverrides>,
) {
    let Some(header_overrides) = overrides.map(|overrides| &overrides.headers) else {
        return;
    };
    for (raw_name, raw_value) in header_overrides {
        let normalized_name = raw_name.trim().to_ascii_lowercase();
        let Ok(name) = HeaderName::from_bytes(normalized_name.as_bytes()) else {
            log::warn!("[LocalProxyOverrides] Ignoring invalid header override name: {raw_name}");
            continue;
        };
        if protected_provider_override_header(&name) {
            log::debug!(
                "[LocalProxyOverrides] Ignoring protected header override: {}",
                name.as_str()
            );
            continue;
        }
        let Ok(value) = HeaderValue::from_str(raw_value) else {
            log::warn!(
                "[LocalProxyOverrides] Ignoring invalid header override value for {}",
                name.as_str()
            );
            continue;
        };
        headers.insert(name, value);
    }
}

fn protected_provider_override_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "upgrade"
            | "accept-encoding"
            | "content-type"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "chatgpt-account-id"
            | "session_id"
            | "x-client-request-id"
            | "x-codex-window-id"
            | "x-forwarded-host"
            | "x-forwarded-port"
            | "x-forwarded-proto"
            | "forwarded"
            | "cf-connecting-ip"
            | "cf-ipcountry"
            | "cf-ray"
            | "cf-visitor"
            | "true-client-ip"
            | "fastly-client-ip"
            | "x-azure-clientip"
            | "x-azure-fdid"
            | "x-azure-ref"
            | "akamai-origin-hop"
            | "x-akamai-config-log-detail"
            | "x-request-id"
            | "x-correlation-id"
            | "x-trace-id"
            | "x-amzn-trace-id"
            | "x-b3-traceid"
            | "x-b3-spanid"
            | "x-b3-parentspanid"
            | "x-b3-sampled"
            | "traceparent"
            | "tracestate"
    )
}

fn derive_full_endpoint(
    base_url: &str,
    transport: CodexRequestTransport,
) -> Result<String, ProxyError> {
    let mut parsed = Url::parse(base_url)
        .map_err(|_| ProxyError::ConfigError("Codex Provider full URL is invalid".to_string()))?;
    let suffix = transport.endpoint_suffix();
    let path = parsed.path().trim_end_matches('/').to_string();
    if path.ends_with(suffix) {
        parsed.set_path(&path);
        return Ok(parsed.to_string());
    }

    let next_path = if let Some(index) = path.find("/v1/") {
        format!("{}/v1{suffix}", &path[..index])
    } else if ends_with_version_segment(&path) {
        format!("{path}{suffix}")
    } else if let Some(root) = match transport {
        CodexRequestTransport::ChatCompletions => path.strip_suffix("/responses"),
        CodexRequestTransport::Responses => path.strip_suffix("/chat/completions"),
    } {
        format!("{root}{suffix}")
    } else {
        return Err(ProxyError::ConfigError(format!(
            "Cannot derive {} endpoint from Codex Provider full URL",
            suffix
        )));
    };
    parsed.set_path(&next_path);
    Ok(parsed.to_string())
}

fn append_query(base_url: &str, query: Option<&str>) -> String {
    match query.map(str::trim).filter(|query| !query.is_empty()) {
        Some(query) if base_url.contains('?') => format!("{base_url}&{query}"),
        Some(query) => format!("{base_url}?{query}"),
        None => base_url.to_string(),
    }
}

fn ends_with_version_segment(path: &str) -> bool {
    let Some(segment) = path.rsplit('/').next() else {
        return false;
    };
    let Some(version) = segment.strip_prefix('v') else {
        return false;
    };
    !version.is_empty() && version.chars().all(|character| character.is_ascii_digit())
}

fn redacted_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return "<invalid-url>".to_string();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
