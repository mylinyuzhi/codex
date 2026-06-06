//! TS `prependUserContext` generator (per-turn baseline user context).
//!
//! TS injects a single `isMeta` `<system-reminder>` user message at the
//! head of every API request via `prependUserContext`
//! (`utils/api.ts:449-474`). Its body wraps the `getUserContext()` map
//! (`context.ts:155-189`) — `{ currentDate, claudeMd? }` — as `# key\nvalue`
//! blocks. coco-rs injects `claudeMd` (CLAUDE.md discovery) through the
//! static system prompt (`app/query::build_prompt`), so this generator
//! carries only the `currentDate` block — the piece TS sources *only* from
//! `prependUserContext` and that the KAIROS memory instruction
//! (`memory/src/prompt/builders.rs`) references as "`currentDate` in your
//! context".
//!
//! Unlike [`DateChangeGenerator`](super::DateChangeGenerator) (a one-shot
//! notice when the local date rolls over mid-session), this fires every
//! turn so the date is always present. The engine supplies
//! `ctx.current_date` each turn; `None` (the unit-test default) suppresses
//! it, matching TS's `NODE_ENV === 'test'` skip.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// Emit the per-turn `currentDate` user-context reminder (TS
/// `prependUserContext`).
#[derive(Debug, Default)]
pub struct UserContextGenerator;

#[async_trait]
impl AttachmentGenerator for UserContextGenerator {
    fn name(&self) -> &str {
        "UserContextGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::UserContext
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.user_context
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(date) = ctx.current_date.as_deref() else {
            return Ok(None);
        };
        if date.is_empty() {
            return Ok(None);
        }
        // TS `prependUserContext` body verbatim (`utils/api.ts:462-472`),
        // minus the outer `<system-reminder>` tags which the injection
        // pipeline re-applies via `wrap_with_tag`. The context map carries
        // only `currentDate` (claudeMd lives in the system prompt). The
        // six-space indent before IMPORTANT is the TS template-literal
        // artifact, preserved for byte-parity.
        let content = format!(
            "As you answer the user's questions, you can use the following context:\n\
             # currentDate\n\
             Today's date is {date}.\n\
             \n      \
             IMPORTANT: this context may or may not be relevant to your tasks. \
             You should not respond to this context unless it is highly relevant to your task."
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::UserContext,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "user_context.test.rs"]
mod tests;
