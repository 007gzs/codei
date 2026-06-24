use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{expand_tilde, user_config_dir};
use crate::ResolvedConfig;

/// Where a skill was discovered on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    CursorUser,
    User,
    CursorProject,
    Project,
}

/// Metadata for a discovered `SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub source: SkillSource,
}

/// Discover skills from user and project directories.
///
/// Search order (later overrides earlier on name conflict):
/// `~/.cursor/skills`, `~/.config/codei/skills`, `{project}/.cursor/skills`, `{project}/.codei/skills`.
pub fn discover_skills(config: &ResolvedConfig) -> Vec<Skill> {
    let mut by_name: HashMap<String, Skill> = HashMap::new();

    scan_and_merge(
        &mut by_name,
        &expand_tilde("~/.cursor/skills"),
        SkillSource::CursorUser,
    );
    scan_and_merge(
        &mut by_name,
        &user_config_dir().join("skills"),
        SkillSource::User,
    );

    if let Some(root) = &config.project_root {
        scan_and_merge(
            &mut by_name,
            &root.join(".cursor/skills"),
            SkillSource::CursorProject,
        );
        scan_and_merge(
            &mut by_name,
            &root.join(".codei/skills"),
            SkillSource::Project,
        );
    }

    let mut skills: Vec<_> = by_name.into_values().collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Format a compact skill index for the system prompt.
pub fn format_skills_for_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Available skills".to_string(),
        "Specialized instructions live in skill files. When a user task matches a skill description, call the `read_skill` tool with the skill name before proceeding.".to_string(),
        String::new(),
    ];
    for skill in skills {
        lines.push(format!("- **{}**: {}", skill.name, skill.description));
    }
    lines.join("\n")
}

/// Find a skill by `name` (case-insensitive) or parent directory name.
pub fn find_skill<'a>(skills: &'a [Skill], query: &str) -> Option<&'a Skill> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_ascii_lowercase();
    skills
        .iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(query))
        .or_else(|| {
            skills.iter().find(|skill| {
                skill
                    .path
                    .parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|name| name.eq_ignore_ascii_case(query))
            })
        })
        .or_else(|| {
            skills
                .iter()
                .find(|skill| skill.name.to_ascii_lowercase() == query_lower)
        })
}

/// Read skill instructions (body without YAML frontmatter).
pub fn read_skill_body(skill: &Skill) -> std::io::Result<String> {
    let raw = fs::read_to_string(&skill.path)?;
    Ok(strip_frontmatter(&raw).trim().to_string())
}

fn scan_and_merge(by_name: &mut HashMap<String, Skill>, root: &Path, source: SkillSource) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        if let Some(skill) = parse_skill_file(&skill_md, source) {
            by_name.insert(skill.name.clone(), skill);
        }
    }
}

fn parse_skill_file(path: &Path, source: SkillSource) -> Option<Skill> {
    let raw = fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&raw);
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "skill".into());

    let mut name = dir_name.clone();
    let mut description = String::new();

    if let Some(meta) = frontmatter {
        for line in meta.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("name:") {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    name = value.to_string();
                }
            } else if let Some(value) = line.strip_prefix("description:") {
                description = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
            }
        }
    }

    if description.is_empty() {
        description = body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or("Specialized agent instructions.")
            .chars()
            .take(200)
            .collect();
    }

    Some(Skill {
        name,
        description,
        path: path.to_path_buf(),
        source,
    })
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        return (None, trimmed);
    }
    let rest = &trimmed[3..];
    let Some(end) = rest.find("\n---") else {
        return (None, trimmed);
    };
    let meta = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n');
    (Some(meta), body)
}

fn strip_frontmatter(content: &str) -> String {
    match split_frontmatter(content) {
        (Some(_), body) => body.to_string(),
        (None, body) => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_skill_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("pdf-helper");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: pdf-helper\ndescription: Process PDF files.\n---\n\n# PDF\nDo work.",
        )
        .unwrap();

        let config = ResolvedConfig {
            config: Default::default(),
            cwd: dir.path().to_path_buf(),
            project_root: Some(dir.path().to_path_buf()),
            user_config_path: PathBuf::from("/tmp/config.toml"),
            project_config_path: None,
        };

        fs::create_dir_all(dir.path().join(".codei/skills")).unwrap();
        fs::rename(skill_dir, dir.path().join(".codei/skills/pdf-helper")).unwrap();

        let skills = discover_skills(&config);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "pdf-helper");
        assert_eq!(skills[0].description, "Process PDF files.");
        assert_eq!(read_skill_body(&skills[0]).unwrap(), "# PDF\nDo work.");
    }

    #[test]
    fn find_skill_matches_name_case_insensitively() {
        let skill = Skill {
            name: "PDF-Helper".into(),
            description: "x".into(),
            path: PathBuf::from("/tmp/pdf-helper/SKILL.md"),
            source: SkillSource::Project,
        };
        assert!(find_skill(&[skill], "pdf-helper").is_some());
    }
}
