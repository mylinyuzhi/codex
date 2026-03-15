use std::collections::HashMap;

use serde_json::Value;
use vercel_ai_provider::ProviderMetadata;

/// Build provider metadata for the Responses API response.
pub fn build_responses_provider_metadata(
    response_id: Option<&str>,
    service_tier: Option<&str>,
) -> Option<ProviderMetadata> {
    let mut meta = HashMap::new();

    if let Some(id) = response_id {
        meta.insert("responseId".into(), Value::String(id.into()));
    }
    if let Some(tier) = service_tier {
        meta.insert("serviceTier".into(), Value::String(tier.into()));
    }

    if meta.is_empty() {
        None
    } else {
        Some(ProviderMetadata(meta))
    }
}

#[cfg(test)]
#[path = "provider_metadata.test.rs"]
mod tests;
