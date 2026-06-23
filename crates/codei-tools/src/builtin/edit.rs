use async_trait::async_trait;
use serde_json::{json, Value};

use crate::path_util::resolve_workspace_path;
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace an exact unique string in a file with new content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing path".into()))?;
        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing old_string".into()))?;
        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing new_string".into()))?;

        let full = resolve_workspace_path(&ctx.cwd, path)?;
        let content = std::fs::read_to_string(&full)?;
        let count = content.matches(old_string).count();
        if count == 0 {
            return Err(ToolError::Failed {
                name: self.name().into(),
                message: "old_string not found".into(),
            });
        }
        if count > 1 {
            return Err(ToolError::Failed {
                name: self.name().into(),
                message: format!("old_string matched {count} times; must be unique"),
            });
        }
        let updated = content.replacen(old_string, new_string, 1);
        std::fs::write(&full, updated)?;

        Ok(ToolResult {
            content: format!("Updated {path}"),
            is_error: false,
        })
    }
}
