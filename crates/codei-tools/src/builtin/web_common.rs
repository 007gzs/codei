use std::time::Duration;

use reqwest::{Client, Url};

use crate::ToolError;

pub fn build_http_client(timeout_secs: u64) -> Result<Client, ToolError> {
    Client::builder()
        .user_agent(format!("codei/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|err| ToolError::Failed {
            name: "http".into(),
            message: err.to_string(),
        })
}

pub fn http_error(tool: &str, err: reqwest::Error) -> ToolError {
    ToolError::Failed {
        name: tool.into(),
        message: err.to_string(),
    }
}

pub fn validate_http_url(url: &str, ssrf_protection: bool) -> Result<Url, ToolError> {
    let parsed = Url::parse(url).map_err(|err| ToolError::InvalidArgs(err.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ToolError::InvalidArgs(format!(
                "unsupported URL scheme: {other}"
            )));
        }
    }
    if ssrf_protection {
        let Some(host) = parsed.host_str() else {
            return Err(ToolError::InvalidArgs("URL must include a host".into()));
        };
        validate_host(host)?;
    }
    Ok(parsed)
}

pub fn validate_response_host(url: &Url, ssrf_protection: bool) -> Result<(), ToolError> {
    if ssrf_protection {
        if let Some(host) = url.host_str() {
            validate_host(host)?;
        }
    }
    Ok(())
}

pub fn validate_host(host: &str) -> Result<(), ToolError> {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower == "metadata.google.internal"
    {
        return Err(ToolError::InvalidArgs(format!("blocked host: {host}")));
    }

    if lower == "::1" || lower == "[::1]" {
        return Err(ToolError::InvalidArgs(format!("blocked host: {host}")));
    }

    if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(ToolError::InvalidArgs(format!("blocked host: {host}")));
        }
    }

    Ok(())
}

fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.octets() == [169, 254, 169, 254]
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

pub fn truncate_bytes(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let truncated = &bytes[..max_bytes];
    (String::from_utf8_lossy(truncated).into_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost_when_ssrf_enabled() {
        assert!(validate_http_url("http://localhost:8080", true).is_err());
    }

    #[test]
    fn allows_localhost_when_ssrf_disabled() {
        assert!(validate_http_url("http://127.0.0.1/api", false).is_ok());
    }
}
