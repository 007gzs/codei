use async_trait::async_trait;
use codei_config::WebSearchProvider;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use super::web_common::{build_http_client, http_error, validate_http_url, validate_response_host};
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct WebSearchTool {
    provider: WebSearchProvider,
    timeout_secs: u64,
    max_results: usize,
    searxng_url: Option<String>,
    ssrf_protection: bool,
}

impl WebSearchTool {
    pub fn new(
        provider: WebSearchProvider,
        timeout_secs: u64,
        max_results: usize,
        searxng_url: Option<String>,
        ssrf_protection: bool,
    ) -> Self {
        Self {
            provider,
            timeout_secs,
            max_results: max_results.max(1),
            searxng_url,
            ssrf_protection,
        }
    }
}

#[derive(Debug, Clone)]
struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web and return titles, URLs, and snippets."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing query".into()))?;

        let client = build_http_client(self.timeout_secs)?;
        let hits = match self.provider {
            WebSearchProvider::Duckduckgo => {
                search_duckduckgo(&client, query, self.max_results).await?
            }
            WebSearchProvider::Searxng => {
                let base = self.searxng_url.as_deref().ok_or_else(|| {
                    ToolError::InvalidArgs(
                        "tools.web_search.searxng_url is required when provider = \"searxng\""
                            .into(),
                    )
                })?;
                search_searxng(&client, base, query, self.max_results, self.ssrf_protection).await?
            }
        };

        if hits.is_empty() {
            return Ok(ToolResult {
                content: format!("No results for: {query}"),
                is_error: false,
            });
        }

        Ok(ToolResult {
            content: format_hits(&hits),
            is_error: false,
        })
    }
}

async fn search_duckduckgo(
    client: &Client,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchHit>, ToolError> {
    let response = client
        .post("https://html.duckduckgo.com/html/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={}", urlencoding(query)))
        .send()
        .await
        .map_err(|err| http_error("web_search", err))?;

    if !response.status().is_success() {
        return Err(ToolError::Failed {
            name: "web_search".into(),
            message: format!("duckduckgo returned {}", response.status()),
        });
    }

    let html = response
        .text()
        .await
        .map_err(|err| http_error("web_search", err))?;
    Ok(parse_duckduckgo_html(&html, max_results))
}

async fn search_searxng(
    client: &Client,
    base_url: &str,
    query: &str,
    max_results: usize,
    ssrf_protection: bool,
) -> Result<Vec<SearchHit>, ToolError> {
    let base = base_url.trim_end_matches('/');
    let endpoint = format!("{base}/search");
    validate_http_url(&endpoint, ssrf_protection)?;

    let response = client
        .get(&endpoint)
        .query(&[("q", query), ("format", "json")])
        .send()
        .await
        .map_err(|err| http_error("web_search", err))?;

    validate_response_host(response.url(), ssrf_protection)?;

    if !response.status().is_success() {
        return Err(ToolError::Failed {
            name: "web_search".into(),
            message: format!("searxng returned {}", response.status()),
        });
    }

    let body: SearxResponse = response
        .json()
        .await
        .map_err(|err| http_error("web_search", err))?;

    Ok(body
        .results
        .into_iter()
        .take(max_results)
        .map(|r| SearchHit {
            title: r.title,
            url: r.url,
            snippet: r.content.unwrap_or_default(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct SearxResponse {
    #[serde(default)]
    results: Vec<SearxResult>,
}

#[derive(Debug, Deserialize)]
struct SearxResult {
    title: String,
    url: String,
    #[serde(default)]
    content: Option<String>,
}

fn parse_duckduckgo_html(html: &str, max_results: usize) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut cursor = 0usize;

    while hits.len() < max_results {
        let Some(rel) = html[cursor..].find("class=\"result__a\"") else {
            break;
        };
        let start = cursor + rel;
        let slice = &html[start..];

        let Some(href) = extract_html_attr(slice, "href") else {
            cursor = start + 1;
            continue;
        };
        let Some(title) = extract_inner_text_after_tag(slice, "result__a") else {
            cursor = start + 1;
            continue;
        };

        let snippet = html
            .get(start..)
            .and_then(|tail| {
                tail.find("class=\"result__snippet\"")
                    .and_then(|off| extract_inner_text_after_tag(&tail[off..], "result__snippet"))
            })
            .unwrap_or_default();

        let url = decode_duckduckgo_href(&href);
        if !url.is_empty() && !title.is_empty() {
            hits.push(SearchHit {
                title: decode_html_entities(&title),
                url,
                snippet: decode_html_entities(&snippet),
            });
        }
        cursor = start + 1;
    }

    hits
}

fn extract_html_attr(html: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = html.find(&pattern)? + pattern.len();
    let rest = html.get(start..)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_inner_text_after_tag(html: &str, class_marker: &str) -> Option<String> {
    let marker = format!("class=\"{class_marker}\"");
    let idx = html.find(&marker)?;
    let after = html.get(idx..)?;
    let open = after.find('>')? + 1;
    let text = after.get(open..)?;
    let close = text.find('<')?;
    let raw = text[..close].trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn decode_duckduckgo_href(href: &str) -> String {
    let normalized = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };

    if let Ok(url) = reqwest::Url::parse(&normalized) {
        for (key, value) in url.query_pairs() {
            if key == "uddg" {
                return value.into_owned();
            }
        }
        if url.scheme() == "http" || url.scheme() == "https" {
            return url.to_string();
        }
    }
    normalized
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn urlencoding(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn format_hits(hits: &[SearchHit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(i, hit)| {
            let mut block = format!("{}. {}\n   URL: {}", i + 1, hit.title, hit.url);
            if !hit.snippet.is_empty() {
                block.push_str(&format!("\n   Snippet: {}", hit.snippet));
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r##"
<div class="result results_links results_links_deep web-result">
  <h2 class="result__title">
    <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Frust-lang.org&amp;rut=abc">Rust Programming Language</a>
  </h2>
  <a class="result__snippet" href="#">A language empowering everyone to build reliable software.</a>
</div>
<div class="result results_links results_links_deep web-result">
  <h2 class="result__title">
    <a rel="nofollow" class="result__a" href="https://doc.rust-lang.org/book/">The Rust Book</a>
  </h2>
  <a class="result__snippet" href="#">The official Rust book.</a>
</div>
"##;

    #[test]
    fn parses_duckduckgo_html() {
        let hits = parse_duckduckgo_html(SAMPLE_HTML, 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://rust-lang.org");
        assert_eq!(hits[0].title, "Rust Programming Language");
        assert!(hits[0].snippet.contains("reliable software"));
        assert_eq!(hits[1].url, "https://doc.rust-lang.org/book/");
    }

    #[test]
    fn decodes_duckduckgo_redirect() {
        let url = decode_duckduckgo_href(
            "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpath&amp;rut=1",
        );
        assert_eq!(url, "https://example.com/path");
    }

    #[test]
    fn formats_hits_as_text() {
        let text = format_hits(&[SearchHit {
            title: "Example".into(),
            url: "https://example.com".into(),
            snippet: "Hello".into(),
        }]);
        assert!(text.contains("1. Example"));
        assert!(text.contains("https://example.com"));
    }
}
