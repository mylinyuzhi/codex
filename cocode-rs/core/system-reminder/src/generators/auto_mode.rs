//! Auto mode generators.
//!
//! CC equivalent: `ZuY` (auto_mode) and `GuY` (auto_mode_exit) in chunks.147.mjs.
//!
//! Auto mode provides instructions for autonomous execution: act immediately,
//! minimize interruptions, prefer action over planning, make reasonable defaults.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for auto mode instructions.
///
/// CC equivalent: `ZuY` (getPlanModeAttachment-style for auto mode).
/// Provides full/sparse instructions when the permission mode is Auto.
#[derive(Debug)]
pub struct AutoModeEnterGenerator;

#[async_trait]
impl AttachmentGenerator for AutoModeEnterGenerator {
    fn name(&self) -> &str {
        "AutoModeEnterGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AutoMode
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.auto_mode
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // CC: TURNS_BETWEEN_ATTACHMENTS = 5, FULL_REMINDER_EVERY_N_ATTACHMENTS = 5
        ThrottleConfig {
            min_turns_between: 5,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_auto_mode {
            return Ok(None);
        }

        let content = if ctx.should_use_full_content(self.attachment_type()) {
            AUTO_MODE_FULL_INSTRUCTIONS
        } else {
            AUTO_MODE_SPARSE_INSTRUCTIONS
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::AutoMode,
            content.to_string(),
        )))
    }
}

/// Generator for auto mode exit notification.
///
/// CC equivalent: `GuY` (getAutoModeExitAttachment).
/// One-time injection when transitioning out of auto mode.
#[derive(Debug)]
pub struct AutoModeExitGenerator;

#[async_trait]
impl AttachmentGenerator for AutoModeExitGenerator {
    fn name(&self) -> &str {
        "AutoModeExitGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AutoModeExit
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.auto_mode
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.auto_mode_exit_pending {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::AutoModeExit,
            AUTO_MODE_EXIT_INSTRUCTIONS.to_string(),
        )))
    }
}

/// Full auto mode instructions (CC: `Rzz` function in chunks.173.mjs).
const AUTO_MODE_FULL_INSTRUCTIONS: &str = r#"## Auto Mode Active

Auto mode is active. The user chose continuous, autonomous execution. You should:

1. **Execute immediately** — Start implementing right away. Make reasonable assumptions and proceed.
2. **Minimize interruptions** — Prefer making reasonable assumptions over asking questions. Use AskUserQuestion only when the task genuinely cannot proceed without user input (e.g., choosing between fundamentally different approaches with no clear default).
3. **Prefer action over planning** — Do not enter plan mode unless the user explicitly asks. When in doubt, start coding.
4. **Make reasonable decisions** — Choose the most sensible approach and keep moving. Don't block on ambiguity that you can resolve with a reasonable default.
5. **Be thorough** — Complete the full task including tests, linting, and verification without stopping to ask."#;

/// Sparse auto mode reminder (CC: `hzz` function in chunks.173.mjs).
const AUTO_MODE_SPARSE_INSTRUCTIONS: &str = "Auto mode still active (see full instructions earlier in conversation). \
     Execute autonomously, minimize interruptions, prefer action over planning.";

/// Auto mode exit instructions (CC: chunks.174.mjs:278-284).
const AUTO_MODE_EXIT_INSTRUCTIONS: &str = r#"## Exited Auto Mode

You have exited auto mode. The user may now want to interact more directly. You should ask clarifying questions when the approach is ambiguous rather than making assumptions."#;

#[cfg(test)]
#[path = "auto_mode.test.rs"]
mod tests;
