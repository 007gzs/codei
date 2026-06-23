use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::io;

use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct DefinitionTool;

#[async_trait]
impl Tool for DefinitionTool {
    fn name(&self) -> &str {
        "definition"
    }

    fn description(&self) -> &str {
        "Find where a symbol is defined (function, struct, class, etc.) in the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol": { "type": "string", "description": "Symbol name to locate" },
                "path": { "type": "string", "description": "Optional file path hint" }
            },
            "required": ["symbol"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let symbol = args
            .get("symbol")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing symbol".into()))?;
        let path_hint = args.get("path").and_then(|v| v.as_str());

        let patterns = [
            format!(r"\bfn\s+{symbol}\b"),
            format!(r"\bstruct\s+{symbol}\b"),
            format!(r"\benum\s+{symbol}\b"),
            format!(r"\btrait\s+{symbol}\b"),
            format!(r"\btype\s+{symbol}\b"),
            format!(r"\bclass\s+{symbol}\b"),
            format!(r"\bfunction\s+{symbol}\b"),
            format!(r"\bdef\s+{symbol}\b"),
            format!(r"\bconst\s+{symbol}\b"),
            format!(r"\blet\s+{symbol}\b"),
        ];

        let mut hits = Vec::new();
        const MAX_HITS: usize = 20;

        'outer: for pattern in patterns {
            let matcher = match RegexMatcher::new(&pattern) {
                Ok(m) => m,
                Err(_) => continue,
            };
            for entry in WalkBuilder::new(&ctx.cwd)
                .hidden(false)
                .git_ignore(true)
                .build()
            {
                if hits.len() >= MAX_HITS {
                    break 'outer;
                }
                let entry = entry.map_err(|e| ToolError::Io(io::Error::other(e)))?;
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let path = entry.path();
                let rel = path
                    .strip_prefix(&ctx.cwd)
                    .map_err(|_| ToolError::PathNotAllowed(path.display().to_string()))?;
                let rel_str = rel.to_string_lossy();
                if let Some(hint) = path_hint {
                    if !rel_str.contains(hint) {
                        continue;
                    }
                }

                let mut searcher = SearcherBuilder::new().line_number(true).build();
                let sink = DefSink {
                    rel: rel_str.to_string(),
                    hits: &mut hits,
                    max: MAX_HITS,
                };
                let _ = searcher.search_path(&matcher, path, sink);
            }
        }

        Ok(ToolResult {
            content: if hits.is_empty() {
                format!("No definition found for `{symbol}`.")
            } else {
                hits.join("\n")
            },
            is_error: false,
        })
    }
}

struct DefSink<'a> {
    rel: String,
    hits: &'a mut Vec<String>,
    max: usize,
}

impl grep_searcher::Sink for DefSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        if self.hits.len() >= self.max {
            return Ok(false);
        }
        let line = String::from_utf8_lossy(mat.bytes());
        self.hits.push(format!(
            "{}:{}:{}",
            self.rel,
            mat.line_number().unwrap_or(0),
            line.trim_end()
        ));
        Ok(true)
    }
}
