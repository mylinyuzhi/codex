//! Plan-mode reminder generators.
//!
//! Three generators, each producing one TS attachment type:
//!
//! - [`PlanModeEnterGenerator`] → `plan_mode` (Full / Sparse cadence)
//! - [`PlanModeExitGenerator`] → `plan_mode_exit` (one-shot after exit)
//! - [`PlanModeReentryGenerator`] → `plan_mode_reentry` (one-shot on first
//!   plan turn after a prior exit)
//!
//! Text templates come from TS (`src/utils/messages.ts` cases `plan_mode`,
//! `plan_mode_exit`, `plan_mode_reentry`) via `coco_context::render_plan_*`,
//! which already tracks TS line-for-line.
//!
//! These generators are thin adapters: they read flags from
//! [`GeneratorContext`], build a [`coco_context::PlanModeAttachment`] /
//! [`coco_context::PlanModeExitAttachment`], and call the renderer. Engine
//! wiring (Phase D) is responsible for populating the context fields from
//! `ToolAppState` (one-shot flags + cadence counters).

use async_trait::async_trait;
use coco_context::PlanModeAttachment;
use coco_context::PlanModeExitAttachment;
use coco_context::ReminderType;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

// ---------------------------------------------------------------------------
// PlanModeEnterGenerator
// ---------------------------------------------------------------------------

/// Emits the steady-state plan-mode reminder while the engine is in plan mode.
///
/// Cadence is governed by [`ThrottleConfig::plan_mode`]:
/// - Minimum 5 turns between successive emissions.
/// - Every 5th emission (#1, #6, #11, …) uses Full content; others Sparse.
///
/// Full/Sparse is pre-computed by the orchestrator into
/// [`GeneratorContext::full_content_flags`]; this generator reads that flag
/// via [`GeneratorContext::should_use_full_content`].
#[derive(Debug, Default)]
pub struct PlanModeEnterGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeEnterGenerator {
    fn name(&self) -> &str {
        "PlanModeEnterGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanMode
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_mode()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_plan_mode {
            return Ok(None);
        }

        let reminder_type = if ctx.should_use_full_content(AttachmentType::PlanMode) {
            ReminderType::Full
        } else {
            ReminderType::Sparse
        };

        let attachment = build_plan_mode_attachment(ctx, reminder_type);
        let text = coco_context::render_plan_mode_reminder(&attachment);
        Ok(Some(SystemReminder::new(AttachmentType::PlanMode, text)))
    }
}

// ---------------------------------------------------------------------------
// PlanModeExitGenerator
// ---------------------------------------------------------------------------

/// One-shot `## Exited Plan Mode` banner.
///
/// Fires when the engine has set
/// [`GeneratorContext::needs_plan_mode_exit_attachment`]. That flag is set by
/// `ExitPlanModeTool` on success and by the engine when it detects an
/// unannounced plan→non-plan transition. Phase D wiring clears the flag
/// after the orchestrator consumes it — this generator does not mutate ctx.
///
/// No throttle (one-shot per flag set). If the flag somehow stays set across
/// multiple turns, the generator will keep emitting — the engine is
/// responsible for clearing it.
#[derive(Debug, Default)]
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
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.needs_plan_mode_exit_attachment {
            return Ok(None);
        }

        let attachment = PlanModeExitAttachment {
            plan_file_path: plan_file_path_string(ctx),
            plan_exists: ctx.plan_exists,
        };
        let text = coco_context::render_plan_mode_exit_reminder(&attachment);
        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeExit,
            text,
        )))
    }
}

// ---------------------------------------------------------------------------
// PlanModeReentryGenerator
// ---------------------------------------------------------------------------

/// One-shot "re-entering plan mode" banner.
///
/// Fires only when all conditions hold:
/// - `is_plan_mode` (we're in plan mode this turn)
/// - `is_plan_reentry` (engine detected a prior exit)
/// - `plan_exists` (there's an existing plan file to reference)
/// - `!is_sub_agent` (TS `attachments.ts:1216` — sub-agents don't re-enter)
///
/// Produced via the same [`coco_context::render_plan_mode_reminder`] as the
/// steady-state reminder but with [`ReminderType::Reentry`], which emits
/// "## Re-entering Plan Mode" text referencing the existing plan file.
#[derive(Debug, Default)]
pub struct PlanModeReentryGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeReentryGenerator {
    fn name(&self) -> &str {
        "PlanModeReentryGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeReentry
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_reentry
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !(ctx.is_plan_mode && ctx.is_plan_reentry && ctx.plan_exists && !ctx.is_sub_agent) {
            return Ok(None);
        }

        let attachment = build_plan_mode_attachment(ctx, ReminderType::Reentry);
        let text = coco_context::render_plan_mode_reminder(&attachment);
        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeReentry,
            text,
        )))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_plan_mode_attachment(
    ctx: &GeneratorContext<'_>,
    reminder_type: ReminderType,
) -> PlanModeAttachment {
    PlanModeAttachment {
        reminder_type,
        workflow: ctx.plan_workflow,
        phase4_variant: ctx.phase4_variant,
        explore_agent_count: ctx.explore_agent_count,
        plan_agent_count: ctx.plan_agent_count,
        is_sub_agent: ctx.is_sub_agent,
        plan_file_path: plan_file_path_string(ctx),
        plan_exists: ctx.plan_exists,
    }
}

fn plan_file_path_string(ctx: &GeneratorContext<'_>) -> String {
    ctx.plan_file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "plan_mode.test.rs"]
mod tests;
