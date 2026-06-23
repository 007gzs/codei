use std::sync::atomic::{AtomicU64, Ordering};

use codei_config::McpServer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::error::McpError;

const PROTOCOL_VERSION: &str = "2024-11-05";
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Metadata for a tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolCallResult {
    #[serde(default)]
    pub content: Vec<McpContentBlock>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpContentBlock {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub text: String,
}

impl McpToolCallResult {
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter(|b| b.kind == "text" || b.kind.is_empty())
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// JSON-RPC client over stdio transport.
pub struct McpClient {
    server_name: String,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: AtomicU64,
}

impl McpClient {
    pub async fn connect(server: &McpServer) -> Result<Self, McpError> {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (key, value) in &server.env {
            cmd.env(key, value);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|source| McpError::Spawn {
            name: server.name.clone(),
            source,
        })?;

        let stdin = child.stdin.take().ok_or_else(|| McpError::Protocol {
            server: server.name.clone(),
            message: "missing stdin".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Protocol {
            server: server.name.clone(),
            message: "missing stdout".into(),
        })?;

        let mut client = Self {
            server_name: server.name.clone(),
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
        };

        client.initialize().await?;
        Ok(client)
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "codei",
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            )
            .await?;

        debug!(
            server = %self.server_name,
            result = %result,
            "MCP initialize complete"
        );

        self.notify("notifications/initialized", json!({})).await?;
        Ok(())
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, McpError> {
        let result = self.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut parsed = Vec::new();
        for tool in tools {
            let info: McpToolInfo =
                serde_json::from_value(tool).map_err(|err| McpError::Protocol {
                    server: self.server_name.clone(),
                    message: format!("invalid tool definition: {err}"),
                })?;
            parsed.push(info);
        }
        Ok(parsed)
    }

    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, McpError> {
        let result = self
            .request(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": arguments,
                }),
            )
            .await?;
        serde_json::from_value(result).map_err(|err| McpError::Protocol {
            server: self.server_name.clone(),
            message: format!("invalid tools/call response: {err}"),
        })
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&message).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&message).await?;

        let response = timeout(
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
            self.read_response(id),
        )
        .await
        .map_err(|_| McpError::Timeout {
            secs: REQUEST_TIMEOUT_SECS,
        })??;

        Ok(response)
    }

    async fn read_response(&mut self, id: u64) -> Result<Value, McpError> {
        loop {
            if self.child.try_wait()?.is_some() {
                return Err(McpError::Exited {
                    name: self.server_name.clone(),
                });
            }

            let line = self.read_line().await?;
            if line.trim().is_empty() {
                continue;
            }

            let value: Value = serde_json::from_str(&line)?;
            if value.get("method").is_some() && value.get("id").is_none() {
                debug!(server = %self.server_name, %line, "MCP notification");
                continue;
            }

            if value.get("id").and_then(|v| v.as_u64()) != Some(id) {
                warn!(server = %self.server_name, %line, "unexpected MCP response id");
                continue;
            }

            if let Some(error) = value.get("error") {
                let message = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return Err(McpError::Protocol {
                    server: self.server_name.clone(),
                    message: message.to_string(),
                });
            }

            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), McpError> {
        let line = serde_json::to_string(message)?;
        debug!(server = %self.server_name, %line, "MCP send");
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_line(&mut self) -> Result<String, McpError> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        if line.is_empty() {
            return Err(McpError::Exited {
                name: self.server_name.clone(),
            });
        }
        Ok(line)
    }
}
