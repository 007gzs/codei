use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: String, tool_calls: Option<Vec<ToolCall>>) -> Self {
        let text = content;
        Self {
            role: Role::Assistant,
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// OpenAI chat completions expect `tool_calls` entries shaped as
/// `{ "id", "type": "function", "function": { "name", "arguments" } }`.
#[derive(Serialize)]
struct OpenAiToolCall<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiToolFunction<'a>,
}

#[derive(Serialize)]
struct OpenAiToolFunction<'a> {
    name: &'a str,
    arguments: &'a str,
}

impl Serialize for ToolCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        OpenAiToolCall {
            id: &self.id,
            kind: "function",
            function: OpenAiToolFunction {
                name: &self.name,
                arguments: &self.arguments,
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolCall {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            arguments: Option<String>,
            #[serde(default)]
            function: Option<OpenAiToolFunctionOwned>,
        }

        #[derive(Deserialize)]
        struct OpenAiToolFunctionOwned {
            name: String,
            arguments: String,
        }

        let raw = Raw::deserialize(deserializer)?;
        if let Some(function) = raw.function {
            return Ok(Self {
                id: raw.id,
                name: function.name,
                arguments: function.arguments,
            });
        }

        Ok(Self {
            id: raw.id,
            name: raw.name.ok_or_else(|| {
                serde::de::Error::custom("tool call missing name/function")
            })?,
            arguments: raw.arguments.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_tool_call_for_openai() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "list_dir".into(),
            arguments: "{}".into(),
        };
        let value = serde_json::to_value(&call).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "list_dir",
                    "arguments": "{}"
                }
            })
        );
    }

    #[test]
    fn deserializes_openai_and_flat_tool_call_shapes() {
        let openai = json!({
            "id": "call_1",
            "type": "function",
            "function": {
                "name": "read",
                "arguments": r#"{"path":"a.rs"}"#
            }
        });
        let flat = json!({
            "id": "call_2",
            "name": "grep",
            "arguments": r#"{"pattern":"foo"}"#
        });

        let from_openai: ToolCall = serde_json::from_value(openai).unwrap();
        let from_flat: ToolCall = serde_json::from_value(flat).unwrap();

        assert_eq!(from_openai.name, "read");
        assert_eq!(from_flat.name, "grep");
    }
}
