//! TS `date_change` generator.
//!
//! Fires when the local ISO date has rolled over relative to the last
//! previously-seen date in the session — e.g. the user coded past midnight.
//! TS source: `getDateChangeAttachments` (`attachments.ts:1415`) +
//! `normalizeAttachmentForAPI` `case 'date_change':` (`messages.ts:4162`).
//!
//! Detection state lives on the engine (per-session "last emitted date"
//! latch). The engine pre-computes by comparing today's `get_local_iso_date`
//! to the latch and writes `Some(new_date)` on the turn the date changes
//! (updating the latch), `None` otherwise. This generator is pure: it reads
//! `ctx.new_date` and renders.
//!
//! Why not re-compute here: the "last emitted date" state needs to survive
//! multiple turns. That's engine / session state, not reminder state. Keep
//! generators side-effect-free.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// Emit a one-shot notification when the local date rolls over mid-session.
#[derive(Debug, Default)]
pub struct DateChangeGenerator;

#[async_trait]
impl AttachmentGenerator for DateChangeGenerator {
    fn name(&self) -> &str {
        "DateChangeGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::DateChange
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.date_change
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(date) = ctx.new_date.as_deref() else {
            return Ok(None);
        };
        if date.is_empty() {
            return Ok(None);
        }
        // TS verbatim: `messages.ts:4162`.
        let content = format!(
            "The date has changed. Today's date is now {date}. DO NOT mention this to the user explicitly because they are already aware."
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::DateChange,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "date_change.test.rs"]
mod tests;
