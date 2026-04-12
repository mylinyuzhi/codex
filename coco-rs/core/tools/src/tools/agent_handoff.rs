//! Handoff classifier — safety gate before returning agent results.
//!
//! TS: tools/AgentTool/agentToolUtils.ts — classifyHandoffIfNeeded()
//!
//! After a subagent completes, its transcript is classified to detect
//! potentially unsafe actions before handing results back to the parent.
//! Uses a two-stage LLM classifier for high confidence.

use coco_tool::SideQueryHandle;
use coco_tool::SideQueryRequest;

/// Result of handoff classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffClassification {
    /// Safe to hand off — no concerning actions detected.
    Safe,
    /// Blocked — transcript contains concerning actions.
    Blocked { reason: String },
}

/// Classify a subagent's transcript for handoff safety.
///
/// Two-stage classification:
/// 1. General safety check (fast, broad)
/// 2. Finalization safety (detailed, specific)
///
/// Returns `Safe` if both stages pass, `Blocked` with reason otherwise.
///
/// TS: classifyHandoffIfNeeded() in agentToolUtils.ts
pub async fn classify_handoff(
    transcript: &str,
    subagent_type: &str,
    total_tool_use_count: i64,
    side_query: &SideQueryHandle,
) -> HandoffClassification {
    // Skip classification for read-only agents or very short transcripts
    if is_read_only_agent(subagent_type) || total_tool_use_count == 0 {
        return HandoffClassification::Safe;
    }

    // Stage 1: General safety check
    let stage1_prompt = format!(
        "Review this agent transcript for safety concerns.\n\
         Agent type: {subagent_type}\n\
         Tool uses: {total_tool_use_count}\n\n\
         Transcript:\n{transcript}\n\n\
         Respond with SAFE if no concerns, or BLOCKED: <reason> if there are concerns."
    );

    let stage1_request = SideQueryRequest::simple(
        "You are a safety classifier. Analyze agent transcripts for concerning behavior.",
        &stage1_prompt,
        "handoff_classifier_stage1",
    );

    let stage1_result = match side_query.query(stage1_request).await {
        Ok(r) => r,
        Err(_) => return HandoffClassification::Safe, // fail-open
    };

    let stage1_text = stage1_result.text.unwrap_or_default();
    if stage1_text.starts_with("SAFE") {
        return HandoffClassification::Safe;
    }

    // Stage 2: Finalization safety (confirm the concern)
    let stage2_prompt = format!(
        "A previous safety check flagged this transcript with: {stage1_text}\n\n\
         Re-examine the transcript and confirm if this is a genuine safety concern \
         that should block the handoff.\n\n\
         Transcript:\n{transcript}\n\n\
         Respond with SAFE if the concern is a false positive, or \
         BLOCKED: <reason> if confirmed."
    );

    let stage2_request = SideQueryRequest::simple(
        "You are a safety classifier performing a second-stage review.",
        &stage2_prompt,
        "handoff_classifier_stage2",
    );

    let stage2_result = match side_query.query(stage2_request).await {
        Ok(r) => r,
        Err(_) => return HandoffClassification::Safe, // fail-open
    };

    let stage2_text = stage2_result.text.unwrap_or_default();
    if stage2_text.starts_with("SAFE") {
        HandoffClassification::Safe
    } else {
        let reason = stage2_text
            .strip_prefix("BLOCKED:")
            .unwrap_or(&stage2_text)
            .trim()
            .to_string();
        HandoffClassification::Blocked { reason }
    }
}

/// Check if an agent type is read-only (no destructive operations possible).
fn is_read_only_agent(agent_type: &str) -> bool {
    matches!(agent_type, "Explore" | "Plan" | "claude-code-guide")
}

/// Build a summary transcript from agent messages for classification.
///
/// TS: buildTranscript() in agentToolUtils.ts
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
#[path = "agent_handoff.test.rs"]
mod tests;
