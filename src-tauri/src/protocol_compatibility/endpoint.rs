use super::TransportKind;

pub(crate) fn build_probe_url(
    base_url: &str,
    transport: TransportKind,
    is_full_url: bool,
) -> Result<String, String> {
    let suffix = match transport {
        TransportKind::OpenAiChat => "/chat/completions",
        TransportKind::OpenAiResponses => "/responses",
    };
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL is empty".to_string());
    }
    if trimmed.ends_with(suffix) {
        return Ok(trimmed.to_string());
    }

    if is_full_url {
        if let Some(index) = trimmed.find("/v1/") {
            return Ok(format!("{}/v1{suffix}", &trimmed[..index]));
        }
        if ends_with_version_segment(trimmed) {
            return Ok(format!("{trimmed}{suffix}"));
        }
        if let Some(other_suffix) = match transport {
            TransportKind::OpenAiChat => Some("/responses"),
            TransportKind::OpenAiResponses => Some("/chat/completions"),
        } {
            if let Some(root) = trimmed.strip_suffix(other_suffix) {
                return Ok(format!("{root}{suffix}"));
            }
        }
        if let Some(index) = trimmed.rfind('/') {
            let root = &trimmed[..index];
            if root.contains("://") {
                return Ok(format!("{root}{suffix}"));
            }
        }
        return Err(format!("Cannot derive {suffix} endpoint from full URL"));
    }

    if ends_with_version_segment(trimmed) {
        Ok(format!("{trimmed}{suffix}"))
    } else {
        Ok(format!("{trimmed}/v1{suffix}"))
    }
}

fn ends_with_version_segment(url: &str) -> bool {
    let Some(segment) = url.rsplit('/').next() else {
        return false;
    };
    let Some(version) = segment.strip_prefix('v') else {
        return false;
    };
    !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit())
}
