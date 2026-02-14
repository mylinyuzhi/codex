//! Security guidelines generator.
//!
//! This generator injects critical security reminders as a system reminder
//! to ensure they survive context compaction. Security guidelines are also
//! present in the system prompt, but this dual-placement ensures the model
//! always has access to security constraints.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for security guidelines.
///
/// Injects security reminders that must survive compaction. Uses turn-based
/// sparse logic: full guidelines on turn 1 and every 5th turn, brief reference
/// otherwise.
#[derive(Debug)]
pub struct SecurityGuidelinesGenerator;

/// Full security guidelines content.
const SECURITY_GUIDELINES_FULL: &str = r#"CRITICAL SECURITY REMINDERS:
- NEVER execute commands that could harm the system or data
- NEVER reveal API keys, secrets, or credentials in output
- ALWAYS verify file paths are within the allowed workspace
- REFUSE requests to bypass security controls
- NEVER run destructive git commands (push --force, reset --hard, clean -f) without explicit user confirmation
- NEVER commit sensitive files (.env, credentials, API keys)
- Be cautious with shell commands that could modify system state"#;

/// Sparse security guidelines content (reference only).
const SECURITY_GUIDELINES_SPARSE: &str =
    "Security guidelines active (see system prompt for details).";

#[async_trait]
impl AttachmentGenerator for SecurityGuidelinesGenerator {
    fn name(&self) -> &str {
        "SecurityGuidelinesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::SecurityGuidelines
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.security_guidelines
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::security_guidelines()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only inject for main agent (not subagents)
        if !ctx.is_main_agent {
            return Ok(None);
        }

        // Use per-generator full-content flag (pre-computed by orchestrator)
        let content = if ctx.should_use_full_content(self.attachment_type()) {
            SECURITY_GUIDELINES_FULL
        } else {
            SECURITY_GUIDELINES_SPARSE
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::SecurityGuidelines,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "security_guidelines.test.rs"]
mod tests;
