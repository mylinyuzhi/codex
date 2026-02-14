//! Plan mode generators.
//!
//! These generators handle plan mode workflow reminders:
//! - PlanModeEnterGenerator: Instructions when entering plan mode
//! - PlanModeApprovedGenerator: One-time plan content after approval
//! - PlanToolReminderGenerator: Periodic reminder to use Write/Edit tools for the plan

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for plan mode entry instructions.
///
/// Provides the 5-phase workflow instructions when the agent
/// enters plan mode.
#[derive(Debug)]
pub struct PlanModeEnterGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeEnterGenerator {
    fn name(&self) -> &str {
        "PlanModeEnterGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeEnter
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_enter
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_mode()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_plan_mode {
            return Ok(None);
        }

        // Build instructions with plan file path if available
        let plan_path_info = ctx
            .plan_file_path
            .as_ref()
            .map(|p| format!("\n\n## Plan File Info:\n\nYour plan file is at: `{}`\n\nYou should create your plan at this path using the Write tool. You can read it and make incremental edits using the Edit tool.", p.display()))
            .unwrap_or_default();

        // Use per-generator full-content flag (pre-computed by orchestrator).
        // is_plan_reentry forces sparse regardless of the flag.
        let use_sparse =
            !ctx.should_use_full_content(self.attachment_type()) || ctx.is_plan_reentry;

        let content = if use_sparse {
            format!("{}{}", PLAN_MODE_SPARSE_INSTRUCTIONS, plan_path_info)
        } else {
            format!("{}{}", PLAN_MODE_FULL_INSTRUCTIONS, plan_path_info)
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeEnter,
            content,
        )))
    }
}

/// Generator for approved plan content.
///
/// Injects the plan content once after ExitPlanMode approval.
#[derive(Debug)]
pub struct PlanModeApprovedGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeApprovedGenerator {
    fn name(&self) -> &str {
        "PlanModeApprovedGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeApproved
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_enter
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - this is a one-time injection
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(approved) = &ctx.approved_plan else {
            return Ok(None);
        };

        let content = format!(
            "## Approved Plan\n\n\
             The user has approved your plan. Here is the plan content for reference:\n\n\
             {}\n\n\
             Now proceed with implementing this plan step by step.",
            approved.content
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeApproved,
            content,
        )))
    }
}

/// Generator for plan tool reminders.
///
/// Periodically reminds the agent to use Write/Edit tools
/// when in plan mode.
#[derive(Debug)]
pub struct PlanToolReminderGenerator;

#[async_trait]
impl AttachmentGenerator for PlanToolReminderGenerator {
    fn name(&self) -> &str {
        "PlanToolReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanToolReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_tool_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_tool_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_plan_mode {
            return Ok(None);
        }

        // Only remind if there's a plan file path
        let Some(plan_path) = &ctx.plan_file_path else {
            return Ok(None);
        };

        let content = format!(
            "Reminder: You are in plan mode. Use the Write tool to create or replace your plan, \
             or the Edit tool to modify it at:\n\
             `{}`\n\n\
             When your plan is ready for approval, use ExitPlanMode to submit it for review.",
            plan_path.display()
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanToolReminder,
            content,
        )))
    }
}

/// Full plan mode instructions shown on first entry (aligned with Claude Code v2.1.7).
const PLAN_MODE_FULL_INSTRUCTIONS: &str = r#"## Plan Mode Active

Plan mode is active. The user indicated that they do not want you to execute yet -- you MUST NOT make any edits (with the exception of the plan file mentioned below), run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supercedes any other instructions.

You should build your plan incrementally by writing to or editing the plan file. NOTE that this is the only file you are allowed to edit.

## Plan Workflow

Follow this 5-phase workflow:

### Phase 1: Understand
- Read and analyze the user's request
- Identify key requirements and constraints
- Ask clarifying questions if needed (use AskUserQuestion)

### Phase 2: Explore
- Search the codebase to understand existing patterns
- Identify files that need to be modified
- Note any dependencies or architectural considerations

### Phase 3: Design
- Create a step-by-step implementation plan
- Consider edge cases and error handling
- Document any assumptions

### Phase 4: Document
- Write your plan to the plan file using the Write tool
- Include specific file paths and changes
- Add test considerations

### Phase 5: Review
- Verify the plan is complete and actionable
- Use ExitPlanMode when ready for user approval

## Important

- End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval)
- Never ask about plan approval via text or AskUserQuestion -- use ExitPlanMode instead
- Do NOT make code changes while in plan mode. Focus only on planning."#;

/// Sparse plan mode instructions shown on re-entry.
const PLAN_MODE_SPARSE_INSTRUCTIONS: &str = r#"## Plan Mode Active

Plan mode still active (see full instructions earlier in conversation).

Read-only except plan file. Follow 5-phase workflow.

End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval).

Never ask about plan approval via text or AskUserQuestion -- use ExitPlanMode instead."#;

#[cfg(test)]
#[path = "plan_mode.test.rs"]
mod tests;
