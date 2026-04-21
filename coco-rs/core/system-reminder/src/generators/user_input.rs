//! User-input-tier reminder generators (5 variants, TS
//! `userInputAttachments` at `attachments.ts:772-815`).
//!
//! All five are `ReminderTier::UserPrompt` — they only run when the
//! user submitted input this turn (`ctx.has_user_input == true`).
//! Engine pre-resolves mentions through `core/context::mention_resolver`
//! and pre-formats IDE state through the `bridge` crate, then passes
//! typed snapshots via `TurnReminderInput`.
//!
//! - `AtMentionedFilesGenerator` → TS `file` (`messages.ts:3545`),
//!   simplified to a path listing + short note. Full file content is
//!   loaded into context via `core/context::Attachment::File`.
//! - `McpResourcesGenerator` → TS `mcp_resource` (`messages.ts:3877`),
//!   simplified to a resource listing.
//! - `AgentMentionsGenerator` → TS `agent_mention` (`messages.ts:3946`).
//! - `IdeSelectionGenerator` → TS `selected_lines_in_ide`
//!   (`messages.ts:3613`).
//! - `IdeOpenedFileGenerator` → TS `opened_file_in_ide`
//!   (`messages.ts:3628`).

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

// ---------------------------------------------------------------------------
// Snapshot types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionedFileEntry {
    pub filename: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpResourceEntry {
    pub server: String,
    pub uri: String,
}

/// Matches TS `agent_mention.agentType` — the agent kind the user
/// referenced (e.g. `"explore"`, `"plan"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentMentionEntry {
    pub agent_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdeSelectionSnapshot {
    pub filename: String,
    pub line_start: i32,
    pub line_end: i32,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdeOpenedFileSnapshot {
    pub filename: String,
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AtMentionedFilesGenerator;

#[async_trait]
impl AttachmentGenerator for AtMentionedFilesGenerator {
    fn name(&self) -> &str {
        "AtMentionedFilesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AtMentionedFiles
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.at_mentioned_files
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.at_mentioned_files.is_empty() {
            return Ok(None);
        }
        let paths: Vec<String> = ctx
            .at_mentioned_files
            .iter()
            .map(|f| format!("- {}", f.display_path))
            .collect();
        let body = format!(
            "The user @-mentioned the following file(s). Their content has been loaded into context:\n{}",
            paths.join("\n")
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::AtMentionedFiles,
            body,
        )))
    }
}

#[derive(Debug, Default)]
pub struct McpResourcesGenerator;

#[async_trait]
impl AttachmentGenerator for McpResourcesGenerator {
    fn name(&self) -> &str {
        "McpResourcesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::McpResources
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.mcp_resources
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.mcp_resources.is_empty() {
            return Ok(None);
        }
        let entries: Vec<String> = ctx
            .mcp_resources
            .iter()
            .map(|r| format!("<mcp-resource server=\"{}\" uri=\"{}\" />", r.server, r.uri))
            .collect();
        let body = format!(
            "The user referenced the following MCP resource(s):\n{}",
            entries.join("\n")
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::McpResources,
            body,
        )))
    }
}

#[derive(Debug, Default)]
pub struct AgentMentionsGenerator;

#[async_trait]
impl AttachmentGenerator for AgentMentionsGenerator {
    fn name(&self) -> &str {
        "AgentMentionsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AgentMentions
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.agent_mentions
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.agent_mentions.is_empty() {
            return Ok(None);
        }
        // TS renders one reminder per mention; coco-rs joins with \n\n.
        let parts: Vec<String> = ctx
            .agent_mentions
            .iter()
            .map(|m| format!(
                "The user has expressed a desire to invoke the agent \"{}\". Please invoke the agent appropriately, passing in the required context to it.",
                m.agent_type
            ))
            .collect();
        Ok(Some(SystemReminder::new(
            AttachmentType::AgentMentions,
            parts.join("\n\n"),
        )))
    }
}

#[derive(Debug, Default)]
pub struct IdeSelectionGenerator;

#[async_trait]
impl AttachmentGenerator for IdeSelectionGenerator {
    fn name(&self) -> &str {
        "IdeSelectionGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::IdeSelection
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.ide_selection
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(sel) = ctx.ide_selection.as_ref() else {
            return Ok(None);
        };
        if sel.filename.is_empty() {
            return Ok(None);
        }
        // TS truncates at 2000 chars (`messages.ts:3614-3619`).
        const MAX_LEN: usize = 2000;
        let content = if sel.content.len() > MAX_LEN {
            let mut truncated = String::with_capacity(MAX_LEN + 20);
            truncated.push_str(&sel.content[..MAX_LEN]);
            truncated.push_str("\n... (truncated)");
            truncated
        } else {
            sel.content.clone()
        };
        let body = format!(
            "The user selected the lines {start} to {end} from {file}:\n{content}\n\nThis may or may not be related to the current task.",
            start = sel.line_start,
            end = sel.line_end,
            file = sel.filename,
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::IdeSelection,
            body,
        )))
    }
}

#[derive(Debug, Default)]
pub struct IdeOpenedFileGenerator;

#[async_trait]
impl AttachmentGenerator for IdeOpenedFileGenerator {
    fn name(&self) -> &str {
        "IdeOpenedFileGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::IdeOpenedFile
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.ide_opened_file
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(f) = ctx.ide_opened_file.as_ref() else {
            return Ok(None);
        };
        if f.filename.is_empty() {
            return Ok(None);
        }
        let body = format!(
            "The user opened the file {} in the IDE. This may or may not be related to the current task.",
            f.filename
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::IdeOpenedFile,
            body,
        )))
    }
}

#[cfg(test)]
#[path = "user_input.test.rs"]
mod tests;
