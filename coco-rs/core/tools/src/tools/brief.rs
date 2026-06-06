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
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Message intent — controls how the UI renders the brief.
///
/// TS parity: `BriefTool.ts` `status: z.enum(['normal', 'proactive'])`.
/// Wire format stays lowercase (matches TS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BriefStatus {
    /// Direct reply to a user request.
    #[default]
    Normal,
    /// Unsolicited update (proactive surfacing).
    Proactive,
}

/// Typed input for [`BriefTool`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BriefInput {
    /// Markdown-formatted message to the user
    pub message: String,
    /// File paths (absolute or relative to cwd) to attach
    #[serde(default)]
    pub attachments: Vec<String>,
    /// Message intent: `normal` for direct replies, `proactive` for
    /// unsolicited updates
    #[serde(default)]
    pub status: BriefStatus,
}

/// Per-attachment metadata returned by [`BriefTool::execute`].
///
/// The shape stays close to the legacy `serde_json::json!({...})`
/// envelope so transcript replay across the migration boundary
/// round-trips without surprises.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefAttachment {
    pub path: String,
    pub exists: bool,
    /// File size in bytes. Cast from `u64` (`std::fs::Metadata::len`)
    /// per the project's `i64`/`u64` convention; realistic file
    /// sizes never approach `i64::MAX` (~9 EiB).
    pub size: i64,
    pub is_image: bool,
}

/// Typed output for [`BriefTool::execute`]. Mirrors the legacy JSON
/// envelope (`message`/`status`/`attachments`/`timestamp`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefOutput {
    pub message: String,
    pub status: BriefStatus,
    pub attachments: Vec<BriefAttachment>,
    /// Millisecond Unix timestamp captured at delivery time, encoded
    /// as a decimal string (mirrors TS where JS numbers can't safely
    /// represent ms-precision Unix timestamps).
    pub timestamp: String,
}

pub struct BriefTool;

#[async_trait::async_trait]
impl Tool for BriefTool {
    type Input = BriefInput;
    coco_tool_runtime::impl_runtime_schema!(BriefInput);
    type Output = BriefOutput;

    fn to_auto_classifier_input(&self, input: &BriefInput) -> Option<String> {
        Some(input.message.clone())
    }

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Brief)
    }
    fn name(&self) -> &str {
        ToolName::Brief.as_str()
    }
    fn description(&self, _input: &BriefInput, _options: &DescriptionOptions) -> String {
        "Send a structured message to the user with optional file attachments.".into()
    }

    fn is_read_only(&self, _input: &BriefInput) -> bool {
        true
    }
    /// Side-channel UI delivery — semantically read-only regardless of
    /// input shape; Plan mode keeps Brief visible.
    fn is_always_read_only(&self) -> bool {
        true
    }

    /// TS `BriefTool.ts`: `isConcurrencySafe() { return true }`. Brief
    /// messages are a side-channel to the user — multiple briefs in the
    /// same turn are independent and stamped with their own timestamps.
    fn is_concurrency_safe(&self, _input: &BriefInput) -> bool {
        true
    }

    /// TS parity: `BriefTool.ts::mapToolResultToToolResultBlockParam`.
    /// The model only needs the delivery confirmation; the message body
    /// + attachments + timestamp are TUI/state concerns and would waste
    /// tokens if JSON-stringified for the model.
    fn render_for_model(&self, out: &BriefOutput) -> Vec<ToolResultContentPart> {
        let suffix = match out.attachments.len() {
            0 => String::new(),
            1 => " (1 attachment included)".to_string(),
            n => format!(" ({n} attachments included)"),
        };
        vec![ToolResultContentPart::Text {
            text: format!("Message delivered to user.{suffix}"),
            provider_options: None,
        }]
    }

    /// #48 / TS `BriefTool.ts:163-168` → `validateAttachmentPaths`
    /// (attachments.ts:26-61): reject non-existent / not-a-regular-file /
    /// inaccessible attachment paths up-front (errorCode 1) so the model
    /// self-corrects instead of receiving a false success.
    fn validate_input(
        &self,
        input: &BriefInput,
        ctx: &ToolUseContext,
    ) -> coco_tool_runtime::ValidationResult {
        use coco_tool_runtime::ValidationResult;
        let resolve_root = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        for raw in &input.attachments {
            let path = if std::path::Path::new(raw).is_absolute() {
                std::path::PathBuf::from(raw)
            } else {
                resolve_root.join(raw)
            };
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    if !meta.is_file() {
                        return ValidationResult::invalid_with_code(
                            format!("Attachment \"{raw}\" is not a regular file."),
                            "1",
                        );
                    }
                }
                Err(e) => {
                    let msg = match e.kind() {
                        std::io::ErrorKind::NotFound => format!(
                            "Attachment \"{raw}\" does not exist. \
                             Current working directory: {}.",
                            resolve_root.display()
                        ),
                        std::io::ErrorKind::PermissionDenied => {
                            format!("Attachment \"{raw}\" is not accessible (permission denied).")
                        }
                        _ => format!("Attachment \"{raw}\" is not accessible ({e})."),
                    };
                    return ValidationResult::invalid_with_code(msg, "1");
                }
            }
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: BriefInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<BriefOutput>, ToolError> {
        // Defensive empty-string guard. Pre-typed-migration this caught
        // omitted `message`; the typed `Input` now rejects missing
        // fields at deserialize time so this only catches the
        // explicitly-empty `""` case (TS parity — zod `z.string()`
        // accepts empty strings unless `.min(1)` is used).
        if input.message.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "message parameter is required".into(),
                error_code: None,
            });
        }

        // Resolve attachments. Relative paths resolve against the
        // context cwd override (worktree-isolated subagents) before
        // falling back to the process cwd, so a teammate inside a
        // worktree sees its own files rather than the host process's.
        let resolve_root = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();

        let mut resolved_attachments: Vec<BriefAttachment> = Vec::new();
        for path_str in &input.attachments {
            let path = if std::path::Path::new(path_str).is_absolute() {
                std::path::PathBuf::from(path_str)
            } else {
                resolve_root.join(path_str)
            };

            let meta = tokio::fs::metadata(&path).await;
            let exists = meta.is_ok();
            let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
            let is_image = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
                    )
                });

            resolved_attachments.push(BriefAttachment {
                path: path.display().to_string(),
                exists,
                size,
                is_image,
            });
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();

        Ok(ToolResult {
            data: BriefOutput {
                message: input.message,
                status: input.status,
                attachments: resolved_attachments,
                timestamp,
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}
