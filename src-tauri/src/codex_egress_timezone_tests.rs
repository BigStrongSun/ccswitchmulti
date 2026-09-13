use super::codex_egress_timezone::{
    apply_automatic_detection, automatic_probe_cooldown_secs, build_detection_from_payloads,
    classify_timezone_match, is_automatic_probe_due, is_non_public_ip, mask_ip,
    monitor_status_from_parts, parse_cloudflare_trace, resolve_launch_timezone,
    should_run_automatic_probe, should_trigger_probe_for_proxy_error, validate_iana_timezone,
    validate_timezone_settings, AutomaticDetectionOutcome, CodexEgressMonitorRuntime,
    CodexEgressMonitorState, CodexEgressProbeTrigger, CodexTimezoneMatch,
};
use crate::proxy::ProxyError;
use crate::settings::{AppSettings, CodexEgressTimezoneMode, CodexEgressTimezoneSettings};
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn cloudflare_trace_uses_observed_egress_instead_of_dns_fake_ip() {
    let trace = parse_cloudflare_trace("fl=29f421\nip=2407:cdc0:f008:46::\nloc=TW\ncolo=TPE\n")
        .expect("valid Cloudflare trace");

    assert_eq!(trace.ip, "2407:cdc0:f008:46::");
    assert_eq!(trace.country_code.as_deref(), Some("TW"));
    assert_eq!(trace.colo.as_deref(), Some("TPE"));
    assert_eq!(mask_ip(&trace.ip), "2407:cdc0:…");
    assert!(is_non_public_ip(IpAddr::V4(Ipv4Addr::new(198, 18, 0, 14))));
    assert!(is_non_public_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 8))));
    assert!(!is_non_public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
}

#[test]
fn timezone_comparison_distinguishes_identifier_and_current_offset() {
    assert_eq!(
        classify_timezone_match("Asia/Shanghai", "Asia/Shanghai", 1_787_875_200),
        CodexTimezoneMatch::Exact,
    );
    assert_eq!(
        classify_timezone_match("Asia/Shanghai", "Asia/Taipei", 1_787_875_200),
        CodexTimezoneMatch::OffsetMatch,
    );
    assert_eq!(
        classify_timezone_match("Asia/Shanghai", "America/New_York", 1_787_875_200),
        CodexTimezoneMatch::Mismatch,
    );
    assert_eq!(
        classify_timezone_match("China Standard Time", "Asia/Taipei", 1_787_875_200),
        CodexTimezoneMatch::Unknown,
    );
}

#[test]
fn launch_timezone_is_opt_in_and_requires_a_valid_iana_zone() {
    let mut settings = AppSettings::default();
    assert_eq!(resolve_launch_timezone(&settings), None);

    settings.codex_egress_timezone = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_timezone: Some("Asia/Taipei".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    assert_eq!(
        resolve_launch_timezone(&settings).as_deref(),
        Some("Asia/Taipei")
    );

    settings.codex_egress_timezone.mode = CodexEgressTimezoneMode::Manual;
    settings.codex_egress_timezone.manual_timezone = Some("America/Los_Angeles".to_string());
    assert_eq!(
        resolve_launch_timezone(&settings).as_deref(),
        Some("America/Los_Angeles")
    );

    settings.codex_egress_timezone.manual_timezone = Some("China Standard Time".to_string());
    assert_eq!(resolve_launch_timezone(&settings), None);
}

#[test]
fn manual_timezone_validation_uses_the_real_iana_database() {
    assert_eq!(
        validate_iana_timezone("America/Los_Angeles").as_deref(),
        Ok("America/Los_Angeles")
    );
    assert!(validate_iana_timezone("America/Fake").is_err());
    assert!(validate_iana_timezone("China Standard Time").is_err());
    assert!(validate_iana_timezone(" ").is_err());

    let mut settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Manual,
        manual_timezone: Some("America/Fake".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    assert!(validate_timezone_settings(&settings).is_err());
    settings.manual_timezone = Some("America/Los_Angeles".to_string());
    assert!(validate_timezone_settings(&settings).is_ok());
    settings.mode = CodexEgressTimezoneMode::Auto;
    settings.detected_timezone = Some("Asia/Taipei".to_string());
    settings.monitor_interval_minutes = 1;
    assert!(validate_timezone_settings(&settings).is_err());
    settings.monitor_interval_minutes = 15;
    settings.detected_timezone = None;
    assert!(validate_timezone_settings(&settings).is_ok());
}

#[test]
fn detection_report_keeps_fake_dns_diagnostic_but_compares_the_real_egress_zone() {
    let report = build_detection_from_payloads(
        "fl=29f421\nip=2407:cdc0:f008:46::\nloc=TW\ncolo=TPE\n",
        r#"{
          "success": true,
          "country_code": "TW",
          "region": "Taipei",
          "city": "Taipei",
          "timezone": {"id": "Asia/Taipei", "utc": "+08:00"}
        }"#,
        vec!["198.18.0.14".to_string()],
        "Asia/Shanghai",
        1_787_875_200,
        "system_or_transparent",
    )
    .expect("valid detection report");

    assert_eq!(report.target_host, "chatgpt.com");
    assert_eq!(report.dns_addresses, vec!["198.18.0.14"]);
    assert!(report.dns_uses_non_public_address);
    assert_eq!(report.egress_ip, "2407:cdc0:…");
    assert_eq!(report.egress_timezone, "Asia/Taipei");
    assert_eq!(report.current_timezone, "Asia/Shanghai");
    assert_eq!(report.timezone_match, CodexTimezoneMatch::OffsetMatch);
    assert_eq!(report.current_utc_offset, "+08:00");
    assert_eq!(report.egress_utc_offset, "+08:00");
}

#[test]
fn automatic_monitor_only_reacts_to_codex_transport_failures() {
    assert!(should_trigger_probe_for_proxy_error(
        "codex",
        &ProxyError::Timeout("upstream first byte".to_string())
    ));
    assert!(should_trigger_probe_for_proxy_error(
        "codex",
        &ProxyError::ForwardFailed("TLS handshake failed".to_string())
    ));
    assert!(!should_trigger_probe_for_proxy_error(
        "claude",
        &ProxyError::Timeout("upstream first byte".to_string())
    ));
    assert!(!should_trigger_probe_for_proxy_error(
        "codex",
        &ProxyError::AuthError("expired token".to_string())
    ));
    assert!(!should_trigger_probe_for_proxy_error(
        "codex",
        &ProxyError::UpstreamError {
            status: 429,
            body: Some("rate limited".to_string()),
        }
    ));
}

#[test]
fn automatic_monitor_uses_staleness_and_cooldown_without_probing_every_request() {
    let mut settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_at: Some(1_000),
        monitor_interval_minutes: 15,
        ..CodexEgressTimezoneSettings::default()
    };

    assert!(!is_automatic_probe_due(&settings, 1_899, None));
    assert!(is_automatic_probe_due(&settings, 1_901, None));
    assert!(!is_automatic_probe_due(&settings, 2_000, Some(1_950)));
    assert!(is_automatic_probe_due(&settings, 2_041, Some(1_950)));

    settings.mode = CodexEgressTimezoneMode::Off;
    assert!(!is_automatic_probe_due(&settings, 10_000, None));
}

#[test]
fn network_events_can_probe_fresh_cache_but_still_respect_global_cooldown() {
    let settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_at: Some(1_950),
        monitor_interval_minutes: 15,
        ..CodexEgressTimezoneSettings::default()
    };

    assert!(should_run_automatic_probe(
        &settings,
        CodexEgressProbeTrigger::ProxyFailure,
        2_000,
        None,
    ));
    assert!(!should_run_automatic_probe(
        &settings,
        CodexEgressProbeTrigger::Periodic,
        2_000,
        None,
    ));
    assert!(!should_run_automatic_probe(
        &settings,
        CodexEgressProbeTrigger::NetworkResume,
        2_000,
        Some(1_950),
    ));
}

#[test]
fn repeated_probe_failures_back_off_without_exceeding_thirty_minutes() {
    assert_eq!(automatic_probe_cooldown_secs(0), 90);
    assert_eq!(automatic_probe_cooldown_secs(1), 90);
    assert_eq!(automatic_probe_cooldown_secs(2), 180);
    assert_eq!(automatic_probe_cooldown_secs(20), 1_800);
}

#[test]
fn changed_egress_timezone_marks_running_codex_for_safe_refresh() {
    let mut settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_timezone: Some("Asia/Taipei".to_string()),
        last_applied_timezone: Some("Asia/Taipei".to_string()),
        detected_egress_ip: Some("203.0.113.\u{2026}".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    let detection = build_detection_from_payloads(
        "fl=29f421\nip=8.8.8.8\nloc=US\ncolo=LAX\n",
        r#"{
          "success": true,
          "country_code": "US",
          "region": "California",
          "city": "Los Angeles",
          "timezone": {"id": "America/Los_Angeles", "utc": "-07:00"}
        }"#,
        vec!["198.18.0.14".to_string()],
        "Asia/Shanghai",
        2_000,
        "system_or_transparent",
    )
    .expect("valid changed egress");

    let outcome = apply_automatic_detection(
        &mut settings,
        &detection,
        CodexEgressProbeTrigger::ProxyFailure,
        true,
    );

    assert_eq!(outcome, AutomaticDetectionOutcome::RestartRequired);
    assert_eq!(
        settings.detected_timezone.as_deref(),
        Some("America/Los_Angeles")
    );
    assert_eq!(settings.detected_at, Some(2_000));
    assert_eq!(
        settings.last_probe_trigger.as_deref(),
        Some("proxy_failure")
    );
}

#[test]
fn changed_ip_in_same_timezone_does_not_request_a_codex_restart() {
    let mut settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_timezone: Some("Asia/Taipei".to_string()),
        last_applied_timezone: Some("Asia/Taipei".to_string()),
        detected_egress_ip: Some("203.0.113.\u{2026}".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    let detection = build_detection_from_payloads(
        "fl=29f421\nip=8.8.8.8\nloc=TW\ncolo=TPE\n",
        r#"{
          "success": true,
          "country_code": "TW",
          "region": "Taipei",
          "city": "Taipei",
          "timezone": {"id": "Asia/Taipei", "utc": "+08:00"}
        }"#,
        Vec::new(),
        "Asia/Shanghai",
        2_000,
        "system_or_transparent",
    )
    .expect("valid same-zone egress");

    assert_eq!(
        apply_automatic_detection(
            &mut settings,
            &detection,
            CodexEgressProbeTrigger::Periodic,
            true,
        ),
        AutomaticDetectionOutcome::Updated
    );
}

#[test]
fn restart_requirement_compares_against_persisted_applied_timezone() {
    let mut settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        // A previous monitor run already persisted the new detection before CCSM
        // restarted, while the still-running Codex process retained the old TZ.
        detected_timezone: Some("America/Los_Angeles".to_string()),
        last_applied_timezone: Some("Asia/Taipei".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    let detection = build_detection_from_payloads(
        "fl=29f421\nip=8.8.8.8\nloc=US\ncolo=LAX\n",
        r#"{
          "success": true,
          "country_code": "US",
          "region": "California",
          "city": "Los Angeles",
          "timezone": {"id": "America/Los_Angeles", "utc": "-07:00"}
        }"#,
        Vec::new(),
        "Asia/Shanghai",
        2_000,
        "system_or_transparent",
    )
    .expect("valid changed egress");

    assert_eq!(
        apply_automatic_detection(
            &mut settings,
            &detection,
            CodexEgressProbeTrigger::Startup,
            true,
        ),
        AutomaticDetectionOutcome::RestartRequired
    );
}

#[test]
fn monitor_status_exposes_restart_requirement_and_next_automatic_check() {
    let settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_timezone: Some("Asia/Taipei".to_string()),
        detected_at: Some(2_000),
        monitor_interval_minutes: 15,
        last_probe_trigger: Some("proxy_failure".to_string()),
        ..CodexEgressTimezoneSettings::default()
    };
    let runtime = CodexEgressMonitorRuntime {
        restart_required: true,
        ..CodexEgressMonitorRuntime::default()
    };

    let status = monitor_status_from_parts(&settings, &runtime, 2_100);

    assert_eq!(status.state, CodexEgressMonitorState::RestartRequired);
    assert_eq!(status.next_check_at, Some(2_900));
    assert_eq!(status.last_trigger.as_deref(), Some("proxy_failure"));
    assert_eq!(status.detected_timezone.as_deref(), Some("Asia/Taipei"));
}

#[test]
fn monitor_status_exposes_failure_backoff_as_the_next_check() {
    let settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_at: Some(1_000),
        monitor_interval_minutes: 15,
        ..CodexEgressTimezoneSettings::default()
    };
    let runtime = CodexEgressMonitorRuntime {
        last_attempt_at: Some(2_000),
        last_error: Some("network unavailable".to_string()),
        consecutive_failures: 2,
        ..CodexEgressMonitorRuntime::default()
    };

    let status = monitor_status_from_parts(&settings, &runtime, 2_010);

    assert_eq!(status.state, CodexEgressMonitorState::Error);
    assert_eq!(status.next_check_at, Some(2_180));
}

#[test]
fn package_activation_reports_renderer_only_without_a_refresh_loop() {
    let runtime = CodexEgressMonitorRuntime {
        process_timezone_unavailable: true,
        restart_required: true,
        ..CodexEgressMonitorRuntime::default()
    };
    for mode in [
        CodexEgressTimezoneMode::Auto,
        CodexEgressTimezoneMode::Manual,
    ] {
        let settings = CodexEgressTimezoneSettings {
            mode,
            ..CodexEgressTimezoneSettings::default()
        };
        let status = monitor_status_from_parts(&settings, &runtime, 2_010);
        assert_eq!(status.state, CodexEgressMonitorState::RendererOnly);
        assert!(!status.restart_required);
    }
    let status =
        monitor_status_from_parts(&CodexEgressTimezoneSettings::default(), &runtime, 2_010);
    assert_eq!(status.state, CodexEgressMonitorState::Disabled);
}

#[test]
fn restarting_ccsm_preserves_the_running_package_timezone_limitation() {
    let settings = CodexEgressTimezoneSettings {
        mode: CodexEgressTimezoneMode::Auto,
        detected_timezone: Some("Asia/Taipei".into()),
        ..CodexEgressTimezoneSettings::default()
    };
    let mut runtime = CodexEgressMonitorRuntime::default();
    super::codex_egress_timezone::initialize_monitor_launch_state(
        &mut runtime,
        &settings,
        true,
        true,
    );
    assert_eq!(
        monitor_status_from_parts(&settings, &runtime, 0).state,
        CodexEgressMonitorState::RendererOnly
    );
    assert!(!runtime.restart_required);
    super::codex_egress_timezone::initialize_monitor_launch_state(
        &mut runtime,
        &settings,
        true,
        false,
    );
    assert!(runtime.restart_required);
}
#[tokio::test]
#[ignore = "Explicit network diagnostic; never run as a regular test gate"]
async fn live_trace_transport_diagnostic() {
    let started = std::time::Instant::now();
    match crate::codex_egress_timezone::detect_codex_egress_timezone().await {
        Ok(detection) => eprintln!(
            "trace + geolocation passed; path={} elapsed={:?}",
            detection.network_path,
            started.elapsed()
        ),
        Err(error) => {
            panic!("trace elapsed={:?}: {error}", started.elapsed());
        }
    }
}
