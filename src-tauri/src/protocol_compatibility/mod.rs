use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    provider::{Provider, ProviderMeta},
    proxy::providers::codex_request::{
        CodexRequestOptions, CodexRequestTransport, CodexThirdPartyRequestPolicy,
        PreparedCodexRequest,
    },
};

mod redaction;
pub use redaction::{redact_json_probe_response, redact_sse_probe_response};

mod classify;
pub(crate) use classify::classify_reasoning_shape;
pub use classify::{
    classify_captured_reasoning_shape, ClassifiedReasoningShape, PreToolVisibleContent,
};

pub mod runtime_observer;

mod runtime_capture;
pub(crate) use runtime_capture::{capture_chat_json_shape, capture_chat_sse_stream};

mod capture;
#[cfg(test)]
pub use capture::{capture_transport_probe, ProbeCaptureError};

mod selection;
#[cfg(test)]
pub use selection::{select_preferred_transport, select_transport_outcome};
pub use selection::{ProbeStageStatus, TransportProbeAssessment};

mod runner;
#[cfg(test)]
pub(crate) use runner::ProbeProgressStage;
pub use runner::{
    run_protocol_compatibility_probe, run_protocol_compatibility_probe_with_reporter,
    ProtocolCompatibilityProbeResult, ProtocolProbeProgressEvent,
};

mod provider;
#[cfg(test)]
pub(crate) use provider::apply_selected_transport_to_provider;
#[cfg(test)]
pub(crate) use provider::compile_provider_probe_candidate;
pub(crate) use provider::compile_provider_probe_candidate_for_model;
pub use provider::{
    apply_probe_selection_to_provider, apply_selected_transport_to_catalog_model,
    compile_codex_router_probe_candidates, compile_provider_probe_candidates,
};

pub(crate) mod profile;
pub use profile::ProtocolCompatibilityRecord;

pub(crate) mod endpoint;

pub const PROBE_PROFILE_VERSION: u32 = 6;
pub(crate) const PROBE_MAX_OUTPUT_TOKENS: u32 = 1024;

const BASELINE_PROMPT: &str =
    "CCSM protocol compatibility probe. Solve 17 + 25 internally. Reply only CCSM_PROTOCOL_BASELINE_OK.";
const TOOL_NAME: &str = "ccsm_protocol_compatibility_probe";
const APPLY_PATCH_TOOL_NAME: &str = "apply_patch";
const APPLY_PATCH_LARK_GRAMMAR: &str = r#"start: begin_patch hunk+ end_patch
begin_patch: "*** Begin Patch" LF
end_patch: "*** End Patch" LF?

hunk: add_hunk | delete_hunk | update_hunk
add_hunk: "*** Add File: " filename LF add_line+
delete_hunk: "*** Delete File: " filename LF
update_hunk: "*** Update File: " filename LF change_move? change?

filename: /(.+)/
add_line: "+" /(.*)/ LF -> line

change_move: "*** Move to: " filename LF
change: (change_context | change_line)+ eof_line?
change_context: ("@@" | "@@ " /(.+)/) LF
change_line: ("+" | "-" | " ") /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF
"#;
const TOOL_DONE_MARKER: &str = "CCSM_PROTOCOL_TOOL_DONE";
const CUSTOM_TOOL_ADMISSION_MARKER: &str = "CCSM_PROTOCOL_CUSTOM_TOOL_ADMISSION_OK";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCase {
    BaselineJson,
    BaselineSse,
    CustomToolAdmissionJson,
    ForcedToolSse,
    ForcedToolRequiredSse,
    ToolContinuationJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    OpenAiChat,
    OpenAiResponses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSemantic {
    Readable,
    Summary,
    Opaque,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSource {
    ReasoningContent,
    Reasoning,
    ReasoningDetails,
    ThinkTags,
    NativeResponses,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryReplay {
    ChatReasoningContent,
    ResponsesReasoningTextContent,
    Omit,
    #[default]
    NativeOnly,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchemaDialect {
    #[default]
    OpenAi,
    MoonshotMfjs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningProjection {
    RawReasoningText,
    ReasoningSummary,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeReadiness {
    Verified,
    Partial,
    Unverified,
}

impl ProbeReadiness {
    pub fn allows_automatic_projection(self) -> bool {
        matches!(self, Self::Verified)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProbeCandidate {
    pub provider_id: Option<String>,
    pub route_id: Option<String>,
    pub public_model: String,
    pub upstream_model: String,
    pub transport: TransportKind,
    endpoint: Url,
    pub authentication_kind: String,
    is_full_url: bool,
    request_policy: Option<CodexThirdPartyRequestPolicy>,
    request_policy_fingerprint: String,
}

impl ProbeCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: Option<impl Into<String>>,
        route_id: Option<impl Into<String>>,
        public_model: impl Into<String>,
        upstream_model: impl Into<String>,
        transport: TransportKind,
        endpoint: &str,
        authentication_kind: impl Into<String>,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            provider_id: provider_id.map(Into::into),
            route_id: route_id.map(Into::into),
            public_model: public_model.into(),
            upstream_model: upstream_model.into(),
            transport,
            endpoint: parse_request_endpoint(endpoint)?,
            authentication_kind: authentication_kind.into(),
            is_full_url: false,
            request_policy: None,
            request_policy_fingerprint: String::new(),
        })
    }

    pub(crate) fn from_request_policy(
        provider_id: Option<impl Into<String>>,
        route_id: Option<impl Into<String>>,
        public_model: impl Into<String>,
        upstream_model: impl Into<String>,
        transport: TransportKind,
        request_policy: CodexThirdPartyRequestPolicy,
    ) -> Result<Self, url::ParseError> {
        let endpoint = parse_request_endpoint(request_policy.canonical_base_url())?;
        let authentication_kind = request_policy.authentication_kind().to_string();
        let is_full_url = request_policy.is_full_url();
        let mut candidate = Self {
            provider_id: provider_id.map(Into::into),
            route_id: route_id.map(Into::into),
            public_model: public_model.into(),
            upstream_model: upstream_model.into(),
            transport,
            endpoint,
            authentication_kind,
            is_full_url,
            request_policy: Some(request_policy),
            request_policy_fingerprint: String::new(),
        };
        candidate.refresh_request_policy_fingerprint();
        Ok(candidate)
    }

    #[cfg(test)]
    pub fn canonical_endpoint(&self) -> String {
        self.endpoint.to_string()
    }

    pub fn with_full_url(mut self, is_full_url: bool) -> Self {
        self.is_full_url = is_full_url;
        if let Some(policy) = self.request_policy.take() {
            self.request_policy = Some(
                policy
                    .with_full_url(is_full_url)
                    .expect("recompiling an already valid request policy must succeed"),
            );
        }
        self.refresh_request_policy_fingerprint();
        self
    }

    #[cfg(test)]
    pub fn is_full_url(&self) -> bool {
        self.is_full_url
    }

    pub fn with_bearer_token(mut self, token: &str) -> Result<Self, String> {
        http::HeaderValue::from_str(&format!("Bearer {}", token.trim()))
            .map_err(|_| "API key cannot be represented as an HTTP header".to_string())?;
        let api_format = match self.transport {
            TransportKind::OpenAiChat => "openai_chat",
            TransportKind::OpenAiResponses => "openai_responses",
        };
        let wire_api = match self.transport {
            TransportKind::OpenAiChat => "chat",
            TransportKind::OpenAiResponses => "responses",
        };
        let mut provider = Provider::with_id(
            self.provider_id
                .clone()
                .unwrap_or_else(|| "unsaved-probe-provider".to_string()),
            "Protocol probe".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": token.trim()},
                "base_url": self.endpoint.as_str(),
                "apiFormat": api_format,
                "config": format!("model = {:?}\nbase_url = {:?}\nwire_api = {:?}\n", self.upstream_model, self.endpoint.as_str(), wire_api),
                "modelCatalog": {"models": [{
                    "model": self.public_model,
                    "upstreamModel": self.upstream_model,
                    "apiFormat": api_format
                }]}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            is_full_url: Some(self.is_full_url),
            ..ProviderMeta::default()
        });
        self.request_policy = Some(
            CodexThirdPartyRequestPolicy::compile(&provider).map_err(|error| error.to_string())?,
        );
        self.authentication_kind = self
            .request_policy
            .as_ref()
            .expect("policy was just compiled")
            .authentication_kind()
            .to_string();
        self.refresh_request_policy_fingerprint();
        Ok(self)
    }

    pub(crate) fn prepare_request(
        &self,
        transport: TransportKind,
        logical_body: Value,
    ) -> Result<PreparedCodexRequest, String> {
        self.prepare_request_with_options(transport, logical_body, CodexRequestOptions::default())
    }

    pub(crate) fn prepare_request_with_options(
        &self,
        transport: TransportKind,
        logical_body: Value,
        options: CodexRequestOptions,
    ) -> Result<PreparedCodexRequest, String> {
        self.request_policy
            .as_ref()
            .ok_or_else(|| "probe candidate has no compiled Provider request policy".to_string())?
            .prepare(request_transport(transport), logical_body, options)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn prepared_endpoint(&self, transport: TransportKind) -> Result<String, String> {
        self.request_policy
            .as_ref()
            .ok_or_else(|| "probe candidate has no compiled Provider request policy".to_string())?
            .prepare_url(request_transport(transport))
            .map_err(|error| error.to_string())
    }

    pub(crate) fn target_key(&self, transport: TransportKind) -> Result<ProbeTargetKey, String> {
        let provider_id = self
            .provider_id
            .as_deref()
            .ok_or_else(|| "providerId is required before persisting probe evidence".to_string())?;
        let endpoint = self.prepared_endpoint(transport)?;
        ProbeTargetKey::new(
            provider_id,
            self.route_id.as_deref(),
            &self.public_model,
            &self.upstream_model,
            transport,
            &endpoint,
            &self.authentication_kind,
        )
        .map_err(|_| "effective probe endpoint is invalid".to_string())
        .map(|target| {
            target
                .with_credential_fingerprint(self.credential_fingerprint())
                .with_request_policy_fingerprint(self.request_policy_fingerprint().to_string())
        })
    }

    pub(crate) fn credential_fingerprint(&self) -> String {
        self.request_policy
            .as_ref()
            .map(|policy| policy.credential_fingerprint().to_string())
            .unwrap_or_default()
    }

    pub(crate) fn request_policy_fingerprint(&self) -> &str {
        &self.request_policy_fingerprint
    }

    fn refresh_request_policy_fingerprint(&mut self) {
        let Some(policy) = self.request_policy.as_ref() else {
            self.request_policy_fingerprint.clear();
            return;
        };
        let mut requests = Vec::new();
        for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
            for case in [
                ProbeCase::BaselineJson,
                ProbeCase::BaselineSse,
                ProbeCase::CustomToolAdmissionJson,
                ProbeCase::ForcedToolSse,
                ProbeCase::ForcedToolRequiredSse,
            ] {
                let logical = build_logical_probe_request(
                    case,
                    &self.upstream_model,
                    "ccsm-policy-fingerprint",
                );
                let Ok(prepared) = policy.prepare(
                    request_transport(transport),
                    logical,
                    CodexRequestOptions::default(),
                ) else {
                    self.request_policy_fingerprint.clear();
                    return;
                };
                let mut header_fingerprints = prepared
                    .headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_string(),
                            format!("{:x}", Sha256::digest(value.as_bytes())),
                        )
                    })
                    .collect::<Vec<_>>();
                header_fingerprints.sort();
                requests.push(json!({
                    "transport": transport,
                    "case": case,
                    "urlFingerprint": format!("{:x}", Sha256::digest(prepared.url.as_bytes())),
                    "headers": header_fingerprints,
                    "body": prepared.body,
                }));
            }
        }
        let material = json!({
            "preparerVersion": crate::proxy::providers::codex_request::CODEX_REQUEST_PREPARER_VERSION,
            "probeProfileVersion": PROBE_PROFILE_VERSION,
            "requests": requests,
        });
        self.request_policy_fingerprint =
            format!("{:x}", Sha256::digest(material.to_string().as_bytes()));
    }

    pub(crate) fn lease_key(&self) -> String {
        let material = serde_json::json!({
            "upstreamModel": self.upstream_model,
            "endpoint": self.endpoint.as_str(),
            "authenticationKind": self.authentication_kind,
            "credentialFingerprint": self.credential_fingerprint(),
            "requestPolicyFingerprint": self.request_policy_fingerprint(),
            "isFullUrl": self.is_full_url,
        });
        format!("{:x}", Sha256::digest(material.to_string().as_bytes()))
    }
}

impl fmt::Debug for ProbeCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProbeCandidate")
            .field("provider_id", &self.provider_id)
            .field("route_id", &self.route_id)
            .field("public_model", &self.public_model)
            .field("upstream_model", &self.upstream_model)
            .field("transport", &self.transport)
            .field("endpoint", &redacted_endpoint_for_debug(&self.endpoint))
            .field("authentication_kind", &self.authentication_kind)
            .field("is_full_url", &self.is_full_url)
            .field("has_bearer_token", &self.request_policy.is_some())
            .field(
                "request_policy_fingerprint",
                &self.request_policy_fingerprint(),
            )
            .finish()
    }
}

fn request_transport(transport: TransportKind) -> CodexRequestTransport {
    match transport {
        TransportKind::OpenAiChat => CodexRequestTransport::ChatCompletions,
        TransportKind::OpenAiResponses => CodexRequestTransport::Responses,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualReasoningOverride {
    pub semantic: ReasoningSemantic,
    pub source: ReasoningSource,
    pub history_replay: HistoryReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningManualOverrideRecord {
    pub target: ProbeTargetKey,
    pub revision: i64,
    pub override_spec: ManualReasoningOverride,
    pub projection: ReasoningProjection,
    pub reason: String,
    pub updated_at: i64,
}

impl ManualReasoningOverride {
    pub fn new(
        semantic: ReasoningSemantic,
        source: ReasoningSource,
        history_replay: HistoryReplay,
    ) -> Self {
        Self {
            semantic,
            source,
            history_replay,
        }
    }

    pub fn validate_against(self, observed: ReasoningSemantic) -> Result<(), &'static str> {
        if observed == ReasoningSemantic::Opaque && self.semantic == ReasoningSemantic::Readable {
            return Err("opaque evidence cannot be projected as readable reasoning");
        }
        if self.semantic == ReasoningSemantic::Readable && self.source == ReasoningSource::None {
            return Err("readable reasoning requires a source");
        }
        if self.source == ReasoningSource::NativeResponses
            && self.history_replay == HistoryReplay::ChatReasoningContent
        {
            return Err("native Responses reasoning cannot replay through Chat reasoning_content");
        }
        Ok(())
    }

    pub fn validate_projection(self, projection: ReasoningProjection) -> Result<(), &'static str> {
        if self.semantic != ReasoningSemantic::Readable
            && projection == ReasoningProjection::RawReasoningText
        {
            return Err("only readable reasoning can use raw reasoning projection");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeTargetKey {
    pub provider_id: String,
    pub route_id: Option<String>,
    pub public_model: String,
    pub upstream_model: String,
    pub transport: TransportKind,
    pub endpoint_fingerprint: String,
    pub authentication_kind: String,
    #[serde(default)]
    pub credential_fingerprint: String,
    #[serde(default)]
    pub request_policy_fingerprint: String,
}

impl ProbeTargetKey {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: impl Into<String>,
        route_id: Option<impl Into<String>>,
        public_model: impl Into<String>,
        upstream_model: impl Into<String>,
        transport: TransportKind,
        endpoint: &str,
        authentication_kind: impl Into<String>,
    ) -> Result<Self, url::ParseError> {
        let parsed = fingerprint_endpoint(endpoint)?;

        let endpoint_fingerprint = format!("{:x}", Sha256::digest(parsed.as_str().as_bytes()));

        Ok(Self {
            provider_id: provider_id.into(),
            route_id: route_id.map(Into::into),
            public_model: public_model.into(),
            upstream_model: upstream_model.into(),
            transport,
            endpoint_fingerprint,
            authentication_kind: authentication_kind.into(),
            credential_fingerprint: String::new(),
            request_policy_fingerprint: String::new(),
        })
    }

    pub fn with_credential(mut self, credential: &str) -> Self {
        self.credential_fingerprint = format!(
            "{:x}",
            Sha256::digest(format!("Bearer {}", credential.trim()).as_bytes())
        );
        self
    }

    pub(crate) fn with_credential_fingerprint(mut self, fingerprint: String) -> Self {
        self.credential_fingerprint = fingerprint;
        self
    }

    pub(crate) fn with_request_policy_fingerprint(mut self, fingerprint: String) -> Self {
        self.request_policy_fingerprint = fingerprint;
        self
    }
}

fn parse_request_endpoint(endpoint: &str) -> Result<Url, url::ParseError> {
    let mut parsed = Url::parse(endpoint)?;
    parsed.set_fragment(None);
    Ok(parsed)
}

fn fingerprint_endpoint(endpoint: &str) -> Result<Url, url::ParseError> {
    parse_request_endpoint(endpoint)
}

fn redacted_endpoint_for_debug(endpoint: &Url) -> String {
    let mut redacted = endpoint.clone();
    redacted
        .set_username("")
        .expect("parsed URL username is mutable");
    redacted
        .set_password(None)
        .expect("parsed URL password is mutable");
    redacted.set_query(None);
    redacted.to_string()
}

pub fn build_logical_probe_request(case: ProbeCase, model: &str, nonce: &str) -> Value {
    let stream = matches!(
        case,
        ProbeCase::BaselineSse | ProbeCase::ForcedToolSse | ProbeCase::ForcedToolRequiredSse
    );
    let mut request = json!({
        "model": model,
        "stream": stream,
        "store": false,
        "max_output_tokens": PROBE_MAX_OUTPUT_TOKENS,
    });

    match case {
        ProbeCase::BaselineJson | ProbeCase::BaselineSse => {
            request["input"] = probe_user_input(BASELINE_PROMPT);
        }
        ProbeCase::CustomToolAdmissionJson => {
            request["input"] = probe_user_input(&format!(
                "CCSM protocol compatibility probe. Do not call any tool. Reply only {CUSTOM_TOOL_ADMISSION_MARKER}."
            ));
            request["tools"] = json!([{
                "type": "custom",
                "name": APPLY_PATCH_TOOL_NAME,
                "description": "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.",
                "format": {
                    "type": "grammar",
                    "syntax": "lark",
                    "definition": APPLY_PATCH_LARK_GRAMMAR
                }
            }]);
            request["tool_choice"] = json!("auto");
        }
        ProbeCase::ForcedToolSse | ProbeCase::ForcedToolRequiredSse => {
            request["input"] = probe_user_input(&format!(
                "CCSM protocol compatibility probe. You must call the only provided function `{TOOL_NAME}` exactly once with nonce {nonce}. Use mode direct. Do not answer with text before the call. After the function result, reply only {TOOL_DONE_MARKER}."
            ));
            request["tools"] = json!([{
                "type": "function",
                "name": TOOL_NAME,
                "description": "Internal CCSM protocol compatibility probe. Call exactly once with the supplied nonce.",
                "parameters": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "nonce": {"const": nonce},
                                "mode": {"const": "direct"}
                            },
                            "required": ["nonce", "mode"],
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "properties": {
                                "nonce": {"const": nonce},
                                "mode": {"enum": ["routed", "suggested"]},
                                "destination": {
                                    "$ref": "#/$defs/probe_destination"
                                }
                            },
                            "required": ["nonce", "mode", "destination"],
                            "additionalProperties": false
                        },
                        {"type": "null"}
                    ],
                    "$defs": {
                        "probe_destination": {
                            "type": "object",
                            "properties": {
                                "kind": {"type": "string"},
                                "id": {"$ref": "#/$defs/probe_identifier"}
                            }
                        },
                        "probe_identifier": {
                            "$ref": "#/$defs/probe_identifier_base",
                            "type": "string",
                            "format": "uuid",
                            "minLength": 1,
                            "description": "Representative Codex dynamic-tool identifier."
                        },
                        "probe_identifier_base": {
                            "type": "string"
                        }
                    }
                }
            }]);
            request["tool_choice"] = json!(if case == ProbeCase::ForcedToolRequiredSse {
                "required"
            } else {
                "auto"
            });
        }
        ProbeCase::ToolContinuationJson => {}
    }

    request
}

fn probe_user_input(text: &str) -> Value {
    json!([{
        "role": "user",
        "content": [{ "type": "input_text", "text": text }]
    }])
}

#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod cases;
#[cfg(test)]
mod classify_tests;
#[cfg(test)]
mod provider_tests;
#[cfg(test)]
mod redaction_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod types;
