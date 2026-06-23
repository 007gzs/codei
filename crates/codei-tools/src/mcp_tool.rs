use std::sync::Arc;

use async_trait::async_trait;
use codei_mcp::{registered_tool_name, McpConnection, McpManager};
use serde_json::Value;

use crate::{Tool, ToolContext, ToolError, ToolResult};

/// Register all MCP tools from connected servers into the registry.
pub fn register_mcp_tools(registry: &mut crate::ToolRegistry, manager: &McpManager) {
    for conn in manager.connections() {
        for tool in &conn.tools {
            registry.register(Box::new(McpRegisteredTool {
                registered_name: registered_tool_name(&conn.server_name, &tool.name),
                remote_name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                connection: Arc::clone(conn),
            }));
        }
    }
}

struct McpRegisteredTool {
    registered_name: String,
    remote_name: String,
    description: String,
    input_schema: Value,
    connection: Arc<McpConnection>,
}

#[async_trait]
impl Tool for McpRegisteredTool {
    fn name(&self) -> &str {
        &self.registered_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let result = self
            .connection
            .call_tool(&self.remote_name, args)
            .await
            .map_err(|err| ToolError::Failed {
                name: self.registered_name.clone(),
                message: err.to_string(),
            })?;
        Ok(ToolResult {
            content: result.text(),
            is_error: result.is_error,
        })
    }
}
