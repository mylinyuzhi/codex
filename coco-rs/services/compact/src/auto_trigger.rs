//! Auto-trigger logic for compaction.
//!
//! TS: autoCompact.ts — triggers compaction when context usage exceeds threshold.
//!
//! Threshold formula (must match TS exactly):
//!   effectiveWindow = contextWindow - min(maxOutputTokens, 20K)
//!   autoCompactThreshold = effectiveWindow - 13K

use crate::types::AUTOCOMPACT_BUFFER_TOKENS;
use crate::types::ERROR_THRESHOLD_BUFFER_TOKENS;
use crate::types::MANUAL_COMPACT_BUFFER_TOKENS;
use crate::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY;
use crate::types::TokenWarningState;
use crate::types::WARNING_THRESHOLD_BUFFER_TOKENS;

/// Compaction recursion guard tag identifying the caller's query source.
///
/// TS uses a `QuerySource` string ("session_memory", "compact",
/// "marble_origami"). We use a typed enum so callers cannot misspell —
/// `Other` is the catch-all for any source not requiring guarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactQuerySource {
    /// Forked agent extracting session memory; must not auto-compact
    /// (would deadlock the parent).
    SessionMemory,
    /// The compact LLM call itself; must not nest.
    Compact,
    /// Any other source (main thread, subagents, SDK).
    Other,
}

/// Read TS-equivalent env vars to short-circuit auto-compact.
///
/// Honors `DISABLE_COMPACT` (kills both manual and auto) and
/// `DISABLE_AUTO_COMPACT` (auto only — manual `/compact` keeps working).
/// The user-supplied `enabled` flag corresponds to TS
/// `globalConfig.autoCompactEnabled`.
#[must_use]
pub fn is_auto_compact_enabled(enabled: bool) -> bool {
    if env_truthy("DISABLE_COMPACT") {
        return false;
    }
    if env_truthy("DISABLE_AUTO_COMPACT") {
        return false;
    }
    enabled
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let lower = v.to_ascii_lowercase();
            matches!(lower.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Apply the optional `CLAUDE_CODE_AUTO_COMPACT_WINDOW` cap.
///
/// TS reads this env var to override the model's reported context window
/// (for tests / debugging). Pure function; caller threads it through.
#[must_use]
pub fn apply_context_window_override(context_window: i64) -> i64 {
    match std::env::var("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|v| *v > 0)
    {
        Some(override_val) => context_window.min(override_val),
        None => context_window,
    }
}

/// Compute the effective context window size after reserving space for summary output.
///
/// TS: `getEffectiveContextWindowSize(model)` in autoCompact.ts.
#[must_use]
pub fn effective_context_window(context_window: i64, max_output_tokens: i64) -> i64 {
    let context_window = apply_context_window_override(context_window);
    let reserved = max_output_tokens.min(MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    (context_window - reserved).max(0)
}

/// Compute the auto-compact trigger threshold.
///
/// TS: `getAutoCompactThreshold(model)` in autoCompact.ts.
/// Honors `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (1-100) for testing.
#[must_use]
pub fn auto_compact_threshold(context_window: i64, max_output_tokens: i64) -> i64 {
    let effective = effective_context_window(context_window, max_output_tokens);
    let default_threshold = (effective - AUTOCOMPACT_BUFFER_TOKENS).max(0);

    if let Some(pct) = std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|p| *p > 0.0 && *p <= 100.0)
    {
        let percentage_threshold = ((effective as f64) * (pct / 100.0)).floor() as i64;
        return percentage_threshold.min(default_threshold);
    }

    default_threshold
}

/// Check if auto-compaction should be triggered.
///
/// Uses the TS formula: `tokens >= effectiveWindow - 13K`.
/// Falls back to the default threshold if the env override is unset.
#[must_use]
pub fn should_auto_compact(
    current_tokens: i64,
    context_window: i64,
    max_output_tokens: i64,
) -> bool {
    if context_window <= 0 {
        return false;
    }
    current_tokens >= auto_compact_threshold(context_window, max_output_tokens)
}

/// Recursion-guarded variant of [`should_auto_compact`].
///
/// TS guards `session_memory` and `compact` query sources to prevent forked
/// agents from re-entering the compaction loop. Pass [`CompactQuerySource`]
/// to opt out of those sources. Also returns `false` when auto-compact is
/// disabled (env vars or user setting).
#[must_use]
pub fn should_auto_compact_guarded(
    current_tokens: i64,
    context_window: i64,
    max_output_tokens: i64,
    auto_compact_enabled: bool,
    source: CompactQuerySource,
) -> bool {
    if matches!(
        source,
        CompactQuerySource::SessionMemory | CompactQuerySource::Compact
    ) {
        return false;
    }
    if !is_auto_compact_enabled(auto_compact_enabled) {
        return false;
    }
    should_auto_compact(current_tokens, context_window, max_output_tokens)
}

/// Calculate full token warning state (matches TS `calculateTokenWarningState`).
///
/// `auto_compact_enabled`: whether the user has auto-compact turned on.
/// Honors `CLAUDE_CODE_BLOCKING_LIMIT_OVERRIDE` for testing.
#[must_use]
pub fn calculate_token_warning_state(
    current_tokens: i64,
    context_window: i64,
    max_output_tokens: i64,
    auto_compact_enabled: bool,
) -> TokenWarningState {
    let effective = effective_context_window(context_window, max_output_tokens);
    let threshold = auto_compact_threshold(context_window, max_output_tokens);

    let blocking_default = (effective - MANUAL_COMPACT_BUFFER_TOKENS).max(0);
    let blocking_limit = std::env::var("CLAUDE_CODE_BLOCKING_LIMIT_OVERRIDE")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(blocking_default);

    // TS uses isAutoCompactEnabled() to pick the warning denominator: when
    // auto-compact is OFF, the user-visible "context left" is until the
    // effective window, not the autocompact threshold.
    let warning_denominator = if auto_compact_enabled {
        threshold
    } else {
        effective
    };

    let percent_left = if warning_denominator > 0 {
        (((warning_denominator - current_tokens).max(0) as f64 / warning_denominator as f64)
            * 100.0)
            .round() as i32
    } else {
        0
    };

    TokenWarningState {
        percent_left,
        is_above_warning_threshold: current_tokens
            >= warning_denominator - WARNING_THRESHOLD_BUFFER_TOKENS,
        is_above_error_threshold: current_tokens
            >= warning_denominator - ERROR_THRESHOLD_BUFFER_TOKENS,
        is_above_auto_compact_threshold: auto_compact_enabled && current_tokens >= threshold,
        is_at_blocking_limit: current_tokens >= blocking_limit,
    }
}

/// Time-based micro-compact configuration.
///
/// TS: GrowthBook-driven (`tengu_slate_heron`). coco-rs takes a struct from
/// the caller — settings can be wired through the config layer if/when
/// remote config arrives.
#[derive(Debug, Clone)]
pub struct TimeBasedMcConfig {
    pub enabled: bool,
    /// Minutes of inactivity before triggering (TS default: 60, matches cache TTL).
    pub gap_threshold_minutes: i32,
    /// Number of recent compactable tool_use_ids to keep (TS default: 5).
    pub keep_recent: i32,
}

impl Default for TimeBasedMcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gap_threshold_minutes: 60,
            keep_recent: 5,
        }
    }
}

/// Decision returned by [`evaluate_time_based_trigger`].
#[derive(Debug, Clone)]
pub struct TimeBasedTrigger {
    pub gap_minutes: f64,
    pub config: TimeBasedMcConfig,
}

/// Whether the time-based trigger should fire.
///
/// TS: `evaluateTimeBasedTrigger` in microCompact.ts. Returns the measured
/// gap (minutes since last assistant) if the trigger fires, otherwise
/// `None`. Caller threads `now_ms` and `last_assistant_ms` to keep the
/// function pure.
///
/// `is_main_thread` mirrors TS's `isMainThreadSource` predicate — TS
/// requires an explicit main-thread query source so analysis-only paths
/// (`/context`, `/compact`, `analyzeContext`) don't trigger.
#[must_use]
pub fn evaluate_time_based_trigger(
    config: &TimeBasedMcConfig,
    now_ms: i64,
    last_assistant_ms: Option<i64>,
    is_main_thread: bool,
) -> Option<TimeBasedTrigger> {
    if !config.enabled || !is_main_thread {
        return None;
    }
    let last_ms = last_assistant_ms?;
    let gap_minutes = (now_ms - last_ms) as f64 / 60_000.0;
    if !gap_minutes.is_finite() || gap_minutes < config.gap_threshold_minutes as f64 {
        return None;
    }
    Some(TimeBasedTrigger {
        gap_minutes,
        config: config.clone(),
    })
}

#[cfg(test)]
#[path = "auto_trigger.test.rs"]
mod tests;
