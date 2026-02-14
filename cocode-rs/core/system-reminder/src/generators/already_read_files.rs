//! Already read files generator.
//!
//! This generator creates synthetic tool_use/tool_result pairs for files
//! that have been previously read. This helps the model know what files
//! it has already seen without including full content.

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ContentBlock;
use crate::types::MessageRole;
use crate::types::ReminderMessage;
use crate::types::SystemReminder;

/// Maximum number of files to include in the reminder.
const MAX_FILES_TO_INCLUDE: usize = 10;

/// Generator for already read files.
///
/// Creates synthetic tool_use/tool_result pairs for files the model has
/// previously read. This allows the model to know which files it has seen
/// without needing to include full file contents.
#[derive(Debug)]
pub struct AlreadyReadFilesGenerator;

#[async_trait]
impl AttachmentGenerator for AlreadyReadFilesGenerator {
    fn name(&self) -> &str {
        "AlreadyReadFilesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AlreadyReadFile
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.already_read_files
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Only inject on first turn or every 5th turn (aligned with full reminders)
        ThrottleConfig {
            min_turns_between: 5,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(tracker) = ctx.file_tracker else {
            return Ok(None);
        };

        let tracked_files = tracker.tracked_files();
        if tracked_files.is_empty() {
            return Ok(None);
        }

        // Build tool_use/tool_result pairs for each tracked file
        let mut messages = Vec::new();

        for path in tracked_files.iter().take(MAX_FILES_TO_INCLUDE) {
            let Some(state) = tracker.get_state(path) else {
                continue;
            };

            let id = format!("synth-read-{}", Uuid::new_v4());
            let path_str = path.display().to_string();

            // Create tool_use message (assistant role)
            let tool_use_block =
                ContentBlock::tool_use(id.clone(), "Read", json!({ "file_path": path_str }));
            messages.push(ReminderMessage {
                role: MessageRole::Assistant,
                blocks: vec![tool_use_block],
                is_meta: true,
            });

            // Create tool_result message (user role)
            let summary = if state.is_partial() {
                let offset = state.offset.unwrap_or(0);
                let limit = state.limit.unwrap_or(0);
                format!(
                    "[Previously read (partial): lines {}–{}, {} bytes]",
                    offset,
                    offset + limit,
                    state.content.len()
                )
            } else {
                format!(
                    "[Previously read: {} lines, {} bytes]",
                    state.content.lines().count(),
                    state.content.len()
                )
            };

            let tool_result_block = ContentBlock::tool_result(id, summary);
            messages.push(ReminderMessage {
                role: MessageRole::User,
                blocks: vec![tool_result_block],
                is_meta: true,
            });
        }

        // Add ellipsis if more files were tracked
        if tracked_files.len() > MAX_FILES_TO_INCLUDE {
            let remaining = tracked_files.len() - MAX_FILES_TO_INCLUDE;
            let id = format!("synth-note-{}", Uuid::new_v4());

            messages.push(ReminderMessage {
                role: MessageRole::Assistant,
                blocks: vec![ContentBlock::tool_use(
                    id.clone(),
                    "Read",
                    json!({ "note": format!("...and {} more files", remaining) }),
                )],
                is_meta: true,
            });

            messages.push(ReminderMessage {
                role: MessageRole::User,
                blocks: vec![ContentBlock::tool_result(
                    id,
                    format!("[{remaining} additional files previously read]"),
                )],
                is_meta: true,
            });
        }

        if messages.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::messages(
            AttachmentType::AlreadyReadFile,
            messages,
        )))
    }
}

#[cfg(test)]
#[path = "already_read_files.test.rs"]
mod tests;
