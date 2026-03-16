use std::collections::HashMap;

use serde_json::json;
use vercel_ai_provider::LanguageModelV4ProviderTool;

/// Create a custom provider tool for the Responses API.
///
/// # Arguments
/// - `name` - The tool name (also used as the tool type)
/// - `input_schema` - JSON schema defining the tool's input
/// - `description` - Optional description of the tool
pub fn openai_custom_tool(
    name: &str,
    input_schema: serde_json::Value,
    description: Option<&str>,
) -> LanguageModelV4ProviderTool {
    let mut args: HashMap<String, serde_json::Value> = HashMap::new();
    args.insert("schema".into(), input_schema);
    if let Some(desc) = description {
        args.insert("description".into(), json!(desc));
    }
    LanguageModelV4ProviderTool {
        id: format!("openai.{name}"),
        name: name.into(),
        args,
    }
}
