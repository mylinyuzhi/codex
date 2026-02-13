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
        assert_eq!(container, Some("container-123".to_string()));
    } else {
        panic!("Expected CodeInterpreter variant");
    }

    // MCP with server URL and allowed tools
    let mcp = Tool::mcp("my-server")
        .with_server_url("https://mcp.example.com")
        .with_allowed_tools(vec!["tool1".to_string(), "tool2".to_string()]);
    if let Tool::Mcp {
        server_url,
        allowed_tools,
        ..
    } = mcp
    {
        assert_eq!(server_url, Some("https://mcp.example.com".to_string()));
        assert_eq!(allowed_tools.len(), 2);
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
    assert!(v.get("function").is_none(), "must not have nested 'function' key");

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
