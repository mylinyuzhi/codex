//! Queue and steering types for user input during agent execution.
//!
//! This module provides two mechanisms for user input while the agent is working:
//!
//! 1. **Queued Commands** (Enter while streaming): Visible commands that are
//!    processed as new turns after the current turn completes.
//!
//! 2. **Steering Attachments** (Shift+Enter): Hidden guidance that is injected
//!    into the conversation as meta messages (visible to model, hidden from user).

use serde::Deserialize;
use serde::Serialize;

/// A visible queued command (Enter during streaming).
///
/// These commands are shown in the UI and processed as new user turns
/// after the current agent turn completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQueuedCommand {
    /// Unique identifier for this command.
    pub id: String,
    /// The prompt/command text.
    pub prompt: String,
    /// Timestamp when queued (Unix milliseconds).
    pub queued_at: i64,
}

impl UserQueuedCommand {
    /// Create a new queued command.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            queued_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Get a preview of the command (first N chars).
    pub fn preview(&self, max_len: usize) -> String {
        if self.prompt.len() <= max_len {
            self.prompt.clone()
        } else {
            format!("{}...", &self.prompt[..max_len])
        }
    }
}

/// Source of a steering attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteeringSource {
    /// User input via Shift+Enter.
    User,
    /// Injected by a hook.
    Hook,
    /// Injected by the system.
    System,
}

impl SteeringSource {
    /// Get the source as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            SteeringSource::User => "user",
            SteeringSource::Hook => "hook",
            SteeringSource::System => "system",
        }
    }
}

impl std::fmt::Display for SteeringSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A hidden steering attachment (Shift+Enter).
///
/// These are injected as meta messages (is_meta=true) that the model sees
/// but are hidden from the user's conversation history. Used for mid-stream
/// guidance like "use TypeScript instead of JavaScript".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringAttachment {
    /// Unique identifier for this attachment.
    pub id: String,
    /// The steering prompt.
    pub prompt: String,
    /// Source of this steering.
    pub source: SteeringSource,
    /// Timestamp when queued (Unix milliseconds).
    pub queued_at: i64,
}

impl SteeringAttachment {
    /// Create a new user-initiated steering attachment.
    pub fn user(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            source: SteeringSource::User,
            queued_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create a new hook-initiated steering attachment.
    pub fn hook(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            source: SteeringSource::Hook,
            queued_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create a new system-initiated steering attachment.
    pub fn system(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            source: SteeringSource::System,
            queued_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_queued_command() {
        let cmd = UserQueuedCommand::new("test command");
        assert_eq!(cmd.prompt, "test command");
        assert!(!cmd.id.is_empty());
        assert!(cmd.queued_at > 0);
    }

    #[test]
    fn test_command_preview() {
        let cmd = UserQueuedCommand::new("this is a very long command that should be truncated");
        let preview = cmd.preview(20);
        assert_eq!(preview, "this is a very long ...");

        let short_cmd = UserQueuedCommand::new("short");
        assert_eq!(short_cmd.preview(20), "short");
    }

    #[test]
    fn test_steering_source() {
        assert_eq!(SteeringSource::User.as_str(), "user");
        assert_eq!(SteeringSource::Hook.as_str(), "hook");
        assert_eq!(SteeringSource::System.as_str(), "system");
    }

    #[test]
    fn test_steering_attachment_user() {
        let steering = SteeringAttachment::user("use TypeScript");
        assert_eq!(steering.prompt, "use TypeScript");
        assert_eq!(steering.source, SteeringSource::User);
        assert!(!steering.id.is_empty());
    }

    #[test]
    fn test_steering_attachment_hook() {
        let steering = SteeringAttachment::hook("hook guidance");
        assert_eq!(steering.source, SteeringSource::Hook);
    }

    #[test]
    fn test_steering_attachment_system() {
        let steering = SteeringAttachment::system("system guidance");
        assert_eq!(steering.source, SteeringSource::System);
    }

    #[test]
    fn test_serde_roundtrip() {
        let cmd = UserQueuedCommand::new("test");
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: UserQueuedCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt, cmd.prompt);

        let steering = SteeringAttachment::user("guidance");
        let json = serde_json::to_string(&steering).unwrap();
        let parsed: SteeringAttachment = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt, steering.prompt);
        assert_eq!(parsed.source, SteeringSource::User);
    }
}
