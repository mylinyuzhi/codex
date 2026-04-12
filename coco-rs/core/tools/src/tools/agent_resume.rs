//! Agent resumption — restore interrupted agents from transcript.
//!
//! TS: tools/AgentTool/resumeAgent.ts
//!
//! Provides transcript filtering and state restoration for resuming
//! agents that were interrupted or backgrounded.

use serde::Deserialize;
use serde::Serialize;

/// State required to resume an agent.
///
/// TS: ResumeAgentConfig in resumeAgent.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResumeState {
    /// The agent's ID to resume.
    pub agent_id: String,
    /// Restored conversation messages (filtered for consistency).
    pub messages: Vec<serde_json::Value>,
    /// Agent type that was running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Model that was in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Worktree path if the agent had isolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// New prompt to append (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_prompt: Option<String>,
}

/// Filter out incomplete or corrupted messages from a transcript.
///
/// TS: filterOrphanedThinkingOnlyMessages() + filterUnresolvedToolUses()
///     + filterWhitespaceOnlyAssistantMessages()
pub fn filter_transcript(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut filtered = Vec::with_capacity(messages.len());

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

        if role == "assistant" {
            // Skip whitespace-only assistant messages
            if is_whitespace_only_assistant(msg) {
                continue;
            }
            // Skip thinking-only messages (no text or tool_use)
            if is_thinking_only_assistant(msg) {
                continue;
            }
        }

        filtered.push(msg.clone());
    }

    // Remove trailing unresolved tool uses
    strip_unresolved_tool_uses(&mut filtered);

    filtered
}

/// Check if an assistant message contains only whitespace text.
fn is_whitespace_only_assistant(msg: &serde_json::Value) -> bool {
    let Some(content) = msg.get("content").and_then(|c| c.as_array()) else {
        // String content
        return msg
            .get("content")
            .and_then(|c| c.as_str())
            .is_some_and(|s| s.trim().is_empty());
    };

    content.iter().all(|block| {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match block_type {
            "text" => block
                .get("text")
                .and_then(|t| t.as_str())
                .is_some_and(|s| s.trim().is_empty()),
            "thinking" => true, // Thinking blocks don't count as content
            _ => false,
        }
    })
}

/// Check if an assistant message has only thinking blocks (no text or tool_use).
fn is_thinking_only_assistant(msg: &serde_json::Value) -> bool {
    let Some(content) = msg.get("content").and_then(|c| c.as_array()) else {
        return false;
    };

    let has_substantive = content.iter().any(|block| {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        matches!(block_type, "text" | "tool_use")
    });

    !has_substantive
}

/// Remove trailing assistant messages with tool_use blocks that have no
/// corresponding tool_result in subsequent messages.
fn strip_unresolved_tool_uses(messages: &mut Vec<serde_json::Value>) {
    // Collect all tool_result IDs
    let mut resolved_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in messages.iter() {
        if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(id) = block.get("tool_use_id").and_then(|v| v.as_str()) {
                        resolved_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // Strip trailing assistant messages with unresolved tool_use
    while let Some(last) = messages.last() {
        let role = last.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "assistant" {
            break;
        }

        let has_unresolved =
            last.get("content")
                .and_then(|c| c.as_array())
                .is_some_and(|content| {
                    content.iter().any(|block| {
                        block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                            && block
                                .get("id")
                                .and_then(|v| v.as_str())
                                .is_some_and(|id| !resolved_ids.contains(id))
                    })
                });

        if has_unresolved {
            messages.pop();
        } else {
            break;
        }
    }
}

#[cfg(test)]
#[path = "agent_resume.test.rs"]
mod tests;
