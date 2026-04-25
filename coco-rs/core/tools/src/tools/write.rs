use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Long-form tool description shown to the model.
///
/// TS: `tools/FileWriteTool/prompt.ts:11-18` `getWriteToolDescription()`.
/// Byte-identical port — Rust Tool trait passes this string as the
/// tool description in the model's tool spec, so the model receives
/// the same usage guidance as TS Claude Code (read-before-write,
/// edit-tool preference, no documentation-by-default, no emojis).
const WRITE_TOOL_DESCRIPTION: &str = "Writes a file to the local filesystem.

Usage:
- This tool will overwrite the existing file if there is one at the provided path.
- If this is an existing file, you MUST use the `Read` tool first to read the file's contents. This tool will fail if you did not read the file first.
- Prefer the Edit tool for modifying existing files — it only sends the diff. Only use this tool to create new files or for complete rewrites.
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User.
- Only use emojis if the user explicitly requests it. Avoid writing emojis to files unless asked.";

/// Write tool — creates or overwrites a file.
/// Creates parent directories as needed.
pub struct WriteTool;

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Write)
    }

    fn name(&self) -> &str {
        ToolName::Write.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        WRITE_TOOL_DESCRIPTION.into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "file_path".into(),
            serde_json::json!({
                "type": "string",
                "description": "The absolute path to the file to write"
            }),
        );
        props.insert(
            "content".into(),
            serde_json::json!({
                "type": "string",
                "description": "The content to write to the file"
            }),
        );
        ToolInputSchema { properties: props }
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let path = input.get("file_path").and_then(|v| v.as_str())?;
        Some(format!("Writing {path}"))
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
        if input.get("content").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: content");
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
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing content".into(),
                error_code: None,
            })?;

        let path = Path::new(file_path);
        let is_new = !path.exists();

        // Read-before-write enforcement + race detection.
        //
        // TS `FileWriteTool.ts:198-206, 279-295`: when overwriting an
        // existing file, the model MUST have read it first in this session.
        // This prevents the "model hallucinates a replacement for a file it
        // never saw" class of bugs. The check has three layers:
        //
        //   1. The file must exist in `readFileState` (Read was called).
        //   2. The stored mtime must still match the on-disk mtime (no one
        //      edited the file since our read).
        //   3. If mtime matches but the stored content differs from the
        //      current disk content (e.g. mtime 1s precision lied), reject.
        //
        // New files bypass all of this — there's nothing to race with.
        if !is_new
            && let Some(frs) = &ctx.file_read_state
            && let Ok(abs_path) = std::fs::canonicalize(path)
        {
            let frs_read = frs.read().await;
            let Some(entry) = frs_read.peek(&abs_path) else {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "{file_path} must be read with the Read tool before it can be \
                         overwritten. This prevents accidental data loss from unseen files."
                    ),
                    source: None,
                });
            };

            // Layer 2: mtime comparison.
            if let Ok(disk_mtime) = coco_context::file_mtime_ms(&abs_path).await
                && entry.mtime_ms != disk_mtime
            {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "{file_path} has been modified since it was last read \
                         (mtime changed). Read it again before overwriting."
                    ),
                    source: None,
                });
            }

            // Layer 3: content hash fallback. mtime can be stable even when
            // content changed on filesystems with <1s precision or under
            // rapid edit loops, so we compare the stored content against
            // the current disk content when the stored entry is a full-view
            // read (offset/limit are None). For partial reads we can't
            // compare meaningfully, so we skip this layer.
            // TS: `FileWriteTool.ts:286-293` fallback content comparison.
            if entry.offset.is_none()
                && entry.limit.is_none()
                && let Ok(raw) = std::fs::read(&abs_path)
            {
                let detected_enc = coco_file_encoding::detect_encoding(&raw);
                if let Ok(current) = detected_enc.decode(&raw)
                    && current != entry.content
                {
                    return Err(ToolError::ExecutionFailed {
                        message: format!(
                            "{file_path} has been modified since it was last read \
                             (content changed). Read it again before overwriting."
                        ),
                        source: None,
                    });
                }
            }
        }

        // Team-memory secret guard. TS `FileWriteTool.ts:156-160`:
        // refuse to write content containing API keys / tokens /
        // credentials into the team memory directory, because team
        // memory is synced to all repo collaborators. Helper is a
        // no-op for paths outside the team-memory pattern.
        if let Some(err) = crate::check_team_mem_secret(ctx, path, content) {
            return Err(ToolError::ExecutionFailed {
                message: err,
                source: None,
            });
        }

        // Track file edit for checkpoint/rewind before modifying.
        // TS: FileWriteTool.ts line 259
        crate::track_file_edit(ctx, path).await;

        // Detect encoding + read existing content for diff + preservation.
        // TS `FileWriteTool.ts:268-277, 297, 305`:
        //   const meta = readFileSyncWithMetadata(path)
        //   const enc  = meta?.encoding ?? 'utf8'
        //   writeTextContent(path, content, enc, 'LF')
        //
        // Key TS design decision (line 300-305): **always write 'LF'** even
        // when the original file used CRLF. The comment explains: "Write is
        // a full content replacement — the model sent explicit line endings
        // in `content` and meant them. Do not rewrite them." We honor the
        // same decision — only encoding is preserved, not line endings.
        let (old_content, detected_encoding): (Option<String>, coco_file_encoding::Encoding) =
            if !is_new {
                match std::fs::read(file_path) {
                    Ok(raw) => {
                        let enc = coco_file_encoding::detect_encoding(&raw);
                        let decoded = enc.decode(&raw).ok();
                        (decoded, enc)
                    }
                    Err(_) => (None, coco_file_encoding::Encoding::Utf8),
                }
            } else {
                (None, coco_file_encoding::Encoding::Utf8)
            };

        // Ensure parent directory exists
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to create directory {}: {e}", parent.display()),
                source: None,
            })?;
        }

        // Encode the content using the detected encoding (UTF-8 default for
        // new files). `write_with_format` handles BOM prepending and line-
        // ending normalization. We always pass `LineEnding::Lf` because TS
        // intentionally writes LF regardless of source format — see the
        // comment block above about the explicit design decision.
        coco_file_encoding::write_with_format(
            path,
            content,
            detected_encoding,
            coco_file_encoding::LineEnding::Lf,
        )
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to write {file_path}: {e}"),
            source: None,
        })?;

        let line_count = content.lines().count();
        let byte_count = content.len();

        let action = if is_new { "created" } else { "updated" };
        let mut msg = format!(
            "File {action} successfully at: {file_path} ({line_count} lines, {byte_count} bytes)"
        );

        // Generate simple diff summary for updates
        if let Some(ref old) = old_content {
            let old_lines = old.lines().count();
            let diff_lines = (line_count as i64 - old_lines as i64).abs();
            let diff_direction = if line_count > old_lines { "+" } else { "-" };
            msg.push_str(&format!(
                "\nDiff: {old_lines} → {line_count} lines ({diff_direction}{diff_lines})"
            ));
        }

        crate::record_file_edit(ctx, path, content.to_string()).await;
        // TS `FileWriteTool.ts` mirrors `FileReadTool.ts:578-591` skill
        // auto-discovery — when a write touches a path under a nested
        // `.claude/skills/` ancestor, the manager picks up the new
        // skill on the next batch boundary.
        crate::track_skill_discovery(ctx, path).await;

        Ok(ToolResult {
            data: serde_json::json!(msg),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "write.test.rs"]
mod tests;
