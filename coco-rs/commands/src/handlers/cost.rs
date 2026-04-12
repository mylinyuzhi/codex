//! `/cost` — show per-model token usage and USD cost for the session.
//!
//! Reads the session file, aggregates token usage from each API
//! request, and calculates cost using per-model pricing.

use std::pin::Pin;

/// Per-model pricing (USD per million tokens).
struct ModelPricing {
    name: &'static str,
    input_per_m: f64,
    output_per_m: f64,
    cache_read_per_m: f64,
    cache_write_per_m: f64,
}

const MODEL_PRICING: &[ModelPricing] = &[
    ModelPricing {
        name: "claude-sonnet-4",
        input_per_m: 3.0,
        output_per_m: 15.0,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelPricing {
        name: "claude-opus-4",
        input_per_m: 15.0,
        output_per_m: 75.0,
        cache_read_per_m: 1.50,
        cache_write_per_m: 18.75,
    },
    ModelPricing {
        name: "claude-haiku-3",
        input_per_m: 0.25,
        output_per_m: 1.25,
        cache_read_per_m: 0.03,
        cache_write_per_m: 0.30,
    },
];

/// Token usage accumulator for one model.
#[derive(Default)]
struct UsageBucket {
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    api_requests: i64,
}

/// Async handler for `/cost`.
///
/// Reads the session file, extracts usage records from each turn,
/// and formats a cost breakdown by model.
pub fn handler(
    _args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let sessions_dir = dirs::home_dir()
            .map(|h| h.join(".cocode").join("sessions"))
            .unwrap_or_default();

        let buckets = collect_usage(&sessions_dir).await;

        let mut out = String::from("## Session Cost\n\n");

        if buckets.is_empty() {
            out.push_str("No API usage recorded yet.\n\n");
            out.push_str("Cost tracking begins when the first API request is made.");
            return Ok(out);
        }

        // Per-model breakdown
        let mut total_cost = 0.0_f64;
        let mut total_input = 0_i64;
        let mut total_output = 0_i64;
        let mut total_requests = 0_i64;

        for bucket in &buckets {
            let pricing = find_pricing(&bucket.model);
            let input_cost = bucket.input_tokens as f64 * pricing.input_per_m / 1_000_000.0;
            let output_cost = bucket.output_tokens as f64 * pricing.output_per_m / 1_000_000.0;
            let cache_read_cost =
                bucket.cache_read_tokens as f64 * pricing.cache_read_per_m / 1_000_000.0;
            let cache_write_cost =
                bucket.cache_write_tokens as f64 * pricing.cache_write_per_m / 1_000_000.0;
            let model_cost = input_cost + output_cost + cache_read_cost + cache_write_cost;

            out.push_str(&format!("### {}\n\n", bucket.model));
            out.push_str(&format!(
                "  Input tokens:       {:>10}\n",
                format_num(bucket.input_tokens)
            ));
            out.push_str(&format!(
                "  Output tokens:      {:>10}\n",
                format_num(bucket.output_tokens)
            ));
            out.push_str(&format!(
                "  Cache read tokens:  {:>10}\n",
                format_num(bucket.cache_read_tokens)
            ));
            out.push_str(&format!(
                "  Cache write tokens: {:>10}\n",
                format_num(bucket.cache_write_tokens)
            ));
            out.push_str(&format!(
                "  API requests:       {:>10}\n",
                bucket.api_requests
            ));
            out.push_str(&format!("  Cost:               ${model_cost:.4}\n\n"));

            total_cost += model_cost;
            total_input += bucket.input_tokens;
            total_output += bucket.output_tokens;
            total_requests += bucket.api_requests;
        }

        // Totals
        out.push_str("### Total\n\n");
        out.push_str(&format!("  Input tokens:  {}\n", format_num(total_input)));
        out.push_str(&format!("  Output tokens: {}\n", format_num(total_output)));
        out.push_str(&format!("  API requests:  {total_requests}\n"));
        out.push_str(&format!("  **Total cost:  ${total_cost:.4}**"));

        Ok(out)
    })
}

/// Collect usage buckets from the most recent session file.
async fn collect_usage(sessions_dir: &std::path::Path) -> Vec<UsageBucket> {
    let content = match read_newest_session(sessions_dir).await {
        Some(c) => c,
        None => return Vec::new(),
    };

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };

    let mut buckets: std::collections::HashMap<String, UsageBucket> =
        std::collections::HashMap::new();

    // Look for usage records in turns or messages
    let turns = parsed.get("turns").and_then(|v| v.as_array());
    if let Some(turns) = turns {
        for turn in turns {
            let model = turn
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let usage = turn.get("usage");
            if let Some(u) = usage {
                let bucket = buckets.entry(model.clone()).or_insert_with(|| UsageBucket {
                    model,
                    ..Default::default()
                });
                bucket.input_tokens += u
                    .get("input_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                bucket.output_tokens += u
                    .get("output_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                bucket.cache_read_tokens += u
                    .get("cache_read_input_tokens")
                    .or_else(|| u.get("cache_read_tokens"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                bucket.cache_write_tokens += u
                    .get("cache_creation_input_tokens")
                    .or_else(|| u.get("cache_write_tokens"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                bucket.api_requests += 1;
            }
        }
    }

    // Also check top-level usage object
    if buckets.is_empty()
        && let Some(u) = parsed.get("usage")
    {
        let model = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mut bucket = UsageBucket {
            model,
            ..Default::default()
        };
        bucket.input_tokens = u
            .get("input_tokens")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        bucket.output_tokens = u
            .get("output_tokens")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        bucket.api_requests = 1;
        buckets.insert(bucket.model.clone(), bucket);
    }

    buckets.into_values().collect()
}

/// Find the pricing for a model name (prefix match).
fn find_pricing(model: &str) -> &'static ModelPricing {
    MODEL_PRICING
        .iter()
        .find(|p| model.starts_with(p.name))
        .unwrap_or(&MODEL_PRICING[0]) // default to sonnet pricing
}

/// Read the newest session JSON from the sessions directory.
async fn read_newest_session(sessions_dir: &std::path::Path) -> Option<String> {
    if !sessions_dir.exists() {
        return None;
    }

    let mut entries = tokio::fs::read_dir(sessions_dir).await.ok()?;
    let mut newest: Option<(std::path::PathBuf, u64)> = None;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(meta) = entry.metadata().await {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            if newest.as_ref().is_none_or(|n| modified > n.1) {
                newest = Some((path, modified));
            }
        }
    }

    let (path, _) = newest?;
    tokio::fs::read_to_string(&path).await.ok()
}

/// Format an integer with thousands separators.
fn format_num(n: i64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
#[path = "cost.test.rs"]
mod tests;
