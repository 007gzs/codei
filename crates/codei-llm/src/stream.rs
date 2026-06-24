use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::AssistantResponse;

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, crate::LlmError>> + Send>>;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl Usage {
    pub fn add_assign(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }

    pub fn total(&self) -> u32 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    Usage(Usage),
    Done,
}

/// Collect a chat stream into a single assistant response.
#[allow(dead_code)]
pub async fn collect_response<S>(mut stream: S) -> Result<AssistantResponse, crate::LlmError>
where
    S: Stream<Item = Result<StreamEvent, crate::LlmError>> + Unpin,
{
    use futures_util::StreamExt;

    let mut response = AssistantResponse::default();
    let mut pending_tools: std::collections::BTreeMap<
        u32,
        (Option<String>, Option<String>, String),
    > = std::collections::BTreeMap::new();

    while let Some(item) = stream.next().await {
        match item? {
            StreamEvent::TextDelta(text) => response.content.push_str(&text),
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                let entry = pending_tools.entry(index).or_default();
                if let Some(id) = id {
                    entry.0 = Some(id);
                }
                if let Some(name) = name {
                    entry.1 = Some(name);
                }
                if let Some(args) = arguments {
                    entry.2.push_str(&args);
                }
            }
            StreamEvent::Usage(usage) => response.usage = Some(usage),
            StreamEvent::Done => {}
        }
    }

    for (_, (id, name, arguments)) in pending_tools {
        if let (Some(id), Some(name)) = (id, name) {
            response.tool_calls.push(crate::ToolCall {
                id,
                name,
                arguments,
            });
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn collects_text_and_tool_calls() {
        let events = vec![
            Ok(StreamEvent::TextDelta("hi".into())),
            Ok(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("read".into()),
                arguments: None,
            }),
            Ok(StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments: Some(r#"{"path":"a.rs"}"#.into()),
            }),
            Ok(StreamEvent::Done),
        ];
        let response = collect_response(stream::iter(events)).await.unwrap();
        assert_eq!(response.content, "hi");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "read");
    }
}
