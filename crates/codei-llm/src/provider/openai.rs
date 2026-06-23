use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{debug, error, warn};

use crate::message::{Message, Role};
use crate::provider::LlmProvider;
use crate::{
    ChatCompletionChunk, ChatRequest, ChatStream, LlmError, OpenAiUsage, StreamEvent, ToolFormat,
    Usage,
};

pub struct OpenAiProvider {
    id: String,
    client: Client,
    api_key: String,
    base_url: String,
    tool_format: ToolFormat,
}

impl OpenAiProvider {
    pub fn new(
        id: impl Into<String>,
        api_key: String,
        base_url: String,
        tool_format: ToolFormat,
    ) -> Self {
        Self {
            id: id.into(),
            client: Client::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            tool_format,
        }
    }

    pub fn from_config(
        id: impl Into<String>,
        api_key: String,
        base_url: Option<&str>,
        tool_format: ToolFormat,
    ) -> Result<Self, LlmError> {
        let base_url = base_url
            .map(str::to_string)
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        Ok(Self::new(id, api_key, base_url, tool_format))
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    functions: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<Value>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, LlmError> {
        let api_messages = convert_messages_for_api(&request.messages, self.tool_format);

        let (tools, tool_choice, functions, function_call) = match self.tool_format {
            ToolFormat::Tools => {
                let defs = request.tools.map(|defs| {
                    defs.iter()
                        .map(|d| d.to_openai_tool())
                        .collect::<Vec<_>>()
                });
                (defs, Some(json!("auto")), None, None)
            }
            ToolFormat::Functions => {
                let defs = request.tools.map(|defs| {
                    defs.iter()
                        .map(|d| d.to_openai_function())
                        .collect::<Vec<_>>()
                });
                (None, None, defs, Some(json!("auto")))
            }
        };

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages: api_messages,
            tools,
            tool_choice,
            functions,
            function_call,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        };

        debug!(
            provider = %self.id,
            url = %format!("{}/chat/completions", self.base_url),
            model = %body.model,
            messages = body.messages.len(),
            tool_format = ?self.tool_format,
            tools = body.tools.as_ref().map(|t| t.len()).unwrap_or(0),
            functions = body.functions.as_ref().map(|f| f.len()).unwrap_or(0),
            "openai chat request"
        );
        if let Ok(json) = serde_json::to_string(&body) {
            debug!(request_json = %truncate(&json, 12_000), "openai request body");
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            error!(
                status,
                body = %truncate(&message, 8_000),
                "openai api error"
            );
            return Err(LlmError::Api { status, message });
        }

        let byte_stream = response.bytes_stream();
        let stream = byte_stream
            .map(|chunk| chunk.map_err(LlmError::from))
            .scan(SseBuffer::default(), |buf, chunk| {
                futures::future::ready(match chunk {
                    Ok(bytes) => Some(parse_sse_chunk(buf, &bytes)),
                    Err(err) => Some(vec![Err(err)]),
                })
            })
            .flat_map(futures::stream::iter);

        Ok(Box::pin(stream))
    }
}

fn convert_messages_for_api(messages: &[Message], format: ToolFormat) -> Vec<Value> {
    messages
        .iter()
        .enumerate()
        .map(|(index, msg)| message_to_api_json(msg, &messages[..index], format))
        .collect()
}

fn message_to_api_json(msg: &Message, prior: &[Message], format: ToolFormat) -> Value {
    if format == ToolFormat::Tools {
        return serde_json::to_value(msg).expect("message serializes");
    }

    match msg.role {
        Role::Assistant => {
            let has_text = msg
                .content
                .as_ref()
                .is_some_and(|text| !text.is_empty());
            let mut obj = json!({
                "role": "assistant",
                "content": if has_text {
                    Value::String(msg.content.clone().unwrap_or_default())
                } else {
                    Value::Null
                }
            });
            if let Some(calls) = &msg.tool_calls {
                if let Some(first) = calls.first() {
                    obj["function_call"] = json!({
                        "name": first.name,
                        "arguments": first.arguments,
                    });
                }
            }
            obj
        }
        Role::Tool => {
            let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("");
            let name = lookup_function_name(prior, tool_call_id).unwrap_or_else(|| {
                warn!(
                    tool_call_id,
                    "could not resolve function name for tool result; using tool_call_id"
                );
                tool_call_id.to_string()
            });
            json!({
                "role": "function",
                "name": name,
                "content": msg.content.as_deref().unwrap_or(""),
            })
        }
        _ => serde_json::to_value(msg).expect("message serializes"),
    }
}

fn lookup_function_name(messages: &[Message], tool_call_id: &str) -> Option<String> {
    for msg in messages.iter().rev() {
        if let Some(calls) = &msg.tool_calls {
            for call in calls {
                if call.id == tool_call_id {
                    return Some(call.name.clone());
                }
            }
        }
    }
    None
}

#[derive(Default)]
struct SseBuffer {
    leftover: String,
}

fn parse_sse_chunk(buf: &mut SseBuffer, bytes: &[u8]) -> Vec<Result<StreamEvent, LlmError>> {
    let text = String::from_utf8_lossy(bytes);
    buf.leftover.push_str(&text);

    let mut events = Vec::new();
    while let Some(pos) = buf.leftover.find("\n\n") {
        let block = buf.leftover[..pos].to_string();
        buf.leftover = buf.leftover[pos + 2..].to_string();

        for line in block.lines() {
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    debug!("openai sse stream done");
                    events.push(Ok(StreamEvent::Done));
                    continue;
                }
                if data.contains("tool_calls") || data.contains("function_call") {
                    debug!(sse_data = %truncate(data, 4_000), "openai sse tool chunk");
                }
                match serde_json::from_str::<ChatCompletionChunk>(data) {
                    Ok(chunk) => events.extend(chunk_to_events(chunk)),
                    Err(err) => {
                        warn!(
                            error = %err,
                            sse_data = %truncate(data, 4_000),
                            "openai sse parse error"
                        );
                        events.push(Err(LlmError::StreamParse(err.to_string())));
                    }
                }
            }
        }
    }

    events
}

fn chunk_to_events(chunk: ChatCompletionChunk) -> Vec<Result<StreamEvent, LlmError>> {
    let mut events = Vec::new();

    if let Some(usage) = chunk.usage {
        events.push(Ok(StreamEvent::Usage(usage_to_usage(&usage))));
    }

    for choice in chunk.choices {
        if let Some(content) = choice.delta.content {
            if !content.is_empty() {
                events.push(Ok(StreamEvent::TextDelta(content)));
            }
        }
        if let Some(function_call) = choice.delta.function_call {
            let name = function_call.name.clone();
            let arguments = function_call.arguments.clone();
            debug!(
                index = 0,
                name = ?name,
                arguments = ?arguments,
                "openai parsed function_call delta"
            );
            events.push(Ok(StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name,
                arguments,
            }));
        }
        if let Some(tool_calls) = choice.delta.tool_calls {
            for tc in tool_calls {
                let id = tc.id();
                let name = tc.name();
                let arguments = tc.arguments();
                debug!(
                    index = tc.index,
                    id = ?id,
                    name = ?name,
                    arguments = ?arguments,
                    "openai parsed tool_call delta"
                );
                events.push(Ok(StreamEvent::ToolCallDelta {
                    index: tc.index,
                    id,
                    name,
                    arguments,
                }));
            }
        }
    }

    events
}

fn usage_to_usage(usage: &OpenAiUsage) -> Usage {
    Usage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    format!("{}… [truncated, total {} bytes]", &value[..max], value.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StreamFunctionDelta, StreamToolCallDelta, ToolCall};

    #[test]
    fn converts_tool_messages_to_function_role() {
        let messages = vec![
            Message::assistant(
                String::new(),
                Some(vec![ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: r#"{"path":"README.md"}"#.into(),
                }]),
            ),
            Message::tool("call_1", "file contents"),
        ];
        let api = convert_messages_for_api(&messages, ToolFormat::Functions);
        assert_eq!(api[0]["function_call"]["name"], "read");
        assert_eq!(api[1]["role"], "function");
        assert_eq!(api[1]["name"], "read");
        assert_eq!(api[1]["content"], "file contents");
    }

    #[test]
    fn parses_nested_and_flat_tool_call_deltas() {
        let nested = StreamToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            function: Some(StreamFunctionDelta {
                name: Some("read".into()),
                arguments: Some(r#"{"path":"#.into()),
            }),
            name: None,
            arguments: None,
        };
        assert_eq!(nested.name(), Some("read".into()));
        assert_eq!(nested.arguments(), Some(r#"{"path":"#.into()));

        let flat = StreamToolCallDelta {
            index: 1,
            id: Some("call_2".into()),
            function: None,
            name: Some("grep".into()),
            arguments: Some(r#"{"pattern":"foo"}"#.into()),
        };
        assert_eq!(flat.name(), Some("grep".into()));
        assert_eq!(flat.arguments(), Some(r#"{"pattern":"foo"}"#.into()));
    }

    #[test]
    fn collects_tool_arguments_after_finish_reason_chunk() {
        let name_chunk = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": { "name": "read", "arguments": "" }
                    }]
                },
                "finish_reason": null
            }]
        });
        let finish_chunk = serde_json::json!({
            "choices": [{
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        });
        let args_chunk = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": r#"{"path":"README.md"}"# }
                    }]
                },
                "finish_reason": null
            }]
        });

        let mut events = Vec::new();
        for value in [name_chunk, finish_chunk, args_chunk] {
            let chunk: ChatCompletionChunk = serde_json::from_value(value).unwrap();
            events.extend(chunk_to_events(chunk));
        }

        let mut args = String::new();
        for event in events {
            if let Ok(StreamEvent::ToolCallDelta { arguments: Some(part), .. }) = event {
                args.push_str(&part);
            }
        }
        assert_eq!(args, r#"{"path":"README.md"}"#);
    }

    #[test]
    fn parses_function_call_stream_delta() {
        let chunk = serde_json::json!({
            "choices": [{
                "delta": {
                    "function_call": {
                        "name": "read",
                        "arguments": r#"{"path":"README.md"}"#
                    }
                }
            }]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(chunk).unwrap();
        let events = chunk_to_events(chunk);
        assert!(matches!(
            events[0],
            Ok(StreamEvent::ToolCallDelta {
                name: Some(ref n),
                ..
            }) if n == "read"
        ));
    }
}
