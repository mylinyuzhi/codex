//! Message normalization for API requests.
//!
//! This module handles transforming tracked messages into the format
//! expected by the API, similar to Claude Code's `normalization.ts`.

use crate::tracked::TrackedMessage;
use hyper_sdk::ContentBlock;
use hyper_sdk::Message;
use hyper_sdk::Role;

/// Options for message normalization.
#[derive(Debug, Clone, Default)]
pub struct NormalizationOptions {
    /// Remove tombstoned messages.
    pub skip_tombstoned: bool,
    /// Merge consecutive messages from the same role.
    pub merge_consecutive: bool,
    /// Strip thinking signatures (for cross-provider compatibility).
    pub strip_thinking_signatures: bool,
    /// Include empty messages.
    pub include_empty: bool,
}

impl NormalizationOptions {
    /// Create options for API requests.
    pub fn for_api() -> Self {
        Self {
            skip_tombstoned: true,
            merge_consecutive: true,
            strip_thinking_signatures: false,
            include_empty: false,
        }
    }

    /// Create options for logging/debugging.
    pub fn for_debug() -> Self {
        Self {
            skip_tombstoned: false,
            merge_consecutive: false,
            strip_thinking_signatures: false,
            include_empty: true,
        }
    }
}

/// Normalize tracked messages for API requests.
///
/// This function transforms a list of tracked messages into the format
/// expected by the API, applying any necessary transformations.
pub fn normalize_messages_for_api(
    messages: &[TrackedMessage],
    options: &NormalizationOptions,
) -> Vec<Message> {
    let mut normalized = Vec::new();

    for tracked in messages {
        // Skip tombstoned messages if configured
        if options.skip_tombstoned && tracked.is_tombstoned() {
            continue;
        }

        // Skip empty messages if configured
        if !options.include_empty && tracked.inner.content.is_empty() {
            continue;
        }

        let mut message = tracked.inner.clone();

        // Strip thinking signatures if needed
        if options.strip_thinking_signatures {
            message = strip_thinking_signatures(&message);
        }

        // Merge with previous if consecutive same role
        if options.merge_consecutive {
            if let Some(last) = normalized.last_mut() {
                if can_merge(last, &message) {
                    merge_messages(last, &message);
                    continue;
                }
            }
        }

        normalized.push(message);
    }

    normalized
}

/// Check if two messages can be merged.
fn can_merge(a: &Message, b: &Message) -> bool {
    // Can only merge consecutive messages of the same role
    if a.role != b.role {
        return false;
    }

    // Don't merge if either has tool use/result blocks
    let has_tool_blocks = |m: &Message| {
        m.content.iter().any(|b| {
            matches!(
                b,
                ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
            )
        })
    };

    !has_tool_blocks(a) && !has_tool_blocks(b)
}

/// Merge two messages by appending content.
fn merge_messages(target: &mut Message, source: &Message) {
    for block in &source.content {
        target.content.push(block.clone());
    }
}

/// Strip thinking signatures from a message.
fn strip_thinking_signatures(message: &Message) -> Message {
    let content = message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Thinking { content, .. } => ContentBlock::Thinking {
                content: content.clone(),
                signature: None,
            },
            other => other.clone(),
        })
        .collect();

    Message {
        role: message.role,
        content,
        provider_options: message.provider_options.clone(),
        metadata: message.metadata.clone(),
    }
}

/// Validate that messages are suitable for API request.
///
/// Returns errors if the message sequence is invalid.
pub fn validate_messages(messages: &[Message]) -> Result<(), ValidationError> {
    if messages.is_empty() {
        return Err(ValidationError::EmptyMessages);
    }

    // Check for proper alternation
    let mut last_role: Option<Role> = None;
    for (idx, msg) in messages.iter().enumerate() {
        // System message can only be first
        if msg.role == Role::System && idx > 0 {
            return Err(ValidationError::SystemNotFirst { index: idx });
        }

        // Check User/Assistant alternation (Tool messages exempt as they follow Assistant)
        if msg.role != Role::System && msg.role != Role::Tool {
            if let Some(prev_role) = last_role {
                // Skip alternation check if previous was System
                if prev_role != Role::System && prev_role != Role::Tool {
                    // Consecutive User or Assistant messages are not allowed
                    if msg.role == prev_role {
                        return Err(ValidationError::InvalidAlternation {
                            index: idx,
                            expected: if msg.role == Role::User {
                                Role::Assistant
                            } else {
                                Role::User
                            },
                            found: msg.role,
                        });
                    }
                }
            }
        }

        // Check for proper tool result pairing
        if msg.role == Role::User {
            for block in &msg.content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    // Tool result should follow an assistant message with matching tool use
                    if !has_matching_tool_use(messages, idx, tool_use_id) {
                        return Err(ValidationError::OrphanToolResult {
                            tool_use_id: tool_use_id.clone(),
                        });
                    }
                }
            }
        }

        last_role = Some(msg.role);
    }

    Ok(())
}

/// Check if there's a matching tool use for a tool result.
fn has_matching_tool_use(messages: &[Message], current_idx: usize, tool_use_id: &str) -> bool {
    // Look backwards for a matching tool use
    for msg in messages[..current_idx].iter().rev() {
        if msg.role == Role::Assistant {
            for block in &msg.content {
                if let ContentBlock::ToolUse { id, .. } = block {
                    if id == tool_use_id {
                        return true;
                    }
                }
            }
            // If we hit an assistant message without the tool use, stop looking
            break;
        }
    }
    false
}

/// Validation errors for message sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Message list is empty.
    EmptyMessages,
    /// System message is not first.
    SystemNotFirst { index: usize },
    /// Tool result without matching tool use.
    OrphanToolResult { tool_use_id: String },
    /// Invalid role alternation (consecutive User or Assistant).
    InvalidAlternation {
        index: usize,
        expected: Role,
        found: Role,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::EmptyMessages => write!(f, "Message list is empty"),
            ValidationError::SystemNotFirst { index } => {
                write!(f, "System message at index {index} is not first")
            }
            ValidationError::OrphanToolResult { tool_use_id } => {
                write!(
                    f,
                    "Tool result for '{tool_use_id}' has no matching tool use"
                )
            }
            ValidationError::InvalidAlternation {
                index,
                expected,
                found,
            } => {
                write!(
                    f,
                    "Invalid role alternation at index {index}: expected {expected:?}, found {found:?}"
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Count tokens in messages (rough estimate).
pub fn estimate_tokens(messages: &[Message]) -> i32 {
    messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => (text.len() / 4) as i32,
                    ContentBlock::Thinking { content, .. } => (content.len() / 4) as i32,
                    ContentBlock::Image { .. } => 1000,
                    ContentBlock::ToolUse { input, .. } => (input.to_string().len() / 4) as i32,
                    ContentBlock::ToolResult { content, .. } => {
                        use hyper_sdk::ToolResultContent;
                        match content {
                            ToolResultContent::Text(t) => (t.len() / 4) as i32,
                            ToolResultContent::Json(v) => (v.to_string().len() / 4) as i32,
                            ToolResultContent::Blocks(blocks) => blocks.len() as i32 * 100,
                        }
                    }
                })
                .sum::<i32>()
        })
        .sum()
}

#[cfg(test)]
#[path = "normalization.test.rs"]
mod tests;
