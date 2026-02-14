//! Hook execution context.
//!
//! Provides all information available to a hook at execution time.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::event::HookEventType;

/// Context passed to hooks during execution.
///
/// Contains information about the event that triggered the hook and the
/// current session environment. Includes event-specific fields that are
/// populated based on the event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    /// The event type that triggered this hook.
    pub event_type: HookEventType,

    /// The tool name (if the event is tool-related).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// The tool input JSON (if the event is tool-related).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,

    /// The current session identifier.
    pub session_id: String,

    /// The working directory for the session.
    pub working_dir: PathBuf,

    // -- Event-specific fields --
    /// The user prompt text (populated for `UserPromptSubmit` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// The session start source (populated for `SessionStart` events).
    /// Values: "startup", "resume", "compact".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Whether a Stop hook is already active (populated for `Stop` events).
    /// Used to prevent infinite loops when Stop hooks trigger further stops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,

    /// The conversation transcript (populated for `Stop` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Value>,

    /// The notification type (populated for `Notification` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// Additional metadata for the hook execution.
    /// Used to pass extra context like "source" = "compact" for post-compact hooks.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl HookContext {
    /// Creates a new `HookContext` with the required fields.
    pub fn new(event_type: HookEventType, session_id: String, working_dir: PathBuf) -> Self {
        Self {
            event_type,
            tool_name: None,
            tool_input: None,
            session_id,
            working_dir,
            prompt: None,
            source: None,
            stop_hook_active: None,
            transcript: None,
            notification_type: None,
            metadata: HashMap::new(),
        }
    }

    /// Sets the tool name and returns `self` for chaining.
    pub fn with_tool_name(mut self, name: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self
    }

    /// Sets the tool input and returns `self` for chaining.
    pub fn with_tool_input(mut self, input: Value) -> Self {
        self.tool_input = Some(input);
        self
    }

    /// Sets both tool name and input, returning `self` for chaining.
    pub fn with_tool(self, name: impl Into<String>, input: Value) -> Self {
        self.with_tool_name(name).with_tool_input(input)
    }

    /// Sets the session ID and returns `self` for chaining.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    /// Sets the working directory and returns `self` for chaining.
    pub fn with_working_dir(mut self, working_dir: PathBuf) -> Self {
        self.working_dir = working_dir;
        self
    }

    /// Sets the user prompt text (for `UserPromptSubmit` events).
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Sets the session start source (for `SessionStart` events).
    /// Typical values: "startup", "resume", "compact".
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Sets whether a Stop hook is already active (for `Stop` events).
    /// Prevents infinite loops when Stop hooks trigger further stops.
    pub fn with_stop_hook_active(mut self, active: bool) -> Self {
        self.stop_hook_active = Some(active);
        self
    }

    /// Sets the conversation transcript (for `Stop` events).
    pub fn with_transcript(mut self, transcript: Value) -> Self {
        self.transcript = Some(transcript);
        self
    }

    /// Sets the notification type (for `Notification` events).
    pub fn with_notification_type(mut self, notification_type: impl Into<String>) -> Self {
        self.notification_type = Some(notification_type.into());
        self
    }

    /// Adds a metadata key-value pair and returns `self` for chaining.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Gets a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }

    /// Returns the match target string for this context based on the event type.
    ///
    /// Different event types match against different fields:
    /// - Tool events (PreToolUse, PostToolUse, PostToolUseFailure): matches `tool_name`
    /// - SessionStart: matches `source`
    /// - Notification: matches `notification_type`
    /// - Other events: matches `tool_name` if present
    pub fn match_target(&self) -> Option<&str> {
        match self.event_type {
            HookEventType::PreToolUse
            | HookEventType::PostToolUse
            | HookEventType::PostToolUseFailure
            | HookEventType::PermissionRequest => self.tool_name.as_deref(),
            HookEventType::SessionStart => self.source.as_deref(),
            HookEventType::Notification => self.notification_type.as_deref(),
            _ => self.tool_name.as_deref(),
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
