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

use crate::input_types::GrepOutputMode;

/// Default head_limit — max number of output entries.
const DEFAULT_HEAD_LIMIT: usize = 250;

/// Grep tool — content search using regex, walking directories with .gitignore.
pub struct GrepTool;

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Grep)
    }

    fn name(&self) -> &str {
        ToolName::Grep.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        "A powerful search tool built on ripgrep.".into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "pattern".into(),
            serde_json::json!({
                "type": "string",
                "description": "The regex pattern to search for"
            }),
        );
        props.insert(
            "path".into(),
            serde_json::json!({
                "type": "string",
                "description": "File or directory to search in. Defaults to cwd."
            }),
        );
        props.insert(
            "output_mode".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["content", "files_with_matches", "count"],
                "default": "files_with_matches"
            }),
        );
        props.insert(
            "glob".into(),
            serde_json::json!({
                "type": "string",
                "description": "Glob pattern to filter files (e.g. \"*.rs\")"
            }),
        );
        props.insert(
            "head_limit".into(),
            serde_json::json!({
                "type": "number",
                "description": "Limit output to first N entries. Default 250."
            }),
        );
        props.insert(
            "-i".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Case insensitive search"
            }),
        );
        props.insert(
            "-n".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Show line numbers (default true for content mode)"
            }),
        );
        props.insert(
            "-C".into(),
            serde_json::json!({
                "type": "number",
                "description": "Lines of context before and after each match"
            }),
        );
        props.insert(
            "-A".into(),
            serde_json::json!({
                "type": "number",
                "description": "Lines of context after each match"
            }),
        );
        props.insert(
            "-B".into(),
            serde_json::json!({
                "type": "number",
                "description": "Lines of context before each match"
            }),
        );
        props.insert(
            "context".into(),
            serde_json::json!({
                "type": "number",
                "description": "Number of lines to show before and after each match (rg -C). Alias for -C."
            }),
        );
        props.insert(
            "type".into(),
            serde_json::json!({
                "type": "string",
                "description": "File type to search (rg --type). Common types: js, py, rust, go, java, etc."
            }),
        );
        props.insert(
            "offset".into(),
            serde_json::json!({
                "type": "number",
                "description": "Skip first N entries before applying head_limit. Defaults to 0."
            }),
        );
        props.insert(
            "multiline".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Enable multiline mode where . matches newlines and patterns can span lines (rg -U --multiline-dotall). Default: false."
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

        let search_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let output_mode = input
            .get("output_mode")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "content" => Some(GrepOutputMode::Content),
                "files_with_matches" => Some(GrepOutputMode::FilesWithMatches),
                "count" => Some(GrepOutputMode::Count),
                _ => None,
            })
            .unwrap_or(GrepOutputMode::FilesWithMatches);

        let case_insensitive = input
            .get("-i")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let head_limit = input
            .get("head_limit")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_HEAD_LIMIT);

        let context_lines = input
            .get("-C")
            .or_else(|| input.get("context"))
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize);
        let after_context = input
            .get("-A")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize);
        let before_context = input
            .get("-B")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize);

        let glob_filter = input.get("glob").and_then(|v| v.as_str());
        let type_filter = input.get("type").and_then(|v| v.as_str());
        let offset = input
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let multiline = input
            .get("multiline")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let path = Path::new(search_path);
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("search path does not exist: {search_path}"),
                source: None,
            });
        }

        // Build regex — multiline mode wraps with (?s) for dotall
        let regex_pattern = {
            let mut pat = String::new();
            if case_insensitive {
                pat.push_str("(?i)");
            }
            if multiline {
                pat.push_str("(?s)");
            }
            pat.push_str(pattern);
            pat
        };
        let re = regex::Regex::new(&regex_pattern).map_err(|e| ToolError::InvalidInput {
            message: format!("invalid regex pattern: {e}"),
            error_code: None,
        })?;

        // Collect results
        let mut output_lines: Vec<String> = Vec::new();
        let mut match_count: usize = 0;

        if path.is_file() {
            // Search single file
            search_file(
                path,
                &re,
                output_mode,
                context_lines,
                before_context,
                after_context,
                &mut output_lines,
                &mut match_count,
                head_limit,
            );
        } else {
            // Walk directory
            let mut walker_builder = ignore::WalkBuilder::new(path);
            walker_builder
                .hidden(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true);

            // Apply type filter (rg --type) or glob filter
            if let Some(file_type) = type_filter {
                let mut types_builder = ignore::types::TypesBuilder::new();
                types_builder.add_defaults();
                types_builder.select(file_type);
                if let Ok(types) = types_builder.build() {
                    walker_builder.types(types);
                }
            } else if let Some(glob_pat) = glob_filter {
                let mut types_builder = ignore::types::TypesBuilder::new();
                types_builder.add("custom", glob_pat).ok();
                types_builder.select("custom");
                if let Ok(types) = types_builder.build() {
                    walker_builder.types(types);
                }
            }

            let walker = walker_builder.build();

            for entry in walker.flatten() {
                if !entry.path().is_file() {
                    continue;
                }
                if output_lines.len() >= head_limit {
                    break;
                }
                search_file(
                    entry.path(),
                    &re,
                    output_mode,
                    context_lines,
                    before_context,
                    after_context,
                    &mut output_lines,
                    &mut match_count,
                    head_limit,
                );
            }
        }

        // Apply offset: skip first N entries
        let after_offset: Vec<String> = output_lines.into_iter().skip(offset).collect();
        let truncated = after_offset.len() > head_limit;
        let display_lines: Vec<String> = after_offset.into_iter().take(head_limit).collect();

        let result_text = if display_lines.is_empty() {
            "No matches found.".to_string()
        } else {
            let mut text = display_lines.join("\n");
            if truncated {
                text.push_str(&format!(
                    "\n\n... (results truncated at {head_limit} entries)"
                ));
            }
            text
        };

        Ok(ToolResult {
            data: serde_json::json!(result_text),
            new_messages: vec![],
        })
    }
}

/// Search a single file for regex matches and append to output.
fn search_file(
    path: &Path,
    re: &regex::Regex,
    mode: GrepOutputMode,
    context: Option<usize>,
    before: Option<usize>,
    after: Option<usize>,
    output: &mut Vec<String>,
    match_count: &mut usize,
    head_limit: usize,
) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return; // skip unreadable files
    };

    let lines: Vec<&str> = content.lines().collect();
    let path_str = path.to_string_lossy();

    match mode {
        GrepOutputMode::FilesWithMatches => {
            if re.is_match(&content) {
                output.push(path_str.to_string());
                *match_count += 1;
            }
        }
        GrepOutputMode::Count => {
            let count = lines.iter().filter(|line| re.is_match(line)).count();
            if count > 0 {
                output.push(format!("{path_str}:{count}"));
                *match_count += count;
            }
        }
        GrepOutputMode::Content => {
            let before_ctx = before.or(context).unwrap_or(0);
            let after_ctx = after.or(context).unwrap_or(0);

            let mut last_printed: Option<usize> = None;

            for (i, line) in lines.iter().enumerate() {
                if output.len() >= head_limit {
                    return;
                }
                if re.is_match(line) {
                    let start = i.saturating_sub(before_ctx);
                    let end = (i + after_ctx + 1).min(lines.len());

                    // Print separator if there's a gap
                    if let Some(last) = last_printed
                        && start > last + 1
                    {
                        output.push("--".to_string());
                    }

                    for j in start..end {
                        if last_printed.is_some_and(|lp| j <= lp) {
                            continue; // already printed
                        }
                        let line_num = j + 1;
                        let sep = if j == i { ":" } else { "-" };
                        output.push(format!("{path_str}{sep}{line_num}{sep}{}", lines[j]));
                        last_printed = Some(j);
                    }

                    *match_count += 1;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "grep.test.rs"]
mod tests;
