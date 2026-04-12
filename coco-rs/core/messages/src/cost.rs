use coco_types::ModelUsage;
use coco_types::TokenUsage;
use std::collections::HashMap;

/// Tracks cost and token usage per model across a session.
#[derive(Debug, Clone, Default)]
pub struct CostTracker {
    pub per_model: HashMap<String, ModelUsage>,
    pub total_api_calls: i64,
    pub total_duration_ms: i64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record usage from a single API call.
    pub fn record(&mut self, model: &str, usage: TokenUsage, cost_usd: f64, duration_ms: i64) {
        let entry = self.per_model.entry(model.to_string()).or_default();
        entry.accumulate(usage, cost_usd);
        self.total_api_calls += 1;
        self.total_duration_ms += duration_ms;
    }

    /// Total cost across all models.
    pub fn total_cost_usd(&self) -> f64 {
        self.per_model.values().map(|u| u.cost_usd).sum()
    }

    /// Total input tokens across all models.
    pub fn total_input_tokens(&self) -> i64 {
        self.per_model.values().map(|u| u.input_tokens).sum()
    }

    /// Total output tokens across all models.
    pub fn total_output_tokens(&self) -> i64 {
        self.per_model.values().map(|u| u.output_tokens).sum()
    }
}

/// Per-model pricing data (USD per million tokens).
///
/// TS: utils/modelCost.ts — MODEL_COSTS record.
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_write_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

/// Get pricing for a model by name.
///
/// Returns None for unknown models (caller should use default).
pub fn get_model_pricing(model: &str) -> Option<ModelPricing> {
    let normalized = model.to_lowercase();

    // Match by model family patterns
    if normalized.contains("opus-4-6") || normalized.contains("opus-4-5") {
        Some(PRICING_TIER_5_25)
    } else if normalized.contains("opus-4-1")
        || normalized.contains("opus-4-0")
        || normalized.contains("opus-4")
    {
        Some(PRICING_TIER_15_75)
    } else if normalized.contains("sonnet") {
        Some(PRICING_TIER_3_15)
    } else if normalized.contains("haiku-4-5") || normalized.contains("haiku-4.5") {
        Some(PRICING_HAIKU_45)
    } else if normalized.contains("haiku") {
        Some(PRICING_HAIKU_35)
    } else {
        None
    }
}

/// Calculate USD cost from token counts and model.
pub fn calculate_cost_usd(model: &str, usage: &TokenUsage) -> f64 {
    let pricing = get_model_pricing(model).unwrap_or(PRICING_TIER_5_25);

    let input_cost = usage.input_tokens as f64 * pricing.input_per_mtok / 1_000_000.0;
    let output_cost = usage.output_tokens as f64 * pricing.output_per_mtok / 1_000_000.0;
    let cache_write_cost =
        usage.cache_creation_input_tokens as f64 * pricing.cache_write_per_mtok / 1_000_000.0;
    let cache_read_cost =
        usage.cache_read_input_tokens as f64 * pricing.cache_read_per_mtok / 1_000_000.0;

    input_cost + output_cost + cache_write_cost + cache_read_cost
}

/// Format cost as a human-readable string.
pub fn format_cost(cost_usd: f64) -> String {
    if cost_usd < 0.01 {
        format!("${cost_usd:.4}")
    } else {
        format!("${cost_usd:.2}")
    }
}

// Pricing tiers (USD per million tokens)
const PRICING_TIER_3_15: ModelPricing = ModelPricing {
    input_per_mtok: 3.0,
    output_per_mtok: 15.0,
    cache_write_per_mtok: 3.75,
    cache_read_per_mtok: 0.3,
};

const PRICING_TIER_15_75: ModelPricing = ModelPricing {
    input_per_mtok: 15.0,
    output_per_mtok: 75.0,
    cache_write_per_mtok: 18.75,
    cache_read_per_mtok: 1.5,
};

const PRICING_TIER_5_25: ModelPricing = ModelPricing {
    input_per_mtok: 5.0,
    output_per_mtok: 25.0,
    cache_write_per_mtok: 6.25,
    cache_read_per_mtok: 0.5,
};

const PRICING_HAIKU_35: ModelPricing = ModelPricing {
    input_per_mtok: 0.8,
    output_per_mtok: 4.0,
    cache_write_per_mtok: 1.0,
    cache_read_per_mtok: 0.08,
};

const PRICING_HAIKU_45: ModelPricing = ModelPricing {
    input_per_mtok: 1.0,
    output_per_mtok: 5.0,
    cache_write_per_mtok: 1.25,
    cache_read_per_mtok: 0.1,
};

#[cfg(test)]
mod cost_tests {
    use super::*;

    #[test]
    fn test_model_pricing_sonnet() {
        let pricing = get_model_pricing("claude-sonnet-4-6-20250514").unwrap();
        assert!((pricing.input_per_mtok - 3.0).abs() < 0.01);
        assert!((pricing.output_per_mtok - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_model_pricing_opus() {
        let pricing = get_model_pricing("claude-opus-4-6").unwrap();
        assert!((pricing.input_per_mtok - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_model_pricing_haiku() {
        let pricing = get_model_pricing("claude-haiku-4-5-20251001").unwrap();
        assert!((pricing.input_per_mtok - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_cost() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let cost = calculate_cost_usd("claude-sonnet-4-6", &usage);
        // 1M input at $3/Mtok = $3.00, 100K output at $15/Mtok = $1.50
        assert!((cost - 4.5).abs() < 0.01);
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(1.23), "$1.23");
        assert_eq!(format_cost(0.005), "$0.0050");
    }
}
