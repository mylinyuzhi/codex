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
        "Writes a file to the local filesystem.".into()
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

        // Track file edit for checkpoint/rewind before modifying.
        // TS: FileWriteTool.ts line 259
        crate::track_file_edit(ctx, path).await;

        // Read existing content for diff generation (if updating)
        let old_content = if !is_new {
            std::fs::read_to_string(file_path).ok()
        } else {
            None
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

        std::fs::write(file_path, content).map_err(|e| ToolError::ExecutionFailed {
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

        Ok(ToolResult {
            data: serde_json::json!(msg),
            new_messages: vec![],
        })
    }
}

#[cfg(test)]
#[path = "write.test.rs"]
mod tests;
