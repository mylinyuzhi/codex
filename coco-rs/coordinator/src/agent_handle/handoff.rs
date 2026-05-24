//! Post-spawn handoff classifier.
//!
//! TS: `agentToolUtils.ts:classifyHandoffIfNeeded`. It fail-opens when
//! the `SideQueryHandle` isn't installed.
//!
//! **W6.2 full**: the underlying logic now lives in free async
//! functions taking pre-cloned `Arc`s, so the detached engine task
//! in [`super::spawn::spawn_subagent`]'s sync path can call them
//! without holding a `&self` borrow across an `await` point.

use super::SwarmAgentHandle;

/// Free-fn implementation of [`SwarmAgentHandle::classify_handoff_if_needed`].
/// Pre-cloned `side_query` lets the function run inside a detached
/// `tokio::spawn` body (W6.2 sync detach race).
pub(crate) async fn classify_handoff_inline(
    agent_type: &str,
    qr: &coco_tool_runtime::AgentQueryResult,
    side_query: Option<&coco_tool_runtime::SideQueryHandle>,
) -> Option<String> {
    if !coco_subagent::should_classify(agent_type, qr.tool_use_count) {
        return qr.response_text.clone();
    }
    let Some(side_query) = side_query else {
        return qr.response_text.clone();
    };

    let transcript = coco_subagent::build_handoff_transcript_summary(&qr.messages);
    let (sys1, user1) =
        coco_subagent::handoff_stage1_prompts(agent_type, &transcript, qr.tool_use_count);

    let stage1_text = match side_query
        .query(coco_tool_runtime::SideQueryRequest::simple(
            &sys1,
            &user1,
            "subagent-handoff-stage1",
        ))
        .await
    {
        Ok(resp) => resp.text.unwrap_or_default(),
        Err(_) => return qr.response_text.clone(),
    };

    let stage1_verdict = coco_subagent::parse_classifier_response(&stage1_text);
    if matches!(stage1_verdict, coco_subagent::HandoffClassification::Safe) {
        return qr.response_text.clone();
    }

    let (sys2, user2) = coco_subagent::handoff_stage2_prompts(&stage1_text, &transcript);
    let stage2_text = match side_query
        .query(coco_tool_runtime::SideQueryRequest::simple(
            &sys2,
            &user2,
            "subagent-handoff-stage2",
        ))
        .await
    {
        Ok(resp) => resp.text.unwrap_or_default(),
        Err(_) => return qr.response_text.clone(),
    };

    let final_verdict = coco_subagent::parse_classifier_response(&stage2_text);
    match coco_subagent::render_block_message(&final_verdict) {
        Some(blocked) => Some(blocked),
        None => qr.response_text.clone(),
    }
}

impl SwarmAgentHandle {
    /// Method wrapper around [`classify_handoff_inline`] for `&self`
    /// callers (tests + any non-detach paths). The W6.2 sync detach
    /// race goes through the free fn directly so it can run inside
    /// a detached `tokio::spawn` closure. Marked `#[allow(dead_code)]`
    /// because production callers use the free fn; tests still
    /// invoke this wrapper.
    #[allow(dead_code)]
    pub(crate) async fn classify_handoff_if_needed(
        &self,
        agent_type: &str,
        qr: &coco_tool_runtime::AgentQueryResult,
    ) -> Option<String> {
        classify_handoff_inline(agent_type, qr, self.side_query()).await
    }
}
