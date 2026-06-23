use std::fs;
use std::path::PathBuf;

use crate::error::ConfigError;
use crate::paths::{user_config_dir, user_config_path};

/// Default user config template written by `codei config init`.
pub fn default_config_template() -> &'static str {
    include_str!("default_config.toml")
}

/// Creates `~/.config/codei/config.toml` if it does not exist.
/// Returns the path and whether a new file was created.
pub fn init_user_config() -> Result<(PathBuf, bool), ConfigError> {
    let path = user_config_path();
    if path.exists() {
        return Ok((path, false));
    }

    let dir = user_config_dir();
    fs::create_dir_all(&dir).map_err(|source| ConfigError::CreateDir {
        path: dir.clone(),
        source,
    })?;

    fs::write(&path, default_config_template()).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;

    Ok((path, true))
}
