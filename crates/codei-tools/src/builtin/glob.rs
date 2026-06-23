use async_trait::async_trait;
use ignore::WalkBuilder;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern under the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern, e.g. **/*.rs" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing pattern".into()))?;

        let glob =
            glob::Pattern::new(pattern).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        let mut matches = Vec::new();

        for entry in WalkBuilder::new(&ctx.cwd)
            .hidden(false)
            .git_ignore(true)
            .build()
        {
            let entry = entry.map_err(|e| ToolError::Io(std::io::Error::other(e)))?;
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            if let Ok(rel) = path.strip_prefix(&ctx.cwd) {
                let rel = rel.to_string_lossy();
                if glob.matches(&rel) {
                    matches.push(rel.to_string());
                }
            }
        }

        matches.sort();
        Ok(ToolResult {
            content: matches.join("\n"),
            is_error: false,
        })
    }
}
