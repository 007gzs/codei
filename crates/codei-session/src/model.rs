use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use codei_llm::{Message, Role as LlmRole, ToolCall};

pub type SessionId = String;
pub type MessageId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub title: Option<String>,
    pub cwd: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<StoredMessage>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: MessageId,
    pub role: Role,
    pub content: MessageContent,
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    pub tool_call_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl Session {
    pub fn new(cwd: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: None,
            cwd,
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn push_user(&mut self, content: impl Into<String>) -> &StoredMessage {
        self.push_message(Role::User, MessageContent::Text(content.into()), None, None)
    }

    pub fn push_assistant(
        &mut self,
        content: String,
        tool_calls: Option<Vec<ToolCallRecord>>,
    ) -> &StoredMessage {
        self.push_message(
            Role::Assistant,
            MessageContent::Text(content),
            tool_calls,
            None,
        )
    }

    pub fn push_tool(
        &mut self,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> &StoredMessage {
        self.push_message(
            Role::Tool,
            MessageContent::Text(content.into()),
            None,
            Some(tool_call_id.into()),
        )
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.touch();
    }

    /// Drop older messages, keeping the most recent `keep_recent` entries.
    pub fn compact(&mut self, keep_recent: usize) {
        if self.messages.len() <= keep_recent {
            return;
        }
        let remove = self.messages.len() - keep_recent;
        self.messages.drain(0..remove);
        self.touch();
    }

    fn push_message(
        &mut self,
        role: Role,
        content: MessageContent,
        tool_calls: Option<Vec<ToolCallRecord>>,
        tool_call_id: Option<String>,
    ) -> &StoredMessage {
        let msg = StoredMessage {
            id: Uuid::new_v4().to_string(),
            role,
            content,
            tool_calls,
            tool_call_id,
            created_at: Utc::now(),
        };
        self.messages.push(msg);
        self.touch();
        self.messages.last().expect("message just pushed")
    }
}

impl From<ToolCall> for ToolCallRecord {
    fn from(value: ToolCall) -> Self {
        Self {
            id: value.id,
            name: value.name,
            arguments: value.arguments,
        }
    }
}

impl StoredMessage {
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            MessageContent::Text(s) => Some(s),
        }
    }
}

pub fn to_llm_messages(session: &Session, system_prompt: &str) -> Vec<Message> {
    let mut messages = vec![Message::system(system_prompt)];
    for msg in &session.messages {
        match msg.role {
            Role::User => {
                if let Some(text) = msg.text() {
                    messages.push(Message::user(text));
                }
            }
            Role::Assistant => {
                let tool_calls: Option<Vec<ToolCall>> = msg.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|c| ToolCall {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            arguments: c.arguments.clone(),
                        })
                        .collect()
                });
                let text = msg.text().unwrap_or("").to_string();
                if let Some(calls) = &tool_calls {
                    if !calls.is_empty() {
                        messages.push(Message {
                            role: LlmRole::Assistant,
                            content: if text.is_empty() { None } else { Some(text) },
                            tool_calls,
                            tool_call_id: None,
                        });
                    } else {
                        messages.push(Message::assistant(text, None));
                    }
                } else {
                    messages.push(Message::assistant(text, None));
                }
            }
            Role::Tool => {
                if let (Some(id), Some(text)) = (&msg.tool_call_id, msg.text()) {
                    messages.push(Message::tool(id.clone(), text));
                }
            }
            Role::System => {}
        }
    }
    messages
}
