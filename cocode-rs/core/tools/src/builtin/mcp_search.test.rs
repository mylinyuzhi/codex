use super::*;

fn make_tool_info(server: &str, name: &str, desc: &str) -> McpToolInfo {
    McpToolInfo {
        server: server.to_string(),
        name: name.to_string(),
        description: Some(desc.to_string()),
        input_schema: serde_json::json!({"type": "object"}),
    }
}

/// Helper to extract text content from a ToolOutput.
fn extract_text(output: &ToolOutput) -> &str {
    match &output.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    }
}

#[tokio::test]
async fn test_search_by_name() {
    let tools = Arc::new(Mutex::new(vec![
        make_tool_info("github", "list_repos", "List GitHub repositories"),
        make_tool_info("github", "create_issue", "Create a GitHub issue"),
        make_tool_info("slack", "send_message", "Send a Slack message"),
    ]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    let result = tool
        .execute(serde_json::json!({"query": "repo"}), &mut ctx)
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("list_repos"));
    assert!(!text.contains("send_message"));
}

#[tokio::test]
async fn test_search_by_description() {
    let tools = Arc::new(Mutex::new(vec![
        make_tool_info("github", "list_repos", "List GitHub repositories"),
        make_tool_info("slack", "send_message", "Send a Slack message"),
    ]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    let result = tool
        .execute(serde_json::json!({"query": "slack"}), &mut ctx)
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("send_message"));
}

#[tokio::test]
async fn test_search_with_server_filter() {
    let tools = Arc::new(Mutex::new(vec![
        make_tool_info("github", "list_repos", "List repos"),
        make_tool_info("gitlab", "list_repos", "List repos"),
    ]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    let result = tool
        .execute(
            serde_json::json!({"query": "repo", "server": "github"}),
            &mut ctx,
        )
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("github"));
    assert!(!text.contains("gitlab"));
}

#[tokio::test]
async fn test_search_no_results() {
    let tools = Arc::new(Mutex::new(vec![make_tool_info(
        "github",
        "list_repos",
        "List repos",
    )]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    let result = tool
        .execute(serde_json::json!({"query": "nonexistent"}), &mut ctx)
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("No MCP tools found"));
}

#[tokio::test]
async fn test_search_empty_query() {
    let tools = Arc::new(Mutex::new(vec![
        make_tool_info("github", "list_repos", "List repos"),
        make_tool_info("slack", "send_message", "Send a message"),
    ]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    // Empty query matches all tools
    let result = tool
        .execute(serde_json::json!({"query": ""}), &mut ctx)
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("list_repos"));
    assert!(text.contains("send_message"));
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let tools = Arc::new(Mutex::new(vec![make_tool_info(
        "github",
        "ListRepos",
        "List GitHub Repositories",
    )]));
    let tool = McpSearchTool::new(tools);
    let mut ctx = ToolContext::new("test", "session", std::path::PathBuf::from("."));

    let result = tool
        .execute(serde_json::json!({"query": "listrepos"}), &mut ctx)
        .await
        .unwrap();
    let text = extract_text(&result);
    assert!(text.contains("ListRepos"));
}

#[test]
fn test_tool_metadata() {
    let tools = Arc::new(Mutex::new(Vec::new()));
    let tool = McpSearchTool::new(tools);
    assert_eq!(tool.name(), "MCPSearch");
    assert!(tool.is_read_only());
    assert!(tool.is_concurrent_safe());
}
