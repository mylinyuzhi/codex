use base64::Engine;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::SearchReadInfo;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::PermissionDecision;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Default number of lines to read if no limit specified.
const DEFAULT_LINE_LIMIT: usize = 2000;

/// Long-form tool description shown to the model.
///
/// TS: `tools/FileReadTool/prompt.ts:27-49` `renderPromptTemplate()`.
/// Byte-identical port for the default template (no `maxSizeInstruction`
/// runtime override; the offset instruction follows TS's
/// `OFFSET_INSTRUCTION_TARGETED` form). The PDF support note is
/// included unconditionally because coco-rs's pdf-extract dep makes
/// it always available.
const READ_TOOL_DESCRIPTION: &str = "Reads a file from the local filesystem. You can access any file directly by using this tool.
Assume this tool is able to read all files on the machine. If the User provides a path to a file assume that path is valid. It is okay to read a file that does not exist; an error will be returned.

Usage:
- The file_path parameter must be an absolute path, not a relative path
- By default, it reads up to 2000 lines starting from the beginning of the file
- When you already know which part of the file you need, only read that part. This can be important for larger files.
- Results are returned using cat -n format, with line numbers starting at 1
- This tool allows Claude Code to read images (eg PNG, JPG, etc). When reading an image file the contents are presented visually as Claude Code is a multimodal LLM.
- This tool can read PDF files (.pdf). For large PDFs (more than 10 pages), you MUST provide the pages parameter to read specific page ranges (e.g., pages: \"1-5\"). Reading a large PDF without the pages parameter will fail. Maximum 20 pages per request.
- This tool can read Jupyter notebooks (.ipynb files) and returns all cells with their outputs, combining code, text, and visualizations.
- This tool can only read files, not directories. To read a directory, use an ls command via the Bash tool.
- You will regularly be asked to read screenshots. If the user provides a path to a screenshot, ALWAYS use this tool to view the file at the path. This tool will work with all temporary file paths.
- If you read a file that exists but has empty contents you will receive a system reminder warning in place of file contents.";

/// Default byte budget for text reads. TS `FileReadTool.ts` +
/// `getDefaultFileReadingLimits()` caps reads at 256KB when `limit` is
/// unspecified. coco-rs previously only capped by line count, so files
/// with very long lines (minified JSON, base64 blobs, single-line logs)
/// could emit multi-megabyte output while staying under the 2000-line
/// budget. Applying a byte cap in parallel with the line cap closes
/// that gap. R5-T12.
const DEFAULT_BYTE_LIMIT: usize = 256_000;

/// Upper bound on the RAW file size before we even attempt to decode.
///
/// This is a safety valve for catastrophically large files (e.g. a 500MB
/// "image" that's actually a typo for a database dump). The image crate
/// itself would handle the decode, but pulling half a gig into memory
/// before recognizing it's not useful is wasteful. TS
/// `FileReadTool.ts:1097-1183` similarly rejects obviously-oversized
/// files before invoking the compression pipeline.
///
/// 32MB is big enough for typical high-resolution photos (e.g. a 24MP
/// JPEG) while still catching path-of-least-resistance abuse.
const MAX_IMAGE_DECODE_BYTES: u64 = 32 * 1024 * 1024;

/// Image media-type table for formats we can actually decode, resize,
/// and send as multimodal content. TS-aligned exactly:
/// `FileReadTool.ts:188` `IMAGE_EXTENSIONS = Set(['png','jpg','jpeg',
/// 'gif','webp'])`. Order matters — first match wins.
///
/// SVG is intentionally NOT in this list: (a) TS doesn't support SVG
/// in the image set, (b) the Anthropic multimodal API doesn't accept
/// `image/svg+xml`, and (c) the `image` crate is raster-only and cannot
/// decode SVG. SVG files fall through to the placeholder path below.
const IMAGE_MEDIA_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// Image extensions we recognize but cannot send as multimodal content.
/// Fall back to a placeholder message so the model knows the file exists.
/// SVG is included here because the Anthropic API doesn't accept it and
/// our raster image pipeline can't decode it — convert to PNG first.
const PLACEHOLDER_IMAGE_EXTENSIONS: &[&str] = &["bmp", "ico", "tiff", "tif", "svg"];

/// Known binary extensions that should not be read as text.
const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "o", "a", "bin", "class", "pyc", "pyo", "wasm", "zip", "tar",
    "gz", "bz2", "xz", "7z", "rar", "mp3", "mp4", "wav", "avi", "mov", "mkv", "flv", "ttf", "otf",
    "woff", "woff2", "eot", "sqlite", "db",
];

/// Device files that must never be read. TS: `FileReadTool.ts:97-114`
/// `BLOCKED_DEVICE_PATHS`. Reading these would hang (stdin/tty) or spew
/// infinite output (/dev/zero, /dev/random, /dev/urandom).
///
/// NOTE: `/dev/null` is intentionally NOT blocked — it's a common sink and
/// reading from it returns EOF immediately, which is harmless and useful.
const BLOCKED_DEVICE_PATHS: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/full",
    "/dev/stdin",
    "/dev/tty",
    "/dev/console",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/fd/0",
    "/dev/fd/1",
    "/dev/fd/2",
];

/// Read tool — reads file contents with line numbers (cat -n format).
/// Supports text files, offset/limit, image detection, binary detection.
pub struct ReadTool;

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Read)
    }

    fn name(&self) -> &str {
        ToolName::Read.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        READ_TOOL_DESCRIPTION.into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "file_path".into(),
            serde_json::json!({
                "type": "string",
                "description": "The absolute path to the file to read"
            }),
        );
        // TS: `FileReadTool.ts:230-235`. Description and semantics match TS
        // exactly: `offset` is the 1-based line number to start reading from
        // (TS converts via `offset === 0 ? 0 : offset - 1`, so both 0 and 1
        // mean "start from the first line"). `limit` is optional; when
        // omitted TS falls back to a byte-size budget, coco-rs uses
        // `DEFAULT_LINE_LIMIT`.
        props.insert(
            "offset".into(),
            serde_json::json!({
                "type": "number",
                "description": "The line number to start reading from. Only provide if the file is too large to read at once",
                "minimum": 0
            }),
        );
        props.insert(
            "limit".into(),
            serde_json::json!({
                "type": "number",
                "description": "The number of lines to read. Only provide if the file is too large to read at once.",
                "exclusiveMinimum": 0
            }),
        );
        props.insert(
            "pages".into(),
            serde_json::json!({
                "type": "string",
                "description": "Page range for PDF files (e.g., \"1-5\", \"3\", \"10-20\"). Only applicable to PDF files."
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
        let path = input.get("file_path").and_then(|v| v.as_str())?;
        Some(format!("Reading {path}"))
    }

    fn is_search_or_read_command(&self, _input: &Value) -> Option<SearchReadInfo> {
        Some(SearchReadInfo {
            is_read: true,
            ..SearchReadInfo::default()
        })
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    /// R6-T20: file-read permission gate. TS routes every Read through
    /// `checkReadPermissionForTool`; coco-rs matches by consulting the
    /// resolved `ctx.tool_config.file_read_ignore_patterns` matcher
    /// (JSON-first, env override via `COCO_FILE_READ_IGNORE_PATTERNS`).
    /// Paths matching any deny glob are rejected before disk access.
    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionDecision {
        let Some(file_path) = input.get("file_path").and_then(|v| v.as_str()) else {
            return PermissionDecision::Allow {
                updated_input: None,
                feedback: None,
            };
        };
        let matcher = crate::tools::read_permissions::file_read_ignore_matcher_from_patterns(
            &ctx.tool_config.file_read_ignore_patterns,
        );
        crate::tools::read_permissions::check_read_permission_with_matcher(
            Path::new(file_path),
            &matcher,
        )
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("file_path").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: file_path");
        }
        if let Some(offset) = input.get("offset").and_then(serde_json::Value::as_i64)
            && offset < 0
        {
            return ValidationResult::invalid("offset must be non-negative");
        }
        if let Some(limit) = input.get("limit").and_then(serde_json::Value::as_i64)
            && limit <= 0
        {
            return ValidationResult::invalid("limit must be positive");
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing file_path".into(),
                error_code: None,
            })?;

        let path = Path::new(file_path);

        // Device file blocklist — reject `/dev/zero`, `/dev/stdin`, etc.
        // BEFORE the existence check because some of these (/dev/stdin)
        // exist but would hang the tool indefinitely. `/dev/null` is OK
        // and falls through. TS: `FileReadTool.ts:486-492`.
        if BLOCKED_DEVICE_PATHS.contains(&file_path) {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "Cannot read device file: {file_path}. \
                     Reading this path would hang or return unbounded data."
                ),
                error_code: None,
            });
        }

        // Check existence
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("File not found: {file_path}"),
                source: None,
            });
        }

        // Check if directory
        if path.is_dir() {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "{file_path} is a directory, not a file. Use Bash with ls to list directory contents."
                ),
                error_code: None,
            });
        }

        // ── R7-T9: file_unchanged dedup ──
        //
        // TS `FileReadTool.ts:523-573`: when the model issues a Read for a
        // file we've already returned in this session, with the same
        // offset/limit and an unchanged disk mtime, return a stub instead
        // of resending the full content. BQ telemetry shows ~18% of Read
        // calls are same-file collisions; the stub saves the cache_creation
        // tokens for the second copy.
        //
        // Gating (matches TS `FileReadTool.ts:540-547`):
        //   1. The cache has an entry for this path.
        //   2. The entry was inserted via the Read tool (not Edit/Write).
        //      Without this gate, an Edit-then-Read sequence would dedup
        //      against the post-edit entry, which the model never saw as
        //      a Read tool result — the stub would be misleading.
        //   3. The entry's offset/limit match the current request's
        //      offset/limit byte-for-byte. Default reads (no offset/limit
        //      args) compare with `(None, None)` on both sides.
        //   4. The disk mtime matches the cached mtime (the file hasn't
        //      changed since we cached it).
        //
        // Notebooks/PDFs/images bypass the dedup because their content
        // type isn't a plain text snapshot — they go through specialized
        // read paths below that overwrite the cache anyway.
        let dedup_offset = input
            .get("offset")
            .and_then(serde_json::Value::as_i64)
            .map(|v| v as i32);
        let dedup_limit = input
            .get("limit")
            .and_then(serde_json::Value::as_i64)
            .map(|v| v as i32);
        // Only attempt dedup for plain text reads. Image/PDF/notebook
        // paths fall through to their dedicated handlers below — they
        // call `record_file_read` themselves and never need a stub.
        let is_special_extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .is_some_and(|ext| {
                IMAGE_MEDIA_TYPES.iter().any(|(e, _)| *e == ext)
                    || PLACEHOLDER_IMAGE_EXTENSIONS.contains(&ext.as_str())
                    || ext == "ipynb"
                    || ext == "pdf"
                    || BINARY_EXTENSIONS.contains(&ext.as_str())
            });
        if !is_special_extension
            && let Some(frs) = &ctx.file_read_state
            && let Ok(abs_path) = std::fs::canonicalize(path)
        {
            let frs_read = frs.read().await;
            // The dedup compares the LITERAL input range stored on the
            // FileReadState (via `read_input_range`) against the current
            // call's input. This avoids the effective-range normalization
            // gotcha where Read(file, limit=2000) followed by Read(file,
            // limit=2000) wouldn't match if the first call's effective
            // limit got rewritten to None.
            if frs_read.is_from_read_tool(&abs_path)
                && let Some(entry) = frs_read.peek(&abs_path)
                && let Some(stored_input) = frs_read.read_input_range(&abs_path)
                && stored_input == (dedup_offset, dedup_limit)
                && let Ok(disk_mtime) = coco_context::file_mtime_ms(&abs_path).await
                && entry.mtime_ms == disk_mtime
            {
                // Cache hit — return the TS-shaped `file_unchanged` stub.
                // The wrapper object matches TS `FileReadTool.ts:563-567`:
                //   { type: 'file_unchanged', file: { filePath } }
                tracing::debug!(
                    "Read dedup hit for {} (offset={:?}, limit={:?})",
                    file_path,
                    dedup_offset,
                    dedup_limit
                );
                return Ok(ToolResult {
                    data: serde_json::json!({
                        "type": "file_unchanged",
                        "file": { "filePath": file_path },
                    }),
                    new_messages: vec![],
                    app_state_patch: None,
                });
            }
        }

        // Check extension for special file types
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();

            // Image files that map to a supported multimodal media type get
            // returned as base64-encoded bytes. TS: `FileReadTool.ts:1097-1183`
            // + `FileReadTool.ts:396-397` (base64 encode step).
            if let Some(media_type) = IMAGE_MEDIA_TYPES
                .iter()
                .find_map(|(e, mt)| (*e == ext_lower).then_some(*mt))
            {
                crate::record_file_read(ctx, path, String::new(), None, None, None, None).await;
                crate::track_nested_memory_attachment(ctx, path).await;
                return read_image_as_base64(file_path, media_type).await;
            }

            // Image formats we recognize but Anthropic multimodal API doesn't
            // accept — return a placeholder so the model knows the file type.
            if PLACEHOLDER_IMAGE_EXTENSIONS.contains(&ext_lower.as_str()) {
                crate::record_file_read(ctx, path, String::new(), None, None, None, None).await;
                crate::track_nested_memory_attachment(ctx, path).await;
                return Ok(ToolResult {
                    // TS-shaped `text` envelope for the placeholder
                    // message — `numLines`/`totalLines` set to 1 since
                    // the placeholder is one synthetic line.
                    data: text_output(
                        file_path,
                        &format!(
                            "[image file ({ext_lower}) — format not supported by multimodal API, \
                             convert to PNG/JPEG/GIF/WEBP first]"
                        ),
                        1,
                        1,
                        1,
                    ),
                    new_messages: vec![],
                    app_state_patch: None,
                });
            }

            if ext_lower == "ipynb" {
                // TS `FileReadTool.ts:848` adds the notebook path to the
                // nested-memory triggers from inside the notebook code path.
                crate::track_nested_memory_attachment(ctx, path).await;
                return read_notebook(file_path);
            }

            if ext_lower == "pdf" {
                crate::record_file_read(ctx, path, String::new(), None, None, None, None).await;
                crate::track_nested_memory_attachment(ctx, path).await;
                let pages = input.get("pages").and_then(|v| v.as_str());
                return read_pdf(file_path, pages);
            }

            // Binary files
            if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return Ok(ToolResult {
                    data: text_output(file_path, &format!("[binary file: {ext_lower}]"), 1, 1, 1),
                    new_messages: vec![],
                    app_state_patch: None,
                });
            }
        }

        // Read file bytes, then detect encoding and decode. TS:
        // `utils/fileRead.ts:20-98` `readFileSyncWithMetadata` does UTF-16LE
        // BOM detection + UTF-8 default. We delegate to `coco-file-encoding`
        // which supports UTF-8, UTF-8-with-BOM, UTF-16LE, and UTF-16BE.
        //
        // Reading as bytes first means UTF-16 files no longer fail with
        // "invalid UTF-8" — a regression vs. TS if we'd stayed on
        // `fs::read_to_string`.
        let raw_bytes = std::fs::read(file_path).map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to read {file_path}: {e}"),
            source: None,
        })?;

        let encoding = coco_file_encoding::detect_encoding(&raw_bytes);
        let content = encoding
            .decode(&raw_bytes)
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to decode {file_path} as {encoding:?}: {e}"),
                source: None,
            })?;

        // Check empty
        if content.is_empty() {
            return Ok(ToolResult {
                data: text_output(file_path, "[file is empty]", 0, 1, 0),
                new_messages: vec![],
                app_state_patch: None,
            });
        }

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // TS: `FileReadTool.ts:497` — default `offset = 1` (1-based). Inputs
        // are read as `u64` so negatives hit serde's bounds and never reach
        // this path. We accept both `0` and `1` as "start from the first
        // line" because TS does (`const lineOffset = offset === 0 ? 0 :
        // offset - 1`).
        let offset = input
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1) as usize;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LINE_LIMIT);

        // Convert user-facing 1-based offset → internal 0-based start index.
        // Matches TS `FileReadTool.ts:1020`: `offset === 0 ? 0 : offset - 1`.
        let start = if offset == 0 { 0 } else { offset - 1 };

        // Offset-beyond-file warning. TS: `FileReadTool.ts:707` emits a
        // `<system-reminder>` when the requested 1-based offset exceeds the
        // total line count. We keep the warning text short and TS-compatible:
        // it includes the original (1-based) offset and total line count.
        if start >= total_lines && total_lines > 0 {
            // Wrapped in the TS `text` discriminated-union shape so the
            // model receives a consistent envelope across all Read paths.
            return Ok(ToolResult {
                data: text_output(
                    file_path,
                    &format!(
                        "[file exists but is shorter than provided offset: \
                         file has {total_lines} line(s), offset is {offset}]"
                    ),
                    0,
                    1,
                    total_lines,
                ),
                new_messages: vec![],
                app_state_patch: None,
            });
        }

        let line_end = (start + limit).min(total_lines);

        // Format as cat -n (1-indexed line numbers). The displayed line
        // number is `start + i + 1`, which evaluates to `offset + i` when
        // offset ≥ 1 and to `i + 1` when offset == 0 — matching TS.
        //
        // R5-T12: in addition to the line cap, enforce a byte budget so
        // minified files / long-line blobs don't blow up the output. We
        // stop emitting new lines once the accumulated byte count
        // exceeds `DEFAULT_BYTE_LIMIT`. `end` tracks how many lines were
        // actually emitted; `byte_truncated` reports the reason in the
        // trailing footer so the model knows which cap hit.
        let mut output = String::new();
        let mut end = start;
        let mut byte_truncated = false;
        for (i, line) in lines[start..line_end].iter().enumerate() {
            let line_num = start + i + 1;
            let next = format!("{line_num}\t{line}\n");
            if !output.is_empty() && output.len() + next.len() > DEFAULT_BYTE_LIMIT {
                // Emit at least one line, then stop if the next would
                // bust the budget. The `!output.is_empty()` guard keeps
                // single-giant-line files from returning zero content.
                byte_truncated = true;
                break;
            }
            output.push_str(&next);
            end = start + i + 1;
        }

        // Append info if truncated by either cap.
        if byte_truncated {
            output.push_str(&format!(
                "\n... ({} more lines not shown — output exceeded {} byte limit. \
                 Use offset/limit to read more.)",
                total_lines - end,
                DEFAULT_BYTE_LIMIT
            ));
        } else if end < total_lines {
            output.push_str(&format!(
                "\n... ({} more lines not shown. Use offset/limit to read more.)",
                total_lines - end
            ));
        }

        // `record_file_read` captures both the EFFECTIVE range (post-truncation)
        // and the LITERAL input range (what the model passed). The effective
        // range drives Edit/Write conflict detection (`offset.is_none() &&
        // limit.is_none()` means "we have the full file"). The literal input
        // range drives the Read tool's `file_unchanged` dedup so a repeat call
        // with the same args matches byte-for-byte.
        let offset_i32 = if offset > 1 {
            Some(offset as i32)
        } else {
            None
        };
        let limit_i32 = if end < total_lines {
            Some((end - start) as i32)
        } else {
            None
        };
        // ── R7-T13: TS-shaped output ──
        //
        // Capture the projected metadata BEFORE handing `content` to
        // `record_file_read` (which moves the String). `numLines` is the
        // count of source lines actually emitted into `output`; `startLine`
        // is the 1-based line number of the first emitted line; `totalLines`
        // is the file's total line count.
        let num_lines = end - start;
        let start_line = if start == 0 { 1 } else { start + 1 };

        crate::record_file_read(
            ctx,
            path,
            content,
            offset_i32,
            limit_i32,
            dedup_offset,
            dedup_limit,
        )
        .await;
        // Fire-and-forget skill discovery: walk up from the file path to
        // the cwd boundary and queue any `.claude/skills/` ancestor dirs
        // for the app/query layer to load. TS `FileReadTool.ts:578-591`
        // calls this same routine on every successful Read.
        crate::track_skill_discovery(ctx, path).await;
        // TS `FileReadTool.ts:848,870,1038`: every successful Read
        // pushes the path into nestedMemoryAttachmentTriggers so the
        // next-turn message builder can attach any nested CLAUDE.md
        // memories from the file's ancestry.
        crate::track_nested_memory_attachment(ctx, path).await;

        Ok(ToolResult {
            // TS `FileReadTool.ts:258-269` discriminated-union `text`
            // variant with `file: { filePath, content, numLines,
            // startLine, totalLines }`.
            data: text_output(file_path, &output, num_lines, start_line, total_lines),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

/// Build the TS-shaped `text` discriminated-union variant for Read
/// output. TS `FileReadTool.ts:258-269`:
///
/// ```js
/// { type: 'text', file: { filePath, content, numLines, startLine, totalLines } }
/// ```
///
/// All Read code paths that previously returned a JSON `String` now
/// route through this helper so the model sees a consistent envelope.
/// The `numLines` field counts emitted lines (post-truncation); the
/// `totalLines` field reflects the full file's line count.
fn text_output(
    file_path: &str,
    content: &str,
    num_lines: usize,
    start_line: usize,
    total_lines: usize,
) -> Value {
    serde_json::json!({
        "type": "text",
        "file": {
            "filePath": file_path,
            "content": content,
            "numLines": num_lines,
            "startLine": start_line,
            "totalLines": total_lines,
        }
    })
}

/// Read a Jupyter notebook (.ipynb) and project each cell into the
/// TS-shaped structured cell array.
///
/// TS: `utils/notebook.ts:163-183` `readNotebook` + `processCell` returns
/// `NotebookCellSource[]`, which the discriminated-union output schema
/// at `FileReadTool.ts:299-305` wraps as
/// `{ type: 'notebook', file: { filePath, cells: NotebookCellSource[] } }`.
///
/// Each cell in the array has:
///
/// ```json
/// {
///   "cellType":       "code" | "markdown" | "raw",
///   "source":         "joined cell source string",
///   "execution_count": <number | null>,        // code cells only
///   "cell_id":        "stable id or 'cell-N'",
///   "language":       "python",                  // code cells only
///   "outputs":        [{ output_type, text, image? }]  // code cells only
/// }
/// ```
///
/// The notebook's top-level `metadata.language_info.name` is used as
/// the language for code cells (defaults to `"python"` per TS). For
/// missing `cell_id` we synthesize `cell-N` to match TS at line 89.
fn read_notebook(file_path: &str) -> Result<ToolResult<Value>, ToolError> {
    let content = std::fs::read_to_string(file_path).map_err(|e| ToolError::ExecutionFailed {
        message: format!("failed to read notebook: {e}"),
        source: None,
    })?;

    let notebook: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ToolError::ExecutionFailed {
            message: format!("invalid notebook JSON: {e}"),
            source: None,
        })?;

    // TS `notebook.metadata.language_info?.name ?? 'python'`.
    let language = notebook
        .get("metadata")
        .and_then(|m| m.get("language_info"))
        .and_then(|li| li.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("python")
        .to_string();

    let cells = notebook
        .get("cells")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "notebook has no cells array".into(),
            source: None,
        })?;

    let projected: Vec<Value> = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| project_notebook_cell(cell, i, &language))
        .collect();

    Ok(ToolResult {
        data: serde_json::json!({
            "type": "notebook",
            "file": {
                "filePath": file_path,
                "cells": projected,
            }
        }),
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Project one notebook cell into the TS `NotebookCellSource` shape.
/// TS `utils/notebook.ts:83-117` `processCell`. Field semantics:
///
///  - `cellType` carries the cell's type (`code` / `markdown` / `raw`).
///  - `source` is the joined source string (notebook source can be a
///    string OR a string array — both formats are valid per nbformat).
///  - `execution_count` only appears for `code` cells.
///  - `cell_id` defaults to `cell-N` when missing.
///  - `language` only appears for `code` cells.
///  - `outputs` only appears for `code` cells with a non-empty
///    `outputs` array.
///
/// TS truncates oversized outputs (`LARGE_OUTPUT_THRESHOLD = 10_000`),
/// substituting a stream cell with a "use Bash + jq" hint. coco-rs
/// applies the same threshold per-cell so transcripts don't blow up
/// on notebooks with embedded base64 plots.
fn project_notebook_cell(cell: &Value, index: usize, code_language: &str) -> Value {
    let cell_type = cell
        .get("cell_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let source = join_cell_source(cell.get("source"));

    let cell_id = cell
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("cell-{index}"));

    let mut obj = serde_json::Map::new();
    obj.insert("cellType".into(), Value::String(cell_type.clone()));
    obj.insert("source".into(), Value::String(source));
    obj.insert("cell_id".into(), Value::String(cell_id));

    if cell_type == "code" {
        // execution_count is `null` for unexecuted cells in nbformat;
        // TS converts to `undefined` (which omits the field). We omit
        // it when null/missing to match.
        if let Some(count) = cell
            .get("execution_count")
            .and_then(serde_json::Value::as_i64)
        {
            obj.insert("execution_count".into(), Value::Number(count.into()));
        }
        obj.insert("language".into(), Value::String(code_language.into()));

        if let Some(outputs) = cell.get("outputs").and_then(|v| v.as_array()) {
            let projected_outputs: Vec<Value> =
                outputs.iter().filter_map(project_notebook_output).collect();
            if !projected_outputs.is_empty() {
                let total_size: usize = projected_outputs
                    .iter()
                    .map(|o| {
                        o.get("text").and_then(|t| t.as_str()).map_or(0, str::len)
                            + o.get("image")
                                .and_then(|i| i.get("image_data"))
                                .and_then(|d| d.as_str())
                                .map_or(0, str::len)
                    })
                    .sum();
                // TS `LARGE_OUTPUT_THRESHOLD = 10000` substitutes a
                // hint when the combined output payload exceeds the
                // budget. Matches `notebook.ts:104-113`.
                const LARGE_OUTPUT_THRESHOLD: usize = 10_000;
                if total_size > LARGE_OUTPUT_THRESHOLD {
                    obj.insert(
                        "outputs".into(),
                        serde_json::json!([{
                            "output_type": "stream",
                            "text": format!(
                                "Outputs are too large to include. Use Bash with: \
                                 cat <notebook_path> | jq '.cells[{index}].outputs'"
                            )
                        }]),
                    );
                } else {
                    obj.insert("outputs".into(), Value::Array(projected_outputs));
                }
            }
        }
    }

    Value::Object(obj)
}

/// Project one cell `outputs[i]` entry into the TS
/// `NotebookCellSourceOutput` shape. Returns `None` for unrecognized
/// `output_type` values so noise from custom kernels doesn't pollute
/// the cell array.
///
/// TS `notebook.ts:59-81` `processOutput`. Switches on `output_type`:
///  - `stream`           → `{ output_type, text }`
///  - `execute_result` /
///    `display_data`     → `{ output_type, text, image? }`
///  - `error`            → `{ output_type, text }` (formatted as
///                         `${ename}: ${evalue}\n${traceback}`)
fn project_notebook_output(output: &Value) -> Option<Value> {
    let output_type = output.get("output_type")?.as_str()?;
    match output_type {
        "stream" => {
            let text = join_cell_source(output.get("text"));
            Some(serde_json::json!({
                "output_type": "stream",
                "text": text,
            }))
        }
        "execute_result" | "display_data" => {
            let data = output.get("data");
            let text = data
                .and_then(|d| d.get("text/plain"))
                .map(|t| join_cell_source(Some(t)))
                .unwrap_or_default();
            let mut entry = serde_json::Map::new();
            entry.insert("output_type".into(), Value::String(output_type.into()));
            entry.insert("text".into(), Value::String(text));
            if let Some(image) = data.and_then(extract_notebook_image) {
                entry.insert("image".into(), image);
            }
            Some(Value::Object(entry))
        }
        "error" => {
            let ename = output.get("ename").and_then(|v| v.as_str()).unwrap_or("");
            let evalue = output.get("evalue").and_then(|v| v.as_str()).unwrap_or("");
            let traceback = output
                .get("traceback")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            Some(serde_json::json!({
                "output_type": "error",
                "text": format!("{ename}: {evalue}\n{traceback}"),
            }))
        }
        _ => None,
    }
}

/// Extract a `{ image_data, media_type }` payload from a notebook
/// output's `data` map. TS `notebook.ts:41-57` `extractImage`.
/// Recognizes PNG and JPEG (the only formats nbformat guarantees).
fn extract_notebook_image(data: &Value) -> Option<Value> {
    if let Some(png) = data.get("image/png").and_then(|v| v.as_str()) {
        return Some(serde_json::json!({
            "image_data": png.replace(char::is_whitespace, ""),
            "media_type": "image/png",
        }));
    }
    if let Some(jpeg) = data.get("image/jpeg").and_then(|v| v.as_str()) {
        return Some(serde_json::json!({
            "image_data": jpeg.replace(char::is_whitespace, ""),
            "media_type": "image/jpeg",
        }));
    }
    None
}

/// Join a notebook source field, which may be a string or an array of
/// strings per the nbformat spec. Both shapes are valid in `.ipynb`
/// files, depending on which Jupyter front-end wrote them. TS
/// `notebook.ts:92` mirrors this: `Array.isArray(cell.source) ?
/// cell.source.join('') : cell.source`.
fn join_cell_source(source: Option<&Value>) -> String {
    match source {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Read an image file and return it as base64-encoded data with its
/// media type, running it through the resize-and-re-encode pipeline.
///
/// Pipeline (TS-aligned at the behavior level, not byte-for-byte):
///
/// 1. Read raw bytes from disk via `spawn_blocking`.
/// 2. Safety cap at [`MAX_IMAGE_DECODE_BYTES`] (32MB) — catches obvious
///    path-of-least-resistance abuse before spending CPU on decode.
/// 3. Delegate to `coco_utils_image::load_for_prompt_bytes` with
///    `PromptImageMode::ResizeToFit`. This:
///    - detects the format (PNG/JPEG/GIF/WebP)
///    - preserves the source bytes when the image is already within
///      `MAX_WIDTH × MAX_HEIGHT` (2048 × 768) bounds
///    - otherwise resizes via Triangle filter and re-encodes to the
///      source format (with PNG fallback for formats we can't round-trip)
/// 4. Base64-encode the post-processing bytes.
///
/// TS `FileReadTool.ts:1097-1183` (`readImageWithTokenBudget`) has a
/// genuinely two-stage pipeline: standard resize, then aggressive JPEG
/// re-encoding if still over the token budget. Our single-stage resize
/// is close enough for the common case — a typical 24MP photo shrinks
/// from ~24MB to ~500KB at 2048×768, well under any reasonable token
/// budget. The aggressive JPEG stage is a follow-up if we hit real
/// budget issues.
///
/// Result payload shape matches the format used by coco-rs elsewhere for
/// multimodal content: a JSON object with `type: "image"`, `source.type:
/// "base64"`, `source.media_type`, and `source.data`. The query/message
/// layer converts this into the provider-specific multimodal block.
///
/// The `media_type` argument is a hint from the file extension; the
/// actual `source.media_type` in the returned payload is the type the
/// image crate decided on after processing (which may differ if a WebP
/// with alpha got re-encoded as PNG, for example).
async fn read_image_as_base64(
    file_path: &str,
    media_type: &str,
) -> Result<ToolResult<Value>, ToolError> {
    // Stage 0: metadata-based raw-size cap. Read the file size without
    // slurping the whole thing into memory first — this catches an
    // accidentally-huge file before we allocate a multi-GB Vec.
    let metadata =
        tokio::fs::metadata(file_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to stat image {file_path}: {e}"),
                source: None,
            })?;
    if metadata.len() > MAX_IMAGE_DECODE_BYTES {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Image file too large to decode: {} bytes > {MAX_IMAGE_DECODE_BYTES} byte \
                 limit. This cap exists to prevent accidentally loading huge files; if you \
                 genuinely need to process a larger image, resize it first with an external \
                 tool (e.g. `magick input.png -resize 2048x2048 output.png`).",
                metadata.len()
            ),
            source: None,
        });
    }

    // Stage 1: blocking read + resize + re-encode. Both the file read
    // and the image decode/encode are CPU-or-IO bound and block, so we
    // run the whole pipeline inside `spawn_blocking`.
    let file_path_owned = file_path.to_string();
    let hint_path = std::path::PathBuf::from(file_path);
    let encoded =
        tokio::task::spawn_blocking(move || -> Result<coco_utils_image::EncodedImage, String> {
            let raw = std::fs::read(&file_path_owned)
                .map_err(|e| format!("failed to read image {file_path_owned}: {e}"))?;
            coco_utils_image::load_for_prompt_bytes(
                &hint_path,
                raw,
                coco_utils_image::PromptImageMode::ResizeToFit,
            )
            .map_err(|e| format!("image processing failed: {e}"))
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("spawn_blocking failed: {e}"),
            source: None,
        })?
        .map_err(|e| ToolError::ExecutionFailed {
            message: e,
            source: None,
        })?;

    // Stage 2: base64-encode the processed bytes. We use the MIME type
    // from the image processing result — if the crate downgraded WebP to
    // PNG, for example, the returned MIME reflects that so the model
    // sees the correct content type.
    let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded.bytes);

    // Debug-log any mismatch between the filename-derived hint and the
    // image crate's actual decision — useful when investigating why a
    // .webp is being returned as image/png.
    if encoded.mime != media_type {
        tracing::debug!(
            "Image MIME adjusted from filename hint {media_type} to {} after processing",
            encoded.mime
        );
    }

    // TS `FileReadTool.ts:270-298` shapes the image discriminated-
    // union variant as:
    //   { type: 'image', file: { base64, type, originalSize,
    //                            dimensions?: { originalWidth,
    //                                           originalHeight,
    //                                           displayWidth,
    //                                           displayHeight } } }
    //
    // R7-T20: dimensions are now plumbed from
    // `coco_utils_image::EncodedImage` so the model can convert click
    // coordinates between the resized display image and the source
    // image's coordinate space.
    Ok(ToolResult {
        data: serde_json::json!({
            "type": "image",
            "file": {
                "base64": b64,
                "type": encoded.mime,
                "originalSize": metadata.len(),
                "dimensions": {
                    "originalWidth": encoded.original_width,
                    "originalHeight": encoded.original_height,
                    "displayWidth": encoded.width,
                    "displayHeight": encoded.height,
                }
            }
        }),
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Maximum number of pages to extract per read. TS
/// `PDF_MAX_PAGES_PER_READ` (`constants/apiLimits.ts`) defaults to 20;
/// we match that upper bound.
const PDF_MAX_PAGES_PER_READ: usize = 20;

/// Read a PDF file and return its text content.
///
/// R6-T16: real PDF parsing via the `pdf-extract` crate (pure Rust,
/// no C dependencies). TS `FileReadTool.ts:987` uses `readPDF()` which
/// wraps `pdf-parse`; both produce a plain-text dump of the document
/// with page breaks. We match the TS output shape by separating pages
/// with a `\n--- Page N ---\n` header and honouring the optional
/// `pages` range param (`"1-5"`, `"3"`, `"10-20"`).
///
/// # Range syntax
///
/// Matches TS `parsePDFPageRange()` in `utils/pdfUtils.ts`:
/// - `"3"`     → page 3 only
/// - `"1-5"`   → pages 1 through 5 inclusive (1-based)
/// - missing   → all pages, capped at [`PDF_MAX_PAGES_PER_READ`]
fn read_pdf(file_path: &str, pages: Option<&str>) -> Result<ToolResult<Value>, ToolError> {
    let bytes = std::fs::read(file_path).map_err(|e| ToolError::ExecutionFailed {
        message: format!("failed to read PDF: {e}"),
        source: None,
    })?;

    // `pdf-extract` prefers a byte slice; extracting text from `bytes`
    // returns the whole document joined with form-feed (`\x0C`)
    // separators between pages, which is how `pdftotext` encodes page
    // breaks too. We split on that separator to get one-page-per-entry.
    let full_text =
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to extract PDF text from {file_path}: {e}"),
            source: None,
        })?;

    // Page separator detection: `pdf-extract` emits `\u{0C}` (form feed)
    // between pages. If the document has no separators (e.g. single-page
    // PDF or a decoder that skipped them), we treat the whole string as
    // one page so the range handling below still works.
    let page_texts: Vec<&str> = if full_text.contains('\u{0C}') {
        full_text.split('\u{0C}').collect()
    } else {
        vec![full_text.as_str()]
    };
    let total_pages = page_texts.len();

    // Parse the `pages` param into a (start, end) 1-based range. Both
    // bounds are inclusive. Invalid ranges fall back to "all pages"
    // (capped at PDF_MAX_PAGES_PER_READ) so the model doesn't have to
    // know the exact page count up front.
    let (start_page, end_page) = match pages {
        Some(spec) => parse_page_range(spec, total_pages)
            .unwrap_or((1, total_pages.min(PDF_MAX_PAGES_PER_READ))),
        None => (1, total_pages.min(PDF_MAX_PAGES_PER_READ)),
    };
    // Clamp the range to what the document actually has.
    let start_idx = start_page
        .saturating_sub(1)
        .min(total_pages.saturating_sub(1));
    let end_idx = end_page.min(total_pages).saturating_sub(1);
    if start_idx > end_idx {
        return Ok(ToolResult {
            data: serde_json::json!({
                "type": "pdf",
                "file": {
                    "filePath": file_path,
                    "content": format!(
                        "[PDF file: {file_path}]\n\
                         Requested page range {start_page}-{end_page} is outside \
                         the document's {total_pages} page(s)."
                    ),
                    "totalPages": total_pages,
                }
            }),
            new_messages: vec![],
            app_state_patch: None,
        });
    }
    // Enforce the per-read page cap even when the user passes a bigger
    // range — TS does the same at `PDF_MAX_PAGES_PER_READ`.
    let effective_end = end_idx.min(start_idx + PDF_MAX_PAGES_PER_READ - 1);

    // Build the output. Page headers (`--- Page N ---`) give the model
    // a visual cue for page boundaries without eating more tokens than
    // necessary.
    let mut out = String::new();
    out.push_str(&format!(
        "[PDF file: {file_path}, {total_pages} page(s), showing page(s) {}-{}]\n\n",
        start_idx + 1,
        effective_end + 1
    ));
    for (i, text) in page_texts[start_idx..=effective_end].iter().enumerate() {
        let page_num = start_idx + i + 1;
        out.push_str(&format!("--- Page {page_num} ---\n"));
        out.push_str(text.trim_end_matches('\n'));
        out.push_str("\n\n");
    }

    // Extra hint when there's more to read.
    if effective_end + 1 < total_pages {
        out.push_str(&format!(
            "\n[{} more page(s) available. Pass pages=\"{}-{}\" to read more.]\n",
            total_pages - effective_end - 1,
            effective_end + 2,
            (effective_end + PDF_MAX_PAGES_PER_READ + 1).min(total_pages)
        ));
    }

    // TS `FileReadTool.ts:306-323` shapes the `pdf` discriminated-
    // union variant as `{ type: 'pdf', file: { filePath, base64,
    // originalSize } }` — TS sends the raw PDF as base64 and lets
    // Anthropic's API parse it natively. coco-rs currently extracts
    // text via `pdf-extract` and surfaces it as `content` instead;
    // the envelope still uses the `pdf` discriminator so model-side
    // pattern-matching works. Sending the raw PDF base64 + Anthropic-
    // native PDF block is a follow-up that depends on plumbing
    // multimodal PDF blocks through the message layer.
    Ok(ToolResult {
        data: serde_json::json!({
            "type": "pdf",
            "file": {
                "filePath": file_path,
                "content": out,
                "totalPages": total_pages,
            }
        }),
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Parse a `pages` spec like `"3"` or `"1-5"` into a 1-based
/// `(start, end)` range. Returns `None` on parse error.
///
/// TS: `utils/pdfUtils.ts::parsePDFPageRange`.
fn parse_page_range(spec: &str, total: usize) -> Option<(usize, usize)> {
    let spec = spec.trim();
    if let Some((a, b)) = spec.split_once('-') {
        let start: usize = a.trim().parse().ok()?;
        let end: usize = b.trim().parse().ok()?;
        if start == 0 || end < start {
            return None;
        }
        Some((start, end.min(total)))
    } else {
        let n: usize = spec.parse().ok()?;
        if n == 0 {
            return None;
        }
        Some((n, n))
    }
}

#[cfg(test)]
#[path = "read.test.rs"]
mod tests;
