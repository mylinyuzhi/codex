use super::*;

#[test]
fn test_format_mcp_result_text() {
    let result = McpToolResult {
        content: vec![McpContentPart::Text {
            text: "Hello, world!".into(),
        }],
        is_error: false,
    };
    assert_eq!(format_mcp_result(&result), "Hello, world!");
}

#[test]
fn test_format_mcp_result_multiple_parts() {
    let result = McpToolResult {
        content: vec![
            McpContentPart::Text { text: "Line 1".into() },
            McpContentPart::Text { text: "Line 2".into() },
            McpContentPart::Image {
                data: "base64...".into(),
                mime_type: "image/png".into(),
            },
        ],
        is_error: false,
    };
    let formatted = format_mcp_result(&result);
    assert!(formatted.contains("Line 1"));
    assert!(formatted.contains("Line 2"));
    assert!(formatted.contains("[Image: image/png]"));
}

#[test]
fn test_format_mcp_result_error_empty() {
    let result = McpToolResult {
        content: vec![],
        is_error: true,
    };
    assert_eq!(
        format_mcp_result(&result),
        "MCP tool execution failed with no output"
    );
}

#[test]
fn test_format_mcp_result_resource_with_text() {
    let result = McpToolResult {
        content: vec![McpContentPart::Resource {
            uri: "file:///data.json".into(),
            text: Some("{\"key\": \"value\"}".into()),
        }],
        is_error: false,
    };
    assert_eq!(format_mcp_result(&result), "{\"key\": \"value\"}");
}

#[test]
fn test_format_mcp_result_resource_without_text() {
    let result = McpToolResult {
        content: vec![McpContentPart::Resource {
            uri: "file:///binary.dat".into(),
            text: None,
        }],
        is_error: false,
    };
    assert_eq!(
        format_mcp_result(&result),
        "[Resource: file:///binary.dat]"
    );
}

#[test]
fn test_format_resource_contents_single() {
    let contents = vec![McpResourceContent {
        uri: "file:///readme.md".into(),
        text: Some("# Hello".into()),
        blob: None,
        mime_type: Some("text/markdown".into()),
    }];
    let result = format_resource_contents(&contents);
    assert!(result.contains("Resource: file:///readme.md"));
    assert!(result.contains("Type: text/markdown"));
    assert!(result.contains("# Hello"));
}

#[test]
fn test_format_resource_contents_empty() {
    assert_eq!(
        format_resource_contents(&[]),
        "No resource content available"
    );
}

#[test]
fn test_format_resource_contents_binary() {
    let contents = vec![McpResourceContent {
        uri: "file:///image.png".into(),
        text: None,
        blob: Some("base64data".into()),
        mime_type: Some("image/png".into()),
    }];
    let result = format_resource_contents(&contents);
    assert!(result.contains("[Binary content]"));
}

#[test]
fn test_format_tool_schemas() {
    let schemas = vec![
        McpToolSchema {
            name: "search".into(),
            description: Some("Search for items".into()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                }
            })),
        },
        McpToolSchema {
            name: "read".into(),
            description: None,
            input_schema: None,
        },
    ];
    let result = format_tool_schemas(&schemas);
    assert!(result.contains("Available tools (2)"));
    assert!(result.contains("search - Search for items"));
    assert!(result.contains("query (string): Search query"));
    assert!(result.contains("read - No description"));
}

#[test]
fn test_format_tool_schemas_empty() {
    assert_eq!(
        format_tool_schemas(&[]),
        "No tools available on this MCP server"
    );
}

#[test]
fn test_classify_for_collapse() {
    let error = McpToolResult {
        content: vec![],
        is_error: true,
    };
    assert_eq!(classify_for_collapse(&error), "error");

    let empty = McpToolResult {
        content: vec![],
        is_error: false,
    };
    assert_eq!(classify_for_collapse(&empty), "empty");

    let brief = McpToolResult {
        content: vec![McpContentPart::Text { text: "ok".into() }],
        is_error: false,
    };
    assert_eq!(classify_for_collapse(&brief), "brief");

    let success = McpToolResult {
        content: vec![McpContentPart::Text {
            text: "a".repeat(200),
        }],
        is_error: false,
    };
    assert_eq!(classify_for_collapse(&success), "success");
}

#[test]
fn test_mcp_tool_id() {
    assert_eq!(mcp_tool_id("slack", "send"), "mcp__slack__send");
}

#[test]
fn test_parse_mcp_tool_id() {
    assert_eq!(
        parse_mcp_tool_id("mcp__slack__send"),
        Some(("slack", "send"))
    );
    assert_eq!(parse_mcp_tool_id("Bash"), None);
    assert_eq!(parse_mcp_tool_id("mcp__no_double"), None);
}

#[test]
fn test_is_result_truncated() {
    assert!(is_result_truncated("some text\n\n... [5 lines truncated]"));
    assert!(!is_result_truncated("normal output"));
}

#[test]
fn test_truncate_mcp_output_within_limit() {
    let short = "Hello world".to_string();
    assert_eq!(truncate_mcp_output(short.clone()), short);
}

#[test]
fn test_truncate_mcp_output_exceeds_limit() {
    // Build a string that exceeds MAX_MCP_RESULT_SIZE_CHARS
    let line = "x".repeat(1000);
    let mut big = String::new();
    for _ in 0..200 {
        big.push_str(&line);
        big.push('\n');
    }
    let result = truncate_mcp_output(big);
    assert!(result.len() <= MAX_MCP_RESULT_SIZE_CHARS + 100); // small overhead for marker
    assert!(result.ends_with("truncated]"));
}
