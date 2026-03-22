//! Auto memory prompt generator.
//!
//! Injects the auto memory prompt (MEMORY.md instructions + content)
//! into the system reminders when the AutoMemory feature is enabled.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for auto memory prompt injection.
#[derive(Debug)]
pub struct AutoMemoryPromptGenerator;

#[async_trait]
impl AttachmentGenerator for AutoMemoryPromptGenerator {
    fn name(&self) -> &str {
        "AutoMemoryPromptGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AutoMemoryPrompt
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        // Enabled when auto memory state is present (feature-gated at initialization)
        true
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle — check every turn (memory may have been updated)
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let state = match ctx.auto_memory_state.as_ref() {
            Some(s) if s.is_enabled() => s,
            _ => return Ok(None),
        };

        let memory_dir = state.memory_dir_str();
        let max_lines = state.config.max_lines;

        // Select prompt variant based on agent role and memory extraction mode.
        // - Subagents always get the read-only prompt (they shouldn't write memory).
        // - When memory extraction is enabled, the main agent also gets read-only
        //   because a background extraction agent handles saves.
        // - Otherwise, the main agent gets the full read/write prompt.
        let prompt = if !ctx.is_main_agent || state.config.memory_extraction_enabled {
            cocode_auto_memory::build_background_agent_memory_prompt(&memory_dir, max_lines)
        } else {
            let index = state.index().await;
            cocode_auto_memory::build_auto_memory_prompt(&memory_dir, index.as_ref(), max_lines)
        };

        Ok(Some(SystemReminder::text(
            AttachmentType::AutoMemoryPrompt,
            prompt,
        )))
    }
}

#[cfg(test)]
#[path = "auto_memory_prompt.test.rs"]
mod tests;
