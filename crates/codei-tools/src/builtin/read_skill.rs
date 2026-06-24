use async_trait::async_trait;
use codei_config::{discover_skills, find_skill, read_skill_body};
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct ReadSkillTool;

#[async_trait]
impl Tool for ReadSkillTool {
    fn name(&self) -> &str {
        "read_skill"
    }

    fn description(&self) -> &str {
        "Load specialized instructions from a named skill. Use when the task matches a skill listed in Available skills."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name from the Available skills list"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing name".into()))?;

        let skills = discover_skills(&ctx.config);
        let skill = find_skill(&skills, name).ok_or_else(|| {
            let available = skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ToolError::InvalidArgs(format!(
                "unknown skill '{name}'{}",
                if available.is_empty() {
                    String::new()
                } else {
                    format!("; available: {available}")
                }
            ))
        })?;

        let body = read_skill_body(skill)?;
        Ok(ToolResult {
            content: format!("# Skill: {}\n\n{body}", skill.name),
            is_error: false,
        })
    }
}
