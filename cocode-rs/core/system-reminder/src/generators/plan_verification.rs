//! Plan verification generator.
//!
//! Fires when the model has completed implementing a plan (all todo items are
//! completed). Emits a reminder asking the model to call the verify tool.
//!
//! Aligned with Claude Code's `verify_plan_reminder` conversion template:
//!
//! > You have completed implementing the plan. Please call the "" tool directly
//! > (NOT the Task tool or an agent) to verify that all plan items were completed
//! > correctly.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::TodoStatus;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Tool name placeholder — CC uses empty string; replace when verify tool is defined.
const VERIFY_TOOL_NAME: &str = "";
/// Subagent tool name (cocode-rs's Task tool).
const SUB_AGENT_TOOL_NAME: &str = "Task";

/// Generator for plan verification reminders.
///
/// Fires when ALL of these conditions hold:
/// 1. Main agent only (not subagents)
/// 2. Not in plan mode (implementation phase)
/// 3. A plan file exists (plan was created)
/// 4. There are tracked todo items
/// 5. All todos have `Completed` status
#[derive(Debug)]
pub struct PlanVerificationGenerator;

#[async_trait]
impl AttachmentGenerator for PlanVerificationGenerator {
    fn name(&self) -> &str {
        "PlanVerificationGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanVerification
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_verification
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig {
            min_turns_between: 5,
            ..ThrottleConfig::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Main agent only (CC tier: MainAgentOnly).
        if !ctx.is_main_agent {
            return Ok(None);
        }
        // Only during implementation (not in plan mode).
        if ctx.is_plan_mode {
            return Ok(None);
        }
        // Only if a plan was created.
        if ctx.plan_file_path.is_none() {
            return Ok(None);
        }
        // Only if there are todo items and all are completed.
        if ctx.todos.is_empty() {
            return Ok(None);
        }
        if !ctx.todos.iter().all(|t| t.status == TodoStatus::Completed) {
            return Ok(None);
        }

        let content = format!(
            "You have completed implementing the plan. \
             Please call the \"{}\" tool directly \
             (NOT the {} tool or an agent) to verify \
             that all plan items were completed correctly.",
            VERIFY_TOOL_NAME, SUB_AGENT_TOOL_NAME,
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanVerification,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "plan_verification.test.rs"]
mod tests;
