use std::sync::Arc;

use async_trait::async_trait;
use codei_config::ResolvedConfig;
use serde_json::Value;

use crate::approval::{ApprovalHandler, ApprovalRequest};
use crate::ToolError;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

#[derive(Clone)]
pub struct ToolContext {
    pub cwd: std::path::PathBuf,
    pub config: Arc<ResolvedConfig>,
    pub approval: Arc<dyn ApprovalHandler>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError>;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn definitions(&self) -> Vec<codei_llm::ToolDefinition> {
        self.tools
            .iter()
            .map(|tool| codei_llm::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    pub async fn execute(
        &self,
        ctx: &ToolContext,
        name: &str,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let tool = self.get(name).ok_or_else(|| ToolError::Failed {
            name: name.to_string(),
            message: "unknown tool".into(),
        })?;

        if tool.requires_approval() {
            let response = ctx
                .approval
                .approve(ApprovalRequest {
                    tool_name: name.to_string(),
                    arguments: args.clone(),
                })
                .await;
            if !response.approved {
                return Err(ToolError::Denied);
            }
        }

        tool.execute(ctx, args).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
