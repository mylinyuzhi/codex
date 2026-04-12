//! Provider-specific option merging for inference requests.
//!
//! Handles base options (e.g., Anthropic beta headers), thinking config,
//! and user overrides with deep-merge semantics.

/// Provider-specific base options keyed by provider name.
pub fn provider_base_options(provider: &str) -> serde_json::Value {
    match provider {
        "anthropic" => serde_json::json!({
            "anthropic-beta": ["prompt-caching-2024-07-31"]
        }),
        "openai" => serde_json::json!({}),
        "google" => serde_json::json!({}),
        _ => serde_json::json!({}),
    }
}

/// Merge provider options: base + thinking + user overrides.
///
/// Each layer deep-merges on top of the previous one. Objects are merged
/// recursively; scalars and arrays are replaced by the overlay value.
pub fn merge_provider_options(
    base: &serde_json::Value,
    thinking: Option<&serde_json::Value>,
    user_overrides: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut merged = base.clone();
    if let Some(thinking_opts) = thinking {
        deep_merge(&mut merged, thinking_opts);
    }
    if let Some(overrides) = user_overrides {
        deep_merge(&mut merged, overrides);
    }
    merged
}

/// Recursively merge `overlay` into `base`.
///
/// Object keys are merged recursively. All other types (arrays, scalars)
/// in `overlay` replace the corresponding value in `base`.
fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                deep_merge(
                    base_map.entry(k.clone()).or_insert(serde_json::Value::Null),
                    v,
                );
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

#[cfg(test)]
#[path = "options_merge.test.rs"]
mod tests;
