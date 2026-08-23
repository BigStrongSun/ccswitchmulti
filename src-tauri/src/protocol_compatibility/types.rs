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
    assert!(!first.endpoint_fingerprint.contains("secret"));
    assert!(!first.endpoint_fingerprint.contains("leak"));
}

#[test]
fn persisted_target_key_never_serializes_the_endpoint_or_path_credentials() {
    let target = ProbeTargetKey::new(
        "provider-a",
        Some("route-a"),
        "public-model",
        "upstream-model",
        TransportKind::OpenAiChat,
        "https://example.test/account-secret/v1/chat/completions?api_key=query-secret",
        "bearer",
    )
    .unwrap();

    let serialized = serde_json::to_string(&target).unwrap();
    assert!(!serialized.contains("https://"));
    assert!(!serialized.contains("account-secret"));
    assert!(!serialized.contains("query-secret"));
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
fn candidate_keeps_probe_credentials_in_memory_without_exposing_them_in_debug() {
    let candidate = ProbeCandidate::new(
        None::<String>,
        None::<String>,
        "public-model",
        "upstream-model",
        TransportKind::OpenAiResponses,
        "https://example.test/v1/responses",
        "bearer",
    )
    .unwrap()
    .with_bearer_token("candidate-secret-token")
    .unwrap()
    .with_full_url(true);

    let debug = format!("{candidate:?}");
    assert!(!debug.contains("candidate-secret-token"));
    assert!(debug.contains("has_bearer_token"));
    assert!(candidate.is_full_url());
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
