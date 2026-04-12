use std::collections::HashMap;

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
fn test_truncate_result_content_text() {
    let mut result = McpToolCallResult {
        content: vec![McpToolContent::Text {
            text: "y".repeat(5000),
        }],
        is_error: false,
        meta: None,
        structured_content: None,
        elapsed_ms: 0,
    };

    truncate_result_content(&mut result);

    if let McpToolContent::Text { text } = &result.content[0] {
        assert!(text.len() < 5000);
        assert!(text.contains("[truncated, 5000 chars total]"));
    } else {
        panic!("expected text content");
    }
}

#[test]
fn test_truncate_result_content_within_limit() {
    let original_text = "short text".to_string();
    let mut result = McpToolCallResult {
        content: vec![McpToolContent::Text {
            text: original_text.clone(),
        }],
        is_error: false,
        meta: None,
        structured_content: None,
        elapsed_ms: 0,
    };

    truncate_result_content(&mut result);

    if let McpToolContent::Text { text } = &result.content[0] {
        assert_eq!(text, &original_text);
    } else {
        panic!("expected text content");
    }
}

#[test]
fn test_format_duration_millis() {
    assert_eq!(format_duration(Duration::from_millis(123)), "123ms");
    assert_eq!(format_duration(Duration::from_millis(0)), "0ms");
    assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
}

#[test]
fn test_format_duration_seconds() {
    assert_eq!(format_duration(Duration::from_millis(1000)), "1s");
    assert_eq!(format_duration(Duration::from_millis(4500)), "4s");
    assert_eq!(format_duration(Duration::from_millis(59999)), "59s");
}

#[test]
fn test_format_duration_minutes() {
    assert_eq!(format_duration(Duration::from_millis(60_000)), "1m 0s");
    assert_eq!(format_duration(Duration::from_millis(90_000)), "1m 30s");
    assert_eq!(format_duration(Duration::from_millis(125_000)), "2m 5s");
}

#[test]
fn test_estimate_content_size() {
    let content = vec![
        McpToolContent::Text {
            text: "hello".to_string(),
        },
        McpToolContent::Image {
            data: "base64data".to_string(),
            mime_type: "image/png".to_string(),
        },
        McpToolContent::Resource {
            uri: "file:///test".to_string(),
            mime_type: None,
            text: Some("content".to_string()),
        },
    ];

    let size = estimate_content_size(&content);
    // "hello" (5) + "base64data" (10) + "content" (7) + "file:///test" (12)
    assert_eq!(size, 34);
}

#[test]
fn test_estimate_content_size_empty() {
    assert_eq!(estimate_content_size(&[]), 0);
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

#[test]
fn test_mcp_tool_call_result_serialization_roundtrip() {
    let result = McpToolCallResult {
        content: vec![McpToolContent::Text {
            text: "done".to_string(),
        }],
        is_error: false,
        meta: Some(HashMap::from([(
            "key".to_string(),
            serde_json::Value::String("val".to_string()),
        )])),
        structured_content: None,
        elapsed_ms: 42,
    };

    let json = serde_json::to_string(&result).unwrap();
    let deserialized: McpToolCallResult = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.is_error, false);
    assert_eq!(deserialized.elapsed_ms, 42);
    assert_eq!(deserialized.content.len(), 1);
}

#[test]
fn test_mcp_progress_status_serialization() {
    let progress = McpToolProgress {
        server_name: "test-server".to_string(),
        tool_name: "my_tool".to_string(),
        status: McpProgressStatus::Progress,
        progress: Some(50.0),
        total: Some(100.0),
        message: Some("halfway".to_string()),
        elapsed_ms: Some(500),
    };

    let json = serde_json::to_string(&progress).unwrap();
    assert!(json.contains("\"status\":\"progress\""));
    assert!(json.contains("\"progress\":50.0"));
}

#[tokio::test]
async fn test_handle_tool_call_unknown_server() {
    let manager = McpConnectionManager::new(std::path::PathBuf::from("/tmp/coco-test"));
    let cancel = tokio_util::sync::CancellationToken::new();

    let options = McpToolCallOptions {
        server_name: "nonexistent".to_string(),
        tool_name: "some_tool".to_string(),
        arguments: HashMap::new(),
        tool_use_id: None,
        timeout_ms: Some(1000),
    };

    let result = handle_mcp_tool_call(&manager, options, cancel).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        McpClientError::ServerNotFound { .. }
    ));
}
