//! Budget USD generator.
//!
//! This generator reports budget warnings when the session budget is low.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Threshold percentage for low budget warning.
const LOW_BUDGET_THRESHOLD: f64 = 10.0;

/// Generator for budget USD warnings.
///
/// Reports budget information only when the budget is low (< 10% remaining).
/// This helps the model be aware of budget constraints and adjust behavior.
#[derive(Debug)]
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

    fn throttle_config(&self) -> ThrottleConfig {
        // Check every turn when budget is low
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(budget) = &ctx.budget else {
            return Ok(None);
        };

        // Only generate if budget is low
        let remaining_percent = if budget.total_usd > 0.0 {
            (budget.remaining_usd / budget.total_usd) * 100.0
        } else {
            100.0 // No budget set
        };

        if remaining_percent > LOW_BUDGET_THRESHOLD && !budget.is_low {
            return Ok(None);
        }

        // Build the warning message
        let used_percent = if budget.total_usd > 0.0 {
            (budget.used_usd / budget.total_usd) * 100.0
        } else {
            0.0
        };

        let content = format!(
            "**Budget Warning:** ${:.2} remaining of ${:.2} ({:.1}% used)\n\n\
            Please be mindful of API costs. Consider:\n\
            - Being more concise in responses\n\
            - Avoiding unnecessary tool calls\n\
            - Completing the current task efficiently",
            budget.remaining_usd, budget.total_usd, used_percent
        );

        Ok(Some(SystemReminder::text(
            AttachmentType::BudgetUsd,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "budget_usd.test.rs"]
mod tests;
