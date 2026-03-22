use serde_json::Value;
use vercel_ai_provider::ProviderMetadata;

/// Build provider metadata for the Responses API response.
/// Nests all fields under the `"openai"` key to match TS SDK behavior.
pub fn build_responses_provider_metadata(
    response_id: Option<&str>,
    service_tier: Option<&str>,
) -> Option<ProviderMetadata> {
    let mut openai_obj = serde_json::Map::new();

    if let Some(id) = response_id {
        openai_obj.insert("responseId".into(), Value::String(id.into()));
    }
    if let Some(tier) = service_tier {
        openai_obj.insert("serviceTier".into(), Value::String(tier.into()));
    }

    if openai_obj.is_empty() {
        None
    } else {
        let mut meta = ProviderMetadata::default();
        meta.0.insert("openai".into(), Value::Object(openai_obj));
        Some(meta)
    }
}

#[cfg(test)]
#[path = "provider_metadata.test.rs"]
mod tests;
