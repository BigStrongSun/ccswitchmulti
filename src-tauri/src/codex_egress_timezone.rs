use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use chrono::{Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::settings::{AppSettings, CodexEgressTimezoneMode, CodexEgressTimezoneSettings};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CloudflareTrace {
    pub(crate) ip: String,
    pub(crate) country_code: Option<String>,
    pub(crate) colo: Option<String>,
}

const CODEX_EGRESS_TRACE_URL: &str = "https://chatgpt.com/cdn-cgi/trace";
const CODEX_EGRESS_TARGET_HOST: &str = "chatgpt.com";
const MAX_DETECTION_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexTimezoneMatch {
    Exact,
    OffsetMatch,
    Mismatch,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEgressTimezoneDetection {
    pub target_host: String,
    pub dns_addresses: Vec<String>,
    pub dns_uses_non_public_address: bool,
    pub egress_ip: String,
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub colo: Option<String>,
    pub egress_timezone: String,
    pub current_timezone: String,
    pub egress_utc_offset: String,
    pub current_utc_offset: String,
    pub timezone_match: CodexTimezoneMatch,
    pub checked_at: i64,
    pub network_path: String,
}

#[derive(Debug, Deserialize)]
struct IpWhoisTimezone {
    id: String,
    #[allow(dead_code)]
    utc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpWhoisResponse {
    success: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    city: Option<String>,
    timezone: Option<IpWhoisTimezone>,
}

pub(crate) fn parse_cloudflare_trace(body: &str) -> Result<CloudflareTrace, String> {
    let mut ip = None;
    let mut country_code = None;
    let mut colo = None;
    for line in body.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "ip" if !value.is_empty() => ip = Some(value.to_string()),
            "loc" if !value.is_empty() => country_code = Some(value.to_string()),
            "colo" if !value.is_empty() => colo = Some(value.to_string()),
            _ => {}
        }
    }
    let ip = ip.ok_or_else(|| "Cloudflare trace did not report an egress IP".to_string())?;
    let parsed = IpAddr::from_str(&ip)
        .map_err(|_| "Cloudflare trace returned an invalid egress IP".to_string())?;
    if is_non_public_ip(parsed) {
        return Err("Cloudflare trace returned a non-public egress IP".to_string());
    }
    Ok(CloudflareTrace {
        ip,
        country_code,
        colo,
    })
}

pub(crate) fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_non_public_ipv4(ip),
        IpAddr::V6(ip) => is_non_public_ipv6(ip),
    }
}

fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && octets[1] == 18)
        || (octets[0] == 198 && octets[1] == 19)
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
}

fn is_non_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
}

pub(crate) fn mask_ip(ip: &str) -> String {
    match IpAddr::from_str(ip) {
        Ok(IpAddr::V4(ip)) => {
            let octets = ip.octets();
            format!("{}.{}.*.*", octets[0], octets[1])
        }
        Ok(IpAddr::V6(ip)) => {
            let segments = ip.segments();
            format!("{:x}:{:x}:…", segments[0], segments[1])
        }
        Err(_) => "未知".to_string(),
    }
}

pub(crate) fn classify_timezone_match(
    current_timezone: &str,
    egress_timezone: &str,
    unix_timestamp: i64,
) -> CodexTimezoneMatch {
    if current_timezone == egress_timezone {
        return CodexTimezoneMatch::Exact;
    }
    let Ok(current) = Tz::from_str(current_timezone) else {
        return CodexTimezoneMatch::Unknown;
    };
    let Ok(egress) = Tz::from_str(egress_timezone) else {
        return CodexTimezoneMatch::Unknown;
    };
    let Some(at) = Utc.timestamp_opt(unix_timestamp, 0).single() else {
        return CodexTimezoneMatch::Unknown;
    };
    let current_offset = current.offset_from_utc_datetime(&at.naive_utc()).fix();
    let egress_offset = egress.offset_from_utc_datetime(&at.naive_utc()).fix();
    if current_offset == egress_offset {
        CodexTimezoneMatch::OffsetMatch
    } else {
        CodexTimezoneMatch::Mismatch
    }
}

fn timezone_utc_offset(timezone: &str, unix_timestamp: i64) -> Option<String> {
    let timezone = Tz::from_str(timezone).ok()?;
    let at = Utc.timestamp_opt(unix_timestamp, 0).single()?;
    let seconds = timezone
        .offset_from_utc_datetime(&at.naive_utc())
        .fix()
        .local_minus_utc();
    let sign = if seconds < 0 { '-' } else { '+' };
    let seconds = seconds.unsigned_abs();
    Some(format!(
        "{sign}{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60
    ))
}

pub(crate) fn build_detection_from_payloads(
    trace_body: &str,
    geolocation_body: &str,
    mut dns_addresses: Vec<String>,
    current_timezone: &str,
    checked_at: i64,
    network_path: &str,
) -> Result<CodexEgressTimezoneDetection, String> {
    let trace = parse_cloudflare_trace(trace_body)?;
    let geolocation: IpWhoisResponse = serde_json::from_str(geolocation_body)
        .map_err(|_| "IP geolocation service returned invalid JSON".to_string())?;
    if !geolocation.success {
        return Err(geolocation
            .message
            .unwrap_or_else(|| "IP geolocation service rejected the egress IP".to_string()));
    }
    let egress_timezone = geolocation
        .timezone
        .map(|timezone| timezone.id)
        .filter(|timezone| Tz::from_str(timezone).is_ok())
        .ok_or_else(|| "IP geolocation service did not return a valid IANA timezone".to_string())?;
    let egress_utc_offset = timezone_utc_offset(&egress_timezone, checked_at)
        .ok_or_else(|| "Could not calculate the egress timezone offset".to_string())?;
    let current_utc_offset =
        timezone_utc_offset(current_timezone, checked_at).unwrap_or_else(|| "未知".to_string());
    dns_addresses.sort();
    dns_addresses.dedup();
    let dns_uses_non_public_address = dns_addresses
        .iter()
        .any(|address| address.parse::<IpAddr>().ok().is_some_and(is_non_public_ip));
    Ok(CodexEgressTimezoneDetection {
        target_host: CODEX_EGRESS_TARGET_HOST.to_string(),
        dns_addresses,
        dns_uses_non_public_address,
        egress_ip: mask_ip(&trace.ip),
        country_code: geolocation.country_code.or(trace.country_code),
        region: geolocation.region,
        city: geolocation.city,
        colo: trace.colo,
        egress_timezone: egress_timezone.clone(),
        current_timezone: current_timezone.to_string(),
        egress_utc_offset,
        current_utc_offset,
        timezone_match: classify_timezone_match(current_timezone, &egress_timezone, checked_at),
        checked_at,
        network_path: network_path.to_string(),
    })
}

async fn resolve_target_dns() -> Vec<String> {
    tokio::net::lookup_host((CODEX_EGRESS_TARGET_HOST, 443))
        .await
        .map(|addresses| addresses.map(|address| address.ip().to_string()).collect())
        .unwrap_or_default()
}

async fn read_bounded_response(response: reqwest::Response, label: &str) -> Result<String, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label} failed with HTTP {status}"));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read {label}: {error}"))?;
    if body.len() > MAX_DETECTION_BODY_BYTES {
        return Err(format!("{label} response exceeded the safety limit"));
    }
    String::from_utf8(body.to_vec()).map_err(|_| format!("{label} response was not UTF-8"))
}

/// Detect the public egress observed by the same ChatGPT hostname used by Codex.
///
/// DNS results are diagnostic only. Transparent-proxy fake IPs are expected and
/// never geolocated; the public address returned by Cloudflare trace is passed
/// explicitly to the geolocation service.
#[tauri::command]
pub async fn detect_codex_egress_timezone() -> Result<CodexEgressTimezoneDetection, String> {
    let dns_addresses = resolve_target_dns().await;
    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    let trace_body = read_bounded_response(
        client
            .get(CODEX_EGRESS_TRACE_URL)
            .header(reqwest::header::USER_AGENT, "CCSwitchMulti timezone probe")
            .send()
            .await
            .map_err(|error| format!("Could not reach ChatGPT egress trace: {error}"))?,
        "ChatGPT egress trace",
    )
    .await?;
    let trace = parse_cloudflare_trace(&trace_body)?;
    let geolocation_url = format!("https://ipwho.is/{}", trace.ip);
    let geolocation_body = read_bounded_response(
        client
            .get(&geolocation_url)
            .header(reqwest::header::USER_AGENT, "CCSwitchMulti timezone probe")
            .send()
            .await
            .map_err(|error| format!("Could not geolocate the observed egress IP: {error}"))?,
        "IP geolocation",
    )
    .await?;
    let current_timezone = iana_time_zone::get_timezone().unwrap_or_else(|_| "unknown".to_string());
    let network_path = if crate::proxy::http_client::get_current_proxy_url().is_some() {
        "ccsm_global_proxy"
    } else {
        "system_or_transparent"
    };
    build_detection_from_payloads(
        &trace_body,
        &geolocation_body,
        dns_addresses,
        &current_timezone,
        Utc::now().timestamp(),
        network_path,
    )
}

pub(crate) fn resolve_launch_timezone(settings: &AppSettings) -> Option<String> {
    let configured = match settings.codex_egress_timezone.mode {
        CodexEgressTimezoneMode::Off => return None,
        CodexEgressTimezoneMode::Auto => {
            settings.codex_egress_timezone.detected_timezone.as_deref()
        }
        CodexEgressTimezoneMode::Manual => {
            settings.codex_egress_timezone.manual_timezone.as_deref()
        }
    }?;
    let configured = configured.trim();
    Tz::from_str(configured)
        .ok()
        .map(|_| configured.to_string())
}

pub(crate) fn validate_iana_timezone(timezone: &str) -> Result<String, String> {
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return Err("IANA 时区不能为空".to_string());
    }
    Tz::from_str(timezone)
        .map(|_| timezone.to_string())
        .map_err(|_| format!("未知的 IANA 时区: {timezone}"))
}

pub(crate) fn validate_timezone_settings(
    settings: &CodexEgressTimezoneSettings,
) -> Result<(), String> {
    let configured = match settings.mode {
        CodexEgressTimezoneMode::Off => return Ok(()),
        CodexEgressTimezoneMode::Auto => settings
            .detected_timezone
            .as_deref()
            .ok_or_else(|| "自动出口时区缺少有效的探测结果，请先执行出口时区探测".to_string())?,
        CodexEgressTimezoneMode::Manual => settings
            .manual_timezone
            .as_deref()
            .ok_or_else(|| "手动出口时区不能为空".to_string())?,
    };
    validate_iana_timezone(configured).map(|_| ())
}

#[tauri::command]
pub fn validate_codex_egress_timezone(timezone: String) -> Result<(), String> {
    validate_iana_timezone(&timezone).map(|_| ())
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeTimezoneInspection {
    pub runtime_timezone: String,
    pub runtime_utc_offset: String,
    pub configured_timezone: Option<String>,
    pub matches_configured: Option<bool>,
    pub timezone_match: CodexTimezoneMatch,
}

#[tauri::command]
pub async fn inspect_codex_runtime_timezone() -> Result<CodexRuntimeTimezoneInspection, String> {
    let runtime = crate::codex_desktop::inspect_codex_runtime_timezone().await?;
    let configured_timezone = resolve_launch_timezone(&crate::settings::get_settings());
    let matches_configured = configured_timezone
        .as_ref()
        .map(|configured| configured == &runtime.timezone);
    let timezone_match = configured_timezone
        .as_ref()
        .map(|configured| {
            classify_timezone_match(configured, &runtime.timezone, Utc::now().timestamp())
        })
        .unwrap_or(CodexTimezoneMatch::Unknown);
    Ok(CodexRuntimeTimezoneInspection {
        runtime_timezone: runtime.timezone,
        runtime_utc_offset: runtime.utc_offset,
        configured_timezone,
        matches_configured,
        timezone_match,
    })
}
