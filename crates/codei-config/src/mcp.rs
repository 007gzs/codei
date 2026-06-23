use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::paths::expand_tilde;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

pub fn mcp_config_path() -> PathBuf {
    expand_tilde("~/.config/codei/mcp-servers.toml")
}

pub fn load_mcp_config() -> Result<McpConfig, ConfigError> {
    let path = mcp_config_path();
    if !path.is_file() {
        return Ok(McpConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    parse_mcp_config(&text)
}

pub fn parse_mcp_config(text: &str) -> Result<McpConfig, ConfigError> {
    let config: McpConfig =
        toml::from_str(text).map_err(|e| ConfigError::McpParse(e.to_string()))?;
    Ok(config)
}

pub fn save_mcp_config(config: &McpConfig) -> Result<(), ConfigError> {
    let path = mcp_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Read {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let text = toml::to_string_pretty(config).map_err(|e| ConfigError::McpParse(e.to_string()))?;
    fs::write(&path, text).map_err(|source| ConfigError::Read { path, source })
}

pub fn init_mcp_config() -> Result<(PathBuf, bool), ConfigError> {
    let path = mcp_config_path();
    if path.is_file() {
        return Ok((path, false));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Read {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, default_mcp_template()).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    Ok((path, true))
}

pub fn default_mcp_template() -> &'static str {
    r#"# MCP server configuration for CodeI
# Docs: https://modelcontextprotocol.io/

[[servers]]
name = "example"
command = "node"
args = ["/path/to/mcp-server.js"]
"#
}
