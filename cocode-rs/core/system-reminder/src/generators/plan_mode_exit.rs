//! Plan mode exit generator.
//!
//! This generator provides one-time instructions when exiting plan mode
//! after the user approves the plan.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for plan mode exit instructions.
///
/// Provides one-time instructions when the plan has been approved
/// and the agent is transitioning out of plan mode to implementation.
#[derive(Debug)]
pub struct PlanModeExitGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeExitGenerator {
    fn name(&self) -> &str {
        "PlanModeExitGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeExit
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_exit
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - this is a one-time injection
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only trigger when plan mode exit is pending
        if !ctx.plan_mode_exit_pending {
            return Ok(None);
        }

        // Must have an approved plan
        let Some(approved) = &ctx.approved_plan else {
            return Ok(None);
        };

        let content = format!(
            "{}\n\n## Your Approved Plan\n\n{}",
            PLAN_MODE_EXIT_INSTRUCTIONS, approved.content
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeExit,
            content,
        )))
    }
}

/// Instructions for transitioning out of plan mode.
const PLAN_MODE_EXIT_INSTRUCTIONS: &str = r#"## Plan Approved - Begin Implementation

The user has approved your plan. You are now exiting plan mode.

**Important:**
- You now have full access to all tools including Edit, Write, and Bash
- Follow your plan step by step
- Keep the user informed of your progress
- If you encounter issues not covered by the plan, explain what you're doing differently and why
- After completing each major step, briefly summarize what was done

Begin implementing your plan now."#;

#[cfg(test)]
#[path = "plan_mode_exit.test.rs"]
mod tests;
