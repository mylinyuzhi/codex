//! Tracked messages with metadata for turn tracking.
//!
//! This module provides [`TrackedMessage`] which wraps vercel-ai-provider's
//! [`LanguageModelMessage`] with additional metadata for tracking in the agent loop.

use chrono::DateTime;
use chrono::Utc;
use cocode_inference::AssistantContentPart;
use cocode_inference::LanguageModelMessage;
use cocode_inference::ToolContentPart;
use cocode_inference::ToolResultContent;
use cocode_inference::ToolResultPart;
use cocode_inference::UserContentPart;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// Source of a message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSource {
    /// User input.
    User,
    /// Assistant response.
    Assistant {
        /// Request ID from the API.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// System instruction.
    System,
    /// Tool result.
    Tool {
        /// Tool call ID this result is for.
        call_id: String,
    },
    /// Subagent response.
    Subagent {
        /// ID of the subagent.
        agent_id: String,
    },
    /// Compaction summary.
    CompactionSummary,
    /// System reminder (dynamic context injection).
    SystemReminder {
        /// The type of reminder (e.g., "changed_files", "plan_mode_enter").
        reminder_type: String,
    },
}

impl MessageSource {
    /// Create an assistant source with optional request ID.
    pub fn assistant(request_id: Option<String>) -> Self {
        MessageSource::Assistant { request_id }
    }

    /// Create a tool source with call ID.
    pub fn tool(call_id: impl Into<String>) -> Self {
        MessageSource::Tool {
            call_id: call_id.into(),
        }
    }

    /// Create a subagent source with agent ID.
    pub fn subagent(agent_id: impl Into<String>) -> Self {
        MessageSource::Subagent {
            agent_id: agent_id.into(),
        }
    }

    /// Create a system reminder source with reminder type.
    pub fn system_reminder(reminder_type: impl Into<String>) -> Self {
        MessageSource::SystemReminder {
            reminder_type: reminder_type.into(),
        }
    }
}

/// A message with tracking metadata.
///
/// This wraps vercel-ai-provider's [`LanguageModelMessage`] with additional
/// information needed for the agent loop, including unique IDs, turn tracking,
/// and timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedMessage {
    /// The underlying message.
    pub inner: LanguageModelMessage,
    /// Unique identifier for this message.
    pub uuid: String,
    /// Turn this message belongs to.
    pub turn_id: String,
    /// Timestamp when the message was created.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub timestamp: DateTime<Utc>,
    /// Source of the message.
    pub source: MessageSource,
    /// Whether this message has been tombstoned (marked for removal).
    #[serde(default)]
    pub tombstoned: bool,
    /// Whether this is a meta message (hidden from user, visible to model).
    ///
    /// Meta messages are included in API requests but not shown in user-facing
    /// conversation history. Used for system reminders and other injected context.
    #[serde(default)]
    pub is_meta: bool,
}

impl TrackedMessage {
    /// Create a new tracked message.
    pub fn new(
        inner: LanguageModelMessage,
        turn_id: impl Into<String>,
        source: MessageSource,
    ) -> Self {
        Self {
            inner,
            uuid: Uuid::new_v4().to_string(),
            turn_id: turn_id.into(),
            timestamp: Utc::now(),
            source,
            tombstoned: false,
            is_meta: false,
        }
    }

    /// Create a new meta message (hidden from user, visible to model).
    pub fn new_meta(
        inner: LanguageModelMessage,
        turn_id: impl Into<String>,
        source: MessageSource,
    ) -> Self {
        Self {
            inner,
            uuid: Uuid::new_v4().to_string(),
            turn_id: turn_id.into(),
            timestamp: Utc::now(),
            source,
            tombstoned: false,
            is_meta: true,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self::new(
            LanguageModelMessage::user_text(content),
            turn_id,
            MessageSource::User,
        )
    }

    /// Create an assistant message.
    pub fn assistant(
        content: impl Into<String>,
        turn_id: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        Self::new(
            LanguageModelMessage::assistant_text(content),
            turn_id,
            MessageSource::assistant(request_id),
        )
    }

    /// Create an assistant message with content blocks.
    pub fn assistant_with_content(
        content: Vec<AssistantContentPart>,
        turn_id: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        Self::new(
            LanguageModelMessage::assistant(content),
            turn_id,
            MessageSource::assistant(request_id),
        )
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self::new(
            LanguageModelMessage::system(content),
            turn_id,
            MessageSource::System,
        )
    }

    /// Create a tool result message.
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        let call_id = tool_use_id.into();
        Self::new(
            LanguageModelMessage::tool(vec![ToolContentPart::ToolResult(ToolResultPart::new(
                &call_id,
                "", // tool_name not always known here
                ToolResultContent::text(content),
            ))]),
            turn_id,
            MessageSource::tool(&call_id),
        )
    }

    /// Create a tool error message.
    pub fn tool_error(
        tool_use_id: impl Into<String>,
        error: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        let call_id = tool_use_id.into();
        Self::new(
            LanguageModelMessage::tool(vec![ToolContentPart::ToolResult(
                ToolResultPart::new(
                    &call_id,
                    "", // tool_name not always known here
                    ToolResultContent::error_text(error),
                )
                .with_error(),
            )]),
            turn_id,
            MessageSource::tool(&call_id),
        )
    }

    /// Create a system reminder message (meta message for dynamic context).
    ///
    /// System reminders are injected as user messages with `is_meta: true`,
    /// meaning they are included in API requests but not shown to the user.
    pub fn system_reminder(
        content: impl Into<String>,
        reminder_type: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        Self::new_meta(
            LanguageModelMessage::user_text(content),
            turn_id,
            MessageSource::system_reminder(reminder_type),
        )
    }

    /// Check if this message is a meta message.
    pub fn is_meta(&self) -> bool {
        self.is_meta
    }

    /// Set the meta flag on this message.
    pub fn set_meta(&mut self, is_meta: bool) {
        self.is_meta = is_meta;
    }

    /// Check if this message is a user message.
    pub fn is_user(&self) -> bool {
        self.inner.is_user()
    }

    /// Check if this message is an assistant message.
    pub fn is_assistant(&self) -> bool {
        self.inner.is_assistant()
    }

    /// Check if this message is a system message.
    pub fn is_system(&self) -> bool {
        self.inner.is_system()
    }

    /// Check if this message is a tool message.
    pub fn is_tool(&self) -> bool {
        self.inner.is_tool()
    }

    /// Get the message content as assistant content parts (if assistant message).
    pub fn assistant_content(&self) -> &[AssistantContentPart] {
        match &self.inner {
            LanguageModelMessage::Assistant { content, .. } => content,
            _ => &[],
        }
    }

    /// Get the message content as user content parts (if user message).
    pub fn user_content(&self) -> &[UserContentPart] {
        match &self.inner {
            LanguageModelMessage::User { content, .. } => content,
            _ => &[],
        }
    }

    /// Get text content from the message.
    pub fn text(&self) -> String {
        crate::type_guards::get_text_content(&self.inner)
    }

    /// Check if this message has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        crate::type_guards::has_tool_use(&self.inner)
    }

    /// Get tool calls from this message.
    pub fn tool_calls(&self) -> Vec<cocode_inference::ToolCall> {
        crate::type_guards::get_tool_calls(&self.inner)
    }

    /// Mark this message as tombstoned.
    pub fn tombstone(&mut self) {
        self.tombstoned = true;
    }

    /// Check if this message is tombstoned.
    pub fn is_tombstoned(&self) -> bool {
        self.tombstoned
    }

    /// Convert to the underlying message for API requests.
    pub fn into_message(self) -> LanguageModelMessage {
        self.inner
    }

    /// Get a reference to the underlying message.
    pub fn as_message(&self) -> &LanguageModelMessage {
        &self.inner
    }
}

impl AsRef<LanguageModelMessage> for TrackedMessage {
    fn as_ref(&self) -> &LanguageModelMessage {
        &self.inner
    }
}

impl From<TrackedMessage> for LanguageModelMessage {
    fn from(tracked: TrackedMessage) -> Self {
        tracked.inner
    }
}

#[cfg(test)]
#[path = "tracked.test.rs"]
mod tests;
