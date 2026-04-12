use coco_types::TokenUsage;
use std::collections::HashMap;

/// Accumulates token usage across multiple API calls.
#[derive(Debug, Clone, Default)]
pub struct UsageAccumulator {
    /// Total usage across all calls.
    pub total: TokenUsage,
    /// Per-model usage breakdown.
    pub per_model: HashMap<String, TokenUsage>,
    /// Number of API calls made.
    pub call_count: i64,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record usage from a single API call.
    pub fn record(&mut self, model: &str, usage: TokenUsage) {
        self.total += usage;
        self.per_model
            .entry(model.to_string())
            .and_modify(|u| *u += usage)
            .or_insert(usage);
        self.call_count += 1;
    }
}
