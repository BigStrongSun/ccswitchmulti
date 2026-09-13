use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use chrono::{Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::proxy::ProxyError;
use crate::settings::{AppSettings, CodexEgressTimezoneMode, CodexEgressTimezoneSettings};

const AUTOMATIC_PROBE_COOLDOWN_SECS: i64 = 90;

pub(crate) fn automatic_probe_cooldown_secs(consecutive_failures: u32) -> i64 {
    let exponent = consecutive_failures.saturating_sub(1).min(5);
    (AUTOMATIC_PROBE_COOLDOWN_SECS * 2_i64.pow(exponent)).min(30 * 60)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexEgressProbeTrigger {
    Startup,
    Periodic,
    ProxyFailure,
    NetworkResume,
    Manual,
}

impl CodexEgressProbeTrigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Periodic => "periodic",
            Self::ProxyFailure => "proxy_failure",
            Self::NetworkResume => "network_resume",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AutomaticDetectionOutcome {
    Updated,
    RestartRequired,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexEgressMonitorState {
    Disabled,
    NotTested,
    Checking,
    Ready,
    RestartRequired,
    RendererOnly,
    Error,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexEgressMonitorRuntime {
    pub running: bool,
    pub last_attempt_at: Option<i64>,
    pub last_trigger: Option<String>,
    pub last_error: Option<String>,
    pub restart_required: bool,
    pub process_timezone_unavailable: bool,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEgressMonitorStatus {
    pub state: CodexEgressMonitorState,
    pub detected_timezone: Option<String>,
    pub detected_egress_ip: Option<String>,
    pub detected_at: Option<i64>,
    pub last_attempt_at: Option<i64>,
    pub last_trigger: Option<String>,
    pub last_error: Option<String>,
    pub next_check_at: Option<i64>,
    pub monitor_interval_minutes: u32,
    pub restart_required: bool,
}

static MONITOR_RUNTIME: OnceLock<Mutex<CodexEgressMonitorRuntime>> = OnceLock::new();
static MONITOR_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);

fn monitor_runtime() -> &'static Mutex<CodexEgressMonitorRuntime> {
    MONITOR_RUNTIME.get_or_init(|| Mutex::new(CodexEgressMonitorRuntime::default()))
}

pub(crate) fn monitor_status_from_parts(
    settings: &CodexEgressTimezoneSettings,
    runtime: &CodexEgressMonitorRuntime,
    _now: i64,
) -> CodexEgressMonitorStatus {
    let process_timezone_unavailable =
        runtime.process_timezone_unavailable && settings.mode != CodexEgressTimezoneMode::Off;
    let restart_required = runtime.restart_required && !process_timezone_unavailable;
    let state = if process_timezone_unavailable {
        CodexEgressMonitorState::RendererOnly
    } else if settings.mode != CodexEgressTimezoneMode::Auto {
        CodexEgressMonitorState::Disabled
    } else if runtime.running {
        CodexEgressMonitorState::Checking
    } else if restart_required {
        CodexEgressMonitorState::RestartRequired
    } else if runtime.last_error.is_some() {
        CodexEgressMonitorState::Error
    } else if settings.detected_timezone.is_some() {
        CodexEgressMonitorState::Ready
    } else {
        CodexEgressMonitorState::NotTested
    };
    let interval = settings.monitor_interval_minutes.clamp(5, 120);
    CodexEgressMonitorStatus {
        state,
        detected_timezone: settings.detected_timezone.clone(),
        detected_egress_ip: settings.detected_egress_ip.clone(),
        detected_at: settings.detected_at,
        last_attempt_at: runtime.last_attempt_at,
        last_trigger: runtime
            .last_trigger
            .clone()
            .or_else(|| settings.last_probe_trigger.clone()),
        last_error: runtime.last_error.clone(),
        next_check_at: if runtime.consecutive_failures > 0 {
            runtime.last_attempt_at.map(|at| {
                at.saturating_add(automatic_probe_cooldown_secs(runtime.consecutive_failures))
            })
        } else {
            settings
                .detected_at
                .map(|at| at.saturating_add(i64::from(interval) * 60))
        },
        monitor_interval_minutes: interval,
        restart_required,
    }
}

fn current_monitor_status() -> CodexEgressMonitorStatus {
    let settings = crate::settings::get_settings().codex_egress_timezone;
    let runtime = monitor_runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    monitor_status_from_parts(&settings, &runtime, Utc::now().timestamp())
}

fn emit_monitor_status(app_handle: Option<&AppHandle>) {
    let app_handle = app_handle.or_else(|| MONITOR_APP_HANDLE.get());
    if let Some(app_handle) = app_handle {
        let _ = app_handle.emit("codex-egress-timezone-status", current_monitor_status());
    }
}

pub(crate) fn should_trigger_probe_for_proxy_error(app_type: &str, error: &ProxyError) -> bool {
    if !app_type.eq_ignore_ascii_case("codex") {
        return false;
    }
    match error {
        ProxyError::ForwardFailed(_)
        | ProxyError::Timeout(_)
        | ProxyError::StreamIdleTimeout(_)
        | ProxyError::ResponsePending(_) => true,
        ProxyError::UpstreamError { status, .. } => (500..=599).contains(status),
        _ => false,
    }
}

pub(crate) fn is_automatic_probe_due(
    settings: &CodexEgressTimezoneSettings,
    now: i64,
    last_attempt_at: Option<i64>,
) -> bool {
    if settings.mode != CodexEgressTimezoneMode::Auto {
        return false;
    }
    if last_attempt_at.is_some_and(|last| now.saturating_sub(last) < AUTOMATIC_PROBE_COOLDOWN_SECS)
    {
        return false;
    }
    let interval_secs = i64::from(settings.monitor_interval_minutes.clamp(5, 120)) * 60;
    settings
        .detected_at
        .is_none_or(|detected| now.saturating_sub(detected) > interval_secs)
}

pub(crate) fn should_run_automatic_probe(
    settings: &CodexEgressTimezoneSettings,
    trigger: CodexEgressProbeTrigger,
    now: i64,
    last_attempt_at: Option<i64>,
) -> bool {
    if settings.mode != CodexEgressTimezoneMode::Auto {
        return false;
    }
    if last_attempt_at.is_some_and(|last| now.saturating_sub(last) < AUTOMATIC_PROBE_COOLDOWN_SECS)
    {
        return false;
    }
    match trigger {
        CodexEgressProbeTrigger::ProxyFailure | CodexEgressProbeTrigger::NetworkResume => true,
        CodexEgressProbeTrigger::Startup
        | CodexEgressProbeTrigger::Periodic
        | CodexEgressProbeTrigger::Manual => is_automatic_probe_due(settings, now, last_attempt_at),
    }
}

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

pub(crate) fn apply_automatic_detection(
    settings: &mut CodexEgressTimezoneSettings,
    detection: &CodexEgressTimezoneDetection,
    trigger: CodexEgressProbeTrigger,
    codex_running: bool,
) -> AutomaticDetectionOutcome {
    let applied_timezone_changed =
        settings.last_applied_timezone.as_deref() != Some(detection.egress_timezone.as_str());
    settings.detected_timezone = Some(detection.egress_timezone.clone());
    settings.detected_at = Some(detection.checked_at);
    settings.detected_egress_ip = Some(detection.egress_ip.clone());
    settings.detected_country_code = detection.country_code.clone();
    settings.detected_region = detection.region.clone();
    settings.detected_city = detection.city.clone();
    settings.detected_colo = detection.colo.clone();
    settings.last_probe_trigger = Some(trigger.as_str().to_string());
    if applied_timezone_changed && codex_running {
        AutomaticDetectionOutcome::RestartRequired
    } else {
        AutomaticDetectionOutcome::Updated
    }
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
    // DNS is diagnostic only: an explicit proxy may resolve the target remotely.
    match tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::lookup_host((CODEX_EGRESS_TARGET_HOST, 443)),
    )
    .await
    {
        Ok(Ok(addresses)) => addresses.map(|address| address.ip().to_string()).collect(),
        _ => Vec::new(),
    }
}

fn detection_transport_error(label: &str, error: reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect (DNS/TCP/TLS)"
    } else if error.is_body() || error.is_decode() {
        "response body"
    } else {
        "request"
    };
    let path = if crate::proxy::http_client::get_current_proxy_url().is_some() {
        "ccsm_global_proxy"
    } else {
        "system_or_transparent"
    };
    // Match the router's error-chain handling without disclosing the geolocation
    // URL (which contains the observed IP) or a proxy's credentials.
    let detail = crate::proxy::error::error_chain_message(&error.without_url());
    let detail = crate::proxy::codex_error_capture::redact(&detail, 1200);
    let message = format!("{label}: {kind}; path={path}; {detail}");
    log::warn!("[CodexEgress] {message}");
    message
}

async fn read_bounded_response(
    mut response: reqwest::Response,
    label: &str,
) -> Result<String, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label} failed with HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DETECTION_BODY_BYTES as u64)
    {
        return Err(format!("{label} response exceeded the safety limit"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| detection_transport_error(label, error))?
    {
        if chunk.len() > MAX_DETECTION_BODY_BYTES.saturating_sub(body.len()) {
            return Err(format!("{label} response exceeded the safety limit"));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| format!("{label} response was not UTF-8"))
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn trace_transport_stream_limit_accepts_exact_boundary_without_content_length() {
        for length in [MAX_DETECTION_BODY_BYTES, MAX_DETECTION_BODY_BYTES + 1] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).await.unwrap();
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
                socket.write_all(&vec![b'x'; length]).await.unwrap();
            });
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{address}"))
                .send()
                .await
                .unwrap();
            let result = read_bounded_response(response, "trace").await;
            server.await.unwrap();
            if length == MAX_DETECTION_BODY_BYTES {
                assert_eq!(result.unwrap().len(), 65536);
            } else {
                assert!(result.unwrap_err().contains("safety limit"));
            }
        }
    }

    #[tokio::test]
    async fn trace_transport_send_failure_reports_connection_stage_without_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/8.8.8.8?api_key=private-value"))
            .send()
            .await
            .unwrap_err();
        let message = detection_transport_error("trace", error);
        assert!(message.contains("connect (DNS/TCP/TLS)"), "{message}");
        assert!(
            message.contains("tcp connect error") || message.contains("10061"),
            "{message}"
        );
        assert!(!message.contains("private-value"), "{message}");
        assert!(!message.contains("8.8.8.8"), "{message}");
    }

    #[tokio::test]
    async fn trace_transport_rejects_oversized_body_before_waiting_for_it() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65537\r\n\r\n")
                .await
                .unwrap();
            // A response with a declared over-limit size must be rejected
            // before this deliberately delayed body (or its timeout).
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(250),
            read_bounded_response(response, "trace"),
        )
        .await;
        server.abort();
        let _ = server.await;
        assert!(result.unwrap().unwrap_err().contains("safety limit"));
    }

    #[tokio::test]
    async fn trace_transport_body_failure_preserves_cause_without_request_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
                .await
                .unwrap();
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/?api_key=private-value"))
            .send()
            .await
            .unwrap();
        let error = read_bounded_response(response, "trace").await.unwrap_err();
        server.await.unwrap();
        assert!(
            error.contains("end of file") || error.contains("IncompleteBody"),
            "{error}"
        );
        assert!(!error.contains("private-value"), "{error}");
    }
}

/// Detect the public egress observed by the same ChatGPT hostname used by Codex.
///
/// DNS results are diagnostic only. Transparent-proxy fake IPs are expected and
/// never geolocated; the public address returned by Cloudflare trace is passed
/// explicitly to the geolocation service.
#[tauri::command]
pub async fn detect_codex_egress_timezone() -> Result<CodexEgressTimezoneDetection, String> {
    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    let (dns_addresses, trace_response) = tokio::join!(
        resolve_target_dns(),
        client
            .get(CODEX_EGRESS_TRACE_URL)
            .header(reqwest::header::USER_AGENT, "CCSwitchMulti timezone probe")
            .timeout(Duration::from_secs(8))
            .send()
    );
    let trace_body = read_bounded_response(
        trace_response.map_err(|error| detection_transport_error("ChatGPT egress trace", error))?,
        "ChatGPT egress trace",
    )
    .await?;
    let trace = parse_cloudflare_trace(&trace_body)?;
    let geolocation_url = format!("https://ipwho.is/{}", trace.ip);
    let geolocation_body = read_bounded_response(
        client
            .get(&geolocation_url)
            .header(reqwest::header::USER_AGENT, "CCSwitchMulti timezone probe")
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|error| detection_transport_error("IP geolocation", error))?,
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

async fn run_automatic_probe(
    trigger: CodexEgressProbeTrigger,
    force: bool,
    app_handle: Option<&AppHandle>,
) -> Result<CodexEgressMonitorStatus, String> {
    let now = Utc::now().timestamp();
    let settings = crate::settings::get_settings().codex_egress_timezone;
    if settings.mode != CodexEgressTimezoneMode::Auto {
        return Ok(current_monitor_status());
    }

    {
        let mut runtime = monitor_runtime()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if runtime.running {
            return Ok(monitor_status_from_parts(&settings, &runtime, now));
        }
        if !force
            && runtime.last_attempt_at.is_some_and(|last| {
                now.saturating_sub(last)
                    < automatic_probe_cooldown_secs(runtime.consecutive_failures)
            })
        {
            return Ok(monitor_status_from_parts(&settings, &runtime, now));
        }
        if !force && !should_run_automatic_probe(&settings, trigger, now, runtime.last_attempt_at) {
            return Ok(monitor_status_from_parts(&settings, &runtime, now));
        }
        if force
            && trigger != CodexEgressProbeTrigger::Manual
            && runtime
                .last_attempt_at
                .is_some_and(|last| now.saturating_sub(last) < AUTOMATIC_PROBE_COOLDOWN_SECS)
        {
            return Ok(monitor_status_from_parts(&settings, &runtime, now));
        }
        runtime.running = true;
        runtime.last_attempt_at = Some(now);
        runtime.last_trigger = Some(trigger.as_str().to_string());
        runtime.last_error = None;
    }
    emit_monitor_status(app_handle);

    let result = detect_codex_egress_timezone().await;
    match result {
        Ok(detection) => {
            let codex_running = crate::codex_desktop::is_codex_desktop_running();
            let outcome = crate::settings::mutate_codex_egress_timezone(|settings| {
                apply_automatic_detection(settings, &detection, trigger, codex_running)
            })
            .map_err(|error| error.to_string());
            let mut runtime = monitor_runtime()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.running = false;
            match outcome {
                Ok(AutomaticDetectionOutcome::RestartRequired) => {
                    runtime.restart_required = true;
                    runtime.last_error = None;
                    runtime.consecutive_failures = 0;
                }
                Ok(AutomaticDetectionOutcome::Updated) => {
                    runtime.restart_required = false;
                    runtime.last_error = None;
                    runtime.consecutive_failures = 0;
                }
                Err(error) => {
                    runtime.last_error = Some(error);
                    runtime.consecutive_failures = runtime.consecutive_failures.saturating_add(1);
                }
            }
        }
        Err(error) => {
            let mut runtime = monitor_runtime()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.running = false;
            runtime.last_error = Some(error);
            runtime.consecutive_failures = runtime.consecutive_failures.saturating_add(1);
        }
    }
    emit_monitor_status(app_handle);
    let status = current_monitor_status();
    if let Some(error) = status.last_error.clone() {
        Err(error)
    } else {
        Ok(status)
    }
}

pub(crate) async fn refresh_automatic_timezone_before_codex_launch() {
    // 启动是应用 TZ 的唯一确定边界，不能复用“尚未过期”的旧出口结果；
    // 节点可能刚刚切换。仍保留单飞与 90 秒硬冷却，避免重复启动动作打爆探测端点。
    if let Err(error) = run_automatic_probe(CodexEgressProbeTrigger::Startup, true, None).await {
        log::warn!("Codex 出口时区启动前自动探测失败，继续使用最近一次有效结果: {error}");
    }
}

pub(crate) fn notify_proxy_failure(app_type: &str, error: &ProxyError) {
    if !should_trigger_probe_for_proxy_error(app_type, error) {
        return;
    }
    tauri::async_runtime::spawn(async {
        if let Err(error) =
            run_automatic_probe(CodexEgressProbeTrigger::ProxyFailure, false, None).await
        {
            log::debug!("Codex 转发异常触发的出口时区探测未完成: {error}");
        }
    });
}

pub(crate) fn mark_codex_timezone_applied() {
    let applied_timezone = resolve_launch_timezone(&crate::settings::get_settings());
    record_codex_launch_timezone(applied_timezone, false);
}

#[cfg(target_os = "windows")]
pub(crate) fn mark_codex_timezone_not_inherited() {
    let configured = resolve_launch_timezone(&crate::settings::get_settings());
    if configured.is_some() {
        log::warn!("Codex MSIX activation cannot inherit TZ; process timezone remains unapplied. Renderer timezone emulation will be attempted over CDP.");
    }
    record_codex_launch_timezone(None, true);
}

fn record_codex_launch_timezone(
    applied_timezone: Option<String>,
    process_timezone_unavailable: bool,
) {
    if let Err(error) = crate::settings::mutate_codex_egress_timezone(|settings| {
        settings.last_applied_timezone = applied_timezone;
        settings.last_applied_at = Some(Utc::now().timestamp());
    }) {
        log::warn!("无法持久化 Codex 已应用的出口时区: {error}");
    }
    let mut runtime = monitor_runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    runtime.restart_required = false;
    runtime.process_timezone_unavailable = process_timezone_unavailable;
    drop(runtime);
    emit_monitor_status(None);
}

pub(crate) fn start_automatic_monitor(app_handle: AppHandle) {
    let _ = MONITOR_APP_HANDLE.set(app_handle.clone());
    if MONITOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let settings = crate::settings::get_settings().codex_egress_timezone;
    let running = crate::codex_desktop::detect_running_codex_main_process();
    #[cfg(target_os = "windows")]
    let packaged = running
        .as_deref()
        .is_some_and(crate::codex_desktop::windows_launch::is_packaged_codex);
    #[cfg(not(target_os = "windows"))]
    let packaged = false;
    initialize_monitor_launch_state(
        &mut monitor_runtime()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        &settings,
        running.is_some(),
        packaged,
    );
    tauri::async_runtime::spawn(async move {
        let mut last_tick = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let elapsed = last_tick.elapsed();
            last_tick = Instant::now();
            let trigger = if elapsed > Duration::from_secs(90) {
                CodexEgressProbeTrigger::NetworkResume
            } else {
                CodexEgressProbeTrigger::Periodic
            };
            if let Err(error) = run_automatic_probe(trigger, false, Some(&app_handle)).await {
                log::debug!("Codex 出口时区后台探测未完成: {error}");
            }
        }
    });
}

pub(crate) fn initialize_monitor_launch_state(
    runtime: &mut CodexEgressMonitorRuntime,
    settings: &CodexEgressTimezoneSettings,
    codex_running: bool,
    packaged: bool,
) {
    runtime.process_timezone_unavailable = codex_running && packaged;
    runtime.restart_required = !runtime.process_timezone_unavailable
        && codex_running
        && settings.mode == CodexEgressTimezoneMode::Auto
        && settings.detected_timezone.is_some()
        && settings.detected_timezone != settings.last_applied_timezone;
}

#[tauri::command]
pub fn get_codex_egress_timezone_monitor_status() -> CodexEgressMonitorStatus {
    current_monitor_status()
}

#[tauri::command]
pub async fn trigger_codex_egress_timezone_probe(
    app_handle: AppHandle,
) -> Result<CodexEgressMonitorStatus, String> {
    run_automatic_probe(CodexEgressProbeTrigger::Manual, true, Some(&app_handle)).await
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
    if !(5..=120).contains(&settings.monitor_interval_minutes) {
        return Err("Codex 出口时区自动检测周期必须在 5 到 120 分钟之间".to_string());
    }
    let configured = match settings.mode {
        CodexEgressTimezoneMode::Off => return Ok(()),
        CodexEgressTimezoneMode::Auto => {
            let Some(timezone) = settings.detected_timezone.as_deref() else {
                return Ok(());
            };
            timezone
        }
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
