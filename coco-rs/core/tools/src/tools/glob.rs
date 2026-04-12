use coco_tool::DescriptionOptions;
use coco_tool::SearchReadInfo;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Default maximum glob results.
const DEFAULT_MAX_RESULTS: usize = 500;

/// Glob tool — fast file pattern matching.
/// Returns matching file paths sorted by modification time (most recent first).
pub struct GlobTool;

#[async_trait::async_trait]
impl Tool for GlobTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Glob)
    }

    fn name(&self) -> &str {
        ToolName::Glob.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        "Fast file pattern matching tool that works with any codebase size.".into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "pattern".into(),
            serde_json::json!({
                "type": "string",
                "description": "The glob pattern to match files against"
            }),
        );
        props.insert(
            "path".into(),
            serde_json::json!({
                "type": "string",
                "description": "The directory to search in. Defaults to current working directory."
            }),
        );
        ToolInputSchema { properties: props }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let pattern = input.get("pattern").and_then(|v| v.as_str())?;
        Some(format!("Searching for {pattern}"))
    }

    fn is_search_or_read_command(&self, _input: &Value) -> Option<SearchReadInfo> {
        Some(SearchReadInfo {
            is_search: true,
            ..SearchReadInfo::default()
        })
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("pattern").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: pattern");
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing pattern".into(),
                error_code: None,
            })?;

        let search_dir = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let search_path = Path::new(search_dir);
        if !search_path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("search path does not exist: {search_dir}"),
                source: None,
            });
        }

        // Build the glob matcher
        let glob = globset::GlobBuilder::new(pattern)
            .literal_separator(false)
            .build()
            .map_err(|e| ToolError::InvalidInput {
                message: format!("invalid glob pattern: {e}"),
                error_code: None,
            })?
            .compile_matcher();

        // Walk directory tree respecting .gitignore
        let walker = ignore::WalkBuilder::new(search_path)
            .hidden(false) // show hidden files if pattern asks for them
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        let mut matches: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();

        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Match against the relative path from search_dir
            let rel_path = path.strip_prefix(search_path).unwrap_or(path);

            if glob.is_match(rel_path) {
                let mtime = path
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                matches.push((path.to_path_buf(), mtime));
            }
        }

        // Sort by mtime descending (most recent first)
        matches.sort_by(|a, b| b.1.cmp(&a.1));

        // Truncate to max results
        let max_results = DEFAULT_MAX_RESULTS;
        let total = matches.len();
        let truncated = total > max_results;
        if truncated {
            matches.truncate(max_results);
        }

        let paths: Vec<String> = matches
            .into_iter()
            .map(|(p, _)| p.to_string_lossy().to_string())
            .collect();

        let mut output = paths.join("\n");
        if truncated {
            output.push_str(&format!(
                "\n\n... ({} more files not shown, {total} total matches)",
                total - max_results
            ));
        }

        if output.is_empty() {
            output = "No files matched the pattern.".to_string();
        }

        Ok(ToolResult {
            data: serde_json::json!(output),
            new_messages: vec![],
        })
    }
}

#[cfg(test)]
#[path = "glob.test.rs"]
mod tests;
