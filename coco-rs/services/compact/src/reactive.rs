//! Reactive compaction — mid-turn compaction when prompt is too long.
//!
//! TS: services/compact/autoCompact.ts + reactiveCompact.ts
//! Triggers compaction during API calls when the prompt exceeds context window.
//!
//! Includes a circuit breaker (`ReactiveCompactState`) that tracks consecutive
//! failures and disables reactive compaction after repeated failures to avoid
//! wasting API calls.

use coco_types::Message;

use crate::tokens;
use crate::types::CLEARED_TOOL_RESULT_MESSAGE;
use crate::types::MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES;

/// Reactive compact configuration.
#[derive(Debug, Clone)]
pub struct ReactiveCompactConfig {
    /// Context window size in tokens.
    pub context_window: i64,
    /// Max output tokens for the model (used in effective window calculation).
    pub max_output_tokens: i64,
    /// Trigger threshold as percentage of effective window (higher than auto-compact).
    /// Default 95% — reactive triggers closer to the limit than auto-compact (~87%).
    pub trigger_threshold_pct: i32,
    /// Number of recent rounds to preserve.
    pub keep_recent_rounds: usize,
}

impl Default for ReactiveCompactConfig {
    fn default() -> Self {
        Self {
            context_window: 200_000,
            max_output_tokens: 16_384,
            trigger_threshold_pct: 95,
            keep_recent_rounds: 2,
        }
    }
}

/// Mutable state tracking reactive compaction attempts and circuit breaker.
///
/// After `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES` (3) consecutive failures,
/// the circuit breaker trips and `should_attempt_reactive_compact` returns
/// `false` until `reset()` is called.
#[derive(Debug, Clone, Default)]
pub struct ReactiveCompactState {
    /// Consecutive failure count.
    failure_count: i32,
    /// Timestamp (ms) of the last compaction attempt.
    last_attempt_ms: i64,
}

impl ReactiveCompactState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether reactive compaction should be attempted.
    pub fn should_attempt_reactive_compact(&self) -> bool {
        self.failure_count < MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES
    }

    /// Record a compaction failure. Increments the consecutive failure count.
    pub fn record_failure(&mut self, timestamp_ms: i64) {
        self.failure_count += 1;
        self.last_attempt_ms = timestamp_ms;
    }

    /// Record a compaction success. Resets the failure count to zero.
    pub fn record_success(&mut self, timestamp_ms: i64) {
        self.failure_count = 0;
        self.last_attempt_ms = timestamp_ms;
    }

    /// Reset the circuit breaker, re-enabling reactive compaction.
    pub fn reset(&mut self) {
        self.failure_count = 0;
        self.last_attempt_ms = 0;
    }

    /// Current consecutive failure count.
    pub fn failure_count(&self) -> i32 {
        self.failure_count
    }

    /// Timestamp of the last attempt (0 if none).
    pub fn last_attempt_ms(&self) -> i64 {
        self.last_attempt_ms
    }
}

/// Check if reactive compaction should trigger.
///
/// Uses a HIGHER threshold than auto-compact (95% vs ~87% of effective window).
/// Reactive compact is a fallback when auto-compact missed or wasn't enough.
#[must_use]
pub fn should_reactive_compact(estimated_tokens: i64, config: &ReactiveCompactConfig) -> bool {
    if config.context_window <= 0 {
        return false;
    }
    let effective = crate::auto_trigger::effective_context_window(
        config.context_window,
        config.max_output_tokens,
    );
    let threshold = effective * config.trigger_threshold_pct as i64 / 100;
    estimated_tokens >= threshold
}

/// Determine how many tokens to drop to get below the threshold.
/// Target 70% of effective context window after compaction.
#[must_use]
pub fn calculate_drop_target(current_tokens: i64, config: &ReactiveCompactConfig) -> i64 {
    let effective = crate::auto_trigger::effective_context_window(
        config.context_window,
        config.max_output_tokens,
    );
    let target = effective * 70 / 100;
    (current_tokens - target).max(0)
}

/// API-level micro-compaction — trim tool results from oldest messages.
///
/// TS: apiMicrocompact.ts — lightweight compaction that clears old tool
/// result content while keeping the structure.
pub fn api_microcompact(messages: &mut [Message], tokens_to_free: i64) {
    let mut freed = 0i64;
    for msg in messages.iter_mut() {
        if freed >= tokens_to_free {
            break;
        }
        if let Message::ToolResult(tr) = msg {
            let est = tokens::estimate_tool_result_tokens(tr);
            if est > 50 {
                tr.message = coco_types::LlmMessage::Tool {
                    content: vec![coco_types::ToolContent::ToolResult(
                        coco_types::ToolResultContent {
                            tool_call_id: tr.tool_use_id.clone(),
                            tool_name: String::new(),
                            output: vercel_ai_provider::ToolResultContent::text(
                                CLEARED_TOOL_RESULT_MESSAGE,
                            ),
                            is_error: false,
                            provider_metadata: None,
                        },
                    )],
                    provider_options: None,
                };
                freed += est;
            }
        }
    }
}

#[cfg(test)]
#[path = "reactive.test.rs"]
mod tests;
