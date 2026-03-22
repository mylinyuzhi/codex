//! Message injection for system reminders.
//!
//! This module provides utilities for injecting system reminders
//! into the message history.

use tracing::debug;

use crate::types::ContentBlock;
use crate::types::MessageRole;
use crate::types::ReminderOutput;
use crate::types::SystemReminder;
use crate::xml::wrap_with_tag;

/// Injection position for system reminders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionPosition {
    /// Before the user's message.
    BeforeUserMessage,
    /// After the user's message.
    AfterUserMessage,
    /// At the end of the conversation.
    EndOfConversation,
}

/// Result of injecting reminders.
#[derive(Debug)]
pub struct InjectionResult {
    /// Number of reminders injected.
    pub count: i32,
    /// Position where reminders were injected.
    pub position: InjectionPosition,
}

// ============================================================================
// Injected Message Types for Multi-Message Injection
// ============================================================================

/// Injected message - unified representation of all reminder output types.
///
/// This enum represents messages that should be injected into the conversation,
/// supporting both simple text reminders and multi-message tool_use/tool_result pairs.
#[derive(Debug, Clone)]
pub enum InjectedMessage {
    /// User text message (wrapped in XML tags).
    UserText {
        /// The wrapped content.
        content: String,
        /// Whether this is metadata (hidden from user, visible to model).
        is_meta: bool,
    },
    /// Assistant message containing content blocks (typically tool_use).
    AssistantBlocks {
        /// Content blocks (text, tool_use).
        blocks: Vec<InjectedBlock>,
        /// Whether this is metadata.
        is_meta: bool,
    },
    /// User message containing content blocks (typically tool_result).
    UserBlocks {
        /// Content blocks (text, tool_result).
        blocks: Vec<InjectedBlock>,
        /// Whether this is metadata.
        is_meta: bool,
    },
}

/// Injected content block.
///
/// Represents a single content block within an injected message.
#[derive(Debug, Clone)]
pub enum InjectedBlock {
    /// Plain text content.
    Text(String),
    /// Tool use block (synthetic tool call).
    ToolUse {
        /// Unique ID for the tool call.
        id: String,
        /// Name of the tool.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
    },
    /// Tool result block.
    ToolResult {
        /// ID of the tool_use this is responding to.
        tool_use_id: String,
        /// Result content.
        content: String,
    },
}

/// Create injected messages from system reminders.
///
/// This function converts `SystemReminder` outputs into a unified `InjectedMessage`
/// representation that can be converted to API messages by the driver.
///
/// - `ReminderOutput::Text` becomes `InjectedMessage::UserText` with XML wrapping
/// - `ReminderOutput::Messages` becomes multiple `InjectedMessage::AssistantBlocks`
///   and `InjectedMessage::UserBlocks` preserving the tool_use/tool_result structure
/// - `ReminderOutput::ModelAttachment` becomes `InjectedMessage::UserText` with JSON
///
/// # Silent Reminders
///
/// Reminders with `is_silent: true` or silent output variants are filtered out and
/// produce no injected messages. This is used for already-read files to reduce token
/// usage while still logging the information for debugging/UI purposes.
///
/// # Arguments
///
/// * `reminders` - The reminders to convert
///
/// # Returns
///
/// A vector of injected messages ready for conversion to API messages.
pub fn create_injected_messages(reminders: Vec<SystemReminder>) -> Vec<InjectedMessage> {
    let mut result = Vec::new();

    for reminder in reminders {
        // Skip silent reminders (zero tokens in API)
        // Check both the is_silent flag and the output variant
        if reminder.is_silent || reminder.output.is_silent() {
            debug!(
                "Skipping silent reminder for {} (zero tokens)",
                reminder.attachment_type
            );
            continue;
        }

        // Extract fields before consuming output
        let xml_tag = reminder.xml_tag();
        let attachment_type = reminder.attachment_type;
        let is_meta = reminder.is_meta;

        match reminder.output {
            ReminderOutput::Text(content) => {
                let wrapped = wrap_with_tag(&content, xml_tag);
                let char_count = wrapped.chars().count();
                let preview: String = wrapped.chars().take(50).collect();
                let ellipsis = if char_count > 50 { "..." } else { "" };
                debug!(
                    "Creating UserText injection for {} ({} chars): {}{}",
                    attachment_type, char_count, preview, ellipsis
                );
                result.push(InjectedMessage::UserText {
                    content: wrapped,
                    is_meta,
                });
            }
            ReminderOutput::Messages(msgs) => {
                debug!(
                    "Creating {} message injections for {}",
                    msgs.len(),
                    attachment_type
                );
                for msg in msgs {
                    let blocks: Vec<InjectedBlock> = msg
                        .blocks
                        .into_iter()
                        .map(|b| match b {
                            ContentBlock::Text { text } => InjectedBlock::Text(text),
                            ContentBlock::ToolUse { id, name, input } => {
                                InjectedBlock::ToolUse { id, name, input }
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                            } => InjectedBlock::ToolResult {
                                tool_use_id,
                                content,
                            },
                        })
                        .collect();

                    match msg.role {
                        MessageRole::Assistant => {
                            result.push(InjectedMessage::AssistantBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            });
                        }
                        MessageRole::User => {
                            result.push(InjectedMessage::UserBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            });
                        }
                    }
                }
            }
            ReminderOutput::ModelAttachment { payload } => {
                // Model attachments are injected as user messages with JSON content
                let content = match serde_json::to_string_pretty(&payload) {
                    Ok(json) => json,
                    Err(_) => payload.to_string(),
                };
                let wrapped = wrap_with_tag(&content, xml_tag);
                debug!(
                    "Creating UserText injection for {} model attachment",
                    attachment_type
                );
                result.push(InjectedMessage::UserText {
                    content: wrapped,
                    is_meta,
                });
            }
            // Silent variants are already filtered out by the early check above,
            // but we include them here for exhaustiveness
            ReminderOutput::Silent
            | ReminderOutput::SilentText { .. }
            | ReminderOutput::SilentMessages { .. }
            | ReminderOutput::SilentAttachment { .. } => {
                debug!("Silent output variant skipped for {}", attachment_type);
            }
        }
    }

    result
}

/// Inject text-based system reminders and return wrapped content.
///
/// This is a simple helper that wraps each text reminder in its XML tags
/// and returns them as a list of strings ready to be converted to messages.
/// Multi-message reminders and silent reminders are skipped.
///
/// # Silent Reminders
///
/// Reminders with `is_silent: true` or silent output variants are filtered out
/// and produce no output.
///
/// # Arguments
///
/// * `reminders` - The reminders to inject
///
/// # Returns
///
/// A vector of wrapped reminder content strings for text reminders only.
pub fn inject_reminders(reminders: Vec<SystemReminder>) -> Vec<String> {
    let mut result = Vec::with_capacity(reminders.len());

    for reminder in reminders {
        // Skip silent reminders (zero tokens in API)
        // Check both the is_silent flag and the output variant
        if reminder.is_silent || reminder.output.is_silent() {
            debug!(
                "Skipping silent reminder for {} (zero tokens)",
                reminder.attachment_type
            );
            continue;
        }

        if let Some(wrapped) = reminder.wrapped_content() {
            let char_count = wrapped.chars().count();
            let preview: String = wrapped.chars().take(50).collect();
            let ellipsis = if char_count > 50 { "..." } else { "" };
            debug!(
                "Injecting {} reminder ({} chars): {}{}",
                reminder.attachment_type, char_count, preview, ellipsis
            );
            result.push(wrapped);
        } else {
            debug!(
                "Skipping {} reminder (non-text type)",
                reminder.attachment_type
            );
        }
    }

    result
}

/// Combine multiple text reminders into a single message.
///
/// This is useful when you want to inject all reminders as a single
/// user message rather than multiple messages. Multi-message reminders
/// and silent reminders are skipped.
pub fn combine_reminders(reminders: Vec<SystemReminder>) -> Option<String> {
    if reminders.is_empty() {
        return None;
    }

    let parts: Vec<String> = reminders
        .iter()
        .filter(|r| !r.is_silent && !r.output.is_silent()) // Skip silent reminders
        .filter_map(super::types::SystemReminder::wrapped_content)
        .collect();

    if parts.is_empty() {
        return None;
    }

    Some(parts.join("\n\n"))
}

/// Information about injected reminders for logging/telemetry.
#[derive(Debug, Default)]
pub struct InjectionStats {
    /// Total number of reminders processed.
    pub total_count: i32,
    /// Total byte size of all text reminders.
    pub total_bytes: i64,
    /// Breakdown by attachment type.
    pub by_type: std::collections::HashMap<String, i32>,
    /// Number of multi-message reminders.
    pub multi_message_count: i32,
    /// Number of model attachment reminders.
    pub model_attachment_count: i32,
    /// Number of silent reminders (zero tokens in API).
    pub silent_count: i32,
}

/// Result of normalizing injected messages.
///
/// Contains messages split into two categories:
/// - Model-visible: Sent to the API
/// - Display-only: Used for UI logging/telemetry only
#[derive(Debug, Default)]
pub struct NormalizedMessages {
    /// Messages to send to the model (API request).
    pub model_visible: Vec<InjectedMessage>,
    /// Messages for UI display only (not sent to API).
    pub display_only: Vec<InjectedMessage>,
}

/// Normalize injected messages by splitting into model-visible and display-only.
///
/// This function processes reminders and categorizes them:
/// - Silent reminders become display-only (zero tokens to API)
/// - Non-silent reminders become model-visible
///
/// # Claude Code Alignment
///
/// This matches Claude Code v2.1.38's behavior:
/// - SilentAttachment types never enter model input
/// - The normalizer returns [] for already_read_file type
/// - UI handles display separately via metadata
///
/// # Arguments
///
/// * `reminders` - The reminders to normalize
///
/// # Returns
///
/// A `NormalizedMessages` struct with messages split by visibility.
pub fn normalize_injected_messages(reminders: Vec<SystemReminder>) -> NormalizedMessages {
    let mut result = NormalizedMessages::default();

    for reminder in reminders {
        // Extract fields before consuming output
        let xml_tag = reminder.xml_tag();
        let attachment_type = reminder.attachment_type;
        let is_meta = reminder.is_meta;

        // Check if this is a silent reminder
        let is_silent = reminder.is_silent || reminder.output.is_silent();

        match reminder.output {
            ReminderOutput::Text(content) => {
                let wrapped = wrap_with_tag(&content, xml_tag);
                let msg = InjectedMessage::UserText {
                    content: wrapped,
                    is_meta,
                };

                if is_silent {
                    debug!("Silent text reminder {} -> display_only", attachment_type);
                    result.display_only.push(msg);
                } else {
                    result.model_visible.push(msg);
                }
            }
            ReminderOutput::Messages(msgs) => {
                let messages: Vec<InjectedMessage> = msgs
                    .into_iter()
                    .map(|msg| {
                        let blocks: Vec<InjectedBlock> = msg
                            .blocks
                            .into_iter()
                            .map(|b| match b {
                                ContentBlock::Text { text } => InjectedBlock::Text(text),
                                ContentBlock::ToolUse { id, name, input } => {
                                    InjectedBlock::ToolUse { id, name, input }
                                }
                                ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                } => InjectedBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                },
                            })
                            .collect();

                        match msg.role {
                            MessageRole::Assistant => InjectedMessage::AssistantBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            },
                            MessageRole::User => InjectedMessage::UserBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            },
                        }
                    })
                    .collect();

                if is_silent {
                    debug!(
                        "Silent messages reminder {} -> display_only",
                        attachment_type
                    );
                    result.display_only.extend(messages);
                } else {
                    result.model_visible.extend(messages);
                }
            }
            ReminderOutput::ModelAttachment { payload } => {
                let content = match serde_json::to_string_pretty(&payload) {
                    Ok(json) => json,
                    Err(_) => payload.to_string(),
                };
                let wrapped = wrap_with_tag(&content, xml_tag);
                let msg = InjectedMessage::UserText {
                    content: wrapped,
                    is_meta,
                };
                result.model_visible.push(msg);
            }
            // Silent variants go to display_only
            ReminderOutput::Silent => {
                debug!("Silent reminder {} -> display_only", attachment_type);
            }
            ReminderOutput::SilentText { content } => {
                let msg = InjectedMessage::UserText { content, is_meta };
                result.display_only.push(msg);
            }
            ReminderOutput::SilentMessages { messages: msgs } => {
                let messages: Vec<InjectedMessage> = msgs
                    .into_iter()
                    .map(|msg| {
                        let blocks: Vec<InjectedBlock> = msg
                            .blocks
                            .into_iter()
                            .map(|b| match b {
                                ContentBlock::Text { text } => InjectedBlock::Text(text),
                                ContentBlock::ToolUse { id, name, input } => {
                                    InjectedBlock::ToolUse { id, name, input }
                                }
                                ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                } => InjectedBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                },
                            })
                            .collect();

                        match msg.role {
                            MessageRole::Assistant => InjectedMessage::AssistantBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            },
                            MessageRole::User => InjectedMessage::UserBlocks {
                                blocks,
                                is_meta: msg.is_meta,
                            },
                        }
                    })
                    .collect();
                result.display_only.extend(messages);
            }
            ReminderOutput::SilentAttachment { payload: _ } => {
                // Silent attachments have structured metadata for UI display
                // They don't become messages - just use the metadata
                debug!(
                    "Silent attachment {} with payload -> display metadata",
                    attachment_type
                );
            }
        }
    }

    result
}

impl InjectionStats {
    /// Create stats from a list of reminders.
    pub fn from_reminders(reminders: &[SystemReminder]) -> Self {
        let mut stats = Self::default();

        for reminder in reminders {
            stats.total_count += 1;

            // Track silent reminders (check both flag and output variant)
            if reminder.is_silent || reminder.output.is_silent() {
                stats.silent_count += 1;
            }

            match &reminder.output {
                ReminderOutput::Text(content) => {
                    stats.total_bytes += content.len() as i64;
                }
                ReminderOutput::Messages(msgs) => {
                    stats.multi_message_count += 1;
                    // Estimate size for multi-message reminders
                    for msg in msgs {
                        for block in &msg.blocks {
                            if let crate::types::ContentBlock::Text { text } = block {
                                stats.total_bytes += text.len() as i64;
                            }
                        }
                    }
                }
                ReminderOutput::ModelAttachment { payload } => {
                    stats.model_attachment_count += 1;
                    stats.total_bytes += payload.to_string().len() as i64;
                }
                // Silent variants don't contribute to token count
                ReminderOutput::Silent
                | ReminderOutput::SilentText { .. }
                | ReminderOutput::SilentMessages { .. }
                | ReminderOutput::SilentAttachment { .. } => {}
            }
            *stats
                .by_type
                .entry(reminder.attachment_type.name().to_string())
                .or_default() += 1;
        }

        stats
    }
}

#[cfg(test)]
#[path = "inject.test.rs"]
mod tests;
