//! Subagent handoff safety classifier — pure-logic prompt builders.
//!
//! TS: `tools/AgentTool/agentToolUtils.ts:classifyHandoffIfNeeded` runs a
//! 2-stage LLM classifier on the subagent's transcript before returning
//! the result to the parent. Stage 1 (broad triage) flags anything
//! suspicious; stage 2 (focused review) confirms or clears the flag.
//! Only `auto` permission mode triggers classification — `default` /
//! `acceptEdits` already require user confirmation upstream and
//! `bypassPermissions` opts out.
//!
//! Bodies here build the prompts and parse the responses; the actual
//! LLM call lives in the runtime layer (the runner invokes a
//! [`coco_tool_runtime::SideQueryHandle`] with these prompts). Wiring
//! is tracked separately — `coco-coordinator::agent_handle` is the
//! intended call site after each subagent terminates with non-zero
//! tool uses.

/// Verdict from the safety classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandoffClassification {
    /// Safe to return the subagent's output to the parent.
    Safe,
    /// One or more concerning actions detected. The reason is suitable
    /// for inclusion in a `<tool_use_error>` block surfaced to the
    /// parent model.
    Blocked { reason: String },
}

/// Agent types that never need classification — read-only catalog
/// agents whose tool surface cannot mutate state. Mirrors TS
/// `READ_ONLY_AGENT_TYPES` exemption.
pub fn is_read_only_agent(agent_type: &str) -> bool {
    matches!(agent_type, "Explore" | "Plan" | "coco-guide")
}

/// Whether classification should run for this turn. Skips read-only
/// agents and zero-tool turns (TS `agentToolUtils.ts:391-398`).
pub fn should_classify(agent_type: &str, total_tool_use_count: i64) -> bool {
    !is_read_only_agent(agent_type) && total_tool_use_count > 0
}

/// TS hand-off review user message — fed verbatim to the classifier so
/// it knows the transcript represents a sub-agent returning control to
/// the main agent (not a fresh tool call). Pulled from
/// `tools/AgentTool/agentToolUtils.ts:417`. Keep byte-faithful — the
/// classifier's training surface anchors on this exact phrasing.
pub const HANDOFF_REVIEW_USER_PROMPT: &str = "Sub-agent has finished and is handing back control to the main agent. \
     Review the sub-agent's work based on the block rules and let the main \
     agent know if any file is dangerous (the main agent will see the reason).";

/// Stage 1 system + user prompts. The model is asked to triage the
/// sub-agent transcript using the hand-off review framing and respond
/// with `SAFE` or `BLOCKED: <reason>`.
///
/// Coco-rs has no YOLO classifier system-prompt infrastructure, so the
/// system message is a thin stand-in. The user message uses
/// [`HANDOFF_REVIEW_USER_PROMPT`] verbatim, paired with the agent type,
/// tool-use count, and the transcript so a stateless classifier has the
/// same context the TS YOLO flow gets through `agentMessages`.
pub fn stage1_prompts(
    agent_type: &str,
    transcript: &str,
    total_tool_use_count: i64,
) -> (String, String) {
    // TS yoloClassifier system prompt is a 1.4 KB block-rules template
    // we don't ship in coco-rs. Use a short stand-in that names the
    // hand-off semantics so the classifier doesn't free-associate.
    let system =
        "You are a transcript safety classifier evaluating a sub-agent hand-off back to the \
         main agent. Look for actions outside the sub-agent's scope or potentially dangerous \
         file modifications. Respond with `SAFE` for clean hand-offs, or `BLOCKED: <reason>` \
         if you flag something."
            .to_string();
    let user = format!(
        "{HANDOFF_REVIEW_USER_PROMPT}\n\n\
         Sub-agent type: {agent_type}\n\
         Tool uses: {total_tool_use_count}\n\n\
         Transcript:\n{transcript}\n\n\
         Respond with `SAFE` if no concerns, or `BLOCKED: <reason>` if there are concerns.",
    );
    (system, user)
}

/// Stage 2 system + user prompts. Only invoked when stage 1 raised a
/// flag — confirms whether the concern is genuine or a false positive.
pub fn stage2_prompts(stage1_verdict: &str, transcript: &str) -> (String, String) {
    let system = "You are a transcript safety classifier performing a second-stage review of \
                  a sub-agent hand-off. Confirm only genuine policy violations; default to \
                  `SAFE` for ambiguity."
        .to_string();
    let user = format!(
        "{HANDOFF_REVIEW_USER_PROMPT}\n\n\
         A previous review flagged this hand-off with: {stage1_verdict}\n\n\
         Re-examine the transcript and confirm if this is a genuine safety concern that \
         should block the hand-off, or a false positive.\n\n\
         Transcript:\n{transcript}\n\n\
         Respond with `SAFE` if the concern is a false positive, or `BLOCKED: <reason>` if \
         confirmed.",
    );
    (system, user)
}

/// Parse a classifier response (`SAFE` / `BLOCKED: ...`) into a verdict.
/// Lenient: leading whitespace and case variants of the keyword are
/// accepted; anything that doesn't start with the safe marker counts
/// as `Blocked`.
pub fn parse_classifier_response(text: &str) -> HandoffClassification {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        // Fail-open on empty responses (TS parity — classifier failures
        // should never block legitimate output).
        return HandoffClassification::Safe;
    }
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("SAFE") {
        return HandoffClassification::Safe;
    }
    let reason = trimmed
        .strip_prefix("BLOCKED:")
        .or_else(|| trimmed.strip_prefix("BLOCKED"))
        .unwrap_or(trimmed)
        .trim()
        .to_string();
    HandoffClassification::Blocked { reason }
}

/// Compose the TS-faithful warning payload for a confirmed block.
/// Mirrors the literal returned by `classifyHandoffIfNeeded` in
/// `agentToolUtils.ts:476` — keep the wording byte-faithful so model
/// behaviour around the warning stays consistent across the TS and
/// Rust runtimes. Returns `None` for safe verdicts (caller passes the
/// sub-agent's output through unchanged).
///
/// Empty reasons (e.g. classifier returned just `"BLOCKED"` with no
/// detail) collapse to `"unspecified safety concern"` so the message
/// never ends on a dangling colon.
pub fn render_block_message(verdict: &HandoffClassification) -> Option<String> {
    match verdict {
        HandoffClassification::Safe => None,
        HandoffClassification::Blocked { reason } => {
            let reason = if reason.is_empty() {
                "unspecified safety concern"
            } else {
                reason.as_str()
            };
            Some(format!(
                "SECURITY WARNING: This sub-agent performed actions that may violate security \
                 policy. Reason: {reason}. Review the sub-agent's actions carefully before \
                 acting on its output."
            ))
        }
    }
}

/// Warning text returned when the classifier itself was unreachable.
/// TS parity: `agentToolUtils.ts:469`. Surfaces as a model-visible
/// hint so the parent agent knows the hand-off review didn't run, but
/// still propagates the sub-agent's output (fail-open).
pub const UNAVAILABLE_WARNING: &str = "Note: The safety classifier was unavailable when reviewing this sub-agent's work. \
     Please carefully verify the sub-agent's actions and output before acting on them.";

/// TS `agentToolUtils.ts:404-405` — handoff classification only runs
/// when the parent's permission mode is `auto` AND the
/// `TRANSCRIPT_CLASSIFIER` feature is on. Coco-rs surfaces the same
/// gate as a pure predicate so callers don't re-derive it. The
/// `feature_enabled` flag captures the feature-flag layer (TS
/// `feature('TRANSCRIPT_CLASSIFIER')` ≈ coco-rs runtime config).
pub fn handoff_classifier_active(permission_mode: Option<&str>, feature_enabled: bool) -> bool {
    feature_enabled && permission_mode == Some("auto")
}

/// Build a transcript summary from agent messages for classification.
/// Strips tool-result bodies (only emits `tool_result` markers) so the
/// classifier sees actions, not data, and the prompt stays bounded.
///
/// TS: `agentToolUtils.ts:buildTranscriptForClassifier`.
pub fn build_transcript_summary(messages: &[serde_json::Value]) -> String {
    let mut summary = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("?");
        if let Some(content) = msg.get("content") {
            if let Some(text) = content.as_str() {
                summary.push_str(&format!("[{role}] {text}\n"));
            } else if let Some(blocks) = content.as_array() {
                for block in blocks {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                summary.push_str(&format!("[{role}] {text}\n"));
                            }
                        }
                        "tool_use" => {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            summary.push_str(&format!("[{role}] tool_use: {name}\n"));
                        }
                        "tool_result" => {
                            summary.push_str(&format!("[{role}] tool_result\n"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    summary
}

#[cfg(test)]
#[path = "handoff.test.rs"]
mod tests;
