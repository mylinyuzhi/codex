//! Fork-subagent pure logic — context cloning, XML rules, recursion guard.
//!
//! The runner (`root/coordinator`) reads these helpers; this crate only owns
//! the rules and the message shape, in line with the pure-logic charter
//! (no tokio, no QueryEngine, no AppState).
//!
//! When the FORK_SUBAGENT feature is enabled and no explicit `subagent_type`
//! is specified, the agent inherits the parent's byte-identical system prompt
//! (enabling prompt cache sharing) and receives the parent's conversation
//! context with `tool_use` results replaced by [`FORK_PLACEHOLDER`].

use std::sync::Arc;

use coco_llm_types::LlmMessage;
use coco_llm_types::ToolContentPart;
use coco_llm_types::ToolResultContent;
use coco_llm_types::UserContentPart;
use coco_types::messages::Message;

/// XML tag wrapping the fork boilerplate rules.
///
/// Used for: (1) wrapping rules, (2) detecting recursive forks via tag scan.
pub const FORK_BOILERPLATE_TAG: &str = "fork-boilerplate";

/// Prefix before the directive text in fork child messages.
///
/// Note the trailing SPACE — the directive text is appended inline,
/// not on a new line.
pub const FORK_DIRECTIVE_PREFIX: &str = "Your directive: ";

/// Placeholder text injected for each `tool_use` result in fork context.
///
/// Em-dash is U+2014. Byte-identical wire format is what matters for the
/// prompt-cache invariant — every fork child must produce the same prefix.
pub const FORK_PLACEHOLDER: &str = "Fork started \u{2014} processing in background";

/// Fork subagent context — carries the parent's conversation history,
/// ready to thread into the child's prior history. The directive itself
/// is wrapped separately by [`build_fork_child_message`] at the spawn
/// site (so the caller can decorate it with skill-preload / hook context
/// first), so it is NOT carried on this struct.
#[derive(Debug, Clone)]
pub struct ForkContext {
    /// Parent's conversation messages (with `tool_result` content
    /// replaced by [`FORK_PLACEHOLDER`]). Ready to thread into the
    /// child's prior history. Shared via `Arc` so the rewrite only
    /// allocates fresh messages for the tool-result variant; every
    /// other entry is a cheap Arc-clone of the parent's history.
    pub messages: Vec<Arc<Message>>,
}

/// Build a fork context from the parent's conversation history.
///
/// Rewrites `Message::ToolResult` bodies to [`FORK_PLACEHOLDER`] so
/// every fork child produces a byte-identical API request prefix
/// (prompt-cache sharing). Non-tool-result messages share the parent's
/// `Arc<Message>` allocation directly; only the rewritten entries
/// allocate.
///
/// **Divergence from TS (deliberate, documented).** TS `buildForkedMessages`
/// keeps the *real* tool results for all prior turns and blanks only the
/// in-flight turn's `tool_use` blocks; coco-rs blanks **every** historical
/// `tool_result`. coco optimises for child↔child cache sharing (all
/// children get an identical prefix regardless of what the parent's tools
/// returned) at the cost of the child not seeing earlier tool output. This
/// is acceptable for the current fork use (short, scoped worker directives)
/// and avoids threading the in-flight assistant message through
/// `SpawnMode::Fork`; revisit if forks need the parent's gathered context.
/// Gated behind the default-off `COCO_FORK_SUBAGENT`.
pub fn build_fork_context(parent_messages: &[Arc<Message>]) -> ForkContext {
    let mut forked: Vec<Arc<Message>> = Vec::with_capacity(parent_messages.len());

    for arc in parent_messages {
        match arc.as_ref() {
            Message::ToolResult(trm) => {
                let mut new_trm = trm.clone();
                if let LlmMessage::Tool { content, .. } = &mut new_trm.message {
                    for part in content.iter_mut() {
                        if let ToolContentPart::ToolResult(tr) = part {
                            tr.output = ToolResultContent::text(FORK_PLACEHOLDER);
                        }
                    }
                }
                forked.push(Arc::new(Message::ToolResult(new_trm)));
            }
            _ => forked.push(arc.clone()),
        }
    }

    ForkContext { messages: forked }
}

/// Build the full child message with XML-wrapped rules + directive.
///
/// Produces a string ending with `</fork-boilerplate>\n\n{prefix}{directive}`
/// and no trailing newline.
pub fn build_fork_child_message(directive: &str) -> String {
    let rules = build_fork_child_rules();
    format!(
        "<{FORK_BOILERPLATE_TAG}>\n{rules}\n</{FORK_BOILERPLATE_TAG}>\n\n{FORK_DIRECTIVE_PREFIX}{directive}"
    )
}

/// Build the child rules body injected between the fork-boilerplate tags.
pub fn build_fork_child_rules() -> String {
    concat!(
        "STOP. READ THIS FIRST.\n",
        "\n",
        "You are a forked worker process. You are NOT the main agent.\n",
        "\n",
        "RULES (non-negotiable):\n",
        // Rule 1 uses U+2014 em-dash (the `—` escape in the original).
        "1. Your system prompt says \"default to forking.\" IGNORE IT \u{2014} that's for the parent. You ARE the fork. Do NOT spawn sub-agents; execute directly.\n",
        "2. Do NOT converse, ask questions, or suggest next steps\n",
        "3. Do NOT editorialize or add meta-commentary\n",
        "4. USE your tools directly: Bash, Read, Write, etc.\n",
        "5. If you modify files, commit your changes before reporting. Include the commit hash in your report.\n",
        "6. Do NOT emit text between tool calls. Use tools silently, then report once at the end.\n",
        // Rule 7 uses inline em-dash U+2014.
        "7. Stay strictly within your directive's scope. If you discover related systems outside your scope, mention them in one sentence at most \u{2014} other workers cover those areas.\n",
        "8. Keep your report under 500 words unless the directive specifies otherwise. Be factual and concise.\n",
        "9. Your response MUST begin with \"Scope:\". No preamble, no thinking-out-loud.\n",
        "10. REPORT structured facts, then stop\n",
        "\n",
        "Output format (plain text labels, not markdown headers):\n",
        "  Scope: <echo back your assigned scope in one sentence>\n",
        "  Result: <the answer or key findings, limited to the scope above>\n",
        // Output lines use inline em-dashes U+2014.
        "  Key files: <relevant file paths \u{2014} include for research tasks>\n",
        "  Files changed: <list with commit hash \u{2014} include only if you modified files>\n",
        "  Issues: <list \u{2014} include only if there are issues to flag>",
    )
    .to_string()
}

/// Check if we are inside a fork child (prevents recursive forking).
///
/// Scans user-role messages for the [`FORK_BOILERPLATE_TAG`] inside
/// any text content part — the tag is only present in fork child
/// contexts (injected by [`build_fork_child_message`]).
///
/// **Known limitation (compaction).** This is a *message-scan* guard:
/// it only detects a fork child while the `<fork-boilerplate>` user turn
/// is still in the live history. The TS reference pairs it with a
/// PRIMARY, history-independent signal (`querySource === 'agent:builtin:fork'`)
/// that survives autocompaction. coco-rs has no fork-source field on
/// `ToolUseContext`, so a long-running fork that compacts its history
/// (summarising away the boilerplate turn) can re-enter the fork path —
/// a fork-of-fork. The whole fork feature is gated behind the default-off
/// `COCO_FORK_SUBAGENT`, so this is latent; closing it requires threading
/// a typed fork marker onto `ToolUseContext` (engine seam), tracked as a
/// follow-up rather than risking the engine's per-call context for an
/// off-by-default path.
pub fn is_in_fork_child(messages: &[Arc<Message>]) -> bool {
    let tag_marker = format!("<{FORK_BOILERPLATE_TAG}>");
    messages.iter().any(|arc| {
        let Message::User(user) = arc.as_ref() else {
            return false;
        };
        let LlmMessage::User { content, .. } = &user.message else {
            return false;
        };
        content.iter().any(|part| {
            matches!(
                part,
                UserContentPart::Text(t) if t.text.contains(&tag_marker)
            )
        })
    })
}

/// Check if fork subagent feature is enabled.
///
/// Enabled when [`coco_config::EnvKey::CocoForkSubagent`] (`COCO_FORK_SUBAGENT`)
/// is truthy (`1`/`true`/`yes`/`on`). **Note**: additional short-circuit to
/// `false` when coordinator mode is on or the session is non-interactive — that
/// gating happens in [`crate::coordinator_mode::is_fork_subagent_active`],
/// which composes this check with the coordinator/interactivity guards.
pub fn is_fork_enabled() -> bool {
    coco_config::env::is_env_truthy(coco_config::EnvKey::CocoForkSubagent)
}

#[cfg(test)]
#[path = "fork.test.rs"]
mod tests;
