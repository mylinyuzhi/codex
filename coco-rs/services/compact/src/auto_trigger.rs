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

/// Compute the effective context window size after reserving space for summary output.
///
/// TS: `getEffectiveContextWindowSize(model)` in autoCompact.ts.
#[must_use]
pub fn effective_context_window(context_window: i64, max_output_tokens: i64) -> i64 {
    let reserved = max_output_tokens.min(MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    (context_window - reserved).max(0)
}

/// Compute the auto-compact trigger threshold.
///
/// TS: `getAutoCompactThreshold(model)` in autoCompact.ts.
/// Returns the token count at which auto-compaction should trigger.
#[must_use]
pub fn auto_compact_threshold(context_window: i64, max_output_tokens: i64) -> i64 {
    let effective = effective_context_window(context_window, max_output_tokens);
    (effective - AUTOCOMPACT_BUFFER_TOKENS).max(0)
}

/// Check if auto-compaction should be triggered.
///
/// Uses the TS formula: `tokens >= effectiveWindow - 13K`.
/// `max_output_tokens` is the model's max output (e.g., 8192 for Haiku, 16384 for Sonnet).
/// Falls back to MAX_OUTPUT_TOKENS_FOR_SUMMARY if not known.
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

/// Calculate full token warning state (matches TS `calculateTokenWarningState`).
///
/// `auto_compact_enabled`: whether the user has auto-compact turned on.
#[must_use]
pub fn calculate_token_warning_state(
    current_tokens: i64,
    context_window: i64,
    max_output_tokens: i64,
    auto_compact_enabled: bool,
) -> TokenWarningState {
    let effective = effective_context_window(context_window, max_output_tokens);
    let threshold = auto_compact_threshold(context_window, max_output_tokens);
    let blocking_limit = (effective - MANUAL_COMPACT_BUFFER_TOKENS).max(0);

    let percent_left = if effective > 0 {
        (((effective - current_tokens).max(0) as f64 / effective as f64) * 100.0).round() as i32
    } else {
        0
    };

    TokenWarningState {
        percent_left,
        is_above_warning_threshold: current_tokens >= effective - WARNING_THRESHOLD_BUFFER_TOKENS,
        is_above_error_threshold: current_tokens >= effective - ERROR_THRESHOLD_BUFFER_TOKENS,
        is_above_auto_compact_threshold: auto_compact_enabled && current_tokens >= threshold,
        is_at_blocking_limit: current_tokens >= blocking_limit,
    }
}

/// Time-based micro-compact configuration.
/// TS: GrowthBook-driven, with gap threshold and keep-recent settings.
#[derive(Debug, Clone)]
pub struct TimeBasedMcConfig {
    pub enabled: bool,
    /// Minutes of inactivity before triggering (TS default: 60, matches cache TTL).
    pub gap_threshold_minutes: i32,
    /// Number of recent API rounds to keep (TS default: 5).
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

#[cfg(test)]
#[path = "auto_trigger.test.rs"]
mod tests;
