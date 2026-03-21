//! Collaboration notifications generator.
//!
//! This generator provides notifications from other agents (sub-agents,
//! background agents, etc.) that need the main agent's attention.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for collaboration notifications.
///
/// Surfaces notifications from other agents that need the main agent's
/// attention, such as completed tasks, errors, or requests for input.
#[derive(Debug)]
pub struct CollabNotificationsGenerator;

#[async_trait]
impl AttachmentGenerator for CollabNotificationsGenerator {
    fn name(&self) -> &str {
        "CollabNotificationsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CollabNotifications
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.collab_notifications
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - always show pending notifications
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.collab_notifications.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["## Agent Notifications\n".to_string()];

        // Group by notification type for better organization
        let mut errors = Vec::new();
        let mut needs_input = Vec::new();
        let mut completed = Vec::new();
        let mut other = Vec::new();

        for notif in &ctx.collab_notifications {
            match notif.notification_type.as_str() {
                "error" => errors.push(notif),
                "needs_input" => needs_input.push(notif),
                "completed" => completed.push(notif),
                _ => other.push(notif),
            }
        }

        // Show errors first (most urgent)
        if !errors.is_empty() {
            lines.push("### Errors\n".to_string());
            for notif in errors {
                lines.push(format!(
                    "- **{}**: {} (turn {})",
                    notif.from_agent, notif.message, notif.received_turn
                ));
            }
            lines.push(String::new());
        }

        // Show input requests next
        if !needs_input.is_empty() {
            lines.push("### Awaiting Input\n".to_string());
            for notif in needs_input {
                lines.push(format!(
                    "- **{}**: {} (turn {})",
                    notif.from_agent, notif.message, notif.received_turn
                ));
            }
            lines.push(String::new());
        }

        // Show completed tasks
        if !completed.is_empty() {
            lines.push("### Completed\n".to_string());
            for notif in completed {
                lines.push(format!("- **{}**: {}", notif.from_agent, notif.message));
            }
            lines.push(String::new());
        }

        // Show other notifications
        if !other.is_empty() {
            lines.push("### Other\n".to_string());
            for notif in other {
                lines.push(format!(
                    "- **{}** ({}): {}",
                    notif.from_agent, notif.notification_type, notif.message
                ));
            }
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::CollabNotifications,
            lines.join("\n").trim().to_string(),
        )))
    }
}

#[cfg(test)]
#[path = "collab_notifications.test.rs"]
mod tests;
