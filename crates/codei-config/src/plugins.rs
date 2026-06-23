use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginsConfig {
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    pub event: HookEvent,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    BeforeTurn,
    AfterTurn,
}

pub fn load_plugins(project_root: &Path) -> PluginsConfig {
    let path = project_root.join(".codei").join("hooks.toml");
    if !path.is_file() {
        return PluginsConfig::default();
    }
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => PluginsConfig::default(),
    }
}

pub fn run_hook(hook: &HookConfig, cwd: &Path, env: &[(&str, String)]) -> Result<(), ConfigError> {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-lc").arg(&hook.command).current_dir(cwd);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let status = cmd.status().map_err(|source| ConfigError::Read {
        path: cwd.to_path_buf(),
        source,
    })?;
    if !status.success() {
        return Err(ConfigError::HookFailed {
            command: hook.command.clone(),
            code: status.code(),
        });
    }
    Ok(())
}

pub fn run_hooks(
    config: &PluginsConfig,
    event: HookEvent,
    cwd: &Path,
    env: &[(&str, String)],
) -> Result<(), ConfigError> {
    for hook in config.hooks.iter().filter(|hook| hook.event == event) {
        run_hook(hook, cwd, env)?;
    }
    Ok(())
}
