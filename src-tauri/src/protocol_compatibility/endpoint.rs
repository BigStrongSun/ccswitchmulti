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
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("Base URL is empty".to_string());
    }
    let mut parsed = url::Url::parse(trimmed).map_err(|_| "Base URL is invalid".to_string())?;
    let path = parsed.path().trim_end_matches('/').to_string();
    if path.ends_with(suffix) {
        parsed.set_path(&path);
        return Ok(parsed.to_string());
    }

    let next_path = if is_full_url {
        if let Some(index) = path.find("/v1/") {
            format!("{}/v1{suffix}", &path[..index])
        } else if ends_with_version_segment(&path) {
            format!("{path}{suffix}")
        } else if let Some(root) = match transport {
            TransportKind::OpenAiChat => path.strip_suffix("/responses"),
            TransportKind::OpenAiResponses => path.strip_suffix("/chat/completions"),
        } {
            format!("{root}{suffix}")
        } else if let Some(index) = path.rfind('/') {
            format!("{}{suffix}", &path[..index])
        } else {
            return Err(format!("Cannot derive {suffix} endpoint from full URL"));
        }
    } else if ends_with_version_segment(&path) {
        format!("{path}{suffix}")
    } else {
        format!("{path}/v1{suffix}")
    };
    parsed.set_path(&next_path);
    Ok(parsed.to_string())
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
