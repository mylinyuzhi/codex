use super::*;

fn noop_elicitation() -> SendElicitation {
    Box::new(|_id, _req| {
        Box::pin(async move {
            Err(coco_rmcp_client::RmcpClientError::generic(
                "not used by test",
            ))
        })
    })
}

#[test]
fn test_truncate_tool_description() {
    let short = "A short description";
    assert_eq!(truncate_tool_description(short), short);

    let long = "x".repeat(3000);
    let truncated = truncate_tool_description(&long);
    assert!(truncated.len() < 3000);
    assert!(truncated.ends_with("...(truncated)"));
}

#[tokio::test]
async fn authenticate_stdio_reports_oauth_not_needed() {
    let mut manager = McpConnectionManager::new(std::env::temp_dir());
    manager.register_server(crate::types::ScopedMcpServerConfig {
        name: "local".into(),
        config: crate::types::McpServerConfig::Stdio(crate::types::McpStdioConfig {
            command: "echo".into(),
            args: vec![],
            env: Default::default(),
            cwd: None,
        }),
        scope: crate::types::ConfigScope::User,
        plugin_source: None,
    });

    let result = manager
        .authenticate("local", noop_elicitation())
        .await
        .unwrap();
    assert_eq!(
        result,
        "MCP server 'local' does not use OAuth authentication."
    );
}
