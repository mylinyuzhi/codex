//! Message factory functions for creating tracked messages.
//!
//! This module provides convenient factory functions for creating
//! various types of messages, similar to Claude Code's `factory.ts`.

use crate::tracked::MessageSource;
use crate::tracked::TrackedMessage;
use cocode_api::AssistantContentPart;
use cocode_api::CollectedResponse;
use cocode_api::LanguageModelMessage;
use cocode_api::ToolContentPart;
use cocode_api::ToolResultContent;
use cocode_api::ToolResultPart;
use cocode_api::UserContentPart;

/// Create a user message.
pub fn create_user_message(
    content: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::user(content, turn_id)
}

/// Create a user message with multiple content parts.
pub fn create_user_message_with_content(
    content: Vec<UserContentPart>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::new(
        LanguageModelMessage::user(content),
        turn_id,
        MessageSource::User,
    )
}

/// Create an assistant message from API response.
pub fn create_assistant_message(
    response: &CollectedResponse,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::new(
        LanguageModelMessage::assistant(response.content.clone()),
        turn_id,
        MessageSource::assistant(None),
    )
}

/// Create an assistant message from content blocks.
pub fn create_assistant_message_with_content(
    content: Vec<AssistantContentPart>,
    turn_id: impl Into<String>,
    request_id: Option<String>,
) -> TrackedMessage {
    TrackedMessage::assistant_with_content(content, turn_id, request_id)
}

/// Create a system message.
pub fn create_system_message(
    content: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::system(content, turn_id)
}

/// Create a tool result message.
pub fn create_tool_result_message(
    call_id: impl Into<String>,
    content: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::tool_result(call_id, content, turn_id)
}

/// Create a tool result message from structured content.
pub fn create_tool_result_structured(
    call_id: impl Into<String>,
    content: ToolResultContent,
    is_error: bool,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    let call_id_str = call_id.into();
    let turn_id_str = turn_id.into();

    let result_part = if is_error {
        let error_content = match content {
            ToolResultContent::Text { value, .. } => ToolResultContent::error_text(value),
            ToolResultContent::Json { value, .. } => {
                ToolResultContent::error_text(value.to_string())
            }
            ToolResultContent::Content { .. } => ToolResultContent::error_text("[complex content]"),
            other => other,
        };
        ToolResultPart::new(&call_id_str, "", error_content).with_error()
    } else {
        ToolResultPart::new(&call_id_str, "", content)
    };

    let message = LanguageModelMessage::tool(vec![ToolContentPart::ToolResult(result_part)]);

    TrackedMessage::new(message, turn_id_str, MessageSource::tool(&call_id_str))
}

/// Create a tool error message.
pub fn create_tool_error_message(
    call_id: impl Into<String>,
    error: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::tool_error(call_id, error, turn_id)
}

/// Create a subagent result message.
pub fn create_subagent_result_message(
    agent_id: impl Into<String>,
    result: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    let agent_id_str = agent_id.into();
    TrackedMessage::new(
        LanguageModelMessage::user_text(result), // Subagent results are typically added as user context
        turn_id,
        MessageSource::subagent(agent_id_str),
    )
}

/// Create a compaction summary message.
pub fn create_compaction_summary(
    summary: impl Into<String>,
    turn_id: impl Into<String>,
) -> TrackedMessage {
    TrackedMessage::new(
        LanguageModelMessage::user_text(format!(
            "<compaction_summary>\n{}\n</compaction_summary>",
            summary.into()
        )),
        turn_id,
        MessageSource::CompactionSummary,
    )
}

/// Create tool result messages for multiple tool calls.
pub fn create_tool_results_batch(
    results: Vec<(String, ToolResultContent, bool)>,
    turn_id: impl Into<String>,
) -> Vec<TrackedMessage> {
    let turn_id = turn_id.into();
    results
        .into_iter()
        .map(|(call_id, content, is_error)| {
            create_tool_result_structured(call_id, content, is_error, turn_id.clone())
        })
        .collect()
}

/// Builder for creating messages with additional metadata.
pub struct MessageBuilder {
    turn_id: String,
}

impl MessageBuilder {
    /// Create a new message builder for a turn.
    pub fn for_turn(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
        }
    }

    /// Create a user message.
    pub fn user(&self, content: impl Into<String>) -> TrackedMessage {
        create_user_message(content, &self.turn_id)
    }

    /// Create a system message.
    pub fn system(&self, content: impl Into<String>) -> TrackedMessage {
        create_system_message(content, &self.turn_id)
    }

    /// Create an assistant message from response.
    pub fn assistant_from_response(&self, response: &CollectedResponse) -> TrackedMessage {
        create_assistant_message(response, &self.turn_id)
    }

    /// Create an assistant message with content.
    pub fn assistant_with_content(
        &self,
        content: Vec<AssistantContentPart>,
        request_id: Option<String>,
    ) -> TrackedMessage {
        create_assistant_message_with_content(content, &self.turn_id, request_id)
    }

    /// Create a tool result message.
    pub fn tool_result(
        &self,
        call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> TrackedMessage {
        create_tool_result_message(call_id, content, &self.turn_id)
    }

    /// Create a tool error message.
    pub fn tool_error(
        &self,
        call_id: impl Into<String>,
        error: impl Into<String>,
    ) -> TrackedMessage {
        create_tool_error_message(call_id, error, &self.turn_id)
    }
}

#[cfg(test)]
#[path = "factory.test.rs"]
mod tests;
