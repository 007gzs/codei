use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Full configuration after merging all sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: DefaultsConfig,

    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub tools: ToolsConfig,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub session: SessionConfig,
}

/// Resolved configuration with metadata about where values came from.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub config: Config,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
    pub user_config_path: PathBuf,
    pub project_config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub model: String,
    pub provider: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Direct API key; takes precedence over `api_key_env` when set.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_style: Option<String>,
    /// `tools` (OpenAI tools API) or `functions` (legacy function calling). Default: `functions`.
    #[serde(default)]
    pub tool_format: Option<String>,
}

impl ProviderConfig {
    /// Resolve API key: `api_key` first, then the environment variable named by `api_key_env`.
    pub fn resolve_api_key(&self) -> Result<String, crate::ConfigError> {
        if let Some(key) = self.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            return Ok(key.to_string());
        }
        let env_name = self
            .api_key_env
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or("OPENAI_API_KEY");
        std::env::var(env_name).map_err(|_| crate::ConfigError::MissingApiKey {
            env: env_name.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_turns: u32,
    pub max_tool_rounds_per_turn: u32,
    pub context_window_tokens: u32,
    pub compaction_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub shell: ShellToolConfig,
    pub write: EnabledToolConfig,
    pub web_search: EnabledToolConfig,
    #[serde(default)]
    pub grep: GrepToolConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepToolConfig {
    pub max_matches: usize,
    pub max_files: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShellSandboxMode {
    #[default]
    Off,
    Restricted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellToolConfig {
    pub enabled: bool,
    pub timeout_secs: u64,
    #[serde(default)]
    pub sandbox: ShellSandboxMode,
    #[serde(default)]
    pub allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledToolConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: UiTheme,
    pub show_tool_output: bool,
    pub confirm_destructive: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    #[default]
    Auto,
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub storage: SessionStorage,
    pub dir: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStorage {
    #[default]
    Sqlite,
    Json,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            defaults: DefaultsConfig::default(),
            providers: default_providers(),
            agent: AgentConfig::default(),
            tools: ToolsConfig::default(),
            ui: UiConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            temperature: 0.2,
            max_tokens: 8192,
            language: "zh-CN".to_string(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_tool_rounds_per_turn: 25,
            context_window_tokens: 128_000,
            compaction_threshold: 0.85,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            shell: ShellToolConfig {
                enabled: true,
                timeout_secs: 120,
                sandbox: ShellSandboxMode::Off,
                allowlist: Vec::new(),
            },
            write: EnabledToolConfig { enabled: true },
            web_search: EnabledToolConfig { enabled: false },
            grep: GrepToolConfig::default(),
        }
    }
}

impl Default for GrepToolConfig {
    fn default() -> Self {
        Self {
            max_matches: 200,
            max_files: 5_000,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: UiTheme::Auto,
            show_tool_output: true,
            confirm_destructive: true,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            storage: SessionStorage::Sqlite,
            dir: "~/.local/share/codei/sessions".to_string(),
        }
    }
}

fn default_providers() -> HashMap<String, ProviderConfig> {
    HashMap::from([
        (
            "openai".to_string(),
            ProviderConfig {
                api_key: None,
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                base_url: Some("https://api.openai.com/v1".to_string()),
                api_style: Some("openai".to_string()),
                tool_format: Some("tools".to_string()),
            },
        ),
        (
            "anthropic".to_string(),
            ProviderConfig {
                api_key: None,
                api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
                base_url: None,
                api_style: Some("anthropic".to_string()),
                tool_format: None,
            },
        ),
        (
            "custom".to_string(),
            ProviderConfig {
                api_key: None,
                api_key_env: Some("CUSTOM_API_KEY".to_string()),
                base_url: Some("http://localhost:8080/v1".to_string()),
                api_style: Some("openai".to_string()),
                tool_format: Some("tools".to_string()),
            },
        ),
    ])
}

impl ResolvedConfig {
    pub fn validate(&self) -> Result<(), crate::ConfigError> {
        let lang = &self.config.defaults.language;
        if lang != "zh-CN" && lang != "en-US" {
            return Err(crate::ConfigError::InvalidLanguage {
                language: lang.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    use super::ProviderConfig;

    #[test]
    fn resolve_api_key_prefers_direct_config() {
        let cfg = ProviderConfig {
            api_key: Some("sk-direct".into()),
            api_key_env: Some("OPENAI_API_KEY".into()),
            base_url: None,
            api_style: None,
            tool_format: None,
        };
        assert_eq!(cfg.resolve_api_key().unwrap(), "sk-direct");
    }

    #[test]
    fn blank_api_key_falls_back_to_env() {
        let cfg = ProviderConfig {
            api_key: Some("   ".into()),
            api_key_env: Some("NONEXISTENT_CODEI_API_KEY".into()),
            base_url: None,
            api_style: None,
            tool_format: None,
        };
        let err = cfg.resolve_api_key().unwrap_err();
        assert!(matches!(
            err,
            crate::ConfigError::MissingApiKey {
                env: ref name
            } if name == "NONEXISTENT_CODEI_API_KEY"
        ));
    }
}
