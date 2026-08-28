use std::{fmt, time::Duration};

use bytes::BytesMut;
use futures::StreamExt;
use reqwest::{header::CONTENT_TYPE, RequestBuilder};
use serde_json::Value;

use super::{
    redact_json_probe_response, redact_sse_probe_response, redaction::RedactedProbeEvidence,
};

const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeCaptureError {
    #[error("protocol probe response timed out")]
    Timeout,
    #[error("protocol probe network request failed")]
    Network,
    #[error("protocol probe upstream returned HTTP {status_code}")]
    HttpStatus { status_code: u16 },
    #[error("protocol probe response exceeded the capture limit")]
    ResponseTooLarge,
    #[error("protocol probe response payload was invalid")]
    InvalidPayload,
}

pub(super) struct CapturedPayload {
    pub event_type: Option<String>,
    pub value: Value,
}

pub struct CapturedProbeExchange {
    status_code: u16,
    is_sse: bool,
    payloads: Vec<CapturedPayload>,
    saw_done: bool,
    evidence: RedactedProbeEvidence,
}

impl CapturedProbeExchange {
    #[cfg(test)]
    pub fn status_code(&self) -> u16 {
        self.status_code
    }

    #[cfg(test)]
    pub fn payload_count(&self) -> usize {
        self.payloads.len()
    }

    #[cfg(test)]
    pub fn saw_done(&self) -> bool {
        self.saw_done
    }

    pub fn evidence(&self) -> &RedactedProbeEvidence {
        &self.evidence
    }

    pub(super) fn payloads(&self) -> &[CapturedPayload] {
        &self.payloads
    }
}

impl fmt::Debug for CapturedProbeExchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedProbeExchange")
            .field("status_code", &self.status_code)
            .field("response_format", &if self.is_sse { "sse" } else { "json" })
            .field("payload_count", &self.payloads.len())
            .field("saw_done", &self.saw_done)
            .field("evidence", &self.evidence)
            .finish()
    }
}

pub async fn capture_transport_probe(
    request: RequestBuilder,
    response_timeout: Duration,
) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    match tokio::time::timeout(response_timeout, capture_response(request)).await {
        Ok(result) => result,
        Err(_) => Err(ProbeCaptureError::Timeout),
    }
}

async fn capture_response(
    request: RequestBuilder,
) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    let response = request
        .send()
        .await
        .map_err(|_| ProbeCaptureError::Network)?;
    let status_code = response.status().as_u16();
    if !response.status().is_success() {
        return Err(ProbeCaptureError::HttpStatus { status_code });
    }

    let is_sse = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        });

    let bytes = collect_bounded_body(response).await?;
    if is_sse {
        capture_sse(status_code, &bytes)
    } else {
        capture_json(status_code, &bytes)
    }
}

async fn collect_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, ProbeCaptureError> {
    let mut body = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ProbeCaptureError::Network)?;
        if body.len().saturating_add(chunk.len()) > MAX_CAPTURE_BYTES {
            return Err(ProbeCaptureError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.to_vec())
}

fn capture_json(status_code: u16, body: &[u8]) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    let value =
        serde_json::from_slice::<Value>(body).map_err(|_| ProbeCaptureError::InvalidPayload)?;
    let evidence = redact_json_probe_response(status_code, &value);
    Ok(CapturedProbeExchange {
        status_code,
        is_sse: false,
        payloads: vec![CapturedPayload {
            event_type: None,
            value,
        }],
        saw_done: false,
        evidence,
    })
}

fn capture_sse(status_code: u16, body: &[u8]) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    let text = std::str::from_utf8(body).map_err(|_| ProbeCaptureError::InvalidPayload)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut payloads = Vec::new();
    let mut saw_done = false;

    for frame in normalized.split("\n\n") {
        let mut event_type = None;
        let mut data_lines = Vec::new();
        for line in frame.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_type = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.strip_prefix(' ').unwrap_or(value));
            }
        }
        if data_lines.is_empty() {
            continue;
        }

        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            saw_done = true;
            continue;
        }
        let value =
            serde_json::from_str::<Value>(&data).map_err(|_| ProbeCaptureError::InvalidPayload)?;
        payloads.push(CapturedPayload { event_type, value });
    }

    let evidence = redact_sse_probe_response(status_code, &normalized);
    Ok(CapturedProbeExchange {
        status_code,
        is_sse: true,
        payloads,
        saw_done,
        evidence,
    })
}
