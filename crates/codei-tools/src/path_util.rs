use std::path::{Component, Path, PathBuf};

use crate::ToolError;

pub fn resolve_workspace_path(cwd: &Path, raw: &str) -> Result<PathBuf, ToolError> {
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(ToolError::PathNotAllowed(raw.to_string()));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(ToolError::PathNotAllowed(raw.to_string()));
        }
    }
    let joined = cwd.join(path);
    let canonical = joined.canonicalize().unwrap_or(joined);
    let cwd_canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if !canonical.starts_with(&cwd_canonical) {
        return Err(ToolError::PathNotAllowed(raw.to_string()));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_dir() {
        let cwd = std::env::current_dir().unwrap();
        assert!(resolve_workspace_path(&cwd, "../etc/passwd").is_err());
    }
}
