//! Fork subagent support — context cloning for cache-efficient spawning.
//!
//! TS: tools/AgentTool/forkSubagent.ts (8.6K LOC)
//!
//! When the FORK_SUBAGENT feature is enabled and no explicit subagent_type
//! is specified, the agent inherits the parent's byte-identical system prompt
//! (enabling prompt cache sharing) and receives the parent's conversation
//! context with tool_use results replaced by placeholders.

use serde::Deserialize;
use serde::Serialize;

/// XML tag wrapping the fork boilerplate rules.
///
/// TS: `FORK_BOILERPLATE_TAG` in forkSubagent.ts
/// Used for: (1) wrapping rules, (2) detecting recursive forks via tag scan.
pub const FORK_BOILERPLATE_TAG: &str = "fork-boilerplate";

/// Prefix before the directive text in fork child messages.
///
/// TS: `FORK_DIRECTIVE_PREFIX` in forkSubagent.ts
pub const FORK_DIRECTIVE_PREFIX: &str = "Your task:\n";

/// Fork subagent context — carries inherited state from parent.
///
/// TS: FORK_AGENT definition + buildForkedMessages() + buildChildMessage()
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkContext {
    /// Parent's conversation messages (with tool results replaced).
    pub messages: Vec<serde_json::Value>,
    /// Directive prepended for this specific fork child.
    pub directive: String,
    /// Whether this fork should use exact parent tools (cache identity).
    pub use_exact_tools: bool,
}

/// Placeholder text injected for each tool_use result in fork context.
///
/// TS: `FORK_PLACEHOLDER` in forkSubagent.ts
pub const FORK_PLACEHOLDER: &str = "Fork started \u{2014} processing in background";

/// Build a fork context from the parent's last assistant message.
///
/// Takes the parent's assistant message (with tool_use blocks) and creates
/// fork context messages where all tool results are replaced with the
/// placeholder text. The directive is prepended to guide the forked child.
///
/// TS: buildForkedMessages(directive, assistantMessage)
pub fn build_fork_context(parent_messages: &[serde_json::Value], directive: &str) -> ForkContext {
    let mut forked = Vec::with_capacity(parent_messages.len());

    for msg in parent_messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

        if role == "user" {
            // Replace tool_result content with placeholder
            if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                let replaced: Vec<serde_json::Value> = content
                    .iter()
                    .map(|block| {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                            let mut replaced_block = block.clone();
                            replaced_block["content"] = serde_json::json!(FORK_PLACEHOLDER);
                            replaced_block
                        } else {
                            block.clone()
                        }
                    })
                    .collect();
                let mut new_msg = msg.clone();
                new_msg["content"] = serde_json::json!(replaced);
                forked.push(new_msg);
            } else {
                forked.push(msg.clone());
            }
        } else {
            // Assistant and system messages pass through unchanged
            forked.push(msg.clone());
        }
    }

    ForkContext {
        messages: forked,
        directive: directive.to_string(),
        use_exact_tools: true,
    }
}

/// Build the child message with XML-wrapped rules + directive.
///
/// TS: buildChildMessage(directive) in forkSubagent.ts
/// Output: `<fork-boilerplate>...rules...</fork-boilerplate>\n\nYour task:\n{directive}`
pub fn build_fork_child_message(directive: &str) -> String {
    let rules = build_fork_child_rules();
    format!(
        "<{FORK_BOILERPLATE_TAG}>\n{rules}\n</{FORK_BOILERPLATE_TAG}>\n\n{FORK_DIRECTIVE_PREFIX}{directive}"
    )
}

/// Build the child rules injected into a forked agent's context.
///
/// TS: buildChildMessage() rules in forkSubagent.ts
pub fn build_fork_child_rules() -> String {
    "STOP. READ THIS FIRST.\n\
     Non-negotiable rules for this fork:\n\
     1. Do NOT fork recursively (no nested Agent calls with fork)\n\
     2. Do NOT converse \u{2014} execute the task directly\n\
     3. Do NOT editorialize \u{2014} output facts only\n\
     4. Use tools silently without narration\n\
     5. Commit changes before reporting (if applicable)\n\
     6. Do NOT ask questions or request clarification\n\
     7. Do NOT summarize what you plan to do\n\
     8. Do NOT explain your reasoning\n\
     9. Output format:\n\
        Scope: [what you worked on]\n\
        Result: [what you found/changed]\n\
        Key files: [file paths]\n\
        Files changed: [if applicable]\n\
        Issues: [if any]\n\
     10. Stop immediately after producing output"
        .to_string()
}

/// Build a worktree notice for forked agents in isolated worktrees.
///
/// TS: buildWorktreeNotice(parentCwd, worktreeCwd)
pub fn build_worktree_notice(parent_cwd: &str, worktree_cwd: &str) -> String {
    format!(
        "You've inherited the conversation context above from a parent agent \
         working in {parent_cwd}. You are operating in an isolated git worktree \
         at {worktree_cwd} \u{2014} same repo, separate working copy. Translate \
         paths accordingly. Re-read files before editing. Your changes stay \
         isolated until explicitly merged."
    )
}

/// Check if we are inside a fork child (prevents recursive forking).
///
/// Scans messages for the FORK_BOILERPLATE_TAG which is only present
/// in fork child contexts.
///
/// TS: isInForkChild(messages) in forkSubagent.ts
pub fn is_in_fork_child(messages: &[serde_json::Value]) -> bool {
    let tag_marker = format!("<{FORK_BOILERPLATE_TAG}>");
    messages.iter().any(|msg| {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "user" {
            return false;
        }
        if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
            content.iter().any(|block| {
                block.get("type").and_then(|t| t.as_str()) == Some("text")
                    && block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .is_some_and(|text| text.contains(&tag_marker))
            })
        } else if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
            text.contains(&tag_marker)
        } else {
            false
        }
    })
}

/// Environment variable to enable/disable fork subagent feature.
///
/// TS: process.env.FORK_SUBAGENT
const FORK_SUBAGENT_ENV: &str = "FORK_SUBAGENT";

/// Check if fork subagent feature is enabled.
///
/// Enabled when FORK_SUBAGENT env var is set to "true" or "1".
/// TS: isForkSubagentEnabled() in forkSubagent.ts
pub fn is_fork_enabled() -> bool {
    std::env::var(FORK_SUBAGENT_ENV)
        .ok()
        .is_some_and(|v| v == "true" || v == "1")
}

/// Check if a fork is allowed for this context.
///
/// Fork requires: feature enabled, depth 0, no explicit subagent_type,
/// and not already inside a fork child.
///
/// TS: Recursive fork guard in forkSubagent.ts
pub fn is_fork_allowed(
    query_depth: i32,
    subagent_type: Option<&str>,
    messages: &[serde_json::Value],
) -> bool {
    is_fork_enabled() && query_depth == 0 && subagent_type.is_none() && !is_in_fork_child(messages)
}

#[cfg(test)]
#[path = "agent_fork.test.rs"]
mod tests;
