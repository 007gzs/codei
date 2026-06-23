use async_trait::async_trait;
use codei_config::GrepToolConfig;
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::io;

use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct GrepTool {
    config: GrepToolConfig,
}

impl GrepTool {
    pub fn new(config: &GrepToolConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regular expression under the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern" },
                "glob": { "type": "string", "description": "Optional file glob filter" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing pattern".into()))?;
        let file_glob = args.get("glob").and_then(|v| v.as_str());

        let matcher = RegexMatcher::new(pattern)
            .map_err(|e| ToolError::InvalidArgs(format!("invalid regex: {e}")))?;

        let mut output = String::new();
        let mut match_count = 0usize;
        let mut files_scanned = 0usize;
        let max_matches = self.config.max_matches;
        let max_files = self.config.max_files;

        for entry in WalkBuilder::new(&ctx.cwd)
            .hidden(false)
            .git_ignore(true)
            .build()
        {
            if files_scanned >= max_files {
                output.push_str(&format!("\n(stopped after scanning {max_files} files)\n"));
                break;
            }

            let entry = entry.map_err(|e| ToolError::Io(io::Error::other(e)))?;
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            files_scanned += 1;
            if files_scanned.is_multiple_of(100) {
                tokio::task::yield_now().await;
            }

            let path = entry.path();
            let rel = path
                .strip_prefix(&ctx.cwd)
                .map_err(|_| ToolError::PathNotAllowed(path.display().to_string()))?;
            let rel_str = rel.to_string_lossy();
            if let Some(glob_pat) = file_glob {
                let glob = glob::Pattern::new(glob_pat)
                    .map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
                if !glob.matches(&rel_str) {
                    continue;
                }
            }

            let mut searcher = SearcherBuilder::new().line_number(true).build();
            let sink = MatchSink {
                rel: rel_str.to_string(),
                output: &mut output,
                count: &mut match_count,
                max: max_matches,
            };
            let _ = searcher.search_path(&matcher, path, sink);
            if match_count >= max_matches {
                output.push_str("\n(truncated at max matches)\n");
                break;
            }
        }

        Ok(ToolResult {
            content: if output.is_empty() {
                "No matches found.".into()
            } else {
                output
            },
            is_error: false,
        })
    }
}

struct MatchSink<'a> {
    rel: String,
    output: &'a mut String,
    count: &'a mut usize,
    max: usize,
}

impl grep_searcher::Sink for MatchSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        if *self.count >= self.max {
            return Ok(false);
        }
        let line = String::from_utf8_lossy(mat.bytes());
        self.output.push_str(&format!(
            "{}:{}:{}\n",
            self.rel,
            mat.line_number().unwrap_or(0),
            line.trim_end()
        ));
        *self.count += 1;
        Ok(true)
    }
}
