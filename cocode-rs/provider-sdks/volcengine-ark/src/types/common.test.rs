use super::*;

#[test]
fn test_role_serialization() {
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
    assert_eq!(
        serde_json::to_string(&Role::Assistant).unwrap(),
        r#""assistant""#
    );
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
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
