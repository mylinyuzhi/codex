//! BriefTool — sends structured messages to the user with optional file attachments.
//!
//! TS: `tools/BriefTool/BriefTool.ts`. Status distinguishes normal
//! replies from proactive (unsolicited) updates so the UI can render
//! them differently.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;
use std::collections::HashMap;

pub struct BriefTool;

#[async_trait::async_trait]
impl Tool for BriefTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Brief)
    }
    fn name(&self) -> &str {
        ToolName::Brief.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Send a structured message to the user with optional file attachments.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "message".into(),
            serde_json::json!({"type": "string", "description": "Markdown-formatted message to the user"}),
        );
        p.insert(
            "attachments".into(),
            serde_json::json!({"type": "array", "items": {"type": "string"}, "description": "File paths (absolute or relative to cwd) to attach"}),
        );
        p.insert(
            "status".into(),
            serde_json::json!({"type": "string", "enum": ["normal", "proactive"], "description": "Message intent: 'normal' for direct replies, 'proactive' for unsolicited updates"}),
        );
        ToolInputSchema {
            properties: p,
            required: Vec::new(),
        }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    /// TS `BriefTool.ts`: `isConcurrencySafe() { return true }`. Brief
    /// messages are a side-channel to the user — multiple briefs in the
    /// same turn are independent and stamped with their own timestamps.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    /// TS parity: `BriefTool.ts::mapToolResultToToolResultBlockParam`.
    /// The model only needs the delivery confirmation; the message body
    /// + attachments + timestamp are TUI/state concerns and would waste
    /// tokens if JSON-stringified for the model.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let n = data
            .get("attachments")
            .and_then(Value::as_array)
            .map_or(0, std::vec::Vec::len);
        let suffix = if n == 0 {
            String::new()
        } else if n == 1 {
            " (1 attachment included)".to_string()
        } else {
            format!(" ({n} attachments included)")
        };
        vec![ToolResultContentPart::Text {
            text: format!("Message delivered to user.{suffix}"),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if message.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "message parameter is required".into(),
                error_code: None,
            });
        }

        let status = input
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("normal");

        // Resolve attachments. Relative paths resolve against the
        // context cwd override (worktree-isolated subagents) before
        // falling back to the process cwd, so a teammate inside a
        // worktree sees its own files rather than the host process's.
        let resolve_root = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut resolved_attachments: Vec<Value> = Vec::new();
        if let Some(attachments) = input.get("attachments").and_then(|v| v.as_array()) {
            for attachment in attachments {
                if let Some(path_str) = attachment.as_str() {
                    let path = if std::path::Path::new(path_str).is_absolute() {
                        std::path::PathBuf::from(path_str)
                    } else {
                        resolve_root.join(path_str)
                    };

                    let meta = tokio::fs::metadata(&path).await;
                    let exists = meta.is_ok();
                    let size = meta.as_ref().map(std::fs::Metadata::len).unwrap_or(0);
                    let is_image = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|ext| {
                            matches!(
                                ext.to_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
                            )
                        });

                    resolved_attachments.push(serde_json::json!({
                        "path": path.display().to_string(),
                        "exists": exists,
                        "size": size,
                        "is_image": is_image,
                    }));
                }
            }
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();

        Ok(ToolResult {
            data: serde_json::json!({
                "message": message,
                "status": status,
                "attachments": resolved_attachments,
                "timestamp": timestamp,
            }),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}
