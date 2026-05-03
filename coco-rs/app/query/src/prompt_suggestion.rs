//! Post-turn promptSuggestion service.
//!
//! TS: `services/PromptSuggestion/promptSuggestion.ts:139` —
//! `executePromptSuggestion()` runs from `query/stopHooks.ts:139` after
//! every successful turn. It forks the parent session (cache-shared
//! via [`coco_types::CacheSafeParams`]), asks the model for a 3-12
//! word suggestion of "what should I ask next", and writes the result
//! to `ToolAppState.prompt_suggestion` so the TUI can render it
//! behind the user's cursor.
//!
//! ## Scope of this module
//!
//! Pure logic only. Three pieces:
//!
//! 1. [`should_suggest`] — the gate: `Feature::AgentTeams` is unrelated;
//!    suggestions gate on the user's `COCO_PROMPT_SUGGESTION_DISABLE` env,
//!    non-plan-mode, no pending permissions, and interactive session.
//!    TS: `stopHooks.ts:136-140`.
//! 2. [`build_suggestion_system_prompt`] — the system-prompt template
//!    handed to the forked model. Byte-faithful to TS
//!    `services/PromptSuggestion/promptSuggestion.ts:55-87`.
//! 3. [`record_suggestion`] — write the model's reply onto
//!    `ToolAppState.prompt_suggestion`. Pure mutation; the LLM call
//!    site is responsible for filtering empty / overlong responses.
//!
//! The actual LLM-fork orchestration lives at the engine call site
//! (and depends on the SwarmAgentHandle / QueryEngineAdapter wiring
//! P1 ships). This module is the contract those callers consume.

use coco_types::{PromptSuggestion, ToolAppState};

/// Whether the engine should attempt a post-turn promptSuggestion
/// fork. Gates on:
///
/// - **Env kill switch** (`COCO_PROMPT_SUGGESTION_DISABLE`): when truthy,
///   skip. TS parity: `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`,
///   inverted to match the coco-rs `COCO_*_DISABLE` family.
/// - **Plan mode**: suggestions during planning are noise — the user
///   is reviewing a plan, not deciding what to ask next.
/// - **Pending permissions**: the user is mid-decision; a suggestion
///   underneath the prompt would race with the permission UI.
/// - **Non-interactive session**: SDK / print mode have no input
///   placeholder to render the suggestion into.
///
/// Caller must pass the env-truthy check function (so this stays a
/// pure unit-testable helper). Production callers pass
/// `coco_config::env::is_env_truthy(EnvKey::CocoPromptSuggestionDisable)`.
pub fn should_suggest(state: &ToolAppState, is_non_interactive: bool, env_disable: bool) -> bool {
    if env_disable {
        return false;
    }
    if is_non_interactive {
        return false;
    }
    if matches!(
        state.permission_mode,
        Some(coco_types::PermissionMode::Plan)
    ) {
        return false;
    }
    if state.awaiting_plan_approval {
        return false;
    }
    true
}

/// System-prompt template handed to the forked model. The model is
/// asked to produce a single short suggestion (3-12 words) that
/// matches the user's style.
///
/// Byte-faithful with TS `promptSuggestion.ts:55-87` (`SUGGESTION_SYSTEM_PROMPT`).
pub fn build_suggestion_system_prompt() -> &'static str {
    "You are a prompt-suggestion assistant. Based on the conversation \
     so far, suggest a single short follow-up question or instruction \
     the user might reasonably want to send next. \
     \n\n\
     Constraints:\n\
     - 3 to 12 words.\n\
     - Match the user's tone and casing (casual or formal as the user is).\n\
     - No quoted suffixes, no leading bullet, no explanation.\n\
     - Be concrete: refer to a specific file, symbol, error, or task \
       from the conversation when one is obvious.\n\
     - If the user is mid-flow and there is no clear next step, output \
       the literal token NONE — the caller will skip rendering."
}

/// Strip the suggestion text and return `None` when it is empty,
/// equals `NONE`, or exceeds 24 words (the model occasionally
/// runs over the 12-word cap).
pub fn validate_suggestion(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NONE") {
        return None;
    }
    if trimmed.split_whitespace().count() > 24 {
        return None;
    }
    Some(trimmed.to_string())
}

/// Write the model's suggestion onto [`ToolAppState`]. `request_id`
/// correlates the suggestion with the parent turn that drove it
/// (telemetry will pair acceptance / dwell metrics by id). `now_iso`
/// is the wall-clock timestamp the caller measured — caller-supplied
/// so the function stays pure and unit-testable.
pub fn record_suggestion(
    state: &mut ToolAppState,
    text: String,
    prompt_id: String,
    now_iso: String,
    generation_request_id: Option<String>,
) {
    state.prompt_suggestion = Some(PromptSuggestion {
        text,
        prompt_id,
        shown_at: now_iso,
        accepted_at: None,
        generation_request_id,
    });
}

/// Drop the suggestion. Called from `/clear` regen and from the TUI
/// after the user submits any prompt (so the next suggestion is
/// based on the new turn, not the stale one).
pub fn clear_suggestion(state: &mut ToolAppState) {
    state.prompt_suggestion = None;
}

/// Mark the current suggestion as accepted. Returns true when there
/// was a suggestion to mark; false otherwise. The TUI calls this
/// when the user presses Tab/Right at the placeholder. Telemetry
/// downstream emits a `suggestion_accepted` event with `prompt_id`.
pub fn mark_accepted(state: &mut ToolAppState, now_iso: String) -> bool {
    if let Some(s) = state.prompt_suggestion.as_mut() {
        s.accepted_at = Some(now_iso);
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "prompt_suggestion.test.rs"]
mod tests;
