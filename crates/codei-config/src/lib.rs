//! Configuration loading and resolution for CodeI.

mod error;
mod load;
mod mcp;
mod model;
mod paths;
mod plugins;
mod skills;
mod template;

pub use error::ConfigError;
pub use load::{load, LoadOptions};
pub use mcp::{
    default_mcp_template, init_mcp_config, load_mcp_config, mcp_config_path, parse_mcp_config,
    save_mcp_config, McpConfig, McpServer,
};
pub use model::{
    AgentConfig, Config, DefaultsConfig, GrepToolConfig, ProviderConfig, ResolvedConfig,
    SessionConfig, SessionStorage, ShellSandboxMode, ToolsConfig, UiConfig, UiTheme,
};
pub use paths::{
    debug_log_path, discover_project_root, expand_tilde, project_config_path, user_config_dir,
    user_config_path, user_log_dir,
};
pub use plugins::{load_plugins, run_hooks, HookConfig, HookEvent, PluginsConfig};
pub use skills::{
    discover_skills, find_skill, format_skills_for_prompt, read_skill_body, Skill, SkillSource,
};
pub use template::{default_config_template, init_user_config};
