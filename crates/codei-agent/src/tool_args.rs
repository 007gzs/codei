use serde_json::{json, Value};
use tracing::warn;

const FILE_EXTENSIONS: &[&str] = &[
    "md", "rs", "toml", "json", "txt", "yml", "yaml", "js", "ts", "tsx", "py", "go", "java", "c",
    "cpp", "h", "css", "html", "sh", "xml", "sql",
];

/// Some local models (e.g. Qwen via vLLM) emit `read` tool calls with `{}` while naming the
/// file in assistant text. Recover the path when possible.
pub fn repair_tool_args(tool_name: &str, assistant_content: &str, args: Value) -> Value {
    match tool_name {
        "read" if args.get("path").and_then(|v| v.as_str()).is_none() => {
            if let Some(path) = extract_file_path_from_text(assistant_content) {
                warn!(
                    path = %path,
                    "inferred read path from assistant text (model sent empty tool arguments)"
                );
                return json!({ "path": path });
            }
        }
        "write" | "edit" if args.get("path").and_then(|v| v.as_str()).is_none() => {
            if let Some(path) = extract_file_path_from_text(assistant_content) {
                warn!(path = %path, tool = tool_name, "inferred path from assistant text");
                return json!({ "path": path });
            }
        }
        _ => {}
    }
    args
}

fn extract_file_path_from_text(text: &str) -> Option<String> {
    if let Some(path) = extract_backtick_path(text) {
        return Some(path);
    }

    for ext in FILE_EXTENSIONS {
        let needle = format!(".{ext}");
        let mut search_from = 0;
        while let Some(rel) = text[search_from..].find(&needle) {
            let dot = search_from + rel;
            let end = dot + needle.len();
            let start = text[..dot]
                .rfind(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'')
                .map(|i| i + 1)
                .unwrap_or(0);
            let raw = &text[start..end];
            let path = trim_path_token(raw);
            if is_plausible_file_path(path) {
                return Some(path.to_string());
            }
            search_from = end;
        }
    }

    None
}

fn extract_backtick_path(text: &str) -> Option<String> {
    let start = text.find('`')?;
    let rest = &text[start + 1..];
    let end = rest.find('`')?;
    let path = trim_path_token(&rest[..end]);
    if is_plausible_file_path(path) {
        Some(path.to_string())
    } else {
        None
    }
}

fn trim_path_token(raw: &str) -> &str {
    raw.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-'
    })
}

fn is_plausible_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.contains('.')
        && !path.starts_with("http")
        && !path.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_readme_from_chinese_sentence() {
        let text = "让我查看 README.md 文件来了解项目的基本信息：";
        let args = repair_tool_args("read", text, json!({}));
        assert_eq!(args["path"], "README.md");
    }

    #[test]
    fn extracts_backtick_path() {
        let text = "Open `server/main.go` next";
        let args = repair_tool_args("read", text, json!({}));
        assert_eq!(args["path"], "server/main.go");
    }

    #[test]
    fn leaves_args_unchanged_when_no_hint() {
        let args = repair_tool_args("read", "no file here", json!({}));
        assert!(args.get("path").is_none());
    }
}
