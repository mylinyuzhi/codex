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

/// Default number of lines to read if no limit specified.
const DEFAULT_LINE_LIMIT: usize = 2000;

/// Known image extensions (returned as "[image file]" placeholder).
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "tiff", "tif",
];

/// Known binary extensions that should not be read as text.
const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "o", "a", "bin", "class", "pyc", "pyo", "wasm", "zip", "tar",
    "gz", "bz2", "xz", "7z", "rar", "mp3", "mp4", "wav", "avi", "mov", "mkv", "flv", "ttf", "otf",
    "woff", "woff2", "eot", "sqlite", "db",
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
        "Reads a file from the local filesystem.".into()
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
        props.insert(
            "offset".into(),
            serde_json::json!({
                "type": "number",
                "description": "The line number to start reading from (0-indexed)",
                "minimum": 0
            }),
        );
        props.insert(
            "limit".into(),
            serde_json::json!({
                "type": "number",
                "description": "The number of lines to read",
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

        // Check extension for special file types
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();

            if IMAGE_EXTENSIONS.contains(&ext_lower.as_str()) {
                crate::record_file_read(ctx, path, String::new(), None, None).await;
                return Ok(ToolResult {
                    data: serde_json::json!("[image file — use multimodal API to view]"),
                    new_messages: vec![],
                });
            }

            if ext_lower == "ipynb" {
                return read_notebook(file_path);
            }

            if ext_lower == "pdf" {
                crate::record_file_read(ctx, path, String::new(), None, None).await;
                let pages = input.get("pages").and_then(|v| v.as_str());
                return read_pdf(file_path, pages);
            }

            // Binary files
            if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return Ok(ToolResult {
                    data: serde_json::json!(format!("[binary file: {ext_lower}]")),
                    new_messages: vec![],
                });
            }
        }

        // Read file content
        let content =
            std::fs::read_to_string(file_path).map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to read {file_path}: {e}"),
                source: None,
            })?;

        // Check empty
        if content.is_empty() {
            return Ok(ToolResult {
                data: serde_json::json!("[file is empty]"),
                new_messages: vec![],
            });
        }

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let offset = input
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LINE_LIMIT);

        // Clamp offset
        let start = offset.min(total_lines);
        let end = (start + limit).min(total_lines);

        // Format as cat -n (line numbers starting at 1)
        let mut output = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1; // 1-indexed
            output.push_str(&format!("{line_num}\t{line}\n"));
        }

        // Append info if truncated
        if end < total_lines {
            output.push_str(&format!(
                "\n... ({} more lines not shown. Use offset/limit to read more.)",
                total_lines - end
            ));
        }

        let offset_i32 = if offset > 0 {
            Some(offset as i32)
        } else {
            None
        };
        let limit_i32 = if end < total_lines {
            Some((end - start) as i32)
        } else {
            None
        };
        crate::record_file_read(ctx, path, content, offset_i32, limit_i32).await;

        Ok(ToolResult {
            data: serde_json::json!(output),
            new_messages: vec![],
        })
    }
}

/// Read a Jupyter notebook (.ipynb) and format as text.
///
/// TS: FileReadTool handles .ipynb by parsing JSON and extracting cells.
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

    let cells = notebook
        .get("cells")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "notebook has no cells array".into(),
            source: None,
        })?;

    let mut output = String::new();
    for (i, cell) in cells.iter().enumerate() {
        let cell_type = cell
            .get("cell_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let source = cell
            .get("source")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        output.push_str(&format!("--- Cell {} ({cell_type}) ---\n", i + 1));
        output.push_str(&source);
        if !source.ends_with('\n') {
            output.push('\n');
        }

        // Include outputs for code cells
        if cell_type == "code" {
            if let Some(outputs) = cell.get("outputs").and_then(|v| v.as_array()) {
                for out in outputs {
                    if let Some(text) = out.get("text").and_then(|v| v.as_array()) {
                        output.push_str("[Output]\n");
                        for line in text {
                            if let Some(s) = line.as_str() {
                                output.push_str(s);
                            }
                        }
                    } else if let Some(data) = out.get("data") {
                        if let Some(text) = data.get("text/plain").and_then(|v| v.as_array()) {
                            output.push_str("[Output]\n");
                            for line in text {
                                if let Some(s) = line.as_str() {
                                    output.push_str(s);
                                }
                            }
                        }
                    }
                }
            }
        }
        output.push('\n');
    }

    Ok(ToolResult {
        data: serde_json::json!(output),
        new_messages: vec![],
    })
}

/// Read a PDF file and return text content.
///
/// TS: FileReadTool handles .pdf with page range support.
/// Note: Full PDF parsing requires a dependency like pdf-extract.
/// For now, returns a helpful message about the file.
fn read_pdf(file_path: &str, pages: Option<&str>) -> Result<ToolResult<Value>, ToolError> {
    let metadata = std::fs::metadata(file_path).map_err(|e| ToolError::ExecutionFailed {
        message: format!("failed to read PDF: {e}"),
        source: None,
    })?;

    let size_kb = metadata.len() / 1024;
    let page_info = pages
        .map(|p| format!(" (requested pages: {p})"))
        .unwrap_or_default();

    Ok(ToolResult {
        data: serde_json::json!(format!(
            "[PDF file: {file_path} ({size_kb}KB){page_info}]\n\
             PDF text extraction requires the pdf-extract crate.\n\
             Use Bash with pdftotext or similar to extract content."
        )),
        new_messages: vec![],
    })
}

#[cfg(test)]
#[path = "read.test.rs"]
mod tests;
