use std::env;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

const APP_QUALIFIER: &str = "com";
const APP_ORG: &str = "codei";
const APP_NAME: &str = "codei";

/// Returns the user-level configuration directory (`~/.config/codei` on Linux).
pub fn user_config_dir() -> PathBuf {
    ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".config/codei"))
}

/// Returns the directory for debug/runtime logs (`~/.local/share/codei/logs` on Linux).
pub fn user_log_dir() -> PathBuf {
    ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
        .map(|dirs| dirs.data_local_dir().join("logs"))
        .unwrap_or_else(|| PathBuf::from(".local/share/codei/logs"))
}

/// Path to the append-only debug log file used with `codei --verbose`.
pub fn debug_log_path() -> PathBuf {
    user_log_dir().join("debug.log")
}

/// Returns the path to the user-level config file.
pub fn user_config_path() -> PathBuf {
    user_config_dir().join("config.toml")
}

/// Returns the project-level config path under `.codei/config.toml`.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".codei").join("config.toml")
}

/// Walks up from `start` looking for project markers.
pub fn discover_project_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let mut current = start;

    loop {
        if is_project_root(&current) {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }

    None
}

fn is_project_root(path: &Path) -> bool {
    path.join(".git").exists() || path.join(".codei").is_dir() || path.join("AGENTS.md").is_file()
}

/// Expands a leading `~` using `HOME`.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_uses_home() {
        if let Ok(home) = env::var("HOME") {
            assert_eq!(
                expand_tilde("~/sessions"),
                PathBuf::from(home).join("sessions")
            );
        }
    }
}
