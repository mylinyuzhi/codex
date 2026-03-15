use super::*;
use serde_json::json;
use vercel_ai_provider::LanguageModelV4ProviderTool;
use vercel_ai_provider::language_model::v4::LanguageModelV4FunctionTool;

#[test]
fn returns_default_for_no_tools() {
    let result = prepare_tools(&None, &None, "gemini-2.0-flash");
    assert!(result.function_declarations.is_none());
    assert!(result.tool_config.is_none());
    assert!(result.tool_entries.is_empty());
}

#[test]
fn converts_function_tool() {
    let tools = vec![LanguageModelV4Tool::Function(
        LanguageModelV4FunctionTool::with_description(
            "get_weather",
            "Get the weather",
            json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }),
        ),
    )];
    let result = prepare_tools(&Some(tools), &None, "gemini-2.0-flash");
    assert!(result.function_declarations.is_some());
    let decls = result.function_declarations.unwrap();
    assert_eq!(decls[0]["name"], "get_weather");
    assert_eq!(decls[0]["description"], "Get the weather");
}

#[test]
fn converts_google_search_tool() {
    let tools = vec![LanguageModelV4Tool::Provider(
        LanguageModelV4ProviderTool::from_id("google.google_search", "google_search"),
    )];
    let result = prepare_tools(&Some(tools), &None, "gemini-2.0-flash");
    assert_eq!(result.tool_entries.len(), 1);
    assert!(result.tool_entries[0].get("googleSearch").is_some());
}

#[test]
fn converts_code_execution_tool() {
    let tools = vec![LanguageModelV4Tool::Provider(
        LanguageModelV4ProviderTool::from_id("google.code_execution", "code_execution"),
    )];
    let result = prepare_tools(&Some(tools), &None, "gemini-2.0-flash");
    assert!(result.tool_entries[0].get("codeExecution").is_some());
}

#[test]
fn maps_auto_tool_choice() {
    let tools = vec![LanguageModelV4Tool::Function(
        LanguageModelV4FunctionTool::new("f", json!({})),
    )];
    let result = prepare_tools(
        &Some(tools),
        &Some(LanguageModelV4ToolChoice::Auto),
        "gemini-2.0-flash",
    );
    let config = result.tool_config.unwrap();
    assert_eq!(config["functionCallingConfig"]["mode"], "AUTO");
}

#[test]
fn maps_required_tool_choice() {
    let tools = vec![LanguageModelV4Tool::Function(
        LanguageModelV4FunctionTool::new("f", json!({})),
    )];
    let result = prepare_tools(
        &Some(tools),
        &Some(LanguageModelV4ToolChoice::Required),
        "gemini-2.0-flash",
    );
    let config = result.tool_config.unwrap();
    assert_eq!(config["functionCallingConfig"]["mode"], "ANY");
}

#[test]
fn maps_specific_tool_choice() {
    let tools = vec![LanguageModelV4Tool::Function(
        LanguageModelV4FunctionTool::new("my_tool", json!({})),
    )];
    let result = prepare_tools(
        &Some(tools),
        &Some(LanguageModelV4ToolChoice::tool("my_tool")),
        "gemini-2.0-flash",
    );
    let config = result.tool_config.unwrap();
    assert_eq!(config["functionCallingConfig"]["mode"], "ANY");
    assert_eq!(
        config["functionCallingConfig"]["allowedFunctionNames"],
        json!(["my_tool"])
    );
}
