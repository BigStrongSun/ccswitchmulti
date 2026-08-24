use serde::{Deserialize, Serialize};

use super::{
    ClassifiedReasoningShape, ProtocolCompatibilityRecord, ReasoningSource, TransportKind,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedResponseShape {
    pub transport: TransportKind,
    pub reasoning_shape: ClassifiedReasoningShape,
    pub tool_call_observed: bool,
    pub field_paths: Vec<String>,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileObservation {
    Match,
    Mismatch { reasons: Vec<ProfileDriftReason> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDriftReason {
    MissingSelectedBranch,
    ReasoningSemantic,
    ReasoningSource,
    PreToolVisibleContent,
    FieldPath,
    EventOrder,
}

pub fn observe_protocol_profile_shape(
    profile: &ProtocolCompatibilityRecord,
    observed: &ObservedResponseShape,
) -> ProfileObservation {
    let Some(branch) = profile.result.branches.iter().find(|branch| {
        branch.assessment.transport == observed.transport
            && profile.result.selected_transport == Some(observed.transport)
    }) else {
        return ProfileObservation::Mismatch {
            reasons: vec![ProfileDriftReason::MissingSelectedBranch],
        };
    };

    let mut reasons = Vec::new();
    if observed.reasoning_shape.semantic != super::ReasoningSemantic::None {
        if branch.reasoning_shape.semantic != observed.reasoning_shape.semantic {
            reasons.push(ProfileDriftReason::ReasoningSemantic);
        }
        if branch.reasoning_shape.source != observed.reasoning_shape.source {
            reasons.push(ProfileDriftReason::ReasoningSource);
        }
    }
    if observed.tool_call_observed
        && branch.reasoning_shape.pre_tool_visible_content
            != observed.reasoning_shape.pre_tool_visible_content
    {
        reasons.push(ProfileDriftReason::PreToolVisibleContent);
    }
    if observed.reasoning_shape.semantic != super::ReasoningSemantic::None {
        if let Some(expected_paths) = reasoning_field_paths(branch.reasoning_shape.source) {
            if !observed
                .field_paths
                .iter()
                .any(|path| expected_paths.contains(&path.as_str()))
            {
                reasons.push(ProfileDriftReason::FieldPath);
            }
        }
    }

    if let Some(expected_events) = branch
        .evidence()
        .iter()
        .map(|evidence| collapse_event_types(&evidence.event_types))
        .find(|events| !events.is_empty())
    {
        let observed_events = collapse_event_types(&observed.event_types);
        if !observed_events.is_empty() && expected_events != observed_events {
            reasons.push(ProfileDriftReason::EventOrder);
        }
    }

    if reasons.is_empty() {
        ProfileObservation::Match
    } else {
        ProfileObservation::Mismatch { reasons }
    }
}

pub(crate) fn observe_and_expire_protocol_profile(
    db: &crate::database::Database,
    target: &super::ProbeTargetKey,
    profile: &ProtocolCompatibilityRecord,
    observed: &ObservedResponseShape,
    now: i64,
) -> ProfileObservation {
    let observation = observe_protocol_profile_shape(profile, observed);
    if let ProfileObservation::Mismatch { reasons } = &observation {
        log::warn!(
            "Codex protocol compatibility profile drifted for provider={} model={} reasons={reasons:?}",
            target.provider_id,
            target.public_model,
        );
        if let Err(error) = db.expire_protocol_compatibility_result(target, now) {
            log::warn!("Failed to expire drifted Codex protocol profile: {error}");
        }
    }
    observation
}

fn reasoning_field_paths(source: ReasoningSource) -> Option<&'static [&'static str]> {
    match source {
        ReasoningSource::ReasoningContent => Some(&[
            "choices[].delta.reasoning_content",
            "choices[].message.reasoning_content",
        ]),
        ReasoningSource::Reasoning => {
            Some(&["choices[].delta.reasoning", "choices[].message.reasoning"])
        }
        ReasoningSource::ReasoningDetails => Some(&[
            "choices[].delta.reasoning_details",
            "choices[].message.reasoning_details",
        ]),
        ReasoningSource::ThinkTags => {
            Some(&["choices[].delta.content", "choices[].message.content"])
        }
        ReasoningSource::NativeResponses => None,
        ReasoningSource::None => None,
    }
}

fn collapse_event_types(events: &[String]) -> Vec<&str> {
    let mut collapsed = Vec::new();
    for event in events {
        let event = event.as_str();
        if collapsed.last().copied() != Some(event) {
            collapsed.push(event);
        }
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::{observe_protocol_profile_shape, ObservedResponseShape, ProfileObservation};
    use crate::protocol_compatibility::{
        ClassifiedReasoningShape, PreToolVisibleContent, ProbeTargetKey,
        ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, ReasoningSemantic,
        ReasoningSource, TransportKind,
    };
    use serde_json::json;

    fn verified_record() -> ProtocolCompatibilityRecord {
        let target = ProbeTargetKey::new(
            "provider-a",
            None::<String>,
            "public-model",
            "upstream-model",
            TransportKind::OpenAiChat,
            "https://example.test/v1/chat/completions",
            "bearer",
        )
        .unwrap();
        let result: ProtocolCompatibilityProbeResult = serde_json::from_value(json!({
            "selected_transport": "open_ai_chat",
            "readiness": "verified",
            "branches": [{
                "assessment": {
                    "transport": "open_ai_chat",
                    "baseline": "passed",
                    "streaming": "passed",
                    "forced_tool": "passed",
                    "continuation": "passed"
                },
                "reasoning_shape": {
                    "semantic": "readable",
                    "source": "reasoning_content",
                    "pre_tool_visible_content": "absent"
                },
                "evidence": [{
                    "status_code": 200,
                    "paths": ["choices[].delta.reasoning_content", "choices[].delta.content"],
                    "fields": [],
                    "event_types": ["data", "data", "done"]
                }]
            }]
        }))
        .expect("verified profile");
        ProtocolCompatibilityRecord::new(target, result, 100, 200)
    }

    fn observed() -> ObservedResponseShape {
        ObservedResponseShape {
            transport: TransportKind::OpenAiChat,
            reasoning_shape: ClassifiedReasoningShape {
                semantic: ReasoningSemantic::Readable,
                source: ReasoningSource::ReasoningContent,
                pre_tool_visible_content: PreToolVisibleContent::Absent,
            },
            tool_call_observed: true,
            field_paths: vec![
                "choices[].delta.reasoning_content".to_string(),
                "choices[].delta.content".to_string(),
            ],
            event_types: vec!["data".to_string(), "data".to_string(), "done".to_string()],
        }
    }

    #[test]
    fn matching_structural_shape_keeps_profile_valid() {
        assert_eq!(
            observe_protocol_profile_shape(&verified_record(), &observed()),
            ProfileObservation::Match
        );
    }

    #[test]
    fn field_source_event_order_and_pre_tool_drift_are_rejected() {
        let profile = verified_record();

        let mut changed_source = observed();
        changed_source.reasoning_shape.source = ReasoningSource::Reasoning;
        changed_source.field_paths = vec!["choices[].delta.reasoning".to_string()];
        assert!(matches!(
            observe_protocol_profile_shape(&profile, &changed_source),
            ProfileObservation::Mismatch { .. }
        ));

        let mut changed_order = observed();
        changed_order.event_types = vec!["done".to_string(), "data".to_string()];
        assert!(matches!(
            observe_protocol_profile_shape(&profile, &changed_order),
            ProfileObservation::Mismatch { .. }
        ));

        let mut pre_tool_drift = observed();
        pre_tool_drift.reasoning_shape.pre_tool_visible_content = PreToolVisibleContent::Present;
        assert!(matches!(
            observe_protocol_profile_shape(&profile, &pre_tool_drift),
            ProfileObservation::Mismatch { .. }
        ));
    }
}
