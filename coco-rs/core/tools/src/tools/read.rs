use base64::Engine;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::SearchReadInfo;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// Default number of lines to read if no limit specified.
const DEFAULT_LINE_LIMIT: usize = 2000;

/// Short per-call UI label. TS `tools/FileReadTool/prompt.ts:11`
/// `DESCRIPTION`, returned by `async description()`.
const READ_TOOL_SHORT_DESCRIPTION: &str = "Read a file from the local filesystem.";

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

/// Maximum total file size for a FULL read (no `limit`). TS
/// `MAX_OUTPUT_SIZE = 0.25 * 1024 * 1024` (`utils/file.ts:48`). A full
/// read of a larger file throws `FileTooLargeError` rather than
/// truncating — TS deliberately reverted truncation (limits.ts) because
/// a ~100-byte error beats ~256KB of truncated content. Partial reads
/// (explicit `limit`) skip this check and rely on the token cap below.
const MAX_READ_OUTPUT_BYTES: usize = 256 * 1024;

/// Default output token budget for a read slice. TS
/// `DEFAULT_MAX_OUTPUT_TOKENS = 25000` (`tools/FileReadTool/limits.ts`).
/// Any read whose slice exceeds this estimate throws
/// `MaxFileReadTokenExceededError` (mirrors `validateContentTokens`).
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 25_000;

/// TS `bytesPerTokenForFileType` (`services/tokenEstimation.ts`): JSON
/// family packs ~2 bytes/token, everything else ~4. Used for the rough
/// pre-API token estimate.
fn bytes_per_token_for_ext(file_path: &str) -> usize {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "json" | "jsonl" | "jsonc" => 2,
        _ => 4,
    }
}

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

/// Typed input for [`ReadTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReadInput {
    /// The absolute path to the file to read
    pub file_path: String,
    // 1-based — TS converts via `offset === 0 ? 0 : offset - 1`, so both 0
    // and 1 mean "start from the first line".
    /// The line number to start reading from. Only provide if the file is too large to read at once
    #[serde(default)]
    pub offset: Option<i64>,
    /// The number of lines to read. Only provide if the file is too large to read at once.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page range for PDF files (e.g., "1-5", "3", "10-20"). Only
    /// applicable to PDF files. Maximum 20 pages per request.
    #[serde(default)]
    pub pages: Option<String>,
}

/// Read tool — reads file contents with line numbers (cat -n format).
/// Supports text files, offset/limit, image detection, binary detection.
pub struct ReadTool;

#[async_trait::async_trait]
impl Tool for ReadTool {
    type Input = ReadInput;
    coco_tool_runtime::impl_runtime_schema!(ReadInput);
    /// Output is `Value` — the wire shape is a tagged union of
    /// `{type: "text", file: {content}}`, `{type: "image", file:
    /// {base64, type}}`, `{type: "pdf", ...}`, `{type: "notebook",
    /// file: {cells: [...]}}` and `{type: "file_unchanged"}`. Modeling
    /// as a tagged enum would mean a big follow-up refactor of the
    /// renderer; deferred to a TS-parity output-typing pass.
    type Output = Value;

    fn to_auto_classifier_input(&self, input: &ReadInput) -> Option<String> {
        Some(input.file_path.clone())
    }

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Read)
    }

    fn name(&self) -> &str {
        ToolName::Read.as_str()
    }

    fn max_result_size_bound(&self) -> coco_tool_runtime::ResultSizeBound {
        // `Read` is the canonical view of a tracked file the model will
        // read again — persistence here would be circular. TS opt-out
        // via `Infinity`.
        coco_tool_runtime::ResultSizeBound::Unbounded
    }

    fn description(&self, _input: &ReadInput, _options: &DescriptionOptions) -> String {
        READ_TOOL_SHORT_DESCRIPTION.into()
    }

    /// Model-facing tool description (schema-listing time). TS
    /// `tools/FileReadTool/FileReadTool.ts:347` `async prompt()` →
    /// `renderPromptTemplate(...)`; we hold the ported text in
    /// [`READ_TOOL_DESCRIPTION`].
    async fn prompt(&self, _options: &coco_tool_runtime::PromptOptions) -> String {
        READ_TOOL_DESCRIPTION.into()
    }

    fn is_read_only(&self, _input: &ReadInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &ReadInput) -> bool {
        true
    }

    fn get_activity_description(&self, input: &ReadInput) -> Option<String> {
        if input.file_path.is_empty() {
            return None;
        }
        Some(format!("Reading {path}", path = input.file_path))
    }

    fn is_search_or_read_command(&self, _input: &ReadInput) -> Option<SearchReadInfo> {
        Some(SearchReadInfo {
            is_read: true,
            ..SearchReadInfo::default()
        })
    }

    fn get_path(&self, input: &ReadInput) -> Option<String> {
        if input.file_path.is_empty() {
            None
        } else {
            Some(input.file_path.clone())
        }
    }

    /// R6-T20: file-read permission gate. TS routes every Read through
    /// `checkReadPermissionForTool`; coco-rs matches by consulting the
    /// resolved `ctx.tool_config.file_read_ignore_patterns` matcher
    /// (JSON-first, env override via `COCO_FILE_READ_IGNORE_PATTERNS`).
    /// Paths matching any deny glob are denied at the central
    /// evaluator's step-1c slot; everything else passes through to
    /// rule + mode-fallthrough evaluation.
    async fn check_permissions(
        &self,
        input: &ReadInput,
        ctx: &ToolUseContext,
    ) -> coco_types::ToolCheckResult {
        if input.file_path.is_empty() {
            return coco_types::ToolCheckResult::Passthrough;
        }
        let matcher = crate::tools::read_permissions::file_read_ignore_matcher_from_patterns(
            &ctx.tool_config.file_read_ignore_patterns,
        );
        crate::tools::read_permissions::check_read_permission_with_matcher(
            Path::new(&input.file_path),
            &matcher,
            ctx,
        )
    }

    fn validate_input(&self, input: &ReadInput, _ctx: &ToolUseContext) -> ValidationResult {
        if input.file_path.is_empty() {
            return ValidationResult::invalid("missing required field: file_path");
        }
        if let Some(offset) = input.offset
            && offset < 0
        {
            return ValidationResult::invalid("offset must be non-negative");
        }
        if let Some(limit) = input.limit
            && limit <= 0
        {
            return ValidationResult::invalid("limit must be positive");
        }
        // #24 / TS `FileReadTool.ts:418-440`: validate the PDF `pages`
        // param up-front (pure string parsing, no I/O). Malformed → 7;
        // a range wider than PDF_MAX_PAGES_PER_READ (incl. the open-ended
        // `"N-"` form, which is unbounded) → 8.
        if let Some(pages) = input.pages.as_deref() {
            match parse_pdf_page_range_spec(pages) {
                None => {
                    return ValidationResult::invalid_with_code(
                        format!(
                            "Invalid pages parameter: \"{pages}\". Use formats like \"1-5\", \
                             \"3\", or \"10-20\". Pages are 1-indexed."
                        ),
                        "7",
                    );
                }
                Some((first, last)) => {
                    let range_size = match last {
                        None => PDF_MAX_PAGES_PER_READ + 1,
                        Some(l) => l - first + 1,
                    };
                    if range_size > PDF_MAX_PAGES_PER_READ {
                        return ValidationResult::invalid_with_code(
                            format!(
                                "Page range \"{pages}\" exceeds maximum of \
                                 {PDF_MAX_PAGES_PER_READ} pages per request. \
                                 Please use a smaller range."
                            ),
                            "8",
                        );
                    }
                }
            }
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: ReadInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        if input.file_path.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "missing file_path".into(),
                error_code: None,
            });
        }
        let file_path = input.file_path.as_str();

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

        // Sandbox pre-flight — give SDK consumers a chance to deny before
        // we touch the filesystem. Platform sandboxes (bwrap/Seatbelt)
        // catch the same violations at kernel level after `read()`, but
        // the structured `PermissionDenied` here surfaces a usable
        // reason to the model instead of an opaque `EACCES`.
        super::sandbox_preflight::preflight_path(ctx, path, /*write=*/ false)?;

        // Check existence
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("File not found: {file_path}"),
                display_data: None,
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
        let dedup_offset = input.offset.map(|v| v as i32);
        let dedup_limit = input.limit.map(|v| v as i32);
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
                    permission_updates: Vec::new(),
                    display_data: None,
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
                    permission_updates: Vec::new(),
                    display_data: None,
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
                let pages = input.pages.as_deref();
                return read_pdf(file_path, pages);
            }

            // Binary files
            if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return Ok(ToolResult {
                    data: text_output(file_path, &format!("[binary file: {ext_lower}]"), 1, 1, 1),
                    new_messages: vec![],
                    app_state_patch: None,
                    permission_updates: Vec::new(),
                    display_data: None,
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
            display_data: None,
            source: None,
        })?;

        // #25 / TS `readFileInRange`: a FULL read (no `limit`) of a file
        // larger than MAX_READ_OUTPUT_BYTES throws instead of truncating —
        // the model must narrow with offset/limit. Partial reads pass
        // through to the line + token caps.
        if input.limit.is_none() && raw_bytes.len() > MAX_READ_OUTPUT_BYTES {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "File content ({} bytes) exceeds maximum allowed size ({} bytes). \
                     Use the offset and limit parameters to read specific portions of the file.",
                    raw_bytes.len(),
                    MAX_READ_OUTPUT_BYTES
                ),
                error_code: None,
            });
        }

        let encoding = coco_file_encoding::detect_encoding(&raw_bytes);
        let content = encoding
            .decode(&raw_bytes)
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to decode {file_path} as {encoding:?}: {e}"),
                display_data: None,
                source: None,
            })?;

        // TS: `FileReadTool.ts:497` — default `offset = 1` (1-based).
        // `validate_input` already rejected negative `offset` / non-positive
        // `limit`, so casting to `usize` here is safe. Both `0` and `1`
        // are treated as "start from the first line" (TS: `const
        // lineOffset = offset === 0 ? 0 : offset - 1`).
        let offset = input
            .offset
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
            .unwrap_or(1);
        let limit = input
            .limit
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_LINE_LIMIT);

        // Empty file. TS `readFileInRangeFast('')` yields one empty selected
        // line → `totalLines = 1`, so this routes to the offset warning in
        // `render_for_model` (`content` empty, `startLine` = the effective
        // offset). The warning string itself is emitted at render time.
        if content.is_empty() {
            return Ok(ToolResult {
                data: text_output(file_path, "", 0, offset, 1),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
            });
        }

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Convert user-facing 1-based offset → internal 0-based start index.
        // Matches TS `FileReadTool.ts:1020`: `offset === 0 ? 0 : offset - 1`.
        let start = if offset == 0 { 0 } else { offset - 1 };

        // Offset-beyond-file. TS: `FileReadTool.ts:707` emits a
        // `<system-reminder>` when the requested 1-based offset exceeds the
        // total line count. The content is left empty here; the warning string
        // (with the effective offset + total line count) is produced in
        // `render_for_model` exactly as TS does at the map layer.
        if start >= total_lines && total_lines > 0 {
            return Ok(ToolResult {
                data: text_output(file_path, "", 0, offset, total_lines),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
            });
        }

        let line_end = (start + limit).min(total_lines);

        // #17 / TS `validateContentTokens`: reject a slice whose rough
        // token estimate exceeds the budget (default 25000) so a single
        // Read can't blow the context. Estimate on the slice content (not
        // the line-number prefixes) to match TS, using the file-type
        // bytes/token ratio. The early `estimate <= max/4` skip in TS is
        // an API-call optimization; with no API counting we compare the
        // estimate directly.
        let slice_bytes: usize = lines[start..line_end].iter().map(|l| l.len() + 1).sum();
        let token_estimate = slice_bytes / bytes_per_token_for_ext(file_path);
        if token_estimate > DEFAULT_MAX_OUTPUT_TOKENS {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "File content ({token_estimate} tokens) exceeds maximum allowed tokens \
                     ({DEFAULT_MAX_OUTPUT_TOKENS}). Use offset and limit parameters to read \
                     specific portions of the file, or search for specific content instead of \
                     reading the whole file."
                ),
                error_code: None,
            });
        }

        // Format as cat -n (1-indexed line numbers). The displayed line
        // number is `start + i + 1`, which evaluates to `offset + i` when
        // offset ≥ 1 and to `i + 1` when offset == 0 — matching TS.
        let mut output = String::new();
        for (i, line) in lines[start..line_end].iter().enumerate() {
            let line_num = start + i + 1;
            output.push_str(&format!("{line_num}\t{line}\n"));
        }
        let end = line_end;

        // Line-cap footer (TS): more lines exist beyond the emitted slice.
        if end < total_lines {
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
        // Walk up to the cwd boundary and queue any `.coco/skills/`
        // ancestor dirs for the app/query layer to load; also queue
        // the file path for conditional-skill activation. TS
        // `FileReadTool.ts:578-591` does both on every successful Read.
        crate::track_skill_triggers(ctx, path).await;
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
            permission_updates: Vec::new(),
            display_data: None,
        })
    }

    /// Project the structured read output into model-facing content
    /// parts.
    ///
    /// TS parity: `FileReadTool.ts::mapToolResultToToolResultBlockParam`
    /// — a `switch (data.type)` over the discriminated union. Each
    /// branch picks the most natural wire shape:
    ///
    ///   - **`image`** → multimodal [`ToolResultContentPart::FileData`].
    ///     Anthropic + Gemini 3+ pass the bytes through to a vision
    ///     model. OpenAI / OpenAI-Compatible degrade to a marker text
    ///     in the provider conversion layer.
    ///   - **`text`** → cat-style line-numbered text. The `content`
    ///     field already carries the formatted body (built by the
    ///     `output` string in `execute`), so we hand it back unchanged
    ///     — replaces the JSON-wrapped envelope at the wire layer for
    ///     a 5–15% token saving on large files.
    ///   - **`pdf`** → page-headed extracted text (already formatted in
    ///     `read_pdf`).
    ///   - **`file_unchanged`** → TS `FILE_UNCHANGED_STUB` system-reminder.
    ///   - **`notebook`** → a single Text part rendering of cells
    ///     (per-cell `--- Cell N (type) ---` header + source + outputs).
    ///     Image outputs in notebook cells are NOT promoted to
    ///     ImageBlocks at the renderer layer in this Phase — TS does
    ///     image-aware merging via `mapNotebookCellsToToolResult`;
    ///     porting that is a follow-up. Most notebook cells are text.
    ///
    /// Anything else (synthetic `file_unchanged` / placeholder
    /// branches that already produce a `text` envelope) falls through
    /// to the default JSON-stringify via the trait's default impl.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
        let file = data.get("file");
        match kind {
            "image" => {
                let base64 = file
                    .and_then(|f| f.get("base64"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let media_type = file
                    .and_then(|f| f.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or("application/octet-stream");
                vec![ToolResultContentPart::FileData {
                    data: base64.to_string(),
                    media_type: media_type.to_string(),
                    filename: None,
                    provider_options: None,
                }]
            }
            "text" | "pdf" => {
                let content = file
                    .and_then(|f| f.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                // TS `mapToolResultToToolResultBlockParam` 'text' branch: when
                // `data.file.content` is empty, emit a `<system-reminder>`
                // warning instead of empty text (only for `text`, not `pdf`).
                let text = if content.is_empty() && kind == "text" {
                    let total_lines = file
                        .and_then(|f| f.get("totalLines"))
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    let start_line = file
                        .and_then(|f| f.get("startLine"))
                        .and_then(Value::as_i64)
                        .unwrap_or(1);
                    if total_lines == 0 {
                        "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>".to_string()
                    } else {
                        format!(
                            "<system-reminder>Warning: the file exists but is shorter than the provided offset ({start_line}). The file has {total_lines} lines.</system-reminder>"
                        )
                    }
                } else {
                    content
                };
                vec![ToolResultContentPart::Text {
                    text,
                    provider_options: None,
                }]
            }
            "file_unchanged" => {
                // TS `FileReadTool/prompt.ts::FILE_UNCHANGED_STUB`. Bare
                // text — TS does NOT wrap in `<system-reminder>`.
                vec![ToolResultContentPart::Text {
                    text: "File unchanged since last read. The content from the earlier Read tool_result in this conversation is still current — refer to that instead of re-reading.".to_string(),
                    provider_options: None,
                }]
            }
            "notebook" => render_notebook_cells(data, file),
            _ => vec![ToolResultContentPart::Text {
                text: serde_json::to_string(data).unwrap_or_default(),
                provider_options: None,
            }],
        }
    }
}

/// Render notebook cells as TS-shaped multi-block content. TS
/// `notebook.ts::cellContentToToolResult` + `cellOutputToToolResult`:
///
/// - Cell content: `<cell id="X">[<cell_type>Y</cell_type>][<language>Z</language>]source</cell id="X">`
///   (cell_type tag only when `!= 'code'`; language tag only when code
///   AND `!= 'python'`)
/// - Each output as a separate block:
///   - text → Text part with leading `\n`
///   - image → FileData part (multimodal pass-through to providers
///     that support it; degraded with marker by OpenAI Chat / Compat)
///
/// Final pass folds adjacent Text parts into one (joined with `'\n'`)
/// to mirror TS `notebook.ts:198-213` `allResults.reduce` — keeps the
/// wire payload tight and matches the TS shape that providers expect.
/// Image parts break the chain.
fn render_notebook_cells(data: &Value, file: Option<&Value>) -> Vec<ToolResultContentPart> {
    let Some(cells) = file.and_then(|f| f.get("cells")).and_then(Value::as_array) else {
        return vec![ToolResultContentPart::Text {
            text: serde_json::to_string(data).unwrap_or_default(),
            provider_options: None,
        }];
    };
    let mut parts: Vec<ToolResultContentPart> = Vec::new();
    for cell in cells {
        let cell_type = cell
            .get("cellType")
            .and_then(Value::as_str)
            .unwrap_or("code");
        let cell_id = cell.get("cell_id").and_then(Value::as_str).unwrap_or("");
        let language = cell.get("language").and_then(Value::as_str).unwrap_or("");
        let source = cell.get("source").and_then(Value::as_str).unwrap_or("");

        let mut metadata = String::new();
        if cell_type != "code" {
            metadata.push_str(&format!("<cell_type>{cell_type}</cell_type>"));
        }
        if cell_type == "code" && !language.is_empty() && language != "python" {
            metadata.push_str(&format!("<language>{language}</language>"));
        }
        let cell_text =
            format!("<cell id=\"{cell_id}\">{metadata}{source}</cell id=\"{cell_id}\">");
        parts.push(ToolResultContentPart::Text {
            text: cell_text,
            provider_options: None,
        });

        if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
            for out in outputs {
                if let Some(text) = out.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    parts.push(ToolResultContentPart::Text {
                        text: format!("\n{text}"),
                        provider_options: None,
                    });
                }
                if let Some(image) = out.get("image") {
                    let image_data = image
                        .get("image_data")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let media_type = image
                        .get("media_type")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png");
                    if !image_data.is_empty() {
                        parts.push(ToolResultContentPart::FileData {
                            data: image_data.to_string(),
                            media_type: media_type.to_string(),
                            filename: None,
                            provider_options: None,
                        });
                    }
                }
            }
        }
    }
    merge_adjacent_text_parts(parts)
}

/// Fold runs of adjacent [`ToolResultContentPart::Text`] entries into a
/// single Text part joined by `'\n'`. Mirrors TS `notebook.ts:198-213`
/// `allResults.reduce(...)` — image parts break the chain so the
/// caller-provided ordering is preserved.
fn merge_adjacent_text_parts(parts: Vec<ToolResultContentPart>) -> Vec<ToolResultContentPart> {
    let mut out: Vec<ToolResultContentPart> = Vec::with_capacity(parts.len());
    for part in parts {
        if let ToolResultContentPart::Text {
            text: ref curr_text,
            provider_options: None,
        } = part
            && let Some(ToolResultContentPart::Text {
                text: prev_text,
                provider_options: None,
            }) = out.last_mut()
        {
            prev_text.push('\n');
            prev_text.push_str(curr_text);
            continue;
        }
        out.push(part);
    }
    out
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
        display_data: None,
        source: None,
    })?;

    let notebook: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ToolError::ExecutionFailed {
            message: format!("invalid notebook JSON: {e}"),
            display_data: None,
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
            display_data: None,
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
        permission_updates: Vec::new(),
        display_data: None,
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
///    `${ename}: ${evalue}\n${traceback}`)
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
                display_data: None,
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
            display_data: None,
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
            display_data: None,
            source: None,
        })?
        .map_err(|e| ToolError::ExecutionFailed {
            message: e,
            display_data: None,
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
        permission_updates: Vec::new(),
        display_data: None,
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
        display_data: None,
        source: None,
    })?;

    // `pdf-extract` prefers a byte slice; extracting text from `bytes`
    // returns the whole document joined with form-feed (`\x0C`)
    // separators between pages, which is how `pdftotext` encodes page
    // breaks too. We split on that separator to get one-page-per-entry.
    let full_text =
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to extract PDF text from {file_path}: {e}"),
            display_data: None,
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
            permission_updates: Vec::new(),
            display_data: None,
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
        permission_updates: Vec::new(),
        display_data: None,
    })
}

/// Parse a `pages` spec like `"3"` or `"1-5"` into a 1-based
/// `(start, end)` range. Returns `None` on parse error.
///
/// TS: `utils/pdfUtils.ts::parsePDFPageRange`.
/// Pure parse of the PDF `pages` spec (no I/O, no `total`), mirroring TS
/// `parsePDFPageRange` (`utils/pdfUtils.ts`). Returns `(first, last)`
/// where `last` is `None` for the open-ended `"N-"` form, and `None` for
/// a malformed spec. Used by `validate_input` for the up-front 7/8
/// error-code checks.
fn parse_pdf_page_range_spec(pages: &str) -> Option<(usize, Option<usize>)> {
    let trimmed = pages.trim();
    if trimmed.is_empty() {
        return None;
    }
    // "N-" open-ended range.
    if let Some(prefix) = trimmed.strip_suffix('-') {
        let first: usize = prefix.trim().parse().ok()?;
        if first < 1 {
            return None;
        }
        return Some((first, None));
    }
    match trimmed.split_once('-') {
        None => {
            let page: usize = trimmed.parse().ok()?;
            if page < 1 {
                return None;
            }
            Some((page, Some(page)))
        }
        Some((a, b)) => {
            let first: usize = a.trim().parse().ok()?;
            let last: usize = b.trim().parse().ok()?;
            if first < 1 || last < 1 || last < first {
                return None;
            }
            Some((first, Some(last)))
        }
    }
}

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
