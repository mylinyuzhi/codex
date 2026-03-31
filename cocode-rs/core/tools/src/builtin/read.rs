//! Read tool for reading file contents.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ContextModifier;
use cocode_protocol::PermissionResult;
use cocode_protocol::RiskSeverity;
use cocode_protocol::RiskType;
use cocode_protocol::SecurityRisk;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use tokio::fs;

/// Tool for reading file contents.
///
/// This is a safe tool that can run concurrently with other tools.
pub struct ReadTool {
    /// Maximum file size to read (bytes).
    max_file_size: i64,
    /// Maximum lines to read.
    max_lines: i32,
}

impl ReadTool {
    /// Create a new Read tool with default settings.
    pub fn new() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10 MB
            max_lines: 2000,
        }
    }

    /// Set the maximum file size.
    pub fn with_max_file_size(mut self, size: i64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set the maximum lines.
    pub fn with_max_lines(mut self, lines: i32) -> Self {
        self.max_lines = lines;
        self
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Image file extensions supported for base64 encoding.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"];

/// PDF file extension.
const PDF_EXTENSION: &str = "pdf";

/// Jupyter notebook extension.
const NOTEBOOK_EXTENSION: &str = "ipynb";

/// Check if a path has an image extension.
fn is_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

/// Check if a path is a PDF file.
fn is_pdf_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PDF_EXTENSION))
}

/// Check if a path is a Jupyter notebook.
fn is_notebook_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTEBOOK_EXTENSION))
}

/// Check if a path is a device file (e.g. /dev/null, /dev/zero).
fn is_device_file(path: &std::path::Path) -> bool {
    path.starts_with("/dev/")
}

/// Check first bytes of a file for null bytes indicating binary content.
fn has_null_bytes(data: &[u8]) -> bool {
    data.contains(&0)
}

/// Format a Jupyter notebook (.ipynb) as human-readable text.
fn format_notebook(content: &str, path: &std::path::Path) -> std::result::Result<String, String> {
    let notebook: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid notebook JSON: {e}"))?;

    let cells = notebook["cells"]
        .as_array()
        .ok_or("Invalid notebook: missing 'cells' array")?;

    let mut output = format!("Jupyter Notebook: {}\n", path.display());
    output.push_str(&format!("Cells: {}\n\n", cells.len()));

    for (i, cell) in cells.iter().enumerate() {
        let cell_type = cell["cell_type"].as_str().unwrap_or("unknown");
        let execution_count = cell["execution_count"].as_i64();

        // Cell header
        match (cell_type, execution_count) {
            ("code", Some(n)) => {
                output.push_str(&format!("--- Cell {} [code] In [{}] ---\n", i + 1, n))
            }
            _ => output.push_str(&format!("--- Cell {} [{}] ---\n", i + 1, cell_type)),
        }

        // Cell source
        if let Some(source) = cell["source"].as_array() {
            for line in source {
                if let Some(s) = line.as_str() {
                    output.push_str(s);
                }
            }
            if !source.is_empty() {
                output.push('\n');
            }
        } else if let Some(source) = cell["source"].as_str() {
            output.push_str(source);
            output.push('\n');
        }

        // Cell outputs (for code cells)
        if cell_type == "code"
            && let Some(outputs) = cell["outputs"].as_array()
        {
            for out in outputs {
                let output_type = out["output_type"].as_str().unwrap_or("");
                match output_type {
                    "stream" => {
                        if let Some(text) = out["text"].as_array() {
                            output.push_str("\n[Output]\n");
                            for line in text {
                                if let Some(s) = line.as_str() {
                                    output.push_str(s);
                                }
                            }
                        }
                    }
                    "execute_result" | "display_data" => {
                        if let Some(data) = out["data"].as_object() {
                            if let Some(text) = data.get("text/plain") {
                                output.push_str("\n[Output]\n");
                                if let Some(arr) = text.as_array() {
                                    for line in arr {
                                        if let Some(s) = line.as_str() {
                                            output.push_str(s);
                                        }
                                    }
                                } else if let Some(s) = text.as_str() {
                                    output.push_str(s);
                                }
                            }
                            if data.contains_key("image/png") || data.contains_key("image/jpeg") {
                                output.push_str("\n[Image output]\n");
                            }
                        }
                    }
                    "error" => {
                        let ename = out["ename"].as_str().unwrap_or("Error");
                        let evalue = out["evalue"].as_str().unwrap_or("");
                        output.push_str(&format!("\n[Error: {ename}: {evalue}]\n"));
                    }
                    _ => {}
                }
            }
        }

        output.push('\n');
    }

    Ok(output)
}

/// Get MIME type for an image extension.
fn image_mime_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Get the total page count of a PDF via `pdfinfo`.
///
/// Returns `None` if `pdfinfo` is not available or output cannot be parsed.
async fn get_pdf_page_count(path: &std::path::Path) -> Option<u32> {
    let output = tokio::process::Command::new("pdfinfo")
        .arg(path)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Parse a page range string (e.g., "3-15") and return the number of pages spanned.
///
/// Returns `None` for malformed input.
fn parse_page_span(range: &str) -> Option<u32> {
    let parts: Vec<&str> = range.split('-').collect();
    match parts.len() {
        1 => {
            // Single page — must be a valid number
            let _page: u32 = parts[0].trim().parse().ok()?;
            Some(1)
        }
        2 => {
            let first: u32 = parts[0].trim().parse().ok()?;
            let last: u32 = parts[1].trim().parse().ok()?;
            Some(last.saturating_sub(first) + 1)
        }
        _ => None,
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Read.as_str()
    }

    fn description(&self) -> &str {
        prompts::READ_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range for PDF files (e.g., \"1-5\", \"3\", \"10-20\"). Only applicable to PDF files. Maximum 20 pages per request."
                }
            },
            "required": ["file_path"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn max_result_size_chars(&self) -> i32 {
        100_000
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(fp) => fp,
            None => return PermissionResult::Passthrough,
        };

        let path = ctx.resolve_path(file_path);

        // Locked directory → Deny
        if crate::sensitive_files::is_locked_directory(&path) {
            return PermissionResult::Denied {
                reason: format!(
                    "Reading locked directory is not allowed: {}",
                    path.display()
                ),
            };
        }

        // Sensitive file → NeedsApproval
        if crate::sensitive_files::is_sensitive_file(&path) {
            return PermissionResult::NeedsApproval {
                request: ApprovalRequest {
                    request_id: format!("sensitive-read-{}", path.display()),
                    tool_name: self.name().to_string(),
                    description: format!("Reading sensitive file: {}", path.display()),
                    risks: vec![SecurityRisk {
                        risk_type: RiskType::SensitiveFile,
                        severity: RiskSeverity::Medium,
                        message: format!(
                            "File '{}' may contain credentials or sensitive configuration",
                            path.display()
                        ),
                    }],
                    allow_remember: true,
                    proposed_prefix_pattern: None,
                    input: Some(input.clone()),
                    source_agent_id: ctx.identity.agent_id.clone(),
                },
            };
        }

        // Outside working directory → NeedsApproval
        if crate::sensitive_files::is_outside_cwd(&path, &ctx.env.cwd) {
            return PermissionResult::NeedsApproval {
                request: ApprovalRequest {
                    request_id: format!("outside-cwd-read-{}", path.display()),
                    tool_name: self.name().to_string(),
                    description: format!(
                        "Reading file outside working directory: {}",
                        path.display()
                    ),
                    risks: vec![],
                    allow_remember: true,
                    proposed_prefix_pattern: None,
                    input: Some(input.clone()),
                    source_agent_id: ctx.identity.agent_id.clone(),
                },
            };
        }

        // In working directory → Allowed
        PermissionResult::Allowed
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let file_path = input["file_path"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "file_path must be a string",
            }
            .build()
        })?;

        let offset = input["offset"].as_i64().unwrap_or(0);
        let limit = input["limit"].as_i64().unwrap_or(self.max_lines as i64);

        // Resolve path
        let path = ctx.resolve_path(file_path);

        // Block device files (/dev/null, /dev/zero, etc.) — reading these can hang
        if is_device_file(&path) {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: "Cannot read device files".to_string(),
            }
            .build());
        }

        // Check if file exists
        if !path.exists() {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("File not found: {}", path.display()),
            }
            .build());
        }

        // Handle image files — return as image content block via ToolOutput.images
        // so the LLM receives proper multimodal vision content.
        if is_image_file(&path) {
            let bytes = fs::read(&path).await.map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to read image file: {e}"),
                }
                .build()
            })?;

            let mime = image_mime_type(&path);
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

            let output = format!(
                "Image file: {}\nSize: {} bytes\nType: {mime}",
                path.display(),
                bytes.len(),
            );

            ctx.record_file_read_with_state(
                &path,
                crate::context::FileReadState::metadata_only(
                    fs::metadata(&path)
                        .await
                        .ok()
                        .and_then(|m| m.modified().ok()),
                    ctx.identity.turn_number,
                ),
            )
            .await;

            return Ok(ToolOutput {
                content: cocode_protocol::ToolResultContent::Text(output),
                is_error: false,
                modifiers: Vec::new(),
                images: vec![cocode_protocol::ImageData {
                    data: b64,
                    media_type: mime.to_string(),
                }],
            });
        }

        // Handle PDF files — extract text or return info
        if is_pdf_file(&path) {
            let pages_param = input["pages"].as_str();

            // Get page count via pdfinfo to validate constraints
            let total_pages = get_pdf_page_count(&path).await;

            // If >10 pages and no explicit pages param, require it
            if let Some(count) = total_pages
                && count > 10
                && pages_param.is_none()
            {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!(
                        "PDF has {count} pages (>10). You must provide the `pages` parameter \
                         to specify which pages to read (e.g., \"1-5\"). Maximum 20 pages per request."
                    ),
                }
                .build());
            }

            // Validate requested page range does not exceed 20 pages
            let pages_arg = pages_param.unwrap_or("1-20");
            if let Some(span) = parse_page_span(pages_arg)
                && span > 20
            {
                return Err(crate::error::tool_error::InvalidInputSnafu {
                    message: format!(
                        "Requested {span} pages but maximum is 20 per request. \
                         Narrow your `pages` range."
                    ),
                }
                .build());
            }

            let output = match tokio::process::Command::new("pdftotext")
                .args([
                    "-f",
                    pages_arg.split('-').next().unwrap_or("1"),
                    "-l",
                    pages_arg.split('-').next_back().unwrap_or("20"),
                    path.to_str().unwrap_or(""),
                    "-",
                ])
                .output()
                .await
            {
                Ok(result) if result.status.success() => {
                    let text = String::from_utf8_lossy(&result.stdout);
                    format!(
                        "PDF file: {} (pages: {})\n\n{}",
                        path.display(),
                        pages_arg,
                        text,
                    )
                }
                _ => {
                    // Fallback: report file info without extraction
                    let metadata = fs::metadata(&path).await?;
                    format!(
                        "PDF file: {}\nSize: {} bytes\nPages requested: {}\n\n\
                         Note: pdftotext (poppler-utils) not available for text extraction. \
                         Install poppler-utils for PDF text extraction support.",
                        path.display(),
                        metadata.len(),
                        pages_arg,
                    )
                }
            };

            ctx.record_file_read_with_state(
                &path,
                crate::context::FileReadState::metadata_only(
                    fs::metadata(&path)
                        .await
                        .ok()
                        .and_then(|m| m.modified().ok()),
                    ctx.identity.turn_number,
                ),
            )
            .await;

            return Ok(ToolOutput::text(output));
        }

        // Handle Jupyter notebooks — parse .ipynb JSON into readable format
        if is_notebook_file(&path) {
            let raw = fs::read_to_string(&path).await.map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to read notebook file: {e}"),
                }
                .build()
            })?;

            let output = match format_notebook(&raw, &path) {
                Ok(formatted) => formatted,
                Err(e) => {
                    return Err(crate::error::tool_error::ExecutionFailedSnafu {
                        message: format!("Failed to parse notebook: {e}"),
                    }
                    .build());
                }
            };

            ctx.record_file_read_with_state(
                &path,
                crate::context::FileReadState::metadata_only(
                    fs::metadata(&path)
                        .await
                        .ok()
                        .and_then(|m| m.modified().ok()),
                    ctx.identity.turn_number,
                ),
            )
            .await;

            return Ok(ToolOutput::text(output));
        }

        // Check file size
        let metadata = fs::metadata(&path).await?;
        if metadata.len() as i64 > self.max_file_size {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "File too large: {} bytes (max: {} bytes)",
                    metadata.len(),
                    self.max_file_size
                ),
            }
            .build());
        }

        // Binary file detection — check first 8KB for null bytes
        {
            let mut file = tokio::fs::File::open(&path).await.map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to open file: {e}"),
                }
                .build()
            })?;
            let mut buf = vec![0u8; 8192.min(metadata.len() as usize)];
            use tokio::io::AsyncReadExt;
            let n = file.read(&mut buf).await.map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to read file: {e}"),
                }
                .build()
            })?;
            if has_null_bytes(&buf[..n]) {
                return Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!(
                        "File appears to be binary: {}. Use appropriate tools for binary files.",
                        path.display()
                    ),
                }
                .build());
            }
        }

        // Get file modification time for tracking
        let file_mtime = metadata.modified().ok();

        // Read file
        let content = fs::read_to_string(&path).await.map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to read file: {e}"),
            }
            .build()
        })?;

        // Apply offset and limit
        let lines: Vec<&str> = content.lines().collect();
        let start = offset.max(0) as usize;
        let end = (start + limit as usize).min(lines.len());
        let is_complete = start == 0 && end >= lines.len();

        // Format with line numbers (cat -n format)
        let mut output = String::new();
        for (idx, line) in lines[start..end].iter().enumerate() {
            let line_num = start + idx + 1;
            // Truncate lines > 2000 characters
            let truncated = if line.len() > 2000 {
                format!("{}...", &line[..line.floor_char_boundary(2000)])
            } else {
                line.to_string()
            };
            output.push_str(&format!("{line_num:>6}\t{truncated}\n"));
        }

        // Record file read with full state tracking
        use crate::context::FileReadState;
        let read_state = if is_complete {
            FileReadState::complete_with_turn(content.clone(), file_mtime, ctx.identity.turn_number)
        } else {
            FileReadState::partial_with_turn(offset, limit, file_mtime, ctx.identity.turn_number)
        };
        ctx.record_file_read_with_state(&path, read_state).await;

        // Register tool call ID to path mapping for compaction cleanup
        ctx.register_file_read_id(&path).await;

        // Create output with file read modifier
        let mut result = ToolOutput::text(output);

        // Convert mtime to milliseconds
        let file_mtime_ms = file_mtime.and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as i64)
        });

        if is_complete {
            result
                .modifiers
                .push(ContextModifier::file_read(path, content, file_mtime_ms));
        } else {
            result.modifiers.push(ContextModifier::file_read_partial(
                path,
                content,
                file_mtime_ms,
                offset,
                limit,
            ));
        }

        Ok(result)
    }
}

#[cfg(test)]
#[path = "read.test.rs"]
mod tests;
