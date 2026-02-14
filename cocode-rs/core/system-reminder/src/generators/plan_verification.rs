//! Plan verification generator.
//!
//! This generator reminds the model to verify that plan steps are being
//! followed during implementation.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::PlanStep;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for plan verification reminders.
///
/// Reminds the model to check their progress against the approved plan
/// during implementation. Shows completed and remaining steps.
#[derive(Debug)]
pub struct PlanVerificationGenerator;

#[async_trait]
impl AttachmentGenerator for PlanVerificationGenerator {
    fn name(&self) -> &str {
        "PlanVerificationGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanVerification
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_verification
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Remind every 5 turns during implementation
        ThrottleConfig {
            min_turns_between: 5,
            min_turns_after_trigger: 3,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only generate during implementation (not in plan mode, but has plan state)
        if ctx.is_plan_mode {
            return Ok(None);
        }

        let Some(plan_state) = &ctx.plan_state else {
            return Ok(None);
        };

        // Don't generate if plan is empty or has no steps
        if plan_state.is_empty || plan_state.steps.is_empty() {
            return Ok(None);
        }

        // Build progress summary
        let completed: Vec<&PlanStep> = plan_state
            .steps
            .iter()
            .filter(|s| s.status == "completed")
            .collect();
        let in_progress: Vec<&PlanStep> = plan_state
            .steps
            .iter()
            .filter(|s| s.status == "in_progress")
            .collect();
        let pending: Vec<&PlanStep> = plan_state
            .steps
            .iter()
            .filter(|s| s.status == "pending")
            .collect();

        let total = plan_state.steps.len();
        let completed_count = completed.len();
        let progress_percent = (completed_count as f64 / total as f64) * 100.0;

        let mut lines = vec![format!(
            "## Plan Progress: {}/{} steps ({:.0}%)\n",
            completed_count, total, progress_percent
        )];

        // Show current step if any
        if !in_progress.is_empty() {
            lines.push("**Current:** ".to_string());
            for step in &in_progress {
                lines.push(format!("- {}", step.step));
            }
            lines.push(String::new());
        }

        // Show next few pending steps
        if !pending.is_empty() {
            lines.push("**Next:** ".to_string());
            for step in pending.iter().take(3) {
                lines.push(format!("- {}", step.step));
            }
            if pending.len() > 3 {
                lines.push(format!("  ... and {} more", pending.len() - 3));
            }
        }

        // Add guidance
        if !in_progress.is_empty() {
            lines.push(String::new());
            lines.push("Focus on completing the current step before moving on.".to_string());
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanVerification,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "plan_verification.test.rs"]
mod tests;
