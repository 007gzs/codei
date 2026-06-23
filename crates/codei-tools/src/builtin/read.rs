use async_trait::async_trait;
use serde_json::{json, Value};

use crate::path_util::resolve_workspace_path;
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file from the workspace. Supports optional 1-based line offset and limit."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative file path" },
                "offset": { "type": "integer", "description": "Start line (1-based)" },
                "limit": { "type": "integer", "description": "Number of lines to read" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing path".into()))?;
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let full = resolve_workspace_path(&ctx.cwd, path)?;
        let content = std::fs::read_to_string(&full)?;
        let lines: Vec<&str> = content.lines().collect();
        let start = offset.saturating_sub(1);
        let end = limit
            .map(|l| start + l)
            .unwrap_or(lines.len())
            .min(lines.len());

        let mut out = String::new();
        for (idx, line) in lines[start..end].iter().enumerate() {
            let line_no = start + idx + 1;
            out.push_str(&format!("{line_no:>6}|{line}\n"));
        }

        Ok(ToolResult {
            content: out,
            is_error: false,
        })
    }
}
