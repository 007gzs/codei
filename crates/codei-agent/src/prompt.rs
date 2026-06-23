use std::env;
use std::fs;
use std::path::Path;

use codei_config::ResolvedConfig;

pub fn build_system_prompt(config: &ResolvedConfig, project_instructions: &str) -> String {
    let cwd = config.cwd.display();
    let os = env::consts::OS;
    let language = &config.config.defaults.language;

    format!(
        r#"You are CodeI, an AI coding assistant running in the user's local terminal.

## Capabilities
- Read, write, and edit files; search the codebase; run shell commands
- Working directory: {cwd}
- Operating system: {os}

## Project instructions
{project_instructions}

## Tool usage
- Read relevant files before editing code
- Prefer specialized tools over shell for file operations
- When using edit, ensure old_string matches exactly once
- Do not make unrelated changes the user did not ask for

## Output
- Communicate with the user in {language}
- Be concise and actionable"#
    )
}

/// Load project instructions from AGENTS.md and `.codei/rules/*.md`.
pub fn load_project_instructions(config: &ResolvedConfig) -> String {
    let Some(root) = config.project_root.as_ref() else {
        return String::new();
    };

    let mut sections = Vec::new();

    for rel in [".codei/AGENTS.md", "AGENTS.md"] {
        let path = root.join(rel);
        if let Some(content) = read_file_if_exists(&path) {
            sections.push(format!("### From `{rel}`\n{content}"));
            break;
        }
    }

    let rules_dir = root.join(".codei/rules");
    if rules_dir.is_dir() {
        let mut rule_files: Vec<_> = fs::read_dir(&rules_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        rule_files.sort_by_key(|e| e.file_name());

        for entry in rule_files {
            let path = entry.path();
            if let Some(content) = read_file_if_exists(&path) {
                let name = entry.file_name().to_string_lossy().into_owned();
                sections.push(format!("### Rule `{name}`\n{content}"));
            }
        }
    }

    sections.join("\n\n")
}

fn read_file_if_exists(path: &Path) -> Option<String> {
    if path.is_file() {
        fs::read_to_string(path).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codei_config::ResolvedConfig;
    use std::path::PathBuf;

    #[test]
    fn loads_rules_directory() {
        let dir = tempfile::tempdir().unwrap();
        let rules = dir.path().join(".codei/rules");
        fs::create_dir_all(&rules).unwrap();
        fs::write(rules.join("rust.md"), "Use idiomatic Rust.").unwrap();

        let config = ResolvedConfig {
            config: Default::default(),
            cwd: dir.path().to_path_buf(),
            project_root: Some(dir.path().to_path_buf()),
            user_config_path: PathBuf::from("/tmp/config.toml"),
            project_config_path: None,
        };

        let text = load_project_instructions(&config);
        assert!(text.contains("Use idiomatic Rust."));
    }
}
