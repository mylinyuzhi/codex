/// Tracks permission denials to detect stuck loops and trigger circuit breaker.
///
/// TS: 3 consecutive denials → fallback to prompting (circuit breaker).
///     20 total denials → suggest /permissions command.
///
/// The circuit breaker prevents wasting API calls on irrecoverable permission
/// failures. When triggered, auto-mode falls back to interactive prompting.
#[derive(Debug, Default)]
pub struct DenialTracker {
    pub consecutive_denials: i32,
    pub total_denials: i32,
    /// Per-tool denial counts for targeted suggestions.
    per_tool_denials: std::collections::HashMap<String, i32>,
    /// Whether the circuit breaker has tripped (auto-mode → prompting fallback).
    circuit_breaker_tripped: bool,
}

/// Threshold for consecutive denials before circuit breaker trips.
const CONSECUTIVE_DENIAL_THRESHOLD: i32 = 3;

/// Threshold for total denials before suggesting /permissions.
const TOTAL_DENIAL_THRESHOLD: i32 = 20;

impl DenialTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a denial for a specific tool.
    pub fn record_denial(&mut self, tool_name: &str) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
        *self
            .per_tool_denials
            .entry(tool_name.to_string())
            .or_default() += 1;

        // Trip circuit breaker on consecutive threshold.
        if self.consecutive_denials >= CONSECUTIVE_DENIAL_THRESHOLD {
            self.circuit_breaker_tripped = true;
        }
    }

    /// Record a denial without a tool name (backward compat).
    pub fn record_denial_anonymous(&mut self) {
        self.record_denial("unknown");
    }

    /// Reset consecutive counter (on successful tool execution).
    pub fn reset_consecutive(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Reset the circuit breaker (e.g., after user adjusts permissions).
    pub fn reset_circuit_breaker(&mut self) {
        self.circuit_breaker_tripped = false;
        self.consecutive_denials = 0;
    }

    /// Whether the agent appears stuck in a denial loop.
    pub fn is_stuck(&self) -> bool {
        self.consecutive_denials >= CONSECUTIVE_DENIAL_THRESHOLD
    }

    /// Whether the circuit breaker has tripped.
    /// When true, auto-mode should fall back to interactive prompting.
    pub fn is_circuit_breaker_tripped(&self) -> bool {
        self.circuit_breaker_tripped
    }

    /// Whether total denials suggest the user should adjust permissions.
    pub fn should_suggest_permissions(&self) -> bool {
        self.total_denials >= TOTAL_DENIAL_THRESHOLD
    }

    /// Get the most frequently denied tool (for targeted suggestions).
    pub fn most_denied_tool(&self) -> Option<(&str, i32)> {
        self.per_tool_denials
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, count)| (name.as_str(), *count))
    }

    /// Get denial count for a specific tool.
    pub fn tool_denial_count(&self, tool_name: &str) -> i32 {
        self.per_tool_denials.get(tool_name).copied().unwrap_or(0)
    }

    /// Build a suggestion message based on denial state.
    pub fn suggestion_message(&self) -> Option<String> {
        if self.should_suggest_permissions() {
            Some(format!(
                "You've had {total} permission denials this session. \
                 Consider running /permissions to adjust your settings.",
                total = self.total_denials
            ))
        } else if self.is_stuck() {
            let tool_hint = self
                .most_denied_tool()
                .map(|(name, _)| format!(" (most denied: {name})"))
                .unwrap_or_default();
            Some(format!(
                "{consecutive} consecutive permission denials{tool_hint}. \
                 The agent may be stuck. Falling back to interactive prompting.",
                consecutive = self.consecutive_denials
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "denial_tracking.test.rs"]
mod tests;
