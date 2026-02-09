//! Skill interface definition (`SKILL.toml` schema).
//!
//! Each skill directory contains a `SKILL.toml` file that describes the
//! skill's metadata and prompt content. This module defines the
//! deserialization target for that file.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Metadata and content of a skill, as defined in `SKILL.toml`.
///
/// A skill must have a `name` and `description`. The prompt content can be
/// provided either inline (`prompt_inline`) or by referencing an external
/// file (`prompt_file`). If both are specified, `prompt_file` takes
/// precedence.
///
/// # Example SKILL.toml
///
/// ```toml
/// name = "commit"
/// description = "Generate a commit message from staged changes"
/// prompt_file = "prompt.md"
/// allowed_tools = ["Bash", "Read"]
///
/// # Optional hooks that run when this skill is active
/// [hooks.PreToolUse]
/// matcher = { type = "or", matchers = [{ type = "exact", value = "Write" }, { type = "exact", value = "Edit" }] }
/// command = "npm run lint"
/// once = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInterface {
    /// Unique skill name (used as slash-command identifier).
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// Path to an external file containing the prompt text.
    /// Relative to the skill directory.
    #[serde(default)]
    pub prompt_file: Option<String>,

    /// Inline prompt text (used when `prompt_file` is not set).
    #[serde(default)]
    pub prompt_inline: Option<String>,

    /// Tools the skill is allowed to invoke.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Guidance for the LLM on when to invoke this skill.
    #[serde(default)]
    pub when_to_use: Option<String>,

    /// Whether this skill can be invoked by the user as a `/command`.
    /// Defaults to `true` if not specified.
    #[serde(default)]
    pub user_invocable: Option<bool>,

    /// Whether to block the LLM from invoking this skill via the Skill tool.
    /// Defaults to `false` if not specified.
    #[serde(default)]
    pub disable_model_invocation: Option<bool>,

    /// Model override for this skill (e.g., "sonnet", "opus", "haiku", "inherit").
    #[serde(default)]
    pub model: Option<String>,

    /// Execution context: "main" (default) or "fork".
    #[serde(default)]
    pub context: Option<String>,

    /// Agent type to use when `context = "fork"`.
    #[serde(default)]
    pub agent: Option<String>,

    /// Usage hint shown in help output (e.g., "<pr-number>").
    #[serde(default)]
    pub argument_hint: Option<String>,

    /// Alternative command names for this skill.
    #[serde(default)]
    pub aliases: Option<Vec<String>>,

    /// Hooks that are registered when this skill starts and removed when it ends.
    ///
    /// The key is the event type (e.g., "PreToolUse", "PostToolUse"), and the
    /// value is a list of hook configurations for that event.
    ///
    /// These hooks are scoped to the skill and automatically cleaned up when
    /// the skill finishes executing.
    #[serde(default)]
    pub hooks: Option<HashMap<String, Vec<SkillHookConfig>>>,
}

/// Hook configuration within a skill's SKILL.toml.
///
/// This is a simplified hook definition that maps to [`HookDefinition`]
/// when the skill is loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHookConfig {
    /// Optional matcher to filter which tool calls trigger this hook.
    #[serde(default)]
    pub matcher: Option<SkillHookMatcher>,

    /// Command to execute. The command receives hook context as JSON on stdin.
    #[serde(default)]
    pub command: Option<String>,

    /// Arguments for the command.
    #[serde(default)]
    pub args: Option<Vec<String>>,

    /// Timeout in seconds (default: 30).
    #[serde(default = "default_timeout")]
    pub timeout_secs: i32,

    /// If true, the hook is removed after its first successful execution.
    #[serde(default)]
    pub once: bool,
}

fn default_timeout() -> i32 {
    30
}

/// Matcher configuration for skill hooks.
///
/// Simplified version of HookMatcher for SKILL.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillHookMatcher {
    /// Exact string match.
    Exact { value: String },
    /// Wildcard pattern with `*` and `?`.
    Wildcard { pattern: String },
    /// Regular expression.
    Regex { pattern: String },
    /// Match any of the given matchers.
    Or { matchers: Vec<SkillHookMatcher> },
    /// Match all values.
    All,
}

#[cfg(test)]
#[path = "interface.test.rs"]
mod tests;
