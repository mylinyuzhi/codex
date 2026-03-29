//! IDE context reminder generator.
//!
//! Injects selected lines and opened files from a connected IDE
//! via MCP, matching Claude Code's selected_lines_in_ide and
//! opened_file_in_ide attachment types.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Maximum characters for IDE selection content before truncation.
const MAX_SELECTION_CHARS: usize = 2000;

#[derive(Debug)]
pub struct IdeContextGenerator;

#[async_trait]
impl AttachmentGenerator for IdeContextGenerator {
    fn name(&self) -> &str {
        "IdeContextGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::SelectedLinesInIde
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.ide_context
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Priority: selected lines > opened file
        if let Some(ref sel) = ctx.ide_selection {
            // Truncate at char boundary to avoid panic on multi-byte UTF-8
            let content = if sel.content.len() > MAX_SELECTION_CHARS {
                let boundary = sel.content.floor_char_boundary(MAX_SELECTION_CHARS);
                format!("{}... (truncated)", &sel.content[..boundary])
            } else {
                sel.content.clone()
            };

            let reminder = format!(
                "The user selected the lines {} to {} from {}:\n{content}\n\n\
                 This may or may not be related to the current task.",
                sel.line_start, sel.line_end, sel.filename
            );

            return Ok(Some(SystemReminder::new(
                AttachmentType::SelectedLinesInIde,
                reminder,
            )));
        }

        if let Some(ref filename) = ctx.ide_opened_file {
            let reminder = format!(
                "The user opened the file {filename} in the IDE. \
                 This may or may not be related to the current task."
            );

            return Ok(Some(SystemReminder::new(
                AttachmentType::OpenedFileInIde,
                reminder,
            )));
        }

        Ok(None)
    }
}
