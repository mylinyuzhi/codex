//! Cron job reminders generator.
//!
//! Injects current cron job state into system reminders so the model
//! knows what jobs are scheduled. This survives compaction since the
//! state is injected per-turn from the persisted cron job store.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for cron job state reminders.
#[derive(Debug)]
pub struct CronRemindersGenerator;

#[async_trait]
impl AttachmentGenerator for CronRemindersGenerator {
    fn name(&self) -> &str {
        "CronRemindersGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CronReminders
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.cron_reminders
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Check every 5 turns or when state changes
        ThrottleConfig::todo_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.cron_jobs.is_empty() {
            return Ok(None);
        }

        let mut content = String::new();
        content.push_str("## Scheduled Cron Jobs\n\n");

        for job in &ctx.cron_jobs {
            let type_label = if job.one_shot { " (one-shot)" } else { "" };
            content.push_str(&format!(
                "- {}: [{}]{} — {}\n",
                job.id, job.cron, type_label, job.description,
            ));
            if job.execution_count > 0 {
                content.push_str(&format!("  executions: {}\n", job.execution_count));
            }
        }

        content.push_str(&format!("\nTotal: {} active job(s)\n", ctx.cron_jobs.len()));

        Ok(Some(SystemReminder::new(
            AttachmentType::CronReminders,
            content,
        )))
    }
}
