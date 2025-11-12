//! Hook system protocol definitions
//!
//! This module defines the protocol for the hook system, which is fully compatible
//! with Claude Code's hook format. Hooks can intercept tool execution at various
//! lifecycle points and make decisions about whether to proceed, block, or modify operations.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Hook event types (fully compatible with Claude Code)
///
/// These represent the various points in the tool execution lifecycle where
/// hooks can be triggered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventName {
    /// Before tool execution
    PreToolUse,
    /// After tool execution
    PostToolUse,
    /// When user submits a prompt
    UserPromptSubmit,
    /// When main agent finishes responding
    Stop,
    /// When subagent completes a task
    SubagentStop,
    /// For system notifications
    Notification,
    /// Before context window compaction
    PreCompact,
    /// At session initialization
    SessionStart,
    /// At session termination
    SessionEnd,
}

/// Hook event context (Claude Code format)
///
/// This is the JSON structure that will be passed to hook actions (e.g., bash scripts)
/// via stdin. It follows Claude Code's exact format for maximum compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventContext {
    /// Session identifier
    pub session_id: String,

    /// Optional path to transcript file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,

    /// Current working directory
    pub cwd: String,

    /// The event that triggered this hook
    pub hook_event_name: HookEventName,

    /// ISO 8601 timestamp
    pub timestamp: String,

    /// Event-specific data (flattened into top level)
    #[serde(flatten)]
    pub event_data: HookEventData,
}

/// Event-specific data that varies by event type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookEventData {
    /// Data for PreToolUse event
    PreToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
    },

    /// Data for PostToolUse event
    PostToolUse {
        tool_name: String,
        tool_output: serde_json::Value,
    },

    /// Data for UserPromptSubmit event
    UserPromptSubmit { prompt: String },

    /// Data for Stop event
    Stop {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Data for SubagentStop event
    SubagentStop { subagent_type: String },

    /// Data for Notification event
    Notification {
        notification_type: String,
        message: String,
    },

    /// Data for PreCompact event
    PreCompact {
        message_count: usize,
        token_count: usize,
    },

    /// Catch-all for events without specific data
    Other,
}

impl Default for HookEventData {
    fn default() -> Self {
        Self::Other
    }
}

/// Hook decision (Claude Code format)
///
/// Indicates what action should be taken after the hook executes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HookDecision {
    /// Approve the operation
    Approve,
    /// Block the operation
    Block,
    /// Deny the operation (synonym for Block)
    Deny,
    /// Allow the operation (synonym for Approve)
    Allow,
    /// Ask the user for confirmation
    Ask,
}

/// Hook output (Claude Code format)
///
/// This is the JSON structure that hook actions should return via stdout.
/// Hooks can also use exit codes: 0=continue, 2=block, other=error.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookOutput {
    /// Whether execution should continue (default: true)
    #[serde(default = "default_true")]
    #[serde(rename = "continue")]
    pub continue_execution: bool,

    /// Explicit decision (optional, inferred from continue if not present)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,

    /// Human-readable reason for the decision
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// System message to display (for logging/debugging)
    #[serde(rename = "systemMessage")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,

    /// Additional context to inject into the conversation
    #[serde(rename = "additionalContext")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,

    /// Hook-specific output data
    #[serde(rename = "hookSpecificOutput")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Hook definition (Claude Code format)
///
/// Defines how hooks should be matched and executed for a specific event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    /// Pattern to match tool names (regex or glob)
    ///
    /// Examples:
    /// - "local_shell" - exact match
    /// - ".*shell.*" - regex match
    /// - "*" - match all
    #[serde(default)]
    pub matcher: String,

    /// Whether hooks should execute sequentially (default: false = parallel)
    #[serde(default)]
    pub sequential: bool,

    /// List of actions to execute for this hook
    pub hooks: Vec<HookActionConfig>,
}

/// Hook action configuration
///
/// Defines what should be executed when a hook is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HookActionConfig {
    /// Execute a shell command
    Command {
        /// Command to execute (can include arguments)
        command: String,

        /// Timeout in milliseconds (default: 30000)
        #[serde(default = "default_timeout")]
        timeout: u64,
    },

    /// Execute a native Rust function
    Native {
        /// Function identifier (registered in NativeHookRegistry)
        function: String,
    },
}

fn default_timeout() -> u64 {
    30000
}

/// Complete hooks configuration
///
/// Maps event types to their hook definitions.
/// This is the top-level structure loaded from configuration files.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Hooks organized by event type
    ///
    /// Example TOML:
    /// ```toml
    /// [[hooks.PreToolUse]]
    /// matcher = "local_shell"
    /// [[hooks.PreToolUse.hooks]]
    /// type = "command"
    /// command = "./validate.sh"
    /// ```
    #[serde(flatten)]
    pub hooks: HashMap<HookEventName, Vec<HookDefinition>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_event_context_serialization() {
        let ctx = HookEventContext {
            session_id: "test-123".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::PreToolUse {
                tool_name: "local_shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
            },
        };

        let json = serde_json::to_string_pretty(&ctx).unwrap();
        assert!(json.contains("\"hook_event_name\": \"PreToolUse\""));
        assert!(json.contains("\"tool_name\": \"local_shell\""));

        // Verify round-trip
        let deserialized: HookEventContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "test-123");
    }

    #[test]
    fn test_hook_output_default() {
        let output = HookOutput::default();
        assert!(output.continue_execution);
        assert!(output.decision.is_none());
    }

    #[test]
    fn test_hook_output_block() {
        let output = HookOutput {
            continue_execution: false,
            decision: Some(HookDecision::Block),
            reason: Some("Test block".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_string(&output).unwrap();
        let deserialized: HookOutput = serde_json::from_str(&json).unwrap();

        assert!(!deserialized.continue_execution);
        assert_eq!(deserialized.decision, Some(HookDecision::Block));
    }

    #[test]
    fn test_hooks_config_from_toml() {
        let toml_str = r#"
[[hooks.PreToolUse]]
matcher = "local_shell"
sequential = true

[[hooks.PreToolUse.hooks]]
type = "command"
command = "./validate.sh"
timeout = 5000

[[hooks.PreToolUse.hooks]]
type = "native"
function = "security_check"
"#;

        let config: HooksConfig = toml::from_str(toml_str).unwrap();

        let pre_tool_hooks = config.hooks.get(&HookEventName::PreToolUse).unwrap();
        assert_eq!(pre_tool_hooks.len(), 1);
        assert_eq!(pre_tool_hooks[0].matcher, "local_shell");
        assert!(pre_tool_hooks[0].sequential);
        assert_eq!(pre_tool_hooks[0].hooks.len(), 2);
    }
}
