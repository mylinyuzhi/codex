//! Subagent transcript summarisation — pure-logic prompt builders (E3).
//!
//! TS: `services/AgentSummary/agentSummary.ts::buildSummaryPrompt`.
//!
//! TS runs this *periodically* (every 30 s) during execution to populate
//! a live `AgentProgress.summary` field shown in the coordinator UI.
//! coco-rs runs it once at subagent completion, populating
//! `SubAgentState.last_message` so the `CoordinatorPanel` and the
//! `presentation::activity` agent surface can show "what did this agent
//! end up doing" at a glance. The cost is one cheap LLM call per spawn,
//! gated on `should_summarize` so trivial transcripts skip.
//!
//! Bodies build the prompts; the LLM call is driven by
//! [`coco_tool_runtime::SideQuery`] in the runtime layer
//! (`coco_coordinator::agent_handle`), which already wires the same
//! primitive for [`crate::handoff`].

/// Whether summarisation should run for this turn. Skips trivial
/// transcripts (zero or one tool use) and read-only agent types — the
/// summary would just restate the agent type. Mirrors TS minimum-
/// transcript guard (`agentSummary.ts:69-75`).
pub fn should_summarize(agent_type: &str, total_tool_use_count: i64) -> bool {
    !crate::handoff::is_read_only_agent(agent_type) && total_tool_use_count >= 2
}

/// Build the system + user prompts for a one-shot summary call.
///
/// Returns `(system_prompt, user_prompt)`. The system prompt is empty
/// — TS folds the entire instruction into the user message, which we
/// preserve byte-faithfully so cache parity with the parent's first
/// turn isn't accidentally invalidated by a stray system-prompt
/// difference.
///
/// `previous` is the prior summary (`None` on first call). Including
/// it in the prompt asks the model to say something *new* rather than
/// restate the previous summary — TS uses this for the periodic
/// version; in coco-rs we only call once but keep the field for
/// forward compatibility when the periodic mode lands.
///
/// **Byte-faithful with TS** `services/AgentSummary/agentSummary.ts::buildSummaryPrompt`
/// — including the empty-line separation around `prev_line`. Use a
/// concatenated raw-block layout instead of indented `\` continuations
/// so leading-whitespace from rustfmt never sneaks into the prompt
/// body (each continuation line in `format!("…\n\
///      indented")` keeps the indent literal in the output, breaking
/// cache parity with TS).
pub fn build_summary_prompts(_agent_type: &str, previous: Option<&str>) -> (String, String) {
    let prev_line = match previous {
        Some(p) if !p.is_empty() => {
            format!("\nPrevious: \"{p}\" \u{2014} say something NEW.\n")
        }
        _ => String::new(),
    };

    let user = format!(
        "Describe your most recent action in 3-5 words using present tense (-ing). \
Name the file or function, not the branch. Do not use tools.
{prev_line}
Good: \"Reading runAgent.ts\"
Good: \"Fixing null check in validate.ts\"
Good: \"Running auth module tests\"
Good: \"Adding retry logic to fetchUser\"

Bad (past tense): \"Analyzed the branch diff\"
Bad (too vague): \"Investigating the issue\"
Bad (too long): \"Reviewing full branch diff and AgentTool.tsx integration\"
Bad (branch name): \"Analyzed adam/background-summary branch diff\""
    );

    (String::new(), user)
}

/// Filter the model's reply down to a clean summary string. Trims
/// whitespace, strips surrounding quotes, and rejects responses that
/// are empty or longer than 80 characters (the same defensive cap TS
/// uses to keep the panel from word-wrapping into multi-line). Returns
/// `None` when the model declined or produced noise.
pub fn sanitize_summary(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 80 || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(trimmed)
}

#[cfg(test)]
#[path = "summary.test.rs"]
mod tests;
