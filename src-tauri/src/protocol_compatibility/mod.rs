use std::fmt;

use reqwest::header::{HeaderValue, InvalidHeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

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
pub use provider::{
    apply_probe_selection_to_provider, compile_codex_router_probe_candidates,
    compile_provider_probe_candidates,
};

pub(crate) mod profile;
pub use profile::ProtocolCompatibilityRecord;

pub(crate) mod endpoint;

pub const PROBE_PROFILE_VERSION: u32 = 1;

const BASELINE_PROMPT: &str =
    "CCSM protocol compatibility probe. Solve 17 + 25 internally. Reply only CCSM_PROTOCOL_BASELINE_OK.";
const TOOL_NAME: &str = "ccsm_protocol_compatibility_probe";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCase {
    BaselineJson,
    BaselineSse,
    ForcedToolSse,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryReplay {
    ChatReasoningContent,
    Omit,
    NativeOnly,
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
    bearer_token: Option<HeaderValue>,
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
            bearer_token: None,
        })
    }

    pub fn canonical_endpoint(&self) -> String {
        self.endpoint.to_string()
    }

    pub fn with_full_url(mut self, is_full_url: bool) -> Self {
        self.is_full_url = is_full_url;
        self
    }

    pub fn is_full_url(&self) -> bool {
        self.is_full_url
    }

    pub fn with_bearer_token(mut self, token: &str) -> Result<Self, InvalidHeaderValue> {
        self.bearer_token = Some(HeaderValue::from_str(&format!("Bearer {}", token.trim()))?);
        Ok(self)
    }

    pub(super) fn bearer_token(&self) -> Option<&HeaderValue> {
        self.bearer_token.as_ref()
    }

    pub(crate) fn credential_fingerprint(&self) -> String {
        self.bearer_token
            .as_ref()
            .map(|value| format!("{:x}", Sha256::digest(value.as_bytes())))
            .unwrap_or_default()
    }

    pub(crate) fn lease_key(&self) -> String {
        let material = serde_json::json!({
            "upstreamModel": self.upstream_model,
            "endpoint": self.endpoint.as_str(),
            "authenticationKind": self.authentication_kind,
            "credentialFingerprint": self.credential_fingerprint(),
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
            .field("has_bearer_token", &self.bearer_token.is_some())
            .finish()
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
    let stream = matches!(case, ProbeCase::BaselineSse | ProbeCase::ForcedToolSse);
    let mut request = json!({
        "model": model,
        "stream": stream,
        "store": false,
        "max_output_tokens": 128,
    });

    match case {
        ProbeCase::BaselineJson | ProbeCase::BaselineSse => {
            request["input"] = probe_user_input(BASELINE_PROMPT);
        }
        ProbeCase::ForcedToolSse => {
            request["input"] = probe_user_input(&format!(
                "CCSM protocol compatibility probe. Call the provided function exactly once with nonce {nonce}. After its result, reply only CCSM_PROTOCOL_TOOL_DONE."
            ));
            request["tools"] = json!([{
                "type": "function",
                "name": TOOL_NAME,
                "description": "Internal CCSM protocol compatibility probe. Call exactly once with the supplied nonce.",
                "parameters": {
                    "type": "object",
                    "properties": { "nonce": { "type": "string" } },
                    "required": ["nonce"]
                }
            }]);
            request["tool_choice"] = json!({ "type": "function", "name": TOOL_NAME });
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
