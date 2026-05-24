use std::collections::HashMap;

use super::*;
use vercel_ai_provider::LanguageModelV4ProviderTool;
use vercel_ai_provider::ToolDefinitionV4 as LanguageModelV4FunctionTool;

#[test]
fn no_tools_returns_none() {
    let result = prepare_anthropic_tools(
        &None, &None, None, false, /*context_management_eligible*/ true, None,
    );
    assert!(result.tools.is_none());
    assert!(result.tool_choice.is_none());
    assert!(result.warnings.is_empty());
}

#[test]
fn empty_tools_returns_none() {
    let result = prepare_anthropic_tools(
        &Some(vec![]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
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
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
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
        /*context_management_eligible*/ true,
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
        /*context_management_eligible*/ true,
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
        /*context_management_eligible*/ true,
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
        /*context_management_eligible*/ true,
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
        /*context_management_eligible*/ true,
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
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
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
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
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
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
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
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
    assert!(
        result.betas.contains("advanced-tool-use-2025-11-20"),
        "expected advanced-tool-use beta for allowedCallers, got: {:?}",
        result.betas
    );
}

/// `deferLoading: true` in the tool's provider_options must surface on
/// the wire as `defer_loading: true`. This is the multi-provider TS
/// parity path: `engine_prompt::build_tool_definitions` writes the
/// `deferLoading` flag for deferred-but-not-discovered tools when the
/// model supports `ServerSideToolReference`, and the Anthropic adapter
/// translates it to the server's wire shape verbatim.
#[test]
fn defer_loading_provider_option_surfaces_on_wire() {
    let mut anthropic = HashMap::new();
    anthropic.insert("deferLoading".to_string(), serde_json::json!(true));
    let mut po_map = HashMap::new();
    po_map.insert("anthropic".to_string(), anthropic);

    let tool = LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "WebFetch".into(),
        description: Some("Fetch a URL".into()),
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
        input_examples: None,
        strict: None,
        provider_options: Some(vercel_ai_provider::ProviderOptions(po_map)),
    });
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
    let tools = result.tools.unwrap_or_else(|| panic!("should have tools"));
    assert_eq!(tools[0]["name"], "WebFetch");
    assert_eq!(
        tools[0]["defer_loading"],
        serde_json::json!(true),
        "deferLoading must round-trip onto the wire: {:?}",
        tools[0]
    );
}

#[test]
fn defer_loading_absent_when_provider_option_false_or_missing() {
    // Sanity: a tool without the deferLoading flag must NOT have
    // `defer_loading` in the wire body. Default Anthropic semantics:
    // tools without the field are eagerly exposed to the model.
    let tool = LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "Bash".into(),
        description: Some("Run a shell command".into()),
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
        input_examples: None,
        strict: None,
        provider_options: None,
    });
    let result = prepare_anthropic_tools(
        &Some(vec![tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
    let tools = result.tools.unwrap();
    assert!(tools[0].get("defer_loading").is_none());
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
    let result = prepare_anthropic_tools(
        &Some(vec![regex_tool, bm25_tool]),
        &None,
        None,
        false,
        /*context_management_eligible*/ true,
        None,
    );
    assert!(
        !result.betas.contains("advanced-tool-use-2025-11-20"),
        "tool_search tools should not add advanced-tool-use beta, got: {:?}",
        result.betas
    );
    let tools = result.tools.unwrap();
    assert_eq!(tools.len(), 2);
}
