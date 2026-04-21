//! TS `verify_plan_reminder` generator.
//!
//! Mirrors `getVerifyPlanReminderAttachment` (`attachments.ts:3894`) +
//! `normalizeAttachmentForAPI` `case 'verify_plan_reminder':`
//! (`messages.ts:4240`).
//!
//! Fires every 10 turns after `ExitPlanMode` while
//! [`ToolAppState::pending_plan_verification`] remains set, nudging the
//! agent to call the `VerifyPlanExecution` tool.
//!
//! **Tier**: [`ReminderTier::MainAgentOnly`] — sub-agents don't own the
//! plan; the reminder would be wasted tokens.
//!
//! **Env gate difference vs. TS**: TS gates on `USER_TYPE=='ant' &&
//! CLAUDE_CODE_VERIFY_PLAN=='true'`. Per coco-rs CLAUDE.md's TS-first
//! policy we keep the feature but move the gate to a user-facing setting
//! (`settings.system_reminder.attachments.verify_plan_reminder`). Default
//! is `false` because coco-rs doesn't yet ship a `VerifyPlanExecution`
//! tool — enabling the reminder without the tool would nag indefinitely.
//!
//! Gate chain (all must pass):
//!
//! 1. Config flag enabled (`config.attachments.verify_plan_reminder`).
//! 2. Main-agent only (enforced by tier filter in the orchestrator).
//! 3. `ctx.has_pending_plan_verification` (set by `ExitPlanModeTool`).
//! 4. `ctx.turns_since_plan_exit >= 10 && ctx.turns_since_plan_exit % 10 == 0`
//!    — fires on turns 10, 20, 30, … matching TS cadence.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// TS `VERIFY_PLAN_REMINDER_CONFIG.TURNS_BETWEEN_REMINDERS = 10`
/// (`attachments.ts:291`).
const TURNS_BETWEEN_REMINDERS: i32 = 10;

/// Verbatim body from `messages.ts:4247`.
///
/// TS resolves `${toolName}` to the literal `"VerifyPlanExecution"` when
/// `CLAUDE_CODE_VERIFY_PLAN=='true'`; we hardcode the same string here so
/// the reminder matches the TS output exactly. A future `ToolName::
/// VerifyPlanExecution` variant can swap this for the typed accessor.
const BODY: &str = "You have completed implementing the plan. Please call the \"VerifyPlanExecution\" tool directly (NOT the Agent tool or an agent) to verify that all plan items were completed correctly.";

/// Nudges the main agent to call `VerifyPlanExecution` after plan exit.
#[derive(Debug, Default)]
pub struct VerifyPlanReminderGenerator;

#[async_trait]
impl AttachmentGenerator for VerifyPlanReminderGenerator {
    fn name(&self) -> &str {
        "VerifyPlanReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::VerifyPlanReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.verify_plan_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::verify_plan_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.has_pending_plan_verification {
            return Ok(None);
        }
        let n = ctx.turns_since_plan_exit;
        if n <= 0 || n % TURNS_BETWEEN_REMINDERS != 0 {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::VerifyPlanReminder,
            BODY.to_string(),
        )))
    }
}

#[cfg(test)]
#[path = "verify_plan.test.rs"]
mod tests;
