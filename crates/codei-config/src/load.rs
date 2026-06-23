use std::env;
use std::path::PathBuf;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};

use crate::error::ConfigError;
use crate::model::{Config, ResolvedConfig};
use crate::paths::{discover_project_root, project_config_path, user_config_path};

/// Options that override layered configuration (CLI flags, etc.).
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub cwd: Option<PathBuf>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub language: Option<String>,
}

/// Loads configuration from defaults, user file, project file, env, and CLI overrides.
pub fn load(opts: &LoadOptions) -> Result<ResolvedConfig, ConfigError> {
    let cwd = opts
        .cwd
        .clone()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let user_config_path = user_config_path();
    let project_root = discover_project_root(&cwd);
    let project_config_path = project_root.as_ref().map(|root| project_config_path(root));

    let mut figment = Figment::new().merge(Serialized::defaults(Config::default()));

    if user_config_path.is_file() {
        figment = figment.merge(Toml::file(&user_config_path));
    }

    if let Some(path) = &project_config_path {
        if path.is_file() {
            figment = figment.merge(Toml::file(path));
        }
    }

    // e.g. CODEI_DEFAULTS__MODEL=gpt-4o-mini
    figment = figment.merge(Env::prefixed("CODEI_").split("__"));

    let mut config: Config = figment
        .extract()
        .map_err(|e| ConfigError::Load(Box::new(e)))?;
    apply_cli_overrides(&mut config, opts);

    let resolved = ResolvedConfig {
        config,
        cwd,
        project_root,
        user_config_path,
        project_config_path,
    };
    resolved.validate()?;
    Ok(resolved)
}

fn apply_cli_overrides(config: &mut Config, opts: &LoadOptions) {
    if let Some(model) = &opts.model {
        config.defaults.model = model.clone();
    }
    if let Some(provider) = &opts.provider {
        config.defaults.provider = provider.clone();
    }
    if let Some(language) = &opts.language {
        config.defaults.language = language.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    use tempfile::TempDir;

    fn write_config(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::File::create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    #[test]
    fn loads_defaults_without_files() {
        let dir = TempDir::new().unwrap();
        let opts = LoadOptions {
            cwd: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let resolved = load(&opts).unwrap();
        assert!(!resolved.config.defaults.provider.is_empty());
        assert!(resolved.config.defaults.max_tokens > 0);
    }

    #[test]
    fn merges_project_config() {
        let dir = TempDir::new().unwrap();
        write_config(
            dir.path(),
            ".codei/config.toml",
            r#"
[defaults]
model = "project-model"
"#,
        );

        let opts = LoadOptions {
            cwd: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let resolved = load(&opts).unwrap();
        assert_eq!(resolved.config.defaults.model, "project-model");
        assert_eq!(
            resolved.project_root.as_deref(),
            Some(dir.path().canonicalize().unwrap().as_path())
        );
    }

    #[test]
    fn cli_overrides_take_precedence() {
        let dir = TempDir::new().unwrap();
        write_config(
            dir.path(),
            ".codei/config.toml",
            r#"
[defaults]
model = "project-model"
"#,
        );

        let opts = LoadOptions {
            cwd: Some(dir.path().to_path_buf()),
            model: Some("cli-model".to_string()),
            ..Default::default()
        };
        let resolved = load(&opts).unwrap();
        assert_eq!(resolved.config.defaults.model, "cli-model");
    }
}
