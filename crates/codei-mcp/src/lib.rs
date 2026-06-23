//! MCP (Model Context Protocol) stdio client for CodeI.

mod client;
mod error;
mod manager;

pub use client::{McpClient, McpContentBlock, McpToolCallResult, McpToolInfo};
pub use error::McpError;
pub use manager::{registered_tool_name, McpConnection, McpManager};
