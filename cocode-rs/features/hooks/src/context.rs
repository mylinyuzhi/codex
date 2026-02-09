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
/// current session environment.
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

    /// Adds a metadata key-value pair and returns `self` for chaining.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Gets a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
