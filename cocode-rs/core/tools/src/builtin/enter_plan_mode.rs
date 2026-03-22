//! EnterPlanMode tool for transitioning to plan mode.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_plan_mode::PlanFileManager;
use cocode_plan_mode::get_unique_slug;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for entering plan mode.
///
/// Transitions the agent into plan mode where it explores the codebase
/// and designs an implementation approach for user approval.
///
/// Plan files are stored at `~/.cocode/plans/{slug}.md` following
/// Claude Code v2.1.7 conventions.
pub struct EnterPlanModeTool {
    /// Whether the interview phase is enabled.
    ///
    /// When true, the tool description omits the "What Happens in Plan Mode"
    /// section because detailed instructions come via the system reminder.
    interview_phase: bool,
}

impl EnterPlanModeTool {
    /// Create a new EnterPlanMode tool (interview phase OFF by default).
    pub fn new() -> Self {
        Self {
            interview_phase: false,
        }
    }

    /// Create a new EnterPlanMode tool with interview phase setting.
    pub fn with_interview_phase(interview_phase: bool) -> Self {
        Self { interview_phase }
    }
}

impl Default for EnterPlanModeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::EnterPlanMode.as_str()
    }

    fn description(&self) -> &str {
        if self.interview_phase {
            prompts::ENTER_PLAN_MODE_DESCRIPTION_INTERVIEW
        } else {
            prompts::ENTER_PLAN_MODE_DESCRIPTION
        }
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    /// Auto-approve: entering plan mode needs no user confirmation.
    async fn check_permission(
        &self,
        _input: &Value,
        _ctx: &ToolContext,
    ) -> cocode_protocol::PermissionResult {
        cocode_protocol::PermissionResult::Allowed
    }

    async fn execute(&self, _input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        ctx.emit_progress("Entering plan mode").await;

        // Get or generate the session-unique slug (cached per session_id)
        let slug = get_unique_slug(&ctx.session_id, None);

        // Create plan file manager and ensure directory exists.
        // Subagent filtering (SYSTEM_BLOCKED) prevents subagents from calling
        // this tool, so we always use the main agent path.
        let manager = PlanFileManager::new(&ctx.session_id);

        let plan_path = match manager.ensure_and_get_path() {
            Ok(path) => path,
            Err(e) => {
                return Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to create plan directory: {e}"),
                }
                .build());
            }
        };

        // Emit plan mode event with the plan file path
        ctx.emit_event(cocode_protocol::LoopEvent::PlanModeEntered {
            plan_file: Some(plan_path.clone()),
        })
        .await;

        tracing::info!(
            session_id = %ctx.session_id,
            plan_file = %plan_path.display(),
            slug = %slug,
            "Entered plan mode"
        );

        // Return structured output so driver.rs can extract path/slug reliably.
        // The message varies based on interview phase:
        // - Interview ON: Brief placeholder (full instructions come via system reminder)
        // - Interview OFF: Step-by-step guide
        let message = if ctx
            .features
            .enabled(cocode_protocol::Feature::PlanModeInterview)
        {
            "Entered plan mode. DO NOT write a plan yet — detailed instructions will follow \
             in the next system message. Wait for those instructions before proceeding."
                .to_string()
        } else {
            "Entered plan mode. Explore the codebase and design your implementation approach.\n\n\
             1. Read and analyze the user's request — identify key requirements\n\
             2. Launch Explore agents to search the codebase in parallel\n\
             3. Synthesize findings into a step-by-step plan\n\
             4. Write the plan to the plan file using the Write tool\n\
             5. Use AskUserQuestion for any clarifications\n\
             6. Call ExitPlanMode when ready for user approval"
                .to_string()
        };

        let response = serde_json::json!({
            "planFilePath": plan_path.display().to_string(),
            "slug": slug,
            "message": message
        });

        Ok(ToolOutput::structured(response))
    }
}

#[cfg(test)]
#[path = "enter_plan_mode.test.rs"]
mod tests;
