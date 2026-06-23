use async_trait::async_trait;
use serde_json::{json, Value};

use crate::path_util::resolve_workspace_path;
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List files and directories in a workspace path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative directory path, default '.'" }
            }
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let full = resolve_workspace_path(&ctx.cwd, path)?;
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&full)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if file_type.is_dir() { "dir" } else { "file" };
            entries.push(format!("[{kind}] {name}"));
        }
        entries.sort();
        Ok(ToolResult {
            content: entries.join("\n"),
            is_error: false,
        })
    }
}
