use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolCheckResult;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
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

/// Typed input for [`EditTool`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditInput {
    /// The absolute path to the file to modify
    pub file_path: String,
    /// The text to replace
    pub old_string: String,
    /// The replacement text (must differ from old_string)
    pub new_string: String,
    /// Replace all occurrences (default false)
    #[serde(default)]
    pub replace_all: bool,
}

/// Typed output for [`EditTool`]. Field names preserve TS camelCase
/// wire format (`FileEditTool.ts:567-568`); `userModified` is always
/// `false` in coco-rs since there's no TUI accept-with-edits overlay
/// equivalent of the TS feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EditOutput {
    #[serde(default, rename = "filePath")]
    pub file_path: String,
    #[serde(default, rename = "replaceAll")]
    pub replace_all: bool,
    #[serde(default, rename = "userModified")]
    pub user_modified: bool,
    #[serde(default, rename = "replacementCount")]
    pub replacement_count: usize,
}

/// Edit tool — performs exact string replacements in files.
/// Single replacement requires unique match; use replace_all for multiple.
pub struct EditTool;

#[async_trait::async_trait]
impl Tool for EditTool {
    type Input = EditInput;
    coco_tool_runtime::impl_runtime_schema!(EditInput);
    type Output = EditOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Edit)
    }

    fn name(&self) -> &str {
        ToolName::Edit.as_str()
    }

    fn description(&self, _input: &EditInput, _options: &DescriptionOptions) -> String {
        EDIT_TOOL_DESCRIPTION.into()
    }

    fn is_destructive(&self, _input: &EditInput) -> bool {
        true
    }

    fn get_activity_description(&self, input: &EditInput) -> Option<String> {
        Some(format!("Editing {path}", path = input.file_path))
    }

    fn get_path(&self, input: &EditInput) -> Option<String> {
        Some(input.file_path.clone())
    }

    fn validate_input(&self, input: &EditInput, _ctx: &ToolUseContext) -> ValidationResult {
        if input.file_path.is_empty() {
            return ValidationResult::invalid("missing required field: file_path");
        }
        // `old_string` and `new_string` are required at the struct
        // level — schema-required, parse-required. The semantic check
        // is that they must differ; empty values are otherwise
        // accepted (insert-at-empty-string is a legit use case).
        if input.old_string == input.new_string {
            return ValidationResult::invalid("old_string and new_string must be different");
        }
        ValidationResult::Valid
    }

    async fn check_permissions(&self, input: &EditInput, ctx: &ToolUseContext) -> ToolCheckResult {
        crate::tools::write_permissions::check_write_permission_for_path(
            &input.file_path,
            ctx,
            ToolName::Edit.as_str(),
            "edit a file",
        )
    }

    /// Branch on `replace_all` to emit the TS-shaped confirmation. TS
    /// parity: `FileEditTool.ts:575-595::mapToolResultToToolResultBlockParam`.
    /// `user_modified` is always false in coco-rs (no TUI accept-with-edits
    /// overlay) — the corresponding modifiedNote branch never fires.
    fn render_for_model(&self, out: &EditOutput) -> Vec<ToolResultContentPart> {
        let file_path = out.file_path.as_str();
        let text = if out.replace_all {
            format!(
                "The file {file_path} has been updated. All occurrences were successfully replaced."
            )
        } else {
            format!("The file {file_path} has been updated successfully.")
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: EditInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<EditOutput>, ToolError> {
        let file_path = input.file_path.as_str();
        let old_string = input.old_string.as_str();
        let new_string = input.new_string.as_str();
        let replace_all = input.replace_all;

        let path = Path::new(file_path);

        // Sandbox pre-flight — deny inadmissible writes before any I/O,
        // so SDK consumers can intercept via the approval bridge.
        super::sandbox_preflight::preflight_path(ctx, path, /*write=*/ true)?;

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

        // Sandboxed write fence (memory-extraction / auto-dream
        // subagents). No-op when `ctx.allowed_write_roots` is empty.
        if let Some(err) = crate::check_write_root_fence(ctx, path) {
            return Err(ToolError::ExecutionFailed {
                message: err,
                source: None,
            });
        }

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

        crate::record_file_edit(ctx, path, new_content).await;
        // TS `FileEditTool.ts` mirrors `FileReadTool.ts:578-591` skill
        // auto-discovery + conditional-skill activation — when an
        // edit touches a path under a nested `.claude/skills/`
        // ancestor or matches a `paths`-gated skill, the next batch
        // boundary picks both up.
        crate::track_skill_triggers(ctx, path).await;
        // TS `FileEditTool.ts` notifies the LSP server of the save so
        // diagnostics refresh after every edit. Best-effort.
        ctx.lsp.notify_save(path).await;

        // TS `FileEditTool.ts:567-568` returns `{filePath, replaceAll, userModified}`
        // and render_for_model branches on those flags. coco-rs doesn't
        // currently track `userModified` (that's a TUI overlay state for
        // a feature we don't have); always emit it as false.
        Ok(ToolResult {
            data: EditOutput {
                file_path: file_path.to_string(),
                replace_all,
                user_modified: false,
                replacement_count: count,
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
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
