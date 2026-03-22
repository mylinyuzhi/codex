//! Hook response generators for system reminders.
//!
//! This module provides generators for injecting hook-related context into
//! the conversation, such as:
//! - Results from async hooks that completed in the background
//! - Context added by hooks (additional_context field)
//! - Error information when hooks block execution

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

// Re-export types from generator.rs for external consumers
pub use crate::generator::AsyncHookResponseInfo;
pub use crate::generator::HookBlockingInfo;
pub use crate::generator::HookContextInfo;

/// Generator for async hook responses.
///
/// This generator injects context from hooks that completed in the background
/// or from hooks that returned additional_context.
#[derive(Debug, Default)]
pub struct AsyncHookResponseGenerator;

#[async_trait]
impl AttachmentGenerator for AsyncHookResponseGenerator {
    fn name(&self) -> &str {
        "async_hook_response"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AsyncHookResponse
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let responses = &ctx.hook_state.async_responses;
        if responses.is_empty() {
            return Ok(None);
        }

        let mut content = String::from("# Async Hook Results\n\n");
        content.push_str("The following hooks completed in the background:\n\n");

        for response in responses {
            content.push_str(&format!("## Hook: {}\n", response.hook_name));
            content.push_str(&format!("- Duration: {}ms\n", response.duration_ms));

            if response.was_blocking {
                content.push_str("- **BLOCKED** execution\n");
                if let Some(reason) = &response.blocking_reason {
                    content.push_str(&format!("- Reason: {reason}\n"));
                }
            }

            if let Some(context) = &response.additional_context {
                content.push_str(&format!("\n### Additional Context\n{context}\n"));
            }

            content.push('\n');
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::AsyncHookResponse,
            content,
        )))
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        true // Always enabled, generates nothing if no responses
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none() // No throttling - inject whenever available
    }
}

/// Generator for hook additional context.
///
/// This generator injects additional context provided by hooks via
/// the `ContinueWithContext` result.
#[derive(Debug, Default)]
pub struct HookAdditionalContextGenerator;

#[async_trait]
impl AttachmentGenerator for HookAdditionalContextGenerator {
    fn name(&self) -> &str {
        "hook_additional_context"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::HookAdditionalContext
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let contexts = &ctx.hook_state.contexts;
        if contexts.is_empty() {
            return Ok(None);
        }

        let mut content = String::from("# Hook Context\n\n");
        content.push_str("The following hooks added context:\n\n");

        for info in contexts {
            content.push_str(&format!("## From hook: {}\n", info.hook_name));
            content.push_str(&format!("- Event: {}\n", info.event_type));
            if let Some(tool) = &info.tool_name {
                content.push_str(&format!("- Tool: {tool}\n"));
            }
            content.push_str(&format!("\n{}\n\n", info.additional_context));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::HookAdditionalContext,
            content,
        )))
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        true
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }
}

/// Generator for hook blocking errors.
///
/// This generator injects information about hooks that blocked execution,
/// helping the model understand why an action was rejected.
#[derive(Debug, Default)]
pub struct HookBlockingErrorGenerator;

#[async_trait]
impl AttachmentGenerator for HookBlockingErrorGenerator {
    fn name(&self) -> &str {
        "hook_blocking_error"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::HookBlockingError
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let blocking = &ctx.hook_state.blocking;
        if blocking.is_empty() {
            return Ok(None);
        }

        let mut content = String::from("# Hook Blocked Execution\n\n");
        content.push_str(
            "The following hooks blocked execution. Review the reasons and adjust your approach:\n\n",
        );

        for info in blocking {
            content.push_str(&format!("## Hook: {}\n", info.hook_name));
            content.push_str(&format!("- Event: {}\n", info.event_type));
            if let Some(tool) = &info.tool_name {
                content.push_str(&format!("- Tool: {tool}\n"));
            }
            content.push_str(&format!("- **Reason**: {}\n\n", info.reason));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::HookBlockingError,
            content,
        )))
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        true
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }
}

#[cfg(test)]
#[path = "hook_response.test.rs"]
mod tests;
