use serde::Serialize;
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

/// Compaction metadata nested under the `"openai"` key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesCompactionProviderMetadata {
    #[serde(rename = "type")]
    pub meta_type: String,
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<String>,
}

/// Build a `ProviderMetadata` for a compaction item.
pub fn build_compaction_provider_metadata(
    item_id: &str,
    encrypted_content: Option<&str>,
) -> ProviderMetadata {
    let meta = ResponsesCompactionProviderMetadata {
        meta_type: "compaction".into(),
        item_id: item_id.into(),
        encrypted_content: encrypted_content.map(String::from),
    };
    let value = serde_json::to_value(&meta).unwrap_or(Value::Null);
    let mut pm = ProviderMetadata::default();
    pm.0.insert("openai".into(), value);
    pm
}

#[cfg(test)]
#[path = "provider_metadata.test.rs"]
mod tests;
