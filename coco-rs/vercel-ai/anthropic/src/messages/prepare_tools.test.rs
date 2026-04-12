use std::collections::HashMap;

use super::*;
use vercel_ai_provider::LanguageModelV4ProviderTool;
use vercel_ai_provider::ToolDefinitionV4 as LanguageModelV4FunctionTool;

#[test]
fn no_tools_returns_none() {
    let result = prepare_anthropic_tools(&None, &None, None, false, None);
    assert!(result.tools.is_none());
    assert!(result.tool_choice.is_none());
    assert!(result.warnings.is_empty());
}

#[test]
fn empty_tools_returns_none() {
    let result = prepare_anthropic_tools(&Some(vec![]), &None, None, false, None);
    assert!(result.tools.is_none());
}

#[test]
fn converts_function_tool() {
    let tool = LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "get_weather".into(),
        description: Some("Get weather".into()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        }),
        input_examples: None,
        strict: None,
        provider_options: None,
    });
    let result = prepare_anthropic_tools(&Some(vec![tool]), &None, None, false, None);
    let tools = result.tools.unwrap_or_else(|| panic!("should have tools"));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "get_weather");
    assert_eq!(tools[0]["description"], "Get weather");
}

#[test]
fn maps_tool_choice_auto() {
    let result = prepare_anthropic_tools(
        &Some(vec![LanguageModelV4Tool::Function(
            LanguageModelV4FunctionTool {
                name: "test".into(),
                description: None,
                input_schema: serde_json::json!({}),
                input_examples: None,
                strict: None,
                provider_options: None,
            },
        )]),
        &Some(LanguageModelV4ToolChoice::Auto),
        None,
        false,
        None,
    );
    assert_eq!(
        result.tool_choice,
        Some(serde_json::json!({"type": "auto"}))
    );
}

#[test]
fn maps_tool_choice_required_to_any() {
    let result = prepare_anthropic_tools(
        &Some(vec![LanguageModelV4Tool::Function(
            LanguageModelV4FunctionTool {
                name: "test".into(),
                description: None,
                input_schema: serde_json::json!({}),
                input_examples: None,
                strict: None,
                provider_options: None,
            },
        )]),
        &Some(LanguageModelV4ToolChoice::Required),
        None,
        false,
        None,
    );
    assert_eq!(result.tool_choice, Some(serde_json::json!({"type": "any"})));
}

#[test]
fn maps_tool_choice_none_removes_tools() {
    let result = prepare_anthropic_tools(
        &Some(vec![LanguageModelV4Tool::Function(
            LanguageModelV4FunctionTool {
                name: "test".into(),
                description: None,
                input_schema: serde_json::json!({}),
                input_examples: None,
                strict: None,
                provider_options: None,
            },
        )]),
        &Some(LanguageModelV4ToolChoice::None),
        None,
        false,
        None,
    );
    assert!(result.tools.is_none());
    assert!(result.tool_choice.is_none());
}

#[test]
fn maps_tool_choice_specific_tool() {
    let result = prepare_anthropic_tools(
        &Some(vec![LanguageModelV4Tool::Function(
            LanguageModelV4FunctionTool {
                name: "search".into(),
                description: None,
                input_schema: serde_json::json!({}),
                input_examples: None,
                strict: None,
                provider_options: None,
            },
        )]),
        &Some(LanguageModelV4ToolChoice::Tool {
            tool_name: "search".into(),
        }),
        None,
        false,
        None,
    );
    assert_eq!(
        result.tool_choice,
        Some(serde_json::json!({"type": "tool", "name": "search"}))
    );
}

#[test]
fn disable_parallel_tool_use_in_tool_choice() {
    let result = prepare_anthropic_tools(
        &Some(vec![LanguageModelV4Tool::Function(
            LanguageModelV4FunctionTool {
                name: "test".into(),
                description: None,
                input_schema: serde_json::json!({}),
                input_examples: None,
                strict: None,
                provider_options: None,
            },
        )]),
        &Some(LanguageModelV4ToolChoice::Auto),
        Some(true),
        false,
        None,
    );
    let tc = result
        .tool_choice
        .unwrap_or_else(|| panic!("expected tool_choice"));
    assert_eq!(tc["type"], "auto");
    assert_eq!(tc["disable_parallel_tool_use"], true);
}

#[test]
fn converts_code_execution_provider_tool() {
    let tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "anthropic.code_execution_20260120".into(),
        name: "code_execution_20260120".into(),
        args: HashMap::new(),
    });
    let result = prepare_anthropic_tools(&Some(vec![tool]), &None, None, false, None);
    let tools = result.tools.unwrap_or_else(|| panic!("should have tools"));
    assert_eq!(tools[0]["type"], "code_execution_20260120");
    assert_eq!(tools[0]["name"], "code_execution");
}

#[test]
fn converts_web_search_provider_tool() {
    let mut args = HashMap::new();
    args.insert("maxUses".into(), serde_json::json!(5));
    args.insert("allowedDomains".into(), serde_json::json!(["example.com"]));
    let tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "anthropic.web_search_20250305".into(),
        name: "web_search_20250305".into(),
        args,
    });
    let result = prepare_anthropic_tools(&Some(vec![tool]), &None, None, false, None);
    let tools = result.tools.unwrap_or_else(|| panic!("should have tools"));
    assert_eq!(tools[0]["type"], "web_search_20250305");
    assert_eq!(tools[0]["max_uses"], 5);
}

#[test]
fn unknown_provider_tool_emits_warning() {
    let tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "anthropic.unknown_tool".into(),
        name: "unknown".into(),
        args: HashMap::new(),
    });
    let result = prepare_anthropic_tools(&Some(vec![tool]), &None, None, false, None);
    assert!(result.tools.is_none());
    assert_eq!(result.warnings.len(), 1);
}

#[test]
fn allowed_callers_adds_advanced_tool_use_beta() {
    let tool = LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "my_tool".into(),
        description: None,
        input_schema: serde_json::json!({}),
        input_examples: None,
        strict: None,
        provider_options: Some(vercel_ai_provider::ProviderOptions({
            let mut po = HashMap::new();
            let mut anthropic = HashMap::new();
            anthropic.insert(
                "allowedCallers".to_string(),
                serde_json::json!(["caller_a"]),
            );
            po.insert("anthropic".into(), anthropic);
            po
        })),
    });
    let result = prepare_anthropic_tools(&Some(vec![tool]), &None, None, false, None);
    assert!(
        result.betas.contains("advanced-tool-use-2025-11-20"),
        "expected advanced-tool-use beta for allowedCallers, got: {:?}",
        result.betas
    );
}

#[test]
fn tool_search_does_not_add_advanced_tool_use_beta() {
    let regex_tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "anthropic.tool_search_regex_20251119".into(),
        name: "tool_search_tool_regex".into(),
        args: HashMap::new(),
    });
    let bm25_tool = LanguageModelV4Tool::Provider(LanguageModelV4ProviderTool {
        id: "anthropic.tool_search_bm25_20251119".into(),
        name: "tool_search_tool_bm25".into(),
        args: HashMap::new(),
    });
    let result =
        prepare_anthropic_tools(&Some(vec![regex_tool, bm25_tool]), &None, None, false, None);
    assert!(
        !result.betas.contains("advanced-tool-use-2025-11-20"),
        "tool_search tools should not add advanced-tool-use beta, got: {:?}",
        result.betas
    );
    let tools = result.tools.unwrap();
    assert_eq!(tools.len(), 2);
}
