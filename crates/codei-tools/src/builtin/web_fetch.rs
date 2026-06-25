use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use super::web_common::{
    build_http_client, http_error, truncate_bytes, validate_http_url, validate_response_host,
};
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct WebFetchTool {
    timeout_secs: u64,
    max_bytes: usize,
    ssrf_protection: bool,
}

impl WebFetchTool {
    pub fn new(timeout_secs: u64, max_bytes: usize, ssrf_protection: bool) -> Self {
        Self {
            timeout_secs,
            max_bytes,
            ssrf_protection,
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a public HTTP(S) URL and return the response body as text."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP or HTTPS URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing url".into()))?;

        let parsed = validate_http_url(url, self.ssrf_protection)?;
        let client = build_http_client(self.timeout_secs)?;

        let response = client
            .get(parsed.clone())
            .send()
            .await
            .map_err(|err| http_error("web_fetch", err))?;

        let status = response.status();
        let final_url = response.url().clone();
        validate_response_host(&final_url, self.ssrf_protection)?;

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        let bytes = response
            .bytes()
            .await
            .map_err(|err| http_error("web_fetch", err))?;
        let (body, truncated) = truncate_bytes(&bytes, self.max_bytes);

        let mut content =
            format!("url: {final_url}\nstatus: {status}\ncontent-type: {content_type}\n\n{body}");
        if truncated {
            content.push_str(&format!(
                "\n\n...(response truncated to {} bytes)",
                self.max_bytes
            ));
        }

        Ok(ToolResult {
            content,
            is_error: !status.is_success(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::web_common::validate_http_url;

    #[test]
    fn accepts_public_https_url() {
        assert!(validate_http_url("https://example.com/docs", true).is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(validate_http_url("file:///etc/passwd", true).is_err());
    }
}
