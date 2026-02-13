//! ExitPlanMode tool for finalizing plan and requesting approval.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_plan_mode::PlanFileManager;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for exiting plan mode.
///
/// Signals that the plan is complete and ready for user review and approval.
/// Returns the plan content read from the plan file.
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
        "ExitPlanMode"
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
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, _input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        ctx.emit_progress("Exiting plan mode - awaiting approval")
            .await;

        let is_agent = ctx.agent_id.is_some();

        // Create plan file manager
        let manager = match ctx.agent_id.as_ref() {
            Some(agent_id) => PlanFileManager::for_agent(&ctx.session_id, agent_id),
            None => PlanFileManager::new(&ctx.session_id),
        };

        // Get plan file path and content
        let plan_path = Some(manager.path());
        let plan_content = manager.read();

        // Log plan submission
        tracing::info!(
            session_id = %ctx.session_id,
            is_agent = is_agent,
            has_plan = plan_content.is_some(),
            "Plan mode exited"
        );

        // Emit plan mode exit event
        ctx.emit_event(cocode_protocol::LoopEvent::PlanModeExited { approved: false })
            .await;

        // Return structured response with plan content (aligned with Claude Code)
        let response = serde_json::json!({
            "plan": plan_content,
            "isAgent": is_agent,
            "filePath": plan_path.map(|p| p.display().to_string())
        });

        Ok(ToolOutput::structured(response))
    }
}

#[cfg(test)]
#[path = "exit_plan_mode.test.rs"]
mod tests;
