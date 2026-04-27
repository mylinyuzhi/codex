//! API-native context management config builder.
//!
//! TS: services/compact/apiMicrocompact.ts. **This module produces the
//! `context_management` payload sent to the Anthropic API**, which then
//! applies the edits server-side without invalidating the prompt cache.
//! Client-side message mutation (the inverse approach that *does* break
//! cache) lives in [`crate::micro_advanced`] under `clear_tool_uses_inplace`
//! and `clear_thinking_inplace`.
//!
//! The two strategies are:
//!
//! - `clear_tool_uses_20250919` — drops tool result content / tool inputs
//!   for older turns when input tokens exceed `trigger`, retaining the
//!   most-recent N tool uses.
//! - `clear_thinking_20251015` — preserves thinking blocks in past turns
//!   without re-sending their full content. Useful when models with native
//!   reasoning support emit chain-of-thought between turns.

use coco_types::ToolName;

use crate::types::ClearToolInputs;
use crate::types::ContextEditStrategy;
use crate::types::ThinkingKeep;

/// Default trigger threshold for `clear_tool_uses` (input tokens).
///
/// Matches TS `DEFAULT_MAX_INPUT_TOKENS` — typical warning threshold.
pub const DEFAULT_API_MAX_INPUT_TOKENS: i64 = 180_000;

/// Default keep-target for `clear_tool_uses` (input tokens after clearing).
///
/// Matches TS `DEFAULT_TARGET_INPUT_TOKENS` — keeps roughly the last 40K of
/// context like the client-side microcompact heuristic.
pub const DEFAULT_API_TARGET_INPUT_TOKENS: i64 = 40_000;

/// Tool names whose results are eligible for `clear_tool_inputs`.
///
/// Matches TS `TOOLS_CLEARABLE_RESULTS` — read/search/web tools that may
/// have produced large but no-longer-essential output.
pub const TOOLS_CLEARABLE_RESULTS: &[ToolName] = &[
    ToolName::Bash,
    ToolName::PowerShell,
    ToolName::Glob,
    ToolName::Grep,
    ToolName::Read,
    ToolName::WebFetch,
    ToolName::WebSearch,
];

/// Tool names that should be **excluded** from `clear_tool_uses`.
///
/// Matches TS `TOOLS_CLEARABLE_USES` — file-mutating tools whose tool_use
/// inputs (the actual edit specifications) carry semantic value beyond the
/// resulting tool_result, so we keep their inputs intact.
pub const TOOLS_EXCLUDE_FROM_CLEAR_USES: &[ToolName] =
    &[ToolName::Edit, ToolName::Write, ToolName::NotebookEdit];

/// Options driving [`get_api_context_management`].
///
/// Mirrors TS `getAPIContextManagement` parameter object.
#[derive(Debug, Clone, Default)]
pub struct ApiContextOptions {
    /// Whether the model has thinking enabled (skip `clear_thinking` when off).
    pub has_thinking: bool,
    /// Whether redact-thinking is active (skip `clear_thinking` — redacted
    /// blocks have no model-visible content).
    pub is_redact_thinking_active: bool,
    /// Force `clear_thinking { keep: 1 }` (TS: long-idle / >1h gap).
    pub clear_all_thinking: bool,
    /// Whether to enable `clear_tool_uses_20250919` clearing tool result
    /// content. TS gates this behind `USE_API_CLEAR_TOOL_RESULTS` env var.
    pub clear_tool_results: bool,
    /// Whether to enable `clear_tool_uses_20250919` clearing entire tool use
    /// blocks (excluding write/edit tools). TS gates this behind
    /// `USE_API_CLEAR_TOOL_USES` env var.
    pub clear_tool_uses: bool,
    /// Override for the trigger threshold; falls back to
    /// `API_MAX_INPUT_TOKENS` env / [`DEFAULT_API_MAX_INPUT_TOKENS`].
    pub trigger_threshold: Option<i64>,
    /// Override for the keep target; falls back to `API_TARGET_INPUT_TOKENS`
    /// env / [`DEFAULT_API_TARGET_INPUT_TOKENS`].
    pub keep_target: Option<i64>,
}

/// Read [`ApiContextOptions`] from process env vars.
///
/// TS `getAPIContextManagement` reads `USE_API_CLEAR_TOOL_RESULTS`,
/// `USE_API_CLEAR_TOOL_USES`, `API_MAX_INPUT_TOKENS`,
/// `API_TARGET_INPUT_TOKENS`. These are ant-only in TS; coco-rs keeps the
/// same names for parity. `has_thinking` etc. are model-state-driven and
/// must be filled by the caller.
#[must_use]
pub fn options_from_env() -> ApiContextOptions {
    ApiContextOptions {
        clear_tool_results: env_truthy("USE_API_CLEAR_TOOL_RESULTS"),
        clear_tool_uses: env_truthy("USE_API_CLEAR_TOOL_USES"),
        trigger_threshold: std::env::var("API_MAX_INPUT_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok()),
        keep_target: std::env::var("API_TARGET_INPUT_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok()),
        ..Default::default()
    }
}

/// Build the API-native context management strategy list.
///
/// Returns an empty `Vec` when no strategies are applicable — callers
/// should treat that as "omit `context_management` from the request" so
/// the API falls back to defaults.
///
/// TS: `getAPIContextManagement(options)`. Output ordering matches TS
/// (thinking first, tool_results second, tool_uses third) so server-side
/// edit application has a stable shape.
#[must_use]
pub fn get_api_context_management(opts: &ApiContextOptions) -> Vec<ContextEditStrategy> {
    let mut strategies = Vec::new();

    // 1. Clear thinking — skip when redact-thinking is on (no visible content).
    if opts.has_thinking && !opts.is_redact_thinking_active {
        strategies.push(ContextEditStrategy::ClearThinking {
            keep: if opts.clear_all_thinking {
                ThinkingKeep::Recent { turns: 1 }
            } else {
                ThinkingKeep::All
            },
        });
    }

    // Tool clearing strategies require explicit opt-in (TS: ant-only).
    if !opts.clear_tool_results && !opts.clear_tool_uses {
        return strategies;
    }

    let trigger = opts
        .trigger_threshold
        .unwrap_or(DEFAULT_API_MAX_INPUT_TOKENS);

    // 2. Clear tool result content — clear_tool_inputs is the per-tool list.
    if opts.clear_tool_results {
        strategies.push(ContextEditStrategy::ClearToolUses {
            trigger: Some(trigger),
            keep_recent: None,
            clear_inputs: ClearToolInputs::SpecificTools(TOOLS_CLEARABLE_RESULTS.to_vec()),
            exclude_tools: Vec::new(),
            exclude_tool_strs: Vec::new(),
        });
    }

    // 3. Clear entire tool_use blocks (excluding write/edit tools).
    if opts.clear_tool_uses {
        strategies.push(ContextEditStrategy::ClearToolUses {
            trigger: Some(trigger),
            keep_recent: None,
            clear_inputs: ClearToolInputs::None,
            exclude_tools: TOOLS_EXCLUDE_FROM_CLEAR_USES.to_vec(),
            exclude_tool_strs: Vec::new(),
        });
    }

    strategies
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let lower = v.to_ascii_lowercase();
            matches!(lower.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "api_compact.test.rs"]
mod tests;
