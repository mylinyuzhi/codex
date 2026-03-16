use std::collections::HashMap;

use serde_json::json;
use vercel_ai_provider::LanguageModelV4ProviderTool;

/// Create a code_interpreter provider tool for the Responses API.
///
/// # Arguments
/// - `container` - Optional container environment identifier
/// - `file_ids` - Optional list of file IDs accessible to the interpreter
pub fn openai_code_interpreter_tool(
    container: Option<&str>,
    file_ids: Option<Vec<String>>,
) -> LanguageModelV4ProviderTool {
    let mut args: HashMap<String, serde_json::Value> = HashMap::new();
    if let Some(c) = container {
        args.insert("container".into(), json!(c));
    }
    if let Some(ids) = file_ids {
        args.insert("file_ids".into(), json!(ids));
    }
    LanguageModelV4ProviderTool {
        id: "openai.code_interpreter".into(),
        name: "code_interpreter".into(),
        args,
    }
}
