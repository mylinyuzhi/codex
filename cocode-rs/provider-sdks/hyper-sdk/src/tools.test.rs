use super::*;

#[test]
fn test_tool_definition() {
    let tool = ToolDefinition::full(
        "get_weather",
        "Get the current weather for a location",
        serde_json::json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City name"
                }
            },
            "required": ["location"]
        }),
    );

    assert_eq!(tool.name, "get_weather");
    assert!(tool.description.is_some());
}

#[test]
fn test_tool_call() {
    let call = ToolCall::new(
        "call_123",
        "get_weather",
        serde_json::json!({"location": "New York"}),
    );

    assert_eq!(call.id, "call_123");
    assert_eq!(call.name, "get_weather");

    #[derive(Deserialize)]
    struct Args {
        location: String,
    }

    let args: Args = call.parse_arguments().unwrap();
    assert_eq!(args.location, "New York");
}

#[test]
fn test_tool_choice_serde() {
    let choice = ToolChoice::Tool {
        name: "get_weather".to_string(),
    };
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains("\"type\":\"tool\""));
    assert!(json.contains("\"name\":\"get_weather\""));
}

#[test]
fn test_tool_result_content() {
    let text = ToolResultContent::text("Success!");
    assert_eq!(text.as_text(), Some("Success!"));

    let json = ToolResultContent::json(serde_json::json!({"status": "ok"}));
    assert!(json.as_text().is_none());
}

#[test]
fn test_tool_result_content_to_text() {
    // Text variant returns the string directly
    let text = ToolResultContent::Text("Hello, world!".to_string());
    assert_eq!(text.to_text(), "Hello, world!");

    // Json variant serializes to JSON string
    let json = ToolResultContent::Json(serde_json::json!({"key": "value"}));
    assert_eq!(json.to_text(), r#"{"key":"value"}"#);

    // Blocks variant concatenates text blocks, ignoring images
    let blocks = ToolResultContent::Blocks(vec![
        ToolResultBlock::Text {
            text: "First ".to_string(),
        },
        ToolResultBlock::Image {
            data: "base64data".to_string(),
            media_type: "image/png".to_string(),
        },
        ToolResultBlock::Text {
            text: "Second".to_string(),
        },
    ]);
    assert_eq!(blocks.to_text(), "First Second");

    // Empty blocks returns empty string
    let empty_blocks = ToolResultContent::Blocks(vec![]);
    assert_eq!(empty_blocks.to_text(), "");
}
