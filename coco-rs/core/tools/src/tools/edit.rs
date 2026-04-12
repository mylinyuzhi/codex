use coco_tool::DescriptionOptions;
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

/// Edit tool — performs exact string replacements in files.
/// Single replacement requires unique match; use replace_all for multiple.
pub struct EditTool;

#[async_trait::async_trait]
impl Tool for EditTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Edit)
    }

    fn name(&self) -> &str {
        ToolName::Edit.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        "Performs exact string replacements in files.".into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "file_path".into(),
            serde_json::json!({
                "type": "string",
                "description": "The absolute path to the file to modify"
            }),
        );
        props.insert(
            "old_string".into(),
            serde_json::json!({
                "type": "string",
                "description": "The text to replace"
            }),
        );
        props.insert(
            "new_string".into(),
            serde_json::json!({
                "type": "string",
                "description": "The replacement text (must differ from old_string)"
            }),
        );
        props.insert(
            "replace_all".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Replace all occurrences (default false)",
                "default": false
            }),
        );
        ToolInputSchema { properties: props }
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let path = input.get("file_path").and_then(|v| v.as_str())?;
        Some(format!("Editing {path}"))
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("file_path").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: file_path");
        }
        if input.get("old_string").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: old_string");
        }
        if input.get("new_string").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: new_string");
        }
        let old = input["old_string"].as_str().unwrap_or("");
        let new = input["new_string"].as_str().unwrap_or("");
        if old == new {
            return ValidationResult::invalid("old_string and new_string must be different");
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing file_path".into(),
                error_code: None,
            })?;
        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing old_string".into(),
                error_code: None,
            })?;
        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing new_string".into(),
                error_code: None,
            })?;
        let replace_all = input
            .get("replace_all")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let path = Path::new(file_path);
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("File not found: {file_path}"),
                source: None,
            });
        }

        // Check if file was modified externally since last read.
        // TS: FileEditTool.ts line 451-468 — rejects edit if mtime diverged.
        if let Some(frs) = &ctx.file_read_state {
            if let Ok(abs_path) = std::fs::canonicalize(path) {
                let frs_read = frs.read().await;
                if let Some(entry) = frs_read.peek(&abs_path) {
                    if let Ok(disk_mtime) = coco_context::file_mtime_ms(&abs_path).await {
                        if entry.mtime_ms != disk_mtime {
                            return Err(ToolError::ExecutionFailed {
                                message: format!(
                                    "{file_path} has been modified since it was last read. \
                                     Read it again before editing."
                                ),
                                source: None,
                            });
                        }
                    }
                }
            }
        }

        // Track file edit for checkpoint/rewind before modifying.
        // TS: FileEditTool.ts line 435
        crate::track_file_edit(ctx, path).await;

        let content =
            std::fs::read_to_string(file_path).map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to read {file_path}: {e}"),
                source: None,
            })?;

        let count = content.matches(old_string).count();

        let new_content = if replace_all {
            if count == 0 {
                return Err(ToolError::InvalidInput {
                    message: format!("old_string not found in {file_path}"),
                    error_code: Some("1".into()),
                });
            }
            content.replace(old_string, new_string)
        } else if count == 0 {
            // Try fuzzy matching: whitespace-normalized comparison
            // TS: findActualString() — tries normalized whitespace, then leading indent
            if let Some(actual) = find_fuzzy_match(&content, old_string) {
                content.replacen(&actual, new_string, 1)
            } else {
                return Err(ToolError::InvalidInput {
                    message: format!("old_string not found in {file_path}"),
                    error_code: Some("1".into()),
                });
            }
        } else if count > 1 {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "old_string found {count} times in {file_path}. Use replace_all or provide more context to make it unique."
                ),
                error_code: Some("3".into()),
            });
        } else {
            content.replacen(old_string, new_string, 1)
        };

        std::fs::write(file_path, &new_content).map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to write {file_path}: {e}"),
            source: None,
        })?;

        let replacements = if replace_all {
            format!("{count} replacement(s)")
        } else {
            "1 replacement".to_string()
        };
        let result_msg =
            format!("The file {file_path} has been updated successfully. ({replacements})");

        crate::record_file_edit(ctx, path, new_content).await;

        Ok(ToolResult {
            data: serde_json::json!(result_msg),
            new_messages: vec![],
        })
    }
}

/// Try to find a fuzzy match for old_string in content.
///
/// TS: findActualString() — tries whitespace-normalized matching.
/// Strategy:
/// 1. Normalize both strings (collapse whitespace) and search
/// 2. Try trimming leading/trailing whitespace from each line
fn find_fuzzy_match(content: &str, old_string: &str) -> Option<String> {
    // Strategy 1: Normalize whitespace
    let normalized_old = normalize_whitespace(old_string);
    let normalized_content = normalize_whitespace(content);

    if let Some(pos) = normalized_content.find(&normalized_old) {
        // Find the original substring that corresponds to this position
        return find_original_at_normalized_pos(content, pos, normalized_old.len());
    }

    // Strategy 2: Try line-by-line trimmed matching
    let old_lines: Vec<&str> = old_string.lines().collect();
    if old_lines.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let first_trimmed = old_lines[0].trim();

    for (i, line) in content_lines.iter().enumerate() {
        if line.trim() == first_trimmed && i + old_lines.len() <= content_lines.len() {
            let all_match = old_lines
                .iter()
                .enumerate()
                .all(|(j, old_line)| content_lines[i + j].trim() == old_line.trim());
            if all_match {
                let matched: Vec<&str> = content_lines[i..i + old_lines.len()].to_vec();
                return Some(matched.join("\n"));
            }
        }
    }

    None
}

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_original_at_normalized_pos(
    original: &str,
    norm_pos: usize,
    norm_len: usize,
) -> Option<String> {
    // Map normalized position back to original string
    let mut orig_idx = 0;
    let mut norm_idx = 0;
    let bytes = original.as_bytes();
    let mut start_orig = None;

    while orig_idx < bytes.len() && norm_idx < norm_pos + norm_len {
        if bytes[orig_idx].is_ascii_whitespace() {
            // Skip whitespace in original but count one space in normalized
            while orig_idx < bytes.len() && bytes[orig_idx].is_ascii_whitespace() {
                orig_idx += 1;
            }
            if norm_idx >= norm_pos && start_orig.is_some() {
                norm_idx += 1; // the normalized space
            } else {
                norm_idx += 1;
            }
        } else {
            if norm_idx == norm_pos && start_orig.is_none() {
                start_orig = Some(orig_idx);
            }
            orig_idx += 1;
            norm_idx += 1;
        }
    }

    let start = start_orig?;
    // Include trailing whitespace from original
    Some(original[start..orig_idx].to_string())
}

#[cfg(test)]
#[path = "edit.test.rs"]
mod tests;
