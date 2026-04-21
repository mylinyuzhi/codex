//! Token count estimation utilities.
//!
//! TS: services/tokenEstimation.ts — rough estimation from character count,
//! with adjustment for code vs prose.

use coco_types::Message;

/// Rough estimate of token count from character count.
///
/// Claude tokenization averages ~3.5-4 chars per token for English prose,
/// ~2.5-3 for code. We use 4 as a conservative default.
const CHARS_PER_TOKEN: f64 = 4.0;

/// Estimate tokens from a string.
pub fn estimate_tokens(text: &str) -> i64 {
    (text.len() as f64 / CHARS_PER_TOKEN).ceil() as i64
}

/// Estimate tokens for a set of messages.
pub fn estimate_tokens_for_messages(messages: &[Message]) -> i64 {
    let mut total = 0i64;

    for msg in messages {
        // Base overhead per message (role, formatting)
        total += 4;

        match msg {
            Message::User(u) => {
                total += estimate_llm_message_tokens(&u.message);
            }
            Message::Assistant(a) => {
                total += estimate_llm_message_tokens(&a.message);
            }
            Message::ToolResult(t) => {
                total += estimate_llm_message_tokens(&t.message);
            }
            Message::Attachment(a) => {
                // Only API-bound attachments consume token budget; silent /
                // file / unit bodies don't reach the LLM.
                if let Some(msg) = a.as_api_message() {
                    total += estimate_llm_message_tokens(msg);
                }
            }
            _ => {
                total += 10; // Small overhead for system/progress/tombstone
            }
        }
    }

    total
}

fn estimate_llm_message_tokens(msg: &coco_types::LlmMessage) -> i64 {
    match msg {
        coco_types::LlmMessage::User { content, .. } => {
            content
                .iter()
                .map(|c| match c {
                    coco_types::UserContent::Text(t) => estimate_tokens(&t.text),
                    _ => 100, // Images, files: rough estimate
                })
                .sum()
        }
        coco_types::LlmMessage::Assistant { content, .. } => content
            .iter()
            .map(|c| match c {
                coco_types::AssistantContent::Text(t) => estimate_tokens(&t.text),
                coco_types::AssistantContent::ToolCall(tc) => {
                    estimate_tokens(&tc.tool_name)
                        + estimate_tokens(&serde_json::to_string(&tc.input).unwrap_or_default())
                }
                _ => 50,
            })
            .sum(),
        coco_types::LlmMessage::System { content, .. } => estimate_tokens(content),
        coco_types::LlmMessage::Tool { content, .. } => content
            .iter()
            .map(|c| match c {
                coco_types::ToolContent::ToolResult(r) => {
                    estimate_tokens(&format!("{:?}", r.output)) + 10
                }
                _ => 20,
            })
            .sum(),
    }
}

/// Check if the current token count exceeds a percentage of the context window.
pub fn is_over_threshold(current_tokens: i64, context_window: i64, threshold_pct: i32) -> bool {
    if context_window <= 0 {
        return false;
    }
    let threshold = context_window * threshold_pct as i64 / 100;
    current_tokens >= threshold
}

#[cfg(test)]
#[path = "token_estimation.test.rs"]
mod tests;
