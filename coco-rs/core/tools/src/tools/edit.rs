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

/// Long-form tool description shown to the model.
///
/// TS: `tools/FileEditTool/prompt.ts:8-28` `getEditToolDescription()`
/// → `getDefaultEditDescription()`. Byte-identical port. The line
/// number prefix format reference defaults to "line number + tab"
/// (TS `isCompactLinePrefixEnabled() = true`), which matches the
/// `cat -n` style produced by the Rust Read tool.
///
/// The `minimalUniquenessHint` (TS-only USER_TYPE='ant' branch) is
/// omitted — coco-rs has no equivalent of TS's per-user feature flag.
const EDIT_TOOL_DESCRIPTION: &str = "Performs exact string replacements in files.

Usage:
- You must use your `Read` tool at least once in the conversation before editing. This tool will error if you attempt an edit without reading the file.
- When editing text from Read tool output, ensure you preserve the exact indentation (tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: line number + tab. Everything after that is the actual file content to match. Never include any part of the line number prefix in the old_string or new_string.
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless explicitly required.
- Only use emojis if the user explicitly requests it. Avoid adding emojis to files unless asked.
- The edit will FAIL if `old_string` is not unique in the file. Either provide a larger string with more surrounding context to make it unique or use `replace_all` to change every instance of `old_string`.
- Use `replace_all` for replacing and renaming strings across the file. This parameter is useful if you want to rename a variable for instance.";

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
        EDIT_TOOL_DESCRIPTION.into()
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

        // Reject edits when the file changed externally since the cached
        // Read. TS `FileEditTool.ts` — two-layer check:
        //   Layer 1: compare stored mtime to current disk mtime.
        //   Layer 2: for full-view reads, if mtime matches, compare stored
        //            content to current disk content as a fallback (mtime
        //            precision is only 1s on some filesystems, so a same-
        //            second overwrite would otherwise slip past).
        // Partial-read cache entries skip Layer 2 because they can't be
        // compared to the whole file. Layer 2 is also bypassed for files
        // over 1 MiB to keep the Edit hot path off O(filesize) I/O.
        const LAYER2_MAX_BYTES: u64 = 1024 * 1024;
        if let Some(frs) = &ctx.file_read_state
            && let Ok(abs_path) = tokio::fs::canonicalize(path).await
        {
            let cached = {
                let frs_read = frs.read().await;
                frs_read.peek(&abs_path).cloned()
            };
            if let Some(entry) = cached {
                if let Ok(disk_mtime) = coco_context::file_mtime_ms(&abs_path).await
                    && entry.mtime_ms != disk_mtime
                {
                    return Err(ToolError::ExecutionFailed {
                        message: format!(
                            "{file_path} has been modified since it was last read \
                             (mtime changed). Read it again before editing."
                        ),
                        source: None,
                    });
                }

                if entry.offset.is_none()
                    && entry.limit.is_none()
                    && let Ok(meta) = tokio::fs::metadata(&abs_path).await
                    && meta.len() <= LAYER2_MAX_BYTES
                    && let Ok(raw) = tokio::fs::read(&abs_path).await
                {
                    let enc = coco_file_encoding::detect_encoding(&raw);
                    if let Ok(current) = enc.decode(&raw)
                        && current != entry.content
                    {
                        return Err(ToolError::ExecutionFailed {
                            message: format!(
                                "{file_path} has been modified since it was last read \
                                 (content changed). Read it again before editing."
                            ),
                            source: None,
                        });
                    }
                }
            }
        }

        // Team-memory secret guard. TS `FileEditTool.ts` reuses the
        // same `checkTeamMemSecrets` invariant as FileWriteTool — an
        // edit that introduces a secret to a synced team-memory path
        // is rejected before the new content hits disk. We check the
        // INTENT (the post-replacement content) below after computing
        // `new_content`, since the secret might come from the
        // replacement string rather than the existing file.

        // Track file edit for checkpoint/rewind before modifying.
        // TS: FileEditTool.ts line 435
        crate::track_file_edit(ctx, path).await;

        let content =
            std::fs::read_to_string(file_path).map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to read {file_path}: {e}"),
                source: None,
            })?;

        // T2: Input normalization BEFORE matching.
        //
        // TS `FileEditTool/utils.ts:581-657` `normalizeFileEditInput`
        // runs two transformations on the (old_string, new_string) pair
        // before the Edit tool's matching logic even sees them:
        //
        //   1. Strip trailing whitespace from new_string (unless the
        //      file is Markdown, which uses trailing spaces as hard
        //      line breaks).
        //   2. Desanitize over-escaped / over-sanitized model output
        //      (e.g. `<fnr>` → `<function_results>`) — see
        //      `desanitization_map` in edit_utils.rs.
        //
        // Normalize (old_string, new_string) before matching — TS
        // `FileEditTool/utils.ts` runs two transforms first:
        //   1. Strip trailing whitespace from new_string (except on
        //      Markdown, where trailing spaces are hard line breaks).
        //   2. Desanitize over-escaped model output (e.g. `<fnr>` →
        //      `<function_results>`) via `desanitization_map` in
        //      edit_utils.rs.
        // Without these, edits from models that over-escape JSON would
        // fail literal matching here where TS succeeds.
        let (normalized_old, normalized_new) = crate::tools::edit_utils::normalize_file_edit_input(
            file_path, &content, old_string, new_string,
        );
        let old_string = normalized_old.as_str();
        let new_string = normalized_new.as_str();

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
            // Matching fallback order (TS-aligned + coco-rs extension):
            //   1. Quote-normalized match — TS `findActualString()` at
            //      `FileEditTool/utils.ts:73-93`. Handles the common case
            //      where the file uses curly quotes (""'') but the model
            //      emitted straight quotes ("'). When the match hits via
            //      quote normalization, `preserve_quote_style` re-applies
            //      the file's curly style to `new_string` so the round-trip
            //      doesn't silently downgrade.
            //   2. Whitespace-normalized match — coco-rs extension from
            //      cocode-rs. Not in TS but useful for Python/YAML where
            //      the model may emit slightly different indentation.
            //      Preserved because it's backwards-compatible with
            //      existing tests.
            if let Some(actual) = crate::tools::edit_utils::find_actual_string(&content, old_string)
            {
                let preserved_new =
                    crate::tools::edit_utils::preserve_quote_style(old_string, actual, new_string);
                content.replacen(actual, &preserved_new, 1)
            } else if let Some(actual) = find_fuzzy_match(&content, old_string) {
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

        // Team-memory secret guard — check the POST-EDIT content (which
        // may contain a secret introduced via `new_string`) against the
        // path's team-memory eligibility. See `check_team_mem_secret`
        // in lib.rs for the layered detection logic.
        if let Some(err) = crate::check_team_mem_secret(ctx, path, &new_content) {
            return Err(ToolError::ExecutionFailed {
                message: err,
                source: None,
            });
        }

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
        // TS `FileEditTool.ts` mirrors `FileReadTool.ts:578-591` skill
        // auto-discovery — when an edit touches a path under a nested
        // `.claude/skills/` ancestor, the manager picks up the change
        // on the next batch boundary.
        crate::track_skill_discovery(ctx, path).await;

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
