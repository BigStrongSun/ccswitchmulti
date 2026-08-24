use super::{
    select_preferred_transport, select_transport_outcome,
    selection::select_transport_outcome_with_reasoning, ProbeReadiness, ProbeStageStatus,
    ReasoningSemantic, TransportKind, TransportProbeAssessment,
};

fn verified(transport: TransportKind) -> TransportProbeAssessment {
    TransportProbeAssessment {
        transport,
        baseline: ProbeStageStatus::Passed,
        streaming: ProbeStageStatus::Passed,
        forced_tool: ProbeStageStatus::Passed,
        continuation: ProbeStageStatus::Passed,
    }
}

#[test]
fn chooses_native_responses_when_both_protocols_complete_the_full_transaction() {
    assert_eq!(
        select_preferred_transport(&[
            verified(TransportKind::OpenAiChat),
            verified(TransportKind::OpenAiResponses),
        ]),
        Some(TransportKind::OpenAiResponses)
    );
}

#[test]
fn chooses_readable_chat_over_equally_capable_opaque_responses() {
    let selection = select_transport_outcome_with_reasoning(&[
        (
            verified(TransportKind::OpenAiResponses),
            ReasoningSemantic::Opaque,
        ),
        (
            verified(TransportKind::OpenAiChat),
            ReasoningSemantic::Readable,
        ),
    ]);

    assert_eq!(
        selection.map(|selection| selection.transport),
        Some(TransportKind::OpenAiChat)
    );
}

#[test]
fn chooses_readable_chat_over_equally_capable_summary_responses() {
    let selection = select_transport_outcome_with_reasoning(&[
        (
            verified(TransportKind::OpenAiResponses),
            ReasoningSemantic::Summary,
        ),
        (
            verified(TransportKind::OpenAiChat),
            ReasoningSemantic::Readable,
        ),
    ]);

    assert_eq!(
        selection.map(|selection| selection.transport),
        Some(TransportKind::OpenAiChat)
    );
}

#[test]
fn chooses_native_responses_when_capability_and_reasoning_fidelity_tie() {
    let selection = select_transport_outcome_with_reasoning(&[
        (
            verified(TransportKind::OpenAiChat),
            ReasoningSemantic::Readable,
        ),
        (
            verified(TransportKind::OpenAiResponses),
            ReasoningSemantic::Readable,
        ),
    ]);

    assert_eq!(
        selection.map(|selection| selection.transport),
        Some(TransportKind::OpenAiResponses)
    );
}

#[test]
fn preserves_stronger_capability_even_when_its_reasoning_is_opaque() {
    let mut responses = verified(TransportKind::OpenAiResponses);
    let chat = verified(TransportKind::OpenAiChat);
    responses.continuation = ProbeStageStatus::Passed;
    let mut weaker_chat = chat;
    weaker_chat.continuation = ProbeStageStatus::Failed;

    let selection = select_transport_outcome_with_reasoning(&[
        (responses, ReasoningSemantic::Opaque),
        (weaker_chat, ReasoningSemantic::Readable),
    ])
    .unwrap();

    assert_eq!(selection.transport, TransportKind::OpenAiResponses);
    assert_eq!(selection.readiness, ProbeReadiness::Verified);
}

#[test]
fn chooses_chat_when_responses_cannot_complete_tool_replay() {
    let mut responses = verified(TransportKind::OpenAiResponses);
    responses.continuation = ProbeStageStatus::Failed;

    assert_eq!(
        select_preferred_transport(&[responses, verified(TransportKind::OpenAiChat)]),
        Some(TransportKind::OpenAiChat)
    );
}

#[test]
fn chooses_the_more_capable_reachable_protocol_even_when_neither_is_fully_verified() {
    let responses = TransportProbeAssessment {
        transport: TransportKind::OpenAiResponses,
        baseline: ProbeStageStatus::Passed,
        streaming: ProbeStageStatus::Passed,
        forced_tool: ProbeStageStatus::Unsupported,
        continuation: ProbeStageStatus::Skipped,
    };
    let chat = TransportProbeAssessment {
        transport: TransportKind::OpenAiChat,
        baseline: ProbeStageStatus::Passed,
        streaming: ProbeStageStatus::Passed,
        forced_tool: ProbeStageStatus::Passed,
        continuation: ProbeStageStatus::Failed,
    };

    assert_eq!(
        select_preferred_transport(&[responses, chat]),
        Some(TransportKind::OpenAiChat)
    );
}

#[test]
fn refuses_to_select_a_protocol_that_cannot_pass_the_baseline() {
    let unreachable = TransportProbeAssessment {
        transport: TransportKind::OpenAiChat,
        baseline: ProbeStageStatus::Failed,
        streaming: ProbeStageStatus::Skipped,
        forced_tool: ProbeStageStatus::Skipped,
        continuation: ProbeStageStatus::Skipped,
    };

    assert_eq!(select_preferred_transport(&[unreachable]), None);
}

#[test]
fn selected_transport_is_verified_only_after_its_full_transaction_passes() {
    let full = select_transport_outcome(&[verified(TransportKind::OpenAiChat)]).unwrap();
    let partial = select_transport_outcome(&[TransportProbeAssessment {
        transport: TransportKind::OpenAiResponses,
        baseline: ProbeStageStatus::Passed,
        streaming: ProbeStageStatus::Passed,
        forced_tool: ProbeStageStatus::Unsupported,
        continuation: ProbeStageStatus::Skipped,
    }])
    .unwrap();

    assert_eq!(full.readiness, ProbeReadiness::Verified);
    assert_eq!(partial.readiness, ProbeReadiness::Partial);
}
