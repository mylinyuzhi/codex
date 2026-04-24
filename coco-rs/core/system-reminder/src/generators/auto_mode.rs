//! Auto-mode exit reminder generator.
//!
//! Phase B ships the exit banner only. Steady-state `auto_mode` Full/Sparse
//! cadence (TS `auto_mode` attachment) lands in Phase C alongside the
//! classifier-backed auto-mode runtime.
//!
//! Text comes from TS (`messages.ts:3863`, case `auto_mode_exit`) via
//! `coco_context::render_auto_mode_exit_reminder`.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// One-shot `## Exited Auto Mode` banner.
///
/// Fires when [`GeneratorContext::needs_auto_mode_exit_attachment`] is set.
/// The engine sets the flag on any Auto→non-Auto transition and clears it
/// after the orchestrator consumes the reminder.
#[derive(Debug, Default)]
pub struct AutoModeExitGenerator;

#[async_trait]
impl AttachmentGenerator for AutoModeExitGenerator {
    fn name(&self) -> &str {
        "AutoModeExitGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AutoModeExit
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.auto_mode_exit
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.needs_auto_mode_exit_attachment {
            return Ok(None);
        }
        // Suppress when still in auto mode — the flag is stale. TS clears
        // stale exit flags in this branch; the engine mirrors that
        // app-state cleanup after generation.
        if ctx.is_auto_mode {
            return Ok(None);
        }
        let text = coco_context::render_auto_mode_exit_reminder();
        Ok(Some(SystemReminder::new(
            AttachmentType::AutoModeExit,
            text,
        )))
    }
}

#[cfg(test)]
#[path = "auto_mode.test.rs"]
mod tests;
