use super::*;

#[test]
fn test_role_serialization() {
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
    assert_eq!(
        serde_json::to_string(&Role::Assistant).unwrap(),
        r#""assistant""#
    );
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
    assert_eq!(
        serde_json::to_string(&Role::Developer).unwrap(),
        r#""developer""#
    );
}

#[test]
fn test_tool_creation() {
    let tool = Tool::function(
        "get_weather",
        Some("Get the weather".to_string()),
        serde_json::json!({"type": "object", "properties": {}}),
    );
    assert!(tool.is_ok());

    // Empty name should fail
    let tool = Tool::function(
        "",
        None,
        serde_json::json!({"type": "object", "properties": {}}),
    );
    assert!(tool.is_err());
}

#[test]
fn test_tool_with_strict() {
    let tool = Tool::function(
        "get_weather",
        None,
        serde_json::json!({"type": "object", "properties": {}}),
    )
    .unwrap()
    .strict(true);

    if let Tool::Function { function } = tool {
        assert_eq!(function.strict, Some(true));
    } else {
        panic!("Expected Function variant");
    }
}

#[test]
fn test_builtin_tools() {
    let web = Tool::web_search();
    let json = serde_json::to_string(&web).unwrap();
    assert!(json.contains(r#""type":"web_search""#));

    let code = Tool::code_interpreter();
    let json = serde_json::to_string(&code).unwrap();
    assert!(json.contains(r#""type":"code_interpreter""#));

    let computer = Tool::computer_use(1920, 1080);
    let json = serde_json::to_string(&computer).unwrap();
    assert!(json.contains(r#""type":"computer_use_preview""#));
    assert!(json.contains(r#""display_width":1920"#));
}

#[test]
fn test_tool_choice_serialization() {
    let auto = serde_json::to_string(&ToolChoice::Auto).unwrap();
    assert!(auto.contains(r#""type":"auto""#));

    let func = serde_json::to_string(&ToolChoice::Function {
        name: "test".to_string(),
    })
    .unwrap();
    assert!(func.contains(r#""type":"function""#));
    assert!(func.contains(r#""name":"test""#));

    // WebSearch serializes with "web_search_preview" tag
    let ws = serde_json::to_string(&ToolChoice::WebSearch).unwrap();
    assert!(ws.contains(r#""type":"web_search_preview""#));

    // Allowed serializes with "allowed_tools" tag
    let allowed = serde_json::to_string(&ToolChoice::Allowed {
        mode: "auto".to_string(),
        tools: vec![serde_json::json!({"type": "function", "name": "foo"})],
    })
    .unwrap();
    assert!(allowed.contains(r#""type":"allowed_tools""#));
    assert!(allowed.contains(r#""mode":"auto""#));

    // Mcp serializes with optional name
    let mcp = serde_json::to_string(&ToolChoice::Mcp {
        server_label: "srv".to_string(),
        name: None,
    })
    .unwrap();
    assert!(mcp.contains(r#""type":"mcp""#));
    assert!(mcp.contains(r#""server_label":"srv""#));
    assert!(!mcp.contains(r#""name""#));

    let mcp_named = serde_json::to_string(&ToolChoice::Mcp {
        server_label: "srv".to_string(),
        name: Some("my_tool".to_string()),
    })
    .unwrap();
    assert!(mcp_named.contains(r#""name":"my_tool""#));
}

#[test]
fn test_tool_choice_deserialize_plain_strings() {
    // OpenAI returns plain strings like "auto" in response.created events
    let auto: ToolChoice = serde_json::from_str(r#""auto""#).unwrap();
    assert!(matches!(auto, ToolChoice::Auto));

    let none: ToolChoice = serde_json::from_str(r#""none""#).unwrap();
    assert!(matches!(none, ToolChoice::None));

    let required: ToolChoice = serde_json::from_str(r#""required""#).unwrap();
    assert!(matches!(required, ToolChoice::Required));

    // Unknown string should error
    let result = serde_json::from_str::<ToolChoice>(r#""unknown""#);
    assert!(result.is_err());
}

#[test]
fn test_tool_choice_deserialize_tagged_objects() {
    // Object form: {"type":"auto"}
    let auto: ToolChoice = serde_json::from_str(r#"{"type":"auto"}"#).unwrap();
    assert!(matches!(auto, ToolChoice::Auto));

    let none: ToolChoice = serde_json::from_str(r#"{"type":"none"}"#).unwrap();
    assert!(matches!(none, ToolChoice::None));

    let required: ToolChoice = serde_json::from_str(r#"{"type":"required"}"#).unwrap();
    assert!(matches!(required, ToolChoice::Required));

    // Function
    let func: ToolChoice =
        serde_json::from_str(r#"{"type":"function","name":"get_weather"}"#).unwrap();
    if let ToolChoice::Function { name } = &func {
        assert_eq!(name, "get_weather");
    } else {
        panic!("Expected Function variant");
    }

    // Allowed
    let allowed: ToolChoice = serde_json::from_str(
        r#"{"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"x"}]}"#,
    )
    .unwrap();
    if let ToolChoice::Allowed { mode, tools } = &allowed {
        assert_eq!(mode, "required");
        assert_eq!(tools.len(), 1);
    } else {
        panic!("Expected Allowed variant");
    }

    // Web search variants
    let ws: ToolChoice = serde_json::from_str(r#"{"type":"web_search_preview"}"#).unwrap();
    assert!(matches!(ws, ToolChoice::WebSearch));

    let ws2: ToolChoice =
        serde_json::from_str(r#"{"type":"web_search_preview_2025_03_11"}"#).unwrap();
    assert!(matches!(ws2, ToolChoice::WebSearchPreview20250311));

    // Other built-in types
    let fs: ToolChoice = serde_json::from_str(r#"{"type":"file_search"}"#).unwrap();
    assert!(matches!(fs, ToolChoice::FileSearch));

    let ci: ToolChoice = serde_json::from_str(r#"{"type":"code_interpreter"}"#).unwrap();
    assert!(matches!(ci, ToolChoice::CodeInterpreter));

    let cu: ToolChoice = serde_json::from_str(r#"{"type":"computer_use_preview"}"#).unwrap();
    assert!(matches!(cu, ToolChoice::ComputerUse));

    let ig: ToolChoice = serde_json::from_str(r#"{"type":"image_generation"}"#).unwrap();
    assert!(matches!(ig, ToolChoice::ImageGeneration));

    let sh: ToolChoice = serde_json::from_str(r#"{"type":"shell"}"#).unwrap();
    assert!(matches!(sh, ToolChoice::Shell));

    let ap: ToolChoice = serde_json::from_str(r#"{"type":"apply_patch"}"#).unwrap();
    assert!(matches!(ap, ToolChoice::ApplyPatch));

    // Mcp with and without name
    let mcp: ToolChoice = serde_json::from_str(r#"{"type":"mcp","server_label":"srv"}"#).unwrap();
    if let ToolChoice::Mcp { server_label, name } = &mcp {
        assert_eq!(server_label, "srv");
        assert!(name.is_none());
    } else {
        panic!("Expected Mcp variant");
    }

    let mcp2: ToolChoice =
        serde_json::from_str(r#"{"type":"mcp","server_label":"srv","name":"tool1"}"#).unwrap();
    if let ToolChoice::Mcp { server_label, name } = &mcp2 {
        assert_eq!(server_label, "srv");
        assert_eq!(name.as_deref(), Some("tool1"));
    } else {
        panic!("Expected Mcp variant");
    }

    // Custom
    let custom: ToolChoice =
        serde_json::from_str(r#"{"type":"custom","name":"my_custom"}"#).unwrap();
    if let ToolChoice::Custom { name } = &custom {
        assert_eq!(name, "my_custom");
    } else {
        panic!("Expected Custom variant");
    }
}

#[test]
fn test_tool_choice_roundtrip() {
    // Serialization produces tagged objects, which roundtrip correctly
    let variants: Vec<ToolChoice> = vec![
        ToolChoice::Auto,
        ToolChoice::None,
        ToolChoice::Required,
        ToolChoice::Function {
            name: "f".to_string(),
        },
        ToolChoice::WebSearch,
        ToolChoice::WebSearchPreview20250311,
        ToolChoice::FileSearch,
        ToolChoice::Shell,
        ToolChoice::ApplyPatch,
        ToolChoice::Custom {
            name: "c".to_string(),
        },
    ];

    for variant in variants {
        let json = serde_json::to_string(&variant).unwrap();
        let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(
            format!("{variant:?}"),
            format!("{parsed:?}"),
            "Roundtrip failed for: {json}"
        );
    }
}

#[test]
fn test_metadata() {
    let meta = Metadata::new()
        .insert("user_id", "123")
        .insert("session_id", "abc");
    assert_eq!(meta.extra.len(), 2);
}

#[test]
fn test_tool_builders() {
    // Web search with context size
    let web = Tool::web_search().with_search_context_size("high");
    if let Tool::WebSearch {
        search_context_size,
        ..
    } = web
    {
        assert_eq!(search_context_size, Some("high".to_string()));
    } else {
        panic!("Expected WebSearch variant");
    }

    // File search with max results
    let file = Tool::file_search(vec!["vs-123".to_string()]).with_max_results(10);
    if let Tool::FileSearch {
        max_num_results, ..
    } = file
    {
        assert_eq!(max_num_results, Some(10));
    } else {
        panic!("Expected FileSearch variant");
    }

    // Code interpreter with container
    let code = Tool::code_interpreter().with_container("container-123");
    if let Tool::CodeInterpreter { container } = code {
        assert_eq!(
            container,
            Some(serde_json::Value::String("container-123".to_string()))
        );
    } else {
        panic!("Expected CodeInterpreter variant");
    }

    // MCP with server URL and allowed tools
    let mcp = Tool::mcp("my-server")
        .with_server_url("https://mcp.example.com")
        .with_allowed_tools(serde_json::json!(["tool1", "tool2"]));
    if let Tool::Mcp {
        server_url,
        allowed_tools,
        ..
    } = mcp
    {
        assert_eq!(server_url, Some("https://mcp.example.com".to_string()));
        assert!(allowed_tools.is_some());
        let tools = allowed_tools.unwrap();
        assert_eq!(tools.as_array().unwrap().len(), 2);
    } else {
        panic!("Expected Mcp variant");
    }

    // Image generation with model and size
    let img = Tool::image_generation()
        .with_model("dall-e-3")
        .with_size("1024x1024")
        .with_quality("hd");
    if let Tool::ImageGeneration {
        model,
        size,
        quality,
        ..
    } = img
    {
        assert_eq!(model, Some("dall-e-3".to_string()));
        assert_eq!(size, Some("1024x1024".to_string()));
        assert_eq!(quality, Some("hd".to_string()));
    } else {
        panic!("Expected ImageGeneration variant");
    }
}

#[test]
fn test_function_tool_flat_serialization() {
    let tool = Tool::function(
        "LS",
        Some("List files".to_string()),
        serde_json::json!({"type": "object", "properties": {}}),
    )
    .unwrap();

    let json = serde_json::to_string(&tool).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Responses API flat format: name/parameters at top level, no nested "function" key.
    assert_eq!(v["type"], "function");
    assert_eq!(v["name"], "LS");
    assert!(v.get("parameters").is_some());
    assert!(
        v.get("function").is_none(),
        "must not have nested 'function' key"
    );

    // Roundtrip
    let parsed: Tool = serde_json::from_str(&json).unwrap();
    if let Tool::Function { function } = &parsed {
        assert_eq!(function.name, "LS");
        assert_eq!(function.description, Some("List files".to_string()));
    } else {
        panic!("Expected Function variant");
    }
}

#[test]
fn test_custom_tool_serialization() {
    // Custom tool with grammar format
    let tool = Tool::custom_with_grammar(
        "apply_patch",
        "Apply a unified diff",
        "lark",
        "start: line+",
    );
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"custom""#));
    assert!(json.contains(r#""name":"apply_patch""#));
    assert!(json.contains(r#""description":"Apply a unified diff""#));
    assert!(json.contains(r#""type":"grammar""#));
    assert!(json.contains(r#""syntax":"lark""#));
    assert!(json.contains(r#""definition":"start: line+""#));

    // Roundtrip
    let parsed: Tool = serde_json::from_str(&json).unwrap();
    if let Tool::Custom {
        name,
        description,
        format,
    } = parsed
    {
        assert_eq!(name, "apply_patch");
        assert_eq!(description, Some("Apply a unified diff".to_string()));
        if let Some(CustomToolInputFormat::Grammar { syntax, definition }) = format {
            assert_eq!(syntax, "lark");
            assert_eq!(definition, "start: line+");
        } else {
            panic!("Expected Grammar format");
        }
    } else {
        panic!("Expected Custom variant");
    }

    // Custom tool with text format
    let text_tool = Tool::custom_text("my_tool", "A text tool");
    let json = serde_json::to_string(&text_tool).unwrap();
    assert!(json.contains(r#""type":"custom""#));
    assert!(json.contains(r#""format":{"type":"text"}"#));

    // Roundtrip
    let parsed: Tool = serde_json::from_str(&json).unwrap();
    if let Tool::Custom { format, .. } = parsed {
        assert!(matches!(format, Some(CustomToolInputFormat::Text)));
    } else {
        panic!("Expected Custom variant");
    }

    // Custom tool with no format
    let bare_tool = Tool::custom("bare_tool");
    let json = serde_json::to_string(&bare_tool).unwrap();
    assert!(json.contains(r#""type":"custom""#));
    assert!(json.contains(r#""name":"bare_tool""#));
    assert!(!json.contains(r#""format""#));
    assert!(!json.contains(r#""description""#));
}
