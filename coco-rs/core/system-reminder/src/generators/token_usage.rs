//! TS `token_usage` generator.
//!
//! Mirrors `getTokenUsageAttachment` (`attachments.ts:3807`) +
//! `normalizeAttachmentForAPI` `case 'token_usage':` (`messages.ts:4058`).
//! Main-thread-only per-turn usage report.
//!
//! Gate chain:
//!
//! 1. `ctx.config.attachments.token_usage` — opt-in (TS gates on the
//!    `CLAUDE_CODE_ENABLE_TOKEN_USAGE_ATTACHMENT` env var; coco-rs
//!    surfaces the same toggle via `settings.json`).
//! 2. `ctx.effective_context_window > 0` — need a real window to
//!    compute remaining; zero means the engine hasn't populated the
//!    field yet (safe to skip).
//!
//! Content is the TS literal at `messages.ts:4062`.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

#[derive(Debug, Default)]
pub struct TokenUsageGenerator;

#[async_trait]
impl AttachmentGenerator for TokenUsageGenerator {
    fn name(&self) -> &str {
        "TokenUsageGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TokenUsage
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.token_usage
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // TS uses `getEffectiveContextWindowSize(model)` as the total.
        // A zero window means the engine hasn't filled the field —
        // skip silently rather than emit a nonsense "used/0".
        let total = ctx.effective_context_window;
        if total <= 0 {
            return Ok(None);
        }
        let used = ctx.used_tokens.max(0);
        let remaining = (total - used).max(0);
        let body = format!("Token usage: {used}/{total}; {remaining} remaining");
        Ok(Some(SystemReminder::new(AttachmentType::TokenUsage, body)))
    }
}

#[cfg(test)]
#[path = "token_usage.test.rs"]
mod tests;
