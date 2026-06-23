use std::sync::Arc;

use codei_config::{load_mcp_config, McpConfig, McpServer};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::client::{McpClient, McpToolInfo};
use crate::error::McpError;

/// A connected MCP server with its discovered tools.
pub struct McpConnection {
    pub server_name: String,
    client: Arc<Mutex<McpClient>>,
    pub tools: Vec<McpToolInfo>,
}

impl McpConnection {
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<crate::client::McpToolCallResult, McpError> {
        let mut client = self.client.lock().await;
        client.call_tool(tool_name, arguments).await
    }
}

/// Manages all configured MCP server connections.
pub struct McpManager {
    connections: Vec<Arc<McpConnection>>,
}

impl McpManager {
    pub async fn connect_all(config: &McpConfig) -> Result<Self, McpError> {
        let mut connections = Vec::new();
        for server in &config.servers {
            match Self::connect_server(server).await {
                Ok(conn) => connections.push(conn),
                Err(err) => {
                    warn!(server = %server.name, %err, "failed to connect MCP server");
                }
            }
        }
        Ok(Self { connections })
    }

    pub async fn connect_from_config() -> Result<Self, McpError> {
        let config = load_mcp_config().map_err(|err| McpError::Protocol {
            server: "config".into(),
            message: err.to_string(),
        })?;
        Self::connect_all(&config).await
    }

    async fn connect_server(server: &McpServer) -> Result<Arc<McpConnection>, McpError> {
        let mut client = McpClient::connect(server).await?;
        let tools = client.list_tools().await?;
        info!(
            server = %server.name,
            tools = tools.len(),
            "connected MCP server"
        );
        Ok(Arc::new(McpConnection {
            server_name: server.name.clone(),
            client: Arc::new(Mutex::new(client)),
            tools,
        }))
    }

    pub fn connections(&self) -> &[Arc<McpConnection>] {
        &self.connections
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    pub fn tool_count(&self) -> usize {
        self.connections.iter().map(|c| c.tools.len()).sum()
    }

    /// Connect to configured servers; returns `None` when none are available.
    pub async fn connect_optional() -> Option<Arc<Self>> {
        match Self::connect_from_config().await {
            Ok(manager) if !manager.is_empty() => Some(Arc::new(manager)),
            Ok(_) => None,
            Err(err) => {
                warn!(%err, "MCP initialization failed");
                None
            }
        }
    }

    /// Resolve `mcp_{server}_{tool}` back to connection + original tool name.
    pub fn resolve_tool(&self, registered_name: &str) -> Option<(Arc<McpConnection>, String)> {
        for conn in &self.connections {
            let prefix = format!("mcp_{}_", sanitize_name(&conn.server_name));
            if let Some(tool_name) = registered_name.strip_prefix(&prefix) {
                return Some((Arc::clone(conn), tool_name.to_string()));
            }
        }
        None
    }
}

/// Build a stable tool name for the LLM registry.
pub fn registered_tool_name(server_name: &str, tool_name: &str) -> String {
    format!("mcp_{}_{}", sanitize_name(server_name), tool_name)
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
