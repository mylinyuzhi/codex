use std::collections::HashMap;

use vercel_ai_provider::LanguageModelV4ProviderTool;

/// Create a shell provider tool for the Responses API.
pub fn openai_shell_tool() -> LanguageModelV4ProviderTool {
    let args: HashMap<String, serde_json::Value> = HashMap::new();
    LanguageModelV4ProviderTool {
        id: "openai.shell".into(),
        name: "shell".into(),
        args,
    }
}
