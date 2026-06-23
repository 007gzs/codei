use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolFormat {
    /// OpenAI `tools` + `tool_choice` + `tool_calls` (supported by vLLM and OpenAI).
    #[default]
    Tools,
    /// OpenAI `functions` + `function_call` (legacy; many vLLM builds ignore this).
    Functions,
}

impl ToolFormat {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("tools") => Self::Tools,
            Some("functions") | Some("function") | Some("function_calling") => Self::Functions,
            _ => Self::default(),
        }
    }
}
