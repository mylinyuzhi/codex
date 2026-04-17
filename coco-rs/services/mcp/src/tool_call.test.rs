use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_truncate_description_short() {
    let short = "A short description";
    assert_eq!(truncate_description(short), short);
}

#[test]
fn test_truncate_description_at_limit() {
    let exact = "x".repeat(MAX_DESCRIPTION_LENGTH);
    assert_eq!(truncate_description(&exact), exact);
}

#[test]
fn test_truncate_description_over_limit() {
    let long = "x".repeat(3000);
    let truncated = truncate_description(&long);
    assert!(truncated.len() < 3000);
    assert!(truncated.ends_with("... [truncated]"));
    assert!(truncated.starts_with(&"x".repeat(MAX_DESCRIPTION_LENGTH)));
}

#[test]
fn test_mcp_tool_content_serialization() {
    let text = McpToolContent::Text {
        text: "result".to_string(),
    };
    let json = serde_json::to_string(&text).unwrap();
    assert!(json.contains("\"type\":\"text\""));

    let image = McpToolContent::Image {
        data: "abc".to_string(),
        mime_type: "image/png".to_string(),
    };
    let json = serde_json::to_string(&image).unwrap();
    assert!(json.contains("\"type\":\"image\""));
}
