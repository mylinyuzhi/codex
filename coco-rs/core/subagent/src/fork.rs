//! Fork-subagent pure logic — context cloning, XML rules, recursion guard.
//!
//! TS: `tools/AgentTool/forkSubagent.ts` (8.6K LOC).
//!
//! Byte-for-byte mirror of TS. The runner (PR #5, `root/coordinator`) reads
//! these helpers; this crate only owns the rules and the message shape, in
//! line with the pure-logic charter (no tokio, no QueryEngine, no AppState).
//!
//! When the FORK_SUBAGENT feature is enabled and no explicit `subagent_type`
//! is specified, the agent inherits the parent's byte-identical system prompt
//! (enabling prompt cache sharing) and receives the parent's conversation
//! context with `tool_use` results replaced by [`FORK_PLACEHOLDER`].

use serde::Deserialize;
use serde::Serialize;

/// XML tag wrapping the fork boilerplate rules.
///
/// TS: `constants/xml.ts:63` `FORK_BOILERPLATE_TAG = 'fork-boilerplate'`.
/// Used for: (1) wrapping rules, (2) detecting recursive forks via tag scan.
pub const FORK_BOILERPLATE_TAG: &str = "fork-boilerplate";

/// Prefix before the directive text in fork child messages.
///
/// TS: `constants/xml.ts:66` `FORK_DIRECTIVE_PREFIX = 'Your directive: '`.
/// Note the trailing SPACE — TS appends the directive text inline, not on
/// a new line. Verified against `forkSubagent.ts:197` where the template
/// literal uses `${FORK_DIRECTIVE_PREFIX}${directive}`.
pub const FORK_DIRECTIVE_PREFIX: &str = "Your directive: ";

/// Placeholder text injected for each `tool_use` result in fork context.
///
/// TS: `forkSubagent.ts:93` `FORK_PLACEHOLDER_RESULT = 'Fork started — processing in background'`.
/// Em-dash is U+2014. Byte-identical wire format is what matters for the
/// prompt-cache invariant — every fork child must produce the same prefix.
pub const FORK_PLACEHOLDER: &str = "Fork started \u{2014} processing in background";

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

/// Build a fork context from the parent's last assistant message.
///
/// Takes the parent's assistant message (with `tool_use` blocks) and creates
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

/// Build the full child message with XML-wrapped rules + directive.
///
/// Byte-for-byte reproduction of TS `forkSubagent.ts:171-198`
/// `buildChildMessage(directive)`. The template literal in TS produces a
/// string ending with `</fork-boilerplate>\n\n{prefix}{directive}` and no
/// trailing newline.
pub fn build_fork_child_message(directive: &str) -> String {
    let rules = build_fork_child_rules();
    format!(
        "<{FORK_BOILERPLATE_TAG}>\n{rules}\n</{FORK_BOILERPLATE_TAG}>\n\n{FORK_DIRECTIVE_PREFIX}{directive}"
    )
}

/// Build the child rules body injected between the fork-boilerplate tags.
///
/// Byte-identical to TS `forkSubagent.ts:173-194`.
pub fn build_fork_child_rules() -> String {
    concat!(
        "STOP. READ THIS FIRST.\n",
        "\n",
        "You are a forked worker process. You are NOT the main agent.\n",
        "\n",
        "RULES (non-negotiable):\n",
        // Rule 1 uses U+2014 em-dash — TS source has it as `—` escape.
        "1. Your system prompt says \"default to forking.\" IGNORE IT \u{2014} that's for the parent. You ARE the fork. Do NOT spawn sub-agents; execute directly.\n",
        "2. Do NOT converse, ask questions, or suggest next steps\n",
        "3. Do NOT editorialize or add meta-commentary\n",
        "4. USE your tools directly: Bash, Read, Write, etc.\n",
        "5. If you modify files, commit your changes before reporting. Include the commit hash in your report.\n",
        "6. Do NOT emit text between tool calls. Use tools silently, then report once at the end.\n",
        // Rule 7 em-dash is inline in TS source, U+2014.
        "7. Stay strictly within your directive's scope. If you discover related systems outside your scope, mention them in one sentence at most \u{2014} other workers cover those areas.\n",
        "8. Keep your report under 500 words unless the directive specifies otherwise. Be factual and concise.\n",
        "9. Your response MUST begin with \"Scope:\". No preamble, no thinking-out-loud.\n",
        "10. REPORT structured facts, then stop\n",
        "\n",
        "Output format (plain text labels, not markdown headers):\n",
        "  Scope: <echo back your assigned scope in one sentence>\n",
        "  Result: <the answer or key findings, limited to the scope above>\n",
        // Lines 192-194 em-dashes are inline in TS source, U+2014.
        "  Key files: <relevant file paths \u{2014} include for research tasks>\n",
        "  Files changed: <list with commit hash \u{2014} include only if you modified files>\n",
        "  Issues: <list \u{2014} include only if there are issues to flag>",
    )
    .to_string()
}

/// Build a worktree notice for forked agents in isolated worktrees.
///
/// Byte-faithful to TS `forkSubagent.ts:205-210` `buildWorktreeNotice`.
/// Em-dash is U+2014. Produces a single line with no embedded newlines —
/// the caller appends it to the inherited context.
pub fn build_worktree_notice(parent_cwd: &str, worktree_cwd: &str) -> String {
    format!(
        "You've inherited the conversation context above from a parent agent working in {parent_cwd}. You are operating in an isolated git worktree at {worktree_cwd} \u{2014} same repository, same relative file structure, separate working copy. Paths in the inherited context refer to the parent's working directory; translate them to your worktree root. Re-read files before editing if the parent may have modified them since they appear in the context. Your changes stay in this worktree and will not affect the parent's files."
    )
}

/// Check if we are inside a fork child (prevents recursive forking).
///
/// Scans messages for the [`FORK_BOILERPLATE_TAG`] which is only present
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

/// Check if fork subagent feature is enabled.
///
/// Enabled when [`coco_config::EnvKey::CocoForkSubagent`] (`COCO_FORK_SUBAGENT`)
/// is truthy (`1`/`true`/`yes`/`on`). TS: `isForkSubagentEnabled()` in
/// `forkSubagent.ts`. **Note**: TS additionally short-circuits to `false` when
/// `isCoordinatorMode()` is true or the session is non-interactive — that
/// gating happens in [`crate::coordinator_mode::is_fork_subagent_active`],
/// which composes this check with the coordinator/interactivity guards.
pub fn is_fork_enabled() -> bool {
    coco_config::env::is_env_truthy(coco_config::EnvKey::CocoForkSubagent)
}

/// Check if a fork is allowed for this context.
///
/// Fork requires: feature enabled, depth 0, no explicit `subagent_type`,
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
#[path = "fork.test.rs"]
mod tests;
