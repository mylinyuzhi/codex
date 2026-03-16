use super::*;
use vercel_ai_provider::LanguageModelV4ProviderTool;
use vercel_ai_provider::language_model::v4::function_tool::LanguageModelV4FunctionTool;

#[test]
fn no_tools() {
    let r = prepare_responses_tools(&None, &None);
    assert!(r.tools.is_none());
    assert!(r.tool_choice.is_none());
}

#[test]
fn function_tool() {
    let tool = LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "get_weather".into(),
        description: Some("Get weather".into()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "city": { "type": "string" } }
        }),
        input_examples: None,
        strict: Some(true),
        provider_options: None,
    });
    let r = prepare_responses_tools(&Some(vec![tool]), &None);
    let tools = r.tools.expect("should have tools");
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "get_weather");
    assert_eq!(tools[0]["strict"], true);
}

#[test]
fn provider_tool_web_search() {
    let tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "openai.web_search".into(),
        name: "web_search".into(),
        args: [("search_context_size".into(), serde_json::json!("medium"))]
            .into_iter()
            .collect(),
    });
    let r = prepare_responses_tools(&Some(vec![tool]), &None);
    let tools = r.tools.expect("should have tools");
    assert_eq!(tools[0]["type"], "web_search");
    assert_eq!(tools[0]["search_context_size"], "medium");
}
