//! Type guards for content blocks and messages.
//!
//! These utilities help identify and extract specific content types from
//! messages, similar to Claude Code's `type-guards.ts`.

use hyper_sdk::ContentBlock;
use hyper_sdk::Message;
use hyper_sdk::Role;
use hyper_sdk::ToolCall;

/// Check if a content block is a text block.
pub fn is_text_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::Text { .. })
}

/// Check if a content block is a tool use block.
pub fn is_tool_use_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::ToolUse { .. })
}

/// Check if a content block is a tool result block.
pub fn is_tool_result_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::ToolResult { .. })
}

/// Check if a content block is a thinking block.
pub fn is_thinking_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::Thinking { .. })
}

/// Check if a content block is an image block.
pub fn is_image_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::Image { .. })
}

/// Extract text from a text block.
pub fn extract_text(block: &ContentBlock) -> Option<&str> {
    match block {
        ContentBlock::Text { text } => Some(text),
        _ => None,
    }
}

/// Extract thinking content from a thinking block.
pub fn extract_thinking(block: &ContentBlock) -> Option<&str> {
    match block {
        ContentBlock::Thinking { content, .. } => Some(content),
        _ => None,
    }
}

/// Extract tool use details from a tool use block.
pub fn extract_tool_use(block: &ContentBlock) -> Option<(&str, &str, &serde_json::Value)> {
    match block {
        ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
        _ => None,
    }
}

/// Extract tool result details from a tool result block.
pub fn extract_tool_result(
    block: &ContentBlock,
) -> Option<(&str, &hyper_sdk::ToolResultContent, bool)> {
    match block {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => Some((tool_use_id, content, *is_error)),
        _ => None,
    }
}

/// Check if a message contains any tool use blocks.
pub fn has_tool_use(message: &Message) -> bool {
    message.content.iter().any(is_tool_use_block)
}

/// Check if a message contains any tool result blocks.
pub fn has_tool_result(message: &Message) -> bool {
    message.content.iter().any(is_tool_result_block)
}

/// Check if a message contains any thinking blocks.
pub fn has_thinking(message: &Message) -> bool {
    message.content.iter().any(is_thinking_block)
}

/// Check if a message is empty (no content blocks).
pub fn is_empty_message(message: &Message) -> bool {
    message.content.is_empty()
}

/// Check if a message is a user message.
pub fn is_user_message(message: &Message) -> bool {
    message.role == Role::User
}

/// Check if a message is an assistant message.
pub fn is_assistant_message(message: &Message) -> bool {
    message.role == Role::Assistant
}

/// Check if a message is a system message.
pub fn is_system_message(message: &Message) -> bool {
    message.role == Role::System
}

/// Check if a message is a tool message.
pub fn is_tool_message(message: &Message) -> bool {
    message.role == Role::Tool
}

/// Get all text content from a message.
pub fn get_text_content(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(extract_text)
        .collect::<Vec<_>>()
        .join("")
}

/// Get all tool calls from a message.
pub fn get_tool_calls(message: &Message) -> Vec<ToolCall> {
    message
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => {
                Some(ToolCall::new(id, name, input.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Get the thinking content from a message if present.
pub fn get_thinking_content(message: &Message) -> Option<String> {
    message.content.iter().find_map(|b| match b {
        ContentBlock::Thinking { content, .. } => Some(content.clone()),
        _ => None,
    })
}

/// Count the number of tool use blocks in a message.
pub fn count_tool_uses(message: &Message) -> usize {
    message
        .content
        .iter()
        .filter(|b| is_tool_use_block(b))
        .count()
}

/// Count the number of tool result blocks in a message.
pub fn count_tool_results(message: &Message) -> usize {
    message
        .content
        .iter()
        .filter(|b| is_tool_result_block(b))
        .count()
}

#[cfg(test)]
#[path = "type_guards.test.rs"]
mod tests;
