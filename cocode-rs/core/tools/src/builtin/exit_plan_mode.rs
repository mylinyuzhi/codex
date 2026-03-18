//! ExitPlanMode tool for finalizing plan and requesting approval.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_plan_mode::PlanFileManager;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for exiting plan mode.
///
/// Signals that the plan is complete and ready for user review and approval.
/// Returns the plan content read from the plan file.
///
/// `check_permission()` returns `NeedsApproval` so the TUI shows an approval
/// overlay. If the user approves, `execute()` runs and marks `approved=true`.
/// If the user denies, the tool never executes and plan mode stays active.
pub struct ExitPlanModeTool;

impl ExitPlanModeTool {
    /// Create a new ExitPlanMode tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExitPlanModeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::ExitPlanMode.as_str()
    }

    fn description(&self) -> &str {
        prompts::EXIT_PLAN_MODE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "allowedPrompts": {
                    "type": "array",
                    "description": "Prompt-based permissions needed to implement the plan",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "The tool this prompt applies to"
                            },
                            "prompt": {
                                "type": "string",
                                "description": "Semantic description of the action"
                            }
                        },
                        "required": ["tool", "prompt"]
                    }
                }
            },
            "additionalProperties": true
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    /// Requires user approval before exiting plan mode.
    ///
    /// The TUI shows an approval overlay where the user can:
    /// - Approve: proceed with implementation (tool executes)
    /// - Deny: stay in plan mode with feedback (tool does not execute)
    async fn check_permission(&self, _input: &Value, ctx: &ToolContext) -> PermissionResult {
        // Build plan preview for the approval dialog
        let manager = PlanFileManager::new(&ctx.session_id);
        let plan_preview = manager.read().unwrap_or_default();
        let truncated = if plan_preview.len() > 2000 {
            // Find a valid UTF-8 char boundary at or before byte 2000
            let end = plan_preview.floor_char_boundary(2000);
            format!("{}...\n\n(truncated)", &plan_preview[..end])
        } else {
            plan_preview
        };

        let description = format!("Exit plan mode?\n\n## Implementation Plan\n\n{truncated}");

        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: ctx.call_id.clone(),
                tool_name: cocode_protocol::ToolName::ExitPlanMode.as_str().to_string(),
                description,
                risks: Vec::new(),
                allow_remember: false,
                proposed_prefix_pattern: None,
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        // If we reach execute(), the user has approved the plan
        ctx.emit_progress("Plan approved - exiting plan mode").await;

        // Extract allowedPrompts from input (prompt-based permission declarations)
        let allowed_prompts = input
            .get("allowedPrompts")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let tool = item.get("tool")?.as_str()?;
                        let prompt = item.get("prompt")?.as_str()?;
                        Some(serde_json::json!({ "tool": tool, "prompt": prompt }))
                    })
                    .collect::<Vec<_>>()
            });

        // Create plan file manager.
        // Subagent filtering (SYSTEM_BLOCKED) prevents subagents from calling
        // this tool, so we always use the main agent path.
        let manager = PlanFileManager::new(&ctx.session_id);

        // Get plan file path and content
        let plan_path = Some(manager.path());
        let plan_content = manager.read();

        // Log plan submission
        tracing::info!(
            session_id = %ctx.session_id,
            has_plan = plan_content.is_some(),
            allowed_prompts_count = allowed_prompts.as_ref().map_or(0, Vec::len),
            "Plan mode exited (approved)"
        );

        // Emit plan mode exit event with approved=true
        ctx.emit_event(cocode_protocol::LoopEvent::PlanModeExited { approved: true })
            .await;

        // Return structured response with plan content and allowed prompts
        let response = serde_json::json!({
            "plan": plan_content,
            "filePath": plan_path.map(|p| p.display().to_string()),
            "approved": true,
            "allowedPrompts": allowed_prompts
        });

        Ok(ToolOutput::structured(response))
    }
}

#[cfg(test)]
#[path = "exit_plan_mode.test.rs"]
mod tests;
