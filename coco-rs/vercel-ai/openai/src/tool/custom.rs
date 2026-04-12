use std::collections::HashMap;

use serde_json::json;
use vercel_ai_provider::LanguageModelV4ProviderTool;

/// Create a custom provider tool for the Responses API.
///
/// The tool name is derived from the `id` field (`"openai.custom"`).
///
/// # Arguments
/// - `description` - Optional description of the tool
/// - `format` - Optional format config (e.g., `{ "type": "grammar", "syntax": ..., "definition": ... }` or `{ "type": "text" }`)
pub fn openai_custom_tool(
    description: Option<&str>,
    format: Option<serde_json::Value>,
) -> LanguageModelV4ProviderTool {
    let mut args: HashMap<String, serde_json::Value> = HashMap::new();
    if let Some(desc) = description {
        args.insert("description".into(), json!(desc));
    }
    if let Some(fmt) = format {
        args.insert("format".into(), fmt);
    }
    LanguageModelV4ProviderTool {
        id: "openai.custom".into(),
        name: "custom".into(),
        args,
    }
}
