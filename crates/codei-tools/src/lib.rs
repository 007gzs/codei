//! Agent tools for CodeI.

mod approval;
mod approval_gate;
mod builtin;
mod error;
mod mcp_tool;
mod path_util;
mod registry;

pub use approval::{
    handler_for_policy, ApprovalHandler, ApprovalPolicy, ApprovalRequest, ApprovalResponse,
    AutoApprove, OnDestructiveApprove, PromptApprove,
};
pub use approval_gate::{GateApprovalHandler, SharedApprovalGate};
pub use error::ToolError;
pub use mcp_tool::register_mcp_tools;
pub use registry::{Tool, ToolContext, ToolRegistry, ToolResult};

use serde_json::json;

pub fn default_registry(config: &codei_config::ResolvedConfig) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(builtin::ReadTool));
    registry.register(Box::new(builtin::ReadSkillTool));
    registry.register(Box::new(builtin::WriteTool));
    registry.register(Box::new(builtin::EditTool));
    registry.register(Box::new(builtin::GlobTool));
    registry.register(Box::new(builtin::GrepTool::new(&config.config.tools.grep)));
    registry.register(Box::new(builtin::ShellTool::new(
        config.config.tools.shell.timeout_secs,
        config.config.tools.shell.sandbox,
        config.config.tools.shell.allowlist.clone(),
    )));
    registry.register(Box::new(builtin::ListDirTool));
    registry.register(Box::new(builtin::DefinitionTool));
    if config.config.tools.web_fetch.enabled {
        registry.register(Box::new(builtin::WebFetchTool::new(
            config.config.tools.web_fetch.timeout_secs,
            config.config.tools.web_fetch.max_bytes,
            config.config.tools.web_fetch.ssrf_protection,
        )));
    }
    if config.config.tools.web_search.enabled {
        let ws = &config.config.tools.web_search;
        registry.register(Box::new(builtin::WebSearchTool::new(
            ws.provider,
            ws.timeout_secs,
            ws.max_results,
            ws.searxng_url.clone(),
            ws.ssrf_protection,
        )));
    }
    registry
}

pub fn tool_definitions(registry: &ToolRegistry) -> Vec<codei_llm::ToolDefinition> {
    registry.definitions()
}

/// JSON schema helpers shared by builtin tools.
pub fn path_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path relative to the workspace root"
            }
        },
        "required": ["path"]
    })
}
