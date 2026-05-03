//! Post-spawn handoff classifier and AgentSummary.
//!
//! TS: `agentToolUtils.ts:classifyHandoffIfNeeded` and
//! `services/AgentSummary/agentSummary.ts`. Both fail-open when the
//! `SideQueryHandle` isn't installed.

use super::SwarmAgentHandle;

impl SwarmAgentHandle {
    /// 2-stage LLM classifier on a completed subagent's transcript.
    /// Returns the original response text on `Safe`; replaces it with the
    /// rendered `<tool_use_error>` payload on `Blocked`. Pure-logic helpers
    /// live in [`coco_subagent::handoff`]; this method owns the LLM I/O.
    ///
    /// Fail-open semantics: any side-query error or empty response is
    /// treated as `Safe` so a flaky classifier doesn't gate legitimate
    /// output. TS parity: `agentToolUtils.ts:classifyHandoffIfNeeded`.
    pub(crate) async fn classify_handoff_if_needed(
        &self,
        agent_type: &str,
        qr: &coco_tool_runtime::AgentQueryResult,
    ) -> Option<String> {
        if !coco_subagent::should_classify(agent_type, qr.tool_use_count) {
            return qr.response_text.clone();
        }

        let Some(side_query) = self.side_query() else {
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

        // Stage 1 short-circuit: bail out cleanly if the model said SAFE.
        let stage1_verdict = coco_subagent::parse_classifier_response(&stage1_text);
        if matches!(stage1_verdict, coco_subagent::HandoffClassification::Safe) {
            return qr.response_text.clone();
        }

        // Stage 2 confirmation pass. Only proceed when stage 1 raised a
        // flag — bounds LLM cost.
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

    /// One-shot AgentSummary fork on a completed subagent. Writes a
    /// 3-5-word summary onto `SubAgentState.last_message` so the panel
    /// widget shows what each agent ended up doing.
    ///
    /// Mirrors TS `services/AgentSummary/agentSummary.ts` but runs once
    /// at completion rather than periodically every 30 s. The
    /// completion-only variant fits coco-rs' panel update model and bounds
    /// the cost to one cheap LLM call per spawn.
    ///
    /// Fail-open: missing handle / dispatch error / NONE response /
    /// `should_summarize` rejection all leave `last_message` unchanged.
    pub(crate) async fn summarize_handoff_if_needed(
        &self,
        agent_type: &str,
        qr: &coco_tool_runtime::AgentQueryResult,
        agent_id: &str,
    ) {
        if !coco_subagent::should_summarize(agent_type, qr.tool_use_count) {
            return;
        }
        let Some(side_query) = self.side_query() else {
            return;
        };

        let (sys, user) = coco_subagent::build_summary_prompts(agent_type, /*previous*/ None);
        let summary_text = match side_query
            .query(coco_tool_runtime::SideQueryRequest::simple(
                &sys,
                &user,
                "subagent-summary",
            ))
            .await
        {
            Ok(resp) => resp.text.unwrap_or_default(),
            Err(_) => return,
        };

        if let Some(clean) = coco_subagent::sanitize_summary(&summary_text) {
            let mut agents = self.agents().write().await;
            if let Some(agent) = agents.iter_mut().find(|a| a.agent_id == agent_id) {
                agent.last_message = Some(clean);
            }
        }
    }
}
