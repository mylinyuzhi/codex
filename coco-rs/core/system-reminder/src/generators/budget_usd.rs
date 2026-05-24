//! TS `budget_usd` generator.
//!
//! Mirrors `getMaxBudgetUsdAttachment` (`attachments.ts:3846`) +
//! `normalizeAttachmentForAPI` `case 'budget_usd':` (`messages.ts:4067`).
//! Main-thread-only per-turn budget report; fires whenever the session
//! has a configured USD cap.
//!
//! Gate chain:
//!
//! 1. `ctx.config.attachments.budget_usd` — default on (TS has no
//!    additional feature flag once the budget is set).
//! 2. `ctx.max_budget_usd.is_some()` — `None` means the user didn't
//!    configure a cap (TS: `maxBudgetUsd === undefined`).
//!
//! Content is the TS literal at `messages.ts:4071`.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

#[derive(Debug, Default)]
pub struct BudgetUsdGenerator;

#[async_trait]
impl AttachmentGenerator for BudgetUsdGenerator {
    fn name(&self) -> &str {
        "BudgetUsdGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::BudgetUsd
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.budget_usd
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(total) = ctx.max_budget_usd else {
            return Ok(None);
        };
        let used = ctx.total_cost_usd;
        let remaining = total - used;
        // TS `messages.ts:4071` template: `USD budget: $${used}/$${total}; $${remaining} remaining`
        // — `$${}` is a literal `$` followed by the interpolated number.
        let body = format!("USD budget: ${used}/${total}; ${remaining} remaining");
        Ok(Some(SystemReminder::new(AttachmentType::BudgetUsd, body)))
    }
}

#[cfg(test)]
#[path = "budget_usd.test.rs"]
mod tests;
