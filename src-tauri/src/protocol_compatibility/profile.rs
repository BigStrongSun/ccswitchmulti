use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult, ReasoningProjection,
    ReasoningSemantic, ReasoningSource,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCompatibilityRecord {
    pub target: ProbeTargetKey,
    pub result: ProtocolCompatibilityProbeResult,
    pub tested_at: i64,
    pub expires_at: i64,
}

impl ProtocolCompatibilityRecord {
    pub fn new(
        target: ProbeTargetKey,
        result: ProtocolCompatibilityProbeResult,
        tested_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            target,
            result,
            tested_at,
            expires_at,
        }
    }

    pub fn storage_key(&self) -> String {
        storage_key_for_target(&self.target)
    }

    pub fn automatic_reasoning_projection(&self, now: i64) -> ReasoningProjection {
        if self.expires_at < now || self.result.readiness != ProbeReadiness::Verified {
            return ReasoningProjection::None;
        }

        let Some(selected_transport) = self.result.selected_transport else {
            return ReasoningProjection::None;
        };
        let Some(branch) = self
            .result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == selected_transport)
        else {
            return ReasoningProjection::None;
        };

        match (
            branch.reasoning_shape.semantic,
            branch.reasoning_shape.source,
        ) {
            (ReasoningSemantic::Readable, source) if source != ReasoningSource::None => {
                ReasoningProjection::RawReasoningText
            }
            (ReasoningSemantic::Summary, _) => ReasoningProjection::ReasoningSummary,
            _ => ReasoningProjection::None,
        }
    }
}

pub(crate) fn storage_key_for_target(target: &ProbeTargetKey) -> String {
    let encoded = serde_json::to_vec(target).expect("probe target keys serialize");
    format!("{:x}", Sha256::digest(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol_compatibility::{ProbeReadiness, TransportKind};
    use serde_json::json;

    fn record(
        selected_transport: Option<TransportKind>,
        readiness: ProbeReadiness,
        branches: serde_json::Value,
        expires_at: i64,
    ) -> ProtocolCompatibilityRecord {
        let target = ProbeTargetKey::new(
            "provider",
            None::<String>,
            "public-model",
            "upstream-model",
            TransportKind::OpenAiChat,
            "https://example.test/v1/chat/completions",
            "bearer",
        )
        .expect("target");
        let result: ProtocolCompatibilityProbeResult = serde_json::from_value(json!({
            "selected_transport": selected_transport,
            "readiness": readiness,
            "branches": branches,
        }))
        .expect("probe result");
        ProtocolCompatibilityRecord::new(target, result, 100, expires_at)
    }

    fn branch(transport: TransportKind, semantic: &str, source: &str) -> serde_json::Value {
        json!({
            "assessment": {
                "transport": transport,
                "baseline": "passed",
                "streaming": "passed",
                "forced_tool": "passed",
                "continuation": "passed"
            },
            "reasoning_shape": {
                "semantic": semantic,
                "source": source,
                "pre_tool_visible_content": "absent"
            },
            "evidence": []
        })
    }

    #[test]
    fn verified_readable_selected_branch_projects_raw_reasoning() {
        let record = record(
            Some(TransportKind::OpenAiChat),
            ProbeReadiness::Verified,
            json!([branch(
                TransportKind::OpenAiChat,
                "readable",
                "reasoning_content"
            )]),
            200,
        );

        assert_eq!(
            record.automatic_reasoning_projection(200),
            ReasoningProjection::RawReasoningText
        );
    }

    #[test]
    fn verified_summary_selected_branch_projects_summary() {
        let record = record(
            Some(TransportKind::OpenAiChat),
            ProbeReadiness::Verified,
            json!([branch(
                TransportKind::OpenAiChat,
                "summary",
                "native_responses"
            )]),
            200,
        );

        assert_eq!(
            record.automatic_reasoning_projection(200),
            ReasoningProjection::ReasoningSummary
        );
    }

    #[test]
    fn partial_or_expired_profiles_do_not_project_reasoning() {
        let partial = record(
            Some(TransportKind::OpenAiChat),
            ProbeReadiness::Partial,
            json!([branch(TransportKind::OpenAiChat, "readable", "reasoning")]),
            200,
        );
        let expired = record(
            Some(TransportKind::OpenAiChat),
            ProbeReadiness::Verified,
            json!([branch(TransportKind::OpenAiChat, "readable", "reasoning")]),
            199,
        );

        assert_eq!(
            partial.automatic_reasoning_projection(200),
            ReasoningProjection::None
        );
        assert_eq!(
            expired.automatic_reasoning_projection(200),
            ReasoningProjection::None
        );
    }

    #[test]
    fn unselected_readable_branch_never_upgrades_selected_none() {
        let record = record(
            Some(TransportKind::OpenAiResponses),
            ProbeReadiness::Verified,
            json!([
                branch(TransportKind::OpenAiChat, "readable", "reasoning_content"),
                branch(TransportKind::OpenAiResponses, "none", "none")
            ]),
            200,
        );

        assert_eq!(
            record.automatic_reasoning_projection(200),
            ReasoningProjection::None
        );
    }
}
