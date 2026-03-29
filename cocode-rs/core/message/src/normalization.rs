//! Message normalization for API requests.
//!
//! This module handles transforming tracked messages into the format
//! expected by the API, similar to Claude Code's `normalization.ts`.

use crate::tracked::TrackedMessage;
use cocode_inference::AssistantContentPart;
use cocode_inference::DataContent;
use cocode_inference::FilePart;
use cocode_inference::LanguageModelMessage;
use cocode_inference::ReasoningPart;
use cocode_inference::TextPart;
use cocode_inference::ToolResultContent;
use cocode_inference::UserContentPart;

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
) -> Vec<LanguageModelMessage> {
    let mut normalized = Vec::new();

    for tracked in messages {
        // Skip tombstoned messages if configured
        if options.skip_tombstoned && tracked.is_tombstoned() {
            continue;
        }

        // Skip empty messages if configured
        if !options.include_empty && crate::type_guards::is_empty_message(&tracked.inner) {
            continue;
        }

        let mut message = tracked.inner.clone();

        // Strip thinking signatures if needed
        if options.strip_thinking_signatures {
            message = strip_thinking_signatures(&message);
        }

        // Merge with previous if consecutive same role
        if options.merge_consecutive
            && let Some(last) = normalized.last_mut()
            && can_merge(last, &message)
        {
            merge_messages(last, &message);
            continue;
        }

        normalized.push(message);
    }

    normalized
}

/// Get a role-like discriminant for merging comparison.
fn message_role_key(msg: &LanguageModelMessage) -> u8 {
    match msg {
        LanguageModelMessage::System { .. } => 0,
        LanguageModelMessage::User { .. } => 1,
        LanguageModelMessage::Assistant { .. } => 2,
        LanguageModelMessage::Tool { .. } => 3,
    }
}

/// Check if two messages can be merged.
fn can_merge(a: &LanguageModelMessage, b: &LanguageModelMessage) -> bool {
    // Can only merge consecutive messages of the same role
    if message_role_key(a) != message_role_key(b) {
        return false;
    }

    // Don't merge assistant messages if either has tool use/result blocks
    if let LanguageModelMessage::Assistant { content: ca, .. } = a
        && let LanguageModelMessage::Assistant { content: cb, .. } = b
    {
        let has_tool_blocks = |content: &[AssistantContentPart]| {
            content.iter().any(|b| {
                matches!(
                    b,
                    AssistantContentPart::ToolCall(_) | AssistantContentPart::ToolResult(_)
                )
            })
        };
        return !has_tool_blocks(ca) && !has_tool_blocks(cb);
    }

    // For user messages, don't merge if either has special content
    if matches!(a, LanguageModelMessage::User { .. })
        && matches!(b, LanguageModelMessage::User { .. })
    {
        return true;
    }

    // Don't merge system or tool messages
    false
}

/// Merge two messages by appending content.
fn merge_messages(target: &mut LanguageModelMessage, source: &LanguageModelMessage) {
    match (target, source) {
        (
            LanguageModelMessage::User {
                content: target_content,
                ..
            },
            LanguageModelMessage::User {
                content: source_content,
                ..
            },
        ) => {
            target_content.extend(source_content.iter().cloned());
        }
        (
            LanguageModelMessage::Assistant {
                content: target_content,
                ..
            },
            LanguageModelMessage::Assistant {
                content: source_content,
                ..
            },
        ) => {
            target_content.extend(source_content.iter().cloned());
        }
        _ => {}
    }
}

/// Strip thinking signatures from a message.
///
/// Creates fresh `ReasoningPart`s that keep the reasoning *text* but drop
/// `provider_metadata` (which holds provider-specific signatures such as
/// Anthropic's `signature`/`redactedData` or Google's `thoughtSignature`).
fn strip_thinking_signatures(message: &LanguageModelMessage) -> LanguageModelMessage {
    match message {
        LanguageModelMessage::Assistant {
            content,
            provider_options,
        } => {
            let content = content
                .iter()
                .map(|block| match block {
                    AssistantContentPart::Reasoning(rp) => {
                        if rp.provider_metadata.is_some() {
                            tracing::trace!(
                                text_len = rp.text.len(),
                                "Stripped provider_metadata from reasoning part"
                            );
                        }
                        AssistantContentPart::Reasoning(ReasoningPart::new(&rp.text))
                    }
                    other => other.clone(),
                })
                .collect();

            LanguageModelMessage::Assistant {
                content,
                provider_options: provider_options.clone(),
            }
        }
        other => other.clone(),
    }
}

/// Validate that messages are suitable for API request.
///
/// Returns errors if the message sequence is invalid.
pub fn validate_messages(messages: &[LanguageModelMessage]) -> Result<(), ValidationError> {
    if messages.is_empty() {
        return Err(ValidationError::EmptyMessages);
    }

    // Check for proper alternation
    let mut last_role_key: Option<u8> = None;
    for (idx, msg) in messages.iter().enumerate() {
        let role_key = message_role_key(msg);

        // System message can only be first
        if msg.is_system() && idx > 0 {
            return Err(ValidationError::SystemNotFirst { index: idx });
        }

        // Check User/Assistant alternation (Tool messages exempt as they follow Assistant)
        if !msg.is_system()
            && !msg.is_tool()
            && let Some(prev_key) = last_role_key
        {
            // Skip alternation check if previous was System or Tool
            let prev_is_system = prev_key == 0;
            let prev_is_tool = prev_key == 3;
            if !prev_is_system && !prev_is_tool {
                // Consecutive User or Assistant messages are not allowed
                if role_key == prev_key {
                    return Err(ValidationError::InvalidAlternation {
                        index: idx,
                        expected: if msg.is_user() { "assistant" } else { "user" },
                        found: if msg.is_user() { "user" } else { "assistant" },
                    });
                }
            }
        }

        // Check for proper tool result pairing
        if let LanguageModelMessage::Tool { content, .. } = msg {
            for part in content {
                if let cocode_inference::ToolContentPart::ToolResult(result_part) = part
                    && !has_matching_tool_use(messages, idx, &result_part.tool_call_id)
                {
                    return Err(ValidationError::OrphanToolResult {
                        tool_use_id: result_part.tool_call_id.clone(),
                    });
                }
            }
        }

        last_role_key = Some(role_key);
    }

    Ok(())
}

/// Check if there's a matching tool use for a tool result.
fn has_matching_tool_use(
    messages: &[LanguageModelMessage],
    current_idx: usize,
    tool_use_id: &str,
) -> bool {
    // Look backwards for a matching tool use
    for msg in messages[..current_idx].iter().rev() {
        if let LanguageModelMessage::Assistant { content, .. } = msg {
            for block in content {
                if let AssistantContentPart::ToolCall(tc) = block
                    && tc.tool_call_id == tool_use_id
                {
                    return true;
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
        expected: &'static str,
        found: &'static str,
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
                    "Invalid role alternation at index {index}: expected {expected}, found {found}"
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Count tokens in messages (rough estimate).
pub fn estimate_tokens(messages: &[LanguageModelMessage]) -> i32 {
    messages
        .iter()
        .map(|m| match m {
            LanguageModelMessage::System { content, .. } => (content.len() / 4) as i32,
            LanguageModelMessage::User { content, .. } => content
                .iter()
                .map(|p| match p {
                    UserContentPart::Text(TextPart { text, .. }) => (text.len() / 4) as i32,
                    UserContentPart::File(fp) => estimate_image_tokens(fp),
                })
                .sum(),
            LanguageModelMessage::Assistant { content, .. } => content
                .iter()
                .map(|b| match b {
                    AssistantContentPart::Text(TextPart { text, .. }) => (text.len() / 4) as i32,
                    AssistantContentPart::Reasoning(rp) => (rp.text.len() / 4) as i32,
                    AssistantContentPart::File(_) | AssistantContentPart::ReasoningFile(_) => 1000,
                    AssistantContentPart::ToolCall(tc) => (tc.input.to_string().len() / 4) as i32,
                    AssistantContentPart::ToolResult(tr) => match &tr.output {
                        ToolResultContent::Text { value, .. } => (value.len() / 4) as i32,
                        ToolResultContent::Json { value, .. } => {
                            (value.to_string().len() / 4) as i32
                        }
                        ToolResultContent::Content { value, .. } => value.len() as i32 * 100,
                        ToolResultContent::ErrorText { value, .. } => (value.len() / 4) as i32,
                        ToolResultContent::ErrorJson { value, .. } => {
                            (value.to_string().len() / 4) as i32
                        }
                        ToolResultContent::ExecutionDenied { .. } => 10,
                    },
                    AssistantContentPart::Source(_) => 10,
                    AssistantContentPart::ToolApprovalRequest(_) => 10,
                    AssistantContentPart::Custom(_) => 10,
                })
                .sum(),
            LanguageModelMessage::Tool { content, .. } => content
                .iter()
                .map(|p| match p {
                    cocode_inference::ToolContentPart::ToolResult(tr) => match &tr.output {
                        ToolResultContent::Text { value, .. } => (value.len() / 4) as i32,
                        ToolResultContent::Json { value, .. } => {
                            (value.to_string().len() / 4) as i32
                        }
                        ToolResultContent::Content { value, .. } => value.len() as i32 * 100,
                        ToolResultContent::ErrorText { value, .. } => (value.len() / 4) as i32,
                        ToolResultContent::ErrorJson { value, .. } => {
                            (value.to_string().len() / 4) as i32
                        }
                        ToolResultContent::ExecutionDenied { .. } => 10,
                    },
                    cocode_inference::ToolContentPart::ToolApprovalResponse(_) => 10,
                })
                .sum(),
        })
        .sum()
}

/// Estimate tokens for an image content block.
///
/// Anthropic bills image tokens as `ceil(width * height / 750)` with a
/// minimum of 85 tokens (the cost of a 258×258 tile).  Without decoded
/// dimensions we approximate from the encoded data size:
///
///   decoded_bytes ≈ base64_len × 3/4
///   tokens        ≈ decoded_bytes / 750   (min 85)
///
/// URL-only images have unknown size — we fall back to 1000 tokens.
fn estimate_image_tokens(fp: &FilePart) -> i32 {
    let decoded_bytes = match &fp.data {
        DataContent::Base64(b64) => (b64.len() as i64 * 3) / 4,
        DataContent::Bytes(bytes) => bytes.len() as i64,
        DataContent::Url(_) => return 1000,
    };
    (decoded_bytes / 750).max(85) as i32
}

/// Maximum number of image/document content blocks allowed in the context.
///
/// Matches Claude Code's `MAX_IMAGES_IN_CONTEXT` (PA4 = 100).
pub const MAX_IMAGES_IN_CONTEXT: i32 = 100;

/// Trim excess image content blocks, removing the oldest first.
///
/// Matches Claude Code's `trimImageCount` (`q9z`). Strategy: count all
/// image/file blocks across the conversation, then strip from the earliest
/// messages so the most recent visual context is preserved for the model.
pub fn trim_image_count(messages: &mut [LanguageModelMessage], max_images: i32) {
    // Count total images across all messages
    let total: i32 = messages.iter().map(count_images_in_message).sum();

    let mut excess = total - max_images;
    if excess <= 0 {
        return;
    }

    // Remove from oldest messages first (front of the vec)
    for msg in messages.iter_mut() {
        if excess <= 0 {
            break;
        }
        remove_images_from_message(msg, &mut excess);
    }
}

/// Count image/file blocks in a single message.
fn count_images_in_message(msg: &LanguageModelMessage) -> i32 {
    match msg {
        LanguageModelMessage::User { content, .. } => content
            .iter()
            .filter(|p| matches!(p, UserContentPart::File(_)))
            .count() as i32,
        LanguageModelMessage::Assistant { content, .. } => content
            .iter()
            .filter(|p| matches!(p, AssistantContentPart::File(_)))
            .count() as i32,
        _ => 0,
    }
}

/// Remove image blocks from a message, decrementing excess counter.
fn remove_images_from_message(msg: &mut LanguageModelMessage, excess: &mut i32) {
    match msg {
        LanguageModelMessage::User { content, .. } => {
            content.retain(|p| {
                if *excess > 0 && matches!(p, UserContentPart::File(_)) {
                    *excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
        LanguageModelMessage::Assistant { content, .. } => {
            content.retain(|p| {
                if *excess > 0 && matches!(p, AssistantContentPart::File(_)) {
                    *excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "normalization.test.rs"]
mod tests;
