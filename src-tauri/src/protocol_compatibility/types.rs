use super::{
    HistoryReplay, ManualReasoningOverride, ProbeCandidate, ProbeReadiness, ProbeTargetKey,
    ReasoningProjection, ReasoningSemantic, ReasoningSource, TransportKind,
};

#[test]
fn target_key_fingerprint_removes_credentials_query_and_fragment() {
    let first = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://user:secret@example.test/v1/chat/completions?api_key=leak#fragment",
        "bearer",
    )
    .unwrap();
    let second = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();

    assert_eq!(first.endpoint_fingerprint, second.endpoint_fingerprint);
    assert_eq!(
        first.canonical_endpoint,
        "https://example.test/v1/chat/completions"
    );
    assert!(!first.endpoint_fingerprint.contains("secret"));
    assert!(!first.endpoint_fingerprint.contains("leak"));
}

#[test]
fn readiness_only_allows_automatic_projection_after_full_verification() {
    assert!(ProbeReadiness::Verified.allows_automatic_projection());
    assert!(!ProbeReadiness::Partial.allows_automatic_projection());
    assert!(!ProbeReadiness::Unverified.allows_automatic_projection());
}

#[test]
fn candidate_compiles_without_a_persisted_provider_id() {
    let candidate = ProbeCandidate::new(
        None::<String>,
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();

    assert_eq!(candidate.provider_id, None);
    assert_eq!(candidate.transport, TransportKind::OpenAiResponses);
    assert_eq!(
        candidate.canonical_endpoint(),
        "https://example.test/v1/responses"
    );
}

#[test]
fn target_key_changes_when_the_canonical_endpoint_changes() {
    let first = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/chat/completions",
        "bearer",
    )
    .unwrap();
    let second = ProbeTargetKey::new(
        "provider-a",
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap();

    assert_ne!(first.endpoint_fingerprint, second.endpoint_fingerprint);
}

#[test]
fn manual_override_cannot_turn_opaque_evidence_into_readable_reasoning() {
    let override_profile = ManualReasoningOverride::new(
        ReasoningSemantic::Readable,
        ReasoningSource::ReasoningContent,
        HistoryReplay::ChatReasoningContent,
    );

    assert!(override_profile
        .validate_against(ReasoningSemantic::Opaque)
        .is_err());
}

#[test]
fn manual_summary_override_cannot_request_raw_reasoning_projection() {
    let override_profile = ManualReasoningOverride::new(
        ReasoningSemantic::Summary,
        ReasoningSource::Reasoning,
        HistoryReplay::ChatReasoningContent,
    );

    assert!(override_profile
        .validate_projection(ReasoningProjection::RawReasoningText)
        .is_err());
}
