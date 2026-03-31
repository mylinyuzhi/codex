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
        let team_enabled = state.config.team_memory_enabled;
        let extraction_enabled = state.config.memory_extraction_enabled;

        let prompt = if !ctx.is_main_agent || extraction_enabled {
            // Subagents and extraction-mode main agents get read-only prompt.
            if team_enabled {
                let team_dir = state.team_memory_dir_str();
                cocode_auto_memory::prompt::build_extract_mode_typed_combined_prompt(
                    &memory_dir,
                    &team_dir,
                    max_lines,
                )
            } else {
                cocode_auto_memory::build_background_agent_memory_prompt(&memory_dir, max_lines)
            }
        } else if team_enabled {
            // Main agent with team memory — use combined prompt.
            let team_dir = state.team_memory_dir_str();
            let index = state.index().await;
            let team_index = state.team_index().await;
            cocode_auto_memory::prompt::build_typed_combined_memory_prompt(
                &memory_dir,
                &team_dir,
                index.as_ref(),
                team_index.as_ref(),
                max_lines,
            )
        } else {
            // Main agent, single directory — standard prompt.
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
