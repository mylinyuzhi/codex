//! Type guards for content blocks and messages.
//!
//! These utilities help identify and extract specific content types from
//! messages, similar to Claude Code's `type-guards.ts`.

use cocode_inference::AssistantContentPart;
use cocode_inference::LanguageModelMessage;
use cocode_inference::TextPart;
use cocode_inference::ToolCallPart;
use cocode_inference::ToolResultPart;
use cocode_inference::UserContentPart;

/// Check if a content block is a text block.
pub fn is_text_block(block: &AssistantContentPart) -> bool {
    matches!(block, AssistantContentPart::Text(_))
}

/// Check if a content block is a tool use block.
pub fn is_tool_use_block(block: &AssistantContentPart) -> bool {
    matches!(block, AssistantContentPart::ToolCall(_))
}

/// Check if a content block is a tool result block.
pub fn is_tool_result_block(block: &AssistantContentPart) -> bool {
    matches!(block, AssistantContentPart::ToolResult(_))
}

/// Check if a content block is a thinking/reasoning block.
pub fn is_thinking_block(block: &AssistantContentPart) -> bool {
    matches!(block, AssistantContentPart::Reasoning(_))
}

/// Check if a content block is an image/file block.
pub fn is_image_block(block: &AssistantContentPart) -> bool {
    matches!(block, AssistantContentPart::File(_))
}

/// Extract text from a text block.
pub fn extract_text(block: &AssistantContentPart) -> Option<&str> {
    match block {
        AssistantContentPart::Text(TextPart { text, .. }) => Some(text),
        _ => None,
    }
}

/// Extract thinking content from a reasoning block.
pub fn extract_thinking(block: &AssistantContentPart) -> Option<&str> {
    match block {
        AssistantContentPart::Reasoning(rp) => Some(&rp.text),
        _ => None,
    }
}

/// Extract tool use details from a tool call block.
pub fn extract_tool_use(block: &AssistantContentPart) -> Option<(&str, &str, &serde_json::Value)> {
    match block {
        AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id,
            tool_name,
            input,
            ..
        }) => Some((tool_call_id, tool_name, input)),
        _ => None,
    }
}

/// Extract tool result details from a tool result block.
pub fn extract_tool_result(
    block: &AssistantContentPart,
) -> Option<(&str, &cocode_inference::ToolResultContent, bool)> {
    match block {
        AssistantContentPart::ToolResult(ToolResultPart {
            tool_call_id,
            output,
            is_error,
            ..
        }) => Some((tool_call_id, output, *is_error)),
        _ => None,
    }
}

/// Check if a message contains any tool use blocks.
pub fn has_tool_use(message: &LanguageModelMessage) -> bool {
    match message {
        LanguageModelMessage::Assistant { content, .. } => content.iter().any(is_tool_use_block),
        _ => false,
    }
}

/// Check if a message contains any tool result blocks.
pub fn has_tool_result(message: &LanguageModelMessage) -> bool {
    match message {
        LanguageModelMessage::Assistant { content, .. } => content.iter().any(is_tool_result_block),
        LanguageModelMessage::Tool { content, .. } => content
            .iter()
            .any(|p| matches!(p, cocode_inference::ToolContentPart::ToolResult(_))),
        _ => false,
    }
}

/// Check if a message contains any thinking/reasoning blocks.
pub fn has_thinking(message: &LanguageModelMessage) -> bool {
    match message {
        LanguageModelMessage::Assistant { content, .. } => content.iter().any(is_thinking_block),
        _ => false,
    }
}

/// Check if a message is empty (no content).
pub fn is_empty_message(message: &LanguageModelMessage) -> bool {
    match message {
        LanguageModelMessage::System { content, .. } => content.is_empty(),
        LanguageModelMessage::User { content, .. } => content.is_empty(),
        LanguageModelMessage::Assistant { content, .. } => content.is_empty(),
        LanguageModelMessage::Tool { content, .. } => content.is_empty(),
    }
}

/// Check if a message is a user message.
pub fn is_user_message(message: &LanguageModelMessage) -> bool {
    message.is_user()
}

/// Check if a message is an assistant message.
pub fn is_assistant_message(message: &LanguageModelMessage) -> bool {
    message.is_assistant()
}

/// Check if a message is a system message.
pub fn is_system_message(message: &LanguageModelMessage) -> bool {
    message.is_system()
}

/// Check if a message is a tool message.
pub fn is_tool_message(message: &LanguageModelMessage) -> bool {
    message.is_tool()
}

/// Get all text content from a message.
pub fn get_text_content(message: &LanguageModelMessage) -> String {
    match message {
        LanguageModelMessage::System { content, .. } => content.clone(),
        LanguageModelMessage::User { content, .. } => content
            .iter()
            .filter_map(|p| match p {
                UserContentPart::Text(TextPart { text, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        LanguageModelMessage::Assistant { content, .. } => content
            .iter()
            .filter_map(extract_text)
            .collect::<Vec<_>>()
            .join(""),
        LanguageModelMessage::Tool { .. } => String::new(),
    }
}

/// Get all tool calls from a message.
pub fn get_tool_calls(message: &LanguageModelMessage) -> Vec<cocode_inference::ToolCall> {
    match message {
        LanguageModelMessage::Assistant { content, .. } => content
            .iter()
            .filter_map(|b| match b {
                AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id,
                    tool_name,
                    input,
                    ..
                }) => Some(cocode_inference::ToolCall {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: input.clone(),
                    provider_executed: None,
                    dynamic: None,
                    provider_metadata: None,
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Get the thinking content from a message if present.
pub fn get_thinking_content(message: &LanguageModelMessage) -> Option<String> {
    match message {
        LanguageModelMessage::Assistant { content, .. } => content.iter().find_map(|b| match b {
            AssistantContentPart::Reasoning(rp) => Some(rp.text.clone()),
            _ => None,
        }),
        _ => None,
    }
}

/// Count the number of tool use blocks in a message.
pub fn count_tool_uses(message: &LanguageModelMessage) -> usize {
    match message {
        LanguageModelMessage::Assistant { content, .. } => {
            content.iter().filter(|b| is_tool_use_block(b)).count()
        }
        _ => 0,
    }
}

/// Count the number of tool result blocks in a message.
pub fn count_tool_results(message: &LanguageModelMessage) -> usize {
    match message {
        LanguageModelMessage::Assistant { content, .. } => {
            content.iter().filter(|b| is_tool_result_block(b)).count()
        }
        LanguageModelMessage::Tool { content, .. } => content
            .iter()
            .filter(|p| matches!(p, cocode_inference::ToolContentPart::ToolResult(_)))
            .count(),
        _ => 0,
    }
}

#[cfg(test)]
#[path = "type_guards.test.rs"]
mod tests;
