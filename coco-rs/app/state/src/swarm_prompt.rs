//! Teammate prompt addendum — system prompt additions for teammates.
//!
//! TS: utils/swarm/teammatePromptAddendum.ts
//!
//! Added to each teammate's system prompt to provide team-aware behavior:
//! mailbox awareness, task coordination, shutdown handling, etc.

use super::swarm_backend::SystemPromptMode;

/// System prompt addendum for in-process teammates.
///
/// TS: `TEAMMATE_SYSTEM_PROMPT_ADDENDUM` in `utils/swarm/teammatePromptAddendum.ts`
///
/// Appended to the full main agent system prompt for teammates.
/// Explains visibility constraints and communication requirements.
pub const TEAMMATE_PROMPT_ADDENDUM: &str = "\
# Agent Teammate Communication

IMPORTANT: You are running as an agent in a team. To communicate with anyone on your team:
- Use the SendMessage tool with `to: \"<name>\"` to send messages to specific teammates
- Use the SendMessage tool with `to: \"*\"` sparingly for team-wide broadcasts

Just writing a response in text is not visible to others on your team - you MUST use the SendMessage tool.

The user interacts primarily with the team lead. Your work is coordinated through the task system and teammate messaging.
";

/// Permission poll interval for in-process teammates (ms).
///
/// TS: `PERMISSION_POLL_INTERVAL_MS = 500`
pub const PERMISSION_POLL_INTERVAL_MS: u64 = 500;

/// Build the complete system prompt for a teammate.
///
/// TS: `runInProcessTeammate()` builds system prompt based on mode.
pub fn build_teammate_system_prompt(
    base_prompt: Option<&str>,
    custom_prompt: Option<&str>,
    mode: SystemPromptMode,
) -> String {
    match mode {
        SystemPromptMode::Replace => {
            // Use only the custom prompt
            custom_prompt.unwrap_or("").to_string()
        }
        SystemPromptMode::Default => {
            // Base prompt + addendum
            let base = base_prompt.unwrap_or("");
            if base.is_empty() {
                TEAMMATE_PROMPT_ADDENDUM.to_string()
            } else {
                format!("{base}\n\n{TEAMMATE_PROMPT_ADDENDUM}")
            }
        }
        SystemPromptMode::Append => {
            // Base prompt + addendum + custom prompt
            let base = base_prompt.unwrap_or("");
            let custom = custom_prompt.unwrap_or("");
            let mut parts = Vec::new();
            if !base.is_empty() {
                parts.push(base);
            }
            parts.push(TEAMMATE_PROMPT_ADDENDUM);
            if !custom.is_empty() {
                parts.push(custom);
            }
            parts.join("\n\n")
        }
    }
}

#[cfg(test)]
#[path = "swarm_prompt.test.rs"]
mod tests;
