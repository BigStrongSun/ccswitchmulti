use super::codex_egress_timezone::{
    build_detection_from_payloads, classify_timezone_match, is_non_public_ip, mask_ip,
    parse_cloudflare_trace, resolve_launch_timezone, validate_iana_timezone,
    validate_timezone_settings, CodexTimezoneMatch,
};
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
