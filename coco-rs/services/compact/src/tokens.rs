//! Token estimation by walking message content parts.
//!
//! Replaces the fragile `format!("{:?}")` approach with proper content-part
//! traversal. Uses ~4 chars/token heuristic (same as TS).

use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::ToolContent;
use coco_types::UserContent;

/// Estimate tokens for a single message.
pub fn estimate_message_tokens(msg: &Message) -> i64 {
    let chars = message_char_count(msg);
    chars_to_tokens(chars)
}

/// Estimate tokens for a slice of messages.
pub fn estimate_tokens(messages: &[Message]) -> i64 {
    let total_chars: i64 = messages.iter().map(message_char_count).sum();
    chars_to_tokens(total_chars)
}

/// Conservative token estimate: `(chars / 4) * 4 / 3` (~33% padding).
/// Matches TS `estimateMessageTokens` padding.
pub fn estimate_tokens_conservative(messages: &[Message]) -> i64 {
    let base = estimate_tokens(messages);
    base * 4 / 3
}

/// Estimate tokens for plain text.
pub fn estimate_text_tokens(text: &str) -> i64 {
    chars_to_tokens(text.len() as i64)
}

/// Extract all text content from a message (for building summary prompts).
pub fn extract_message_text(msg: &Message) -> Option<String> {
    match msg {
        Message::User(u) => extract_llm_message_text(&u.message),
        Message::Assistant(a) => extract_llm_message_text(&a.message),
        Message::ToolResult(tr) => extract_llm_message_text(&tr.message),
        Message::System(_) => Some("[system]".to_string()),
        Message::Attachment(a) => extract_llm_message_text(&a.message),
        _ => None,
    }
}

/// Estimate tokens for a tool result (used by micro-compaction).
pub fn estimate_tool_result_tokens(tr: &coco_types::ToolResultMessage) -> i64 {
    let chars = llm_message_char_count(&tr.message);
    chars_to_tokens(chars)
}

// ── Internal helpers ────────────────────────────────────────────────

fn chars_to_tokens(chars: i64) -> i64 {
    // ~4 characters per token heuristic
    chars / 4
}

fn message_char_count(msg: &Message) -> i64 {
    match msg {
        Message::User(u) => llm_message_char_count(&u.message),
        Message::Assistant(a) => llm_message_char_count(&a.message),
        Message::ToolResult(tr) => llm_message_char_count(&tr.message),
        Message::Attachment(a) => llm_message_char_count(&a.message),
        Message::System(_) => 20, // minimal overhead
        Message::Progress(_) | Message::Tombstone(_) | Message::ToolUseSummary(_) => 0,
    }
}

fn llm_message_char_count(msg: &LlmMessage) -> i64 {
    match msg {
        LlmMessage::System { content, .. } => content.len() as i64,
        LlmMessage::User { content, .. } => user_content_chars(content),
        LlmMessage::Assistant { content, .. } => assistant_content_chars(content),
        LlmMessage::Tool { content, .. } => tool_content_chars(content),
    }
}

fn user_content_chars(parts: &[UserContent]) -> i64 {
    parts
        .iter()
        .map(|p| match p {
            UserContent::Text(t) => t.text.len() as i64,
            UserContent::File(f) => {
                // Images/documents: use a fixed estimate
                f.filename.as_ref().map_or(0, |n| n.len() as i64)
                    + crate::types::IMAGE_MAX_TOKEN_SIZE * 4 // convert back to chars
            }
        })
        .sum()
}

fn assistant_content_chars(parts: &[AssistantContent]) -> i64 {
    parts
        .iter()
        .map(|p| match p {
            AssistantContent::Text(t) => t.text.len() as i64,
            AssistantContent::Reasoning(r) => r.text.len() as i64,
            AssistantContent::ToolCall(tc) => {
                tc.tool_name.len() as i64 + tc.input.to_string().len() as i64
            }
            _ => 50, // File, ReasoningFile, Custom, ToolResult, Source — small overhead
        })
        .sum()
}

fn tool_content_chars(parts: &[ToolContent]) -> i64 {
    parts
        .iter()
        .map(|p| match p {
            ToolContent::ToolResult(tr) => tool_result_content_chars(&tr.output),
            _ => 20,
        })
        .sum()
}

fn tool_result_content_chars(output: &vercel_ai_provider::ToolResultContent) -> i64 {
    match output {
        vercel_ai_provider::ToolResultContent::Text { value, .. } => value.len() as i64,
        vercel_ai_provider::ToolResultContent::Json { value, .. } => value.to_string().len() as i64,
        vercel_ai_provider::ToolResultContent::Content { value, .. } => value
            .iter()
            .map(|part| match part {
                vercel_ai_provider::ToolResultContentPart::Text { text, .. } => text.len() as i64,
                _ => 100, // file data, images — small estimate
            })
            .sum(),
        vercel_ai_provider::ToolResultContent::ExecutionDenied { reason, .. } => {
            reason.as_ref().map_or(20, |r| r.len() as i64)
        }
        vercel_ai_provider::ToolResultContent::ErrorText { value, .. } => value.len() as i64,
        vercel_ai_provider::ToolResultContent::ErrorJson { value, .. } => {
            value.to_string().len() as i64
        }
    }
}

fn extract_llm_message_text(msg: &LlmMessage) -> Option<String> {
    match msg {
        LlmMessage::System { content, .. } => Some(content.clone()),
        LlmMessage::User { content, .. } => {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|p| match p {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        LlmMessage::Assistant { content, .. } => {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|p| match p {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        LlmMessage::Tool { content, .. } => {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|p| match p {
                    ToolContent::ToolResult(tr) => Some(format!(
                        "tool_use_id={}: {}",
                        tr.tool_call_id,
                        tool_result_text(&tr.output)
                    )),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
    }
}

fn tool_result_text(output: &vercel_ai_provider::ToolResultContent) -> String {
    match output {
        vercel_ai_provider::ToolResultContent::Text { value, .. } => value.clone(),
        vercel_ai_provider::ToolResultContent::Json { value, .. } => value.to_string(),
        vercel_ai_provider::ToolResultContent::Content { value, .. } => value
            .iter()
            .filter_map(|p| match p {
                vercel_ai_provider::ToolResultContentPart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        vercel_ai_provider::ToolResultContent::ExecutionDenied { reason, .. } => {
            reason.clone().unwrap_or_default()
        }
        vercel_ai_provider::ToolResultContent::ErrorText { value, .. } => value.clone(),
        vercel_ai_provider::ToolResultContent::ErrorJson { value, .. } => value.to_string(),
    }
}

#[cfg(test)]
#[path = "tokens.test.rs"]
mod tests;
