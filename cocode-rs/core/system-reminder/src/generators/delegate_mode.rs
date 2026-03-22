//! Delegate mode generator.
//!
//! This generator provides instructions when operating in delegate mode,
//! where the main agent delegates work to specialized sub-agents.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for delegate mode instructions.
///
/// Provides context and instructions when the main agent is operating
/// in delegate mode, coordinating with specialized sub-agents.
#[derive(Debug)]
pub struct DelegateModeGenerator;

#[async_trait]
impl AttachmentGenerator for DelegateModeGenerator {
    fn name(&self) -> &str {
        "DelegateModeGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::DelegateMode
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.delegate_mode
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Reminder every 5 turns while in delegate mode
        ThrottleConfig {
            min_turns_between: 5,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_delegate_mode {
            return Ok(None);
        }

        // Build agent status section if there are delegated agents
        let agent_status = if !ctx.delegated_agents.is_empty() {
            let mut lines = vec!["## Active Agents\n".to_string()];
            for agent in &ctx.delegated_agents {
                lines.push(format!(
                    "- **{}** ({}): {} - {}",
                    agent.agent_id, agent.agent_type, agent.status, agent.description
                ));
            }
            lines.join("\n")
        } else {
            String::new()
        };

        // Different message if exiting delegate mode
        let content = if ctx.delegate_mode_exiting {
            format!("{DELEGATE_MODE_EXIT_INSTRUCTIONS}\n\n{agent_status}")
        } else {
            format!("{DELEGATE_MODE_INSTRUCTIONS}\n\n{agent_status}")
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::DelegateMode,
            content.trim().to_string(),
        )))
    }
}

/// Instructions for delegate mode.
const DELEGATE_MODE_INSTRUCTIONS: &str = r#"## Delegate Mode Active

You are operating in delegate mode, coordinating with specialized agents.

**Guidelines:**
- Monitor agent progress and handle any issues
- Synthesize results from completed agents
- Delegate appropriate tasks to specialized agents when beneficial
- Keep the user informed of overall progress
- You can run multiple agents in parallel when tasks are independent"#;

/// Instructions when exiting delegate mode.
const DELEGATE_MODE_EXIT_INSTRUCTIONS: &str = r#"## Exiting Delegate Mode

Delegate mode is ending. Please:

1. Review outputs from all completed agents
2. Synthesize the results into a coherent response
3. Address any incomplete or failed tasks
4. Provide a summary to the user"#;

#[cfg(test)]
#[path = "delegate_mode.test.rs"]
mod tests;
