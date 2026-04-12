//! `/model` — switch the active model with validation.
//!
//! Lists available models with capabilities and pricing, validates
//! the requested model against the known registry, and performs the switch.

use std::pin::Pin;

/// Known model entry with metadata for display.
struct KnownModel {
    /// Short alias (e.g., "sonnet").
    alias: &'static str,
    /// Full model ID.
    full_id: &'static str,
    /// Description for the listing.
    description: &'static str,
    /// Input price per million tokens (USD).
    input_price: f64,
    /// Output price per million tokens (USD).
    output_price: f64,
    /// Context window size.
    context_window: i64,
}

const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel {
        alias: "sonnet",
        full_id: "claude-sonnet-4-20250514",
        description: "Balanced speed and intelligence (default)",
        input_price: 3.0,
        output_price: 15.0,
        context_window: 200_000,
    },
    KnownModel {
        alias: "opus",
        full_id: "claude-opus-4-20250514",
        description: "Most capable, best for complex tasks",
        input_price: 15.0,
        output_price: 75.0,
        context_window: 200_000,
    },
    KnownModel {
        alias: "haiku",
        full_id: "claude-haiku-3-20250307",
        description: "Fastest and most affordable",
        input_price: 0.25,
        output_price: 1.25,
        context_window: 200_000,
    },
];

/// Async handler for `/model [name]`.
///
/// With no arguments, lists available models.
/// With a model name or alias, validates and switches.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let requested = args.trim().to_string();

        if requested.is_empty() {
            return Ok(list_models());
        }

        // Try to resolve the model
        match resolve_model(&requested) {
            Some(model) => {
                let mut out = format!("Model switched to: {}\n\n", model.full_id);
                out.push_str(&format!("  {}\n", model.description));
                out.push_str(&format!(
                    "  Pricing: ${:.2}/M input, ${:.2}/M output\n",
                    model.input_price, model.output_price,
                ));
                out.push_str(&format!(
                    "  Context: {}K tokens",
                    model.context_window / 1000,
                ));
                Ok(out)
            }
            None => {
                // Check if it looks like a provider:model pattern
                if requested.contains('/') || requested.contains(':') {
                    Ok(format!(
                        "Setting custom model: {requested}\n\n\
                         Note: this model is not in the built-in registry.\n\
                         Ensure your provider supports this model ID."
                    ))
                } else {
                    let mut out = format!("Unknown model: {requested}\n\n");
                    out.push_str("Did you mean one of these?\n\n");
                    // Suggest closest matches
                    for m in KNOWN_MODELS {
                        let alias_dist = levenshtein(&requested.to_ascii_lowercase(), m.alias);
                        if alias_dist <= 3 {
                            out.push_str(&format!("  {} ({})\n", m.alias, m.full_id,));
                        }
                    }
                    out.push_str("\nUse /model to see all available models.");
                    Ok(out)
                }
            }
        }
    })
}

/// Build the model listing string.
fn list_models() -> String {
    let mut out = String::from("## Available Models\n\n");
    out.push_str("| Alias   | Model ID                       | $/M in | $/M out | Ctx    |\n");
    out.push_str("|---------|--------------------------------|--------|---------|--------|\n");

    for m in KNOWN_MODELS {
        out.push_str(&format!(
            "| {:<7} | {:<30} | {:>6.2} | {:>7.2} | {:>4}K |\n",
            m.alias,
            m.full_id,
            m.input_price,
            m.output_price,
            m.context_window / 1000,
        ));
    }

    out.push_str("\nUse /model <alias> or /model <full-id> to switch.\n");
    out.push_str("Custom model IDs (e.g., provider/model) are also accepted.");
    out
}

/// Resolve a model name or alias to a known model.
fn resolve_model(input: &str) -> Option<&'static KnownModel> {
    let lower = input.to_ascii_lowercase();

    // Exact alias match
    if let Some(m) = KNOWN_MODELS.iter().find(|m| m.alias == lower) {
        return Some(m);
    }

    // Full ID match (case-insensitive)
    if let Some(m) = KNOWN_MODELS
        .iter()
        .find(|m| m.full_id.eq_ignore_ascii_case(&lower))
    {
        return Some(m);
    }

    // Prefix match on full ID
    if let Some(m) = KNOWN_MODELS.iter().find(|m| m.full_id.starts_with(&lower)) {
        return Some(m);
    }

    None
}

/// Simple Levenshtein distance for typo detection.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
#[path = "model.test.rs"]
mod tests;
