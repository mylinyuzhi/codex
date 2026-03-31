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
///
/// Field names use Claude Code v2.1.7 JSON conventions for compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    /// The event type that triggered this hook.
    #[serde(rename = "hook_event_name")]
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
    #[serde(rename = "cwd")]
    pub working_dir: PathBuf,

    // -- Common fields (all events) --
    /// The permission mode in effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,

    /// Path to the session transcript file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,

    // -- Tool events (PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest) --
    /// The tool use ID from the model response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,

    /// The tool response (PostToolUse only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<Value>,

    // -- PostToolUseFailure --
    /// The error message from the tool failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Whether the failure was caused by an interrupt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_interrupt: Option<bool>,

    // -- UserPromptSubmit --
    /// The user prompt text (populated for `UserPromptSubmit` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    // -- SessionStart --
    /// The session start source (populated for `SessionStart` events).
    /// Values: "startup", "resume", "compact".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// The model name (populated for `SessionStart` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // -- Stop --
    /// Whether a Stop hook is already active (populated for `Stop` events).
    /// Used to prevent infinite loops when Stop hooks trigger further stops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,

    /// The conversation transcript (populated for `Stop` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Value>,

    /// The last assistant message text (populated for `Stop` and `SubagentStop`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,

    // -- SubagentStart / SubagentStop --
    /// The sub-agent's identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// The sub-agent's type (e.g., "explore", "plan").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,

    /// Path to the sub-agent's transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_transcript_path: Option<String>,

    // -- Notification --
    /// The notification type (populated for `Notification` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// The notification message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// The notification title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    // -- TeammateIdle --
    /// The teammate's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,

    /// The team name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,

    // -- TaskCompleted --
    /// The task identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// The task subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_subject: Option<String>,

    /// The task description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_description: Option<String>,

    // -- PreCompact --
    /// The compaction trigger (e.g., "auto", "manual").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,

    /// Custom instructions for the compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,

    // -- ConfigChange --
    /// The config key that changed (populated for `ConfigChange` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,

    /// The new config value (populated for `ConfigChange` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_value: Option<Value>,

    // -- SessionEnd --
    /// The reason the session ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    // -- WorktreeCreate / WorktreeRemove --
    /// The path to the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,

    /// The branch name of the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,

    /// Additional metadata for the hook execution.
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
            permission_mode: None,
            transcript_path: None,
            tool_use_id: None,
            tool_response: None,
            error: None,
            is_interrupt: None,
            prompt: None,
            source: None,
            model: None,
            stop_hook_active: None,
            transcript: None,
            last_assistant_message: None,
            agent_id: None,
            agent_type: None,
            agent_transcript_path: None,
            notification_type: None,
            message: None,
            title: None,
            teammate_name: None,
            team_name: None,
            task_id: None,
            task_subject: None,
            task_description: None,
            trigger: None,
            custom_instructions: None,
            config_key: None,
            config_value: None,
            reason: None,
            worktree_path: None,
            worktree_branch: None,
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

    /// Sets the tool use ID from the model response.
    pub fn with_tool_use_id(mut self, id: impl Into<String>) -> Self {
        self.tool_use_id = Some(id.into());
        self
    }

    /// Sets the tool response (for `PostToolUse` events).
    pub fn with_tool_response(mut self, response: Value) -> Self {
        self.tool_response = Some(response);
        self
    }

    /// Sets the error message (for `PostToolUseFailure` events).
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sets whether the failure was an interrupt (for `PostToolUseFailure` events).
    pub fn with_is_interrupt(mut self, is_interrupt: bool) -> Self {
        self.is_interrupt = Some(is_interrupt);
        self
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

    /// Sets the permission mode.
    pub fn with_permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.permission_mode = Some(mode.into());
        self
    }

    /// Sets the transcript path.
    pub fn with_transcript_path(mut self, path: impl Into<String>) -> Self {
        self.transcript_path = Some(path.into());
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

    /// Sets the model name (for `SessionStart` events).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
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

    /// Sets the last assistant message text (for `Stop` and `SubagentStop` events).
    pub fn with_last_assistant_message(mut self, msg: impl Into<String>) -> Self {
        self.last_assistant_message = Some(msg.into());
        self
    }

    /// Sets the sub-agent ID (for `SubagentStart`/`SubagentStop` events).
    pub fn with_agent_id(mut self, id: impl Into<String>) -> Self {
        self.agent_id = Some(id.into());
        self
    }

    /// Sets the sub-agent type (for `SubagentStart`/`SubagentStop` events).
    pub fn with_agent_type(mut self, agent_type: impl Into<String>) -> Self {
        self.agent_type = Some(agent_type.into());
        self
    }

    /// Sets the sub-agent transcript path.
    pub fn with_agent_transcript_path(mut self, path: impl Into<String>) -> Self {
        self.agent_transcript_path = Some(path.into());
        self
    }

    /// Sets the notification type (for `Notification` events).
    pub fn with_notification_type(mut self, notification_type: impl Into<String>) -> Self {
        self.notification_type = Some(notification_type.into());
        self
    }

    /// Sets the notification message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Sets the notification title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the teammate name (for `TeammateIdle` events).
    pub fn with_teammate_name(mut self, name: impl Into<String>) -> Self {
        self.teammate_name = Some(name.into());
        self
    }

    /// Sets the team name (for `TeammateIdle` events).
    pub fn with_team_name(mut self, name: impl Into<String>) -> Self {
        self.team_name = Some(name.into());
        self
    }

    /// Sets the task ID (for `TaskCompleted` events).
    pub fn with_task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = Some(id.into());
        self
    }

    /// Sets the task subject (for `TaskCompleted` events).
    pub fn with_task_subject(mut self, subject: impl Into<String>) -> Self {
        self.task_subject = Some(subject.into());
        self
    }

    /// Sets the task description (for `TaskCompleted` events).
    pub fn with_task_description(mut self, description: impl Into<String>) -> Self {
        self.task_description = Some(description.into());
        self
    }

    /// Sets the compaction trigger (for `PreCompact` events).
    pub fn with_trigger(mut self, trigger: impl Into<String>) -> Self {
        self.trigger = Some(trigger.into());
        self
    }

    /// Sets custom instructions (for `PreCompact` events).
    pub fn with_custom_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.custom_instructions = Some(instructions.into());
        self
    }

    /// Sets the config key (for `ConfigChange` events).
    pub fn with_config_key(mut self, key: impl Into<String>) -> Self {
        self.config_key = Some(key.into());
        self
    }

    /// Sets the config value (for `ConfigChange` events).
    pub fn with_config_value(mut self, value: Value) -> Self {
        self.config_value = Some(value);
        self
    }

    /// Sets the session end reason (for `SessionEnd` events).
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Sets the worktree path (for `WorktreeCreate`/`WorktreeRemove` events).
    pub fn with_worktree_path(mut self, path: impl Into<String>) -> Self {
        self.worktree_path = Some(path.into());
        self
    }

    /// Sets the worktree branch (for `WorktreeCreate` events).
    pub fn with_worktree_branch(mut self, branch: impl Into<String>) -> Self {
        self.worktree_branch = Some(branch.into());
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
    /// - Tool events (PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest): `tool_name`
    /// - SessionStart: `source`
    /// - SessionEnd: `reason`
    /// - Notification: `notification_type`
    /// - SubagentStart/SubagentStop: `agent_type`
    /// - PreCompact: `trigger`
    /// - ConfigChange: `config_key`
    /// - UserPromptSubmit, Stop, TeammateIdle, TaskCompleted: no matcher
    pub fn match_target(&self) -> Option<&str> {
        match self.event_type {
            HookEventType::PreToolUse
            | HookEventType::PostToolUse
            | HookEventType::PostToolUseFailure
            | HookEventType::PermissionRequest => self.tool_name.as_deref(),
            HookEventType::SessionStart => self.source.as_deref(),
            HookEventType::SessionEnd => self.reason.as_deref(),
            HookEventType::Notification => self.notification_type.as_deref(),
            HookEventType::SubagentStart | HookEventType::SubagentStop => {
                self.agent_type.as_deref()
            }
            HookEventType::PreCompact => self.trigger.as_deref(),
            HookEventType::ConfigChange => self.config_key.as_deref(),
            HookEventType::UserPromptSubmit
            | HookEventType::Stop
            | HookEventType::TeammateIdle
            | HookEventType::TaskCompleted
            | HookEventType::PostCompact
            | HookEventType::WorktreeCreate
            | HookEventType::WorktreeRemove
            | HookEventType::Setup
            | HookEventType::Elicitation
            | HookEventType::ElicitationResult
            | HookEventType::InstructionsLoaded => None,
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
