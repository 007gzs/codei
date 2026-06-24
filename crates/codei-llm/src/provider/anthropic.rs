use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::message::{Message, Role};
use crate::provider::build_http_client;
use crate::provider::LlmProvider;
use crate::{ChatRequest, ChatStream, LlmError, StreamEvent, Usage};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";

pub struct AnthropicProvider {
    id: String,
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn from_config(
        id: impl Into<String>,
        api_key: String,
        base_url: Option<&str>,
    ) -> Result<Self, LlmError> {
        let base_url = base_url
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Self {
            id: id.into(),
            client: build_http_client()?,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamWrapper {
    #[serde(rename = "type")]
    _event_type: String,
    #[serde(default)]
    delta: Option<Delta>,
    #[serde(default)]
    content_block: Option<ContentBlock>,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(rename = "type")]
    _delta_type: Option<String>,
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, LlmError> {
        let (system, messages) = split_messages(request.messages);
        let tools = request.tools.map(|defs| {
            defs.iter()
                .map(|d| {
                    json!({
                        "name": d.name,
                        "description": d.description,
                        "input_schema": d.parameters,
                    })
                })
                .collect()
        });

        let body = AnthropicRequest {
            model: request.model,
            max_tokens: request.max_tokens.unwrap_or(8192),
            system,
            messages,
            tools,
            temperature: request.temperature,
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(LlmError::Api { status, message });
        }

        let byte_stream = response.bytes_stream();
        let stream = byte_stream
            .map(|chunk| chunk.map_err(LlmError::from))
            .scan(SseBuffer::default(), |buf, chunk| {
                futures::future::ready(match chunk {
                    Ok(bytes) => Some(parse_anthropic_sse(buf, &bytes)),
                    Err(err) => Some(vec![Err(err)]),
                })
            })
            .flat_map(futures::stream::iter);

        Ok(Box::pin(stream))
    }
}

fn split_messages(messages: Vec<Message>) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut out = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                if let Some(text) = msg.content {
                    system_parts.push(text);
                }
            }
            Role::User => {
                out.push(json!({
                    "role": "user",
                    "content": msg.content.unwrap_or_default(),
                }));
            }
            Role::Assistant => {
                let mut blocks = Vec::new();
                if let Some(text) = msg.content.filter(|t| !t.is_empty()) {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                if let Some(calls) = msg.tool_calls {
                    for call in calls {
                        let input: Value =
                            serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": call.id,
                            "name": call.name,
                            "input": input,
                        }));
                    }
                }
                out.push(json!({"role": "assistant", "content": blocks}));
            }
            Role::Tool => {
                out.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": msg.tool_call_id,
                        "content": msg.content.unwrap_or_default(),
                    }],
                }));
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, out)
}

#[derive(Default)]
struct SseBuffer {
    leftover: String,
}

fn parse_anthropic_sse(buf: &mut SseBuffer, bytes: &[u8]) -> Vec<Result<StreamEvent, LlmError>> {
    let text = String::from_utf8_lossy(bytes);
    buf.leftover.push_str(&text);

    let mut events = Vec::new();
    while let Some(pos) = buf.leftover.find("\n\n") {
        let block = buf.leftover[..pos].to_string();
        buf.leftover = buf.leftover[pos + 2..].to_string();

        let mut event_name = String::new();
        let mut data = String::new();
        for line in block.lines() {
            if let Some(name) = line.strip_prefix("event: ") {
                event_name = name.trim().to_string();
            } else if let Some(payload) = line.strip_prefix("data: ") {
                data = payload.to_string();
            }
        }

        if data.is_empty() {
            continue;
        }

        match serde_json::from_str::<StreamWrapper>(&data) {
            Ok(wrapper) => events.extend(map_anthropic_event(&event_name, wrapper)),
            Err(err) => events.push(Err(LlmError::StreamParse(err.to_string()))),
        }
    }

    events
}

fn map_anthropic_event(
    event_name: &str,
    wrapper: StreamWrapper,
) -> Vec<Result<StreamEvent, LlmError>> {
    let mut events = Vec::new();
    match event_name {
        "content_block_delta" => {
            if let Some(delta) = wrapper.delta {
                if let Some(text) = delta.text {
                    if !text.is_empty() {
                        events.push(Ok(StreamEvent::TextDelta(text)));
                    }
                }
                if let (Some(index), Some(partial)) = (wrapper.index, delta.partial_json) {
                    events.push(Ok(StreamEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments: Some(partial),
                    }));
                }
            }
        }
        "content_block_start" => {
            if let (Some(index), Some(block)) = (wrapper.index, wrapper.content_block) {
                if block.block_type.as_deref() == Some("tool_use") {
                    events.push(Ok(StreamEvent::ToolCallDelta {
                        index,
                        id: block.id,
                        name: block.name,
                        arguments: None,
                    }));
                }
            }
        }
        "message_delta" => {
            if let Some(usage) = wrapper.usage {
                events.push(Ok(StreamEvent::Usage(Usage {
                    input_tokens: usage.input_tokens.unwrap_or(0),
                    output_tokens: usage.output_tokens.unwrap_or(0),
                })));
            }
            events.push(Ok(StreamEvent::Done));
        }
        "message_stop" => {
            events.push(Ok(StreamEvent::Done));
        }
        _ => {}
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_system_from_messages() {
        let messages = vec![Message::system("sys"), Message::user("hello")];
        let (system, rest) = split_messages(messages);
        assert_eq!(system.as_deref(), Some("sys"));
        assert_eq!(rest.len(), 1);
    }
}
