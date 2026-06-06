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

#[test]
fn headers_helper_output_must_be_string_map() {
    let ok = parse_headers_helper_output("srv", r#"{"Authorization":"Bearer x"}"#).unwrap();
    assert_eq!(ok.get("Authorization").unwrap(), "Bearer x");

    let err = parse_headers_helper_output("srv", r#"{"Authorization":123}"#).unwrap_err();
    assert!(err.to_string().contains("non-string"));
}

#[tokio::test]
async fn resolve_http_headers_dynamic_overrides_static() {
    let headers = resolve_http_headers(
        "srv",
        "https://example.test",
        &HashMap::from([
            ("Authorization".to_string(), "Bearer old".to_string()),
            ("X-Static".to_string(), "yes".to_string()),
        ]),
        &Some("printf '{\"Authorization\":\"Bearer new\"}'".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(headers.get("Authorization").unwrap(), "Bearer new");
    assert_eq!(headers.get("X-Static").unwrap(), "yes");
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

#[tokio::test]
async fn unregister_server_drops_config_and_connection_state() {
    let mut manager = McpConnectionManager::new(std::env::temp_dir());
    manager.register_server(crate::types::ScopedMcpServerConfig {
        name: "plugin:p:local".into(),
        config: crate::types::McpServerConfig::Stdio(crate::types::McpStdioConfig {
            command: "echo".into(),
            args: vec![],
            env: Default::default(),
            cwd: None,
        }),
        scope: crate::types::ConfigScope::Dynamic,
        plugin_source: None,
    });
    // register_server seeds a Pending connection state + a config entry.
    assert!(
        manager
            .registered_server_names()
            .contains(&"plugin:p:local".to_string())
    );
    assert!(manager.get_state("plugin:p:local").await.is_some());

    manager.unregister_server("plugin:p:local").await;
    assert!(
        !manager
            .registered_server_names()
            .contains(&"plugin:p:local".to_string()),
        "config entry must be dropped"
    );
    assert!(
        manager.get_state("plugin:p:local").await.is_none(),
        "connection state must be dropped"
    );
}

#[tokio::test]
async fn ensure_xaa_tokens_skips_exchange_when_stored_tokens_exist() {
    let home = tempfile::tempdir().unwrap();
    coco_rmcp_client::save_oauth_access_token(coco_rmcp_client::OAuthAccessTokenSave {
        server_name: "enterprise",
        url: "https://mcp.example.test",
        client_id: "as-client",
        access_token: "stored-token".to_string(),
        refresh_token: None,
        expires_in: Some(3600),
        scopes: None,
        store_mode: OAuthCredentialsStoreMode::File,
        config_home: home.path(),
    })
    .unwrap();

    let oauth = McpOAuthConfig {
        client_id: Some("as-client".into()),
        xaa: Some(crate::types::McpXaaConfig {
            client_id: None,
            client_secret: Some("as-secret".into()),
            idp_client_id: Some("idp-client".into()),
            idp_client_secret: None,
            idp_id_token: None,
            idp_token_endpoint: Some("https://idp.example.test/token".into()),
            scope: None,
        }),
    };

    let result = ensure_xaa_tokens(
        "enterprise",
        "https://mcp.example.test",
        Some(&oauth),
        home.path(),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn ensure_xaa_tokens_errors_on_missing_idp_token_without_stored_tokens() {
    let home = tempfile::tempdir().unwrap();
    let oauth = McpOAuthConfig {
        client_id: Some("as-client".into()),
        xaa: Some(crate::types::McpXaaConfig {
            client_id: None,
            client_secret: Some("as-secret".into()),
            idp_client_id: Some("idp-client".into()),
            idp_client_secret: None,
            idp_id_token: None,
            idp_token_endpoint: Some("https://idp.example.test/token".into()),
            scope: None,
        }),
    };

    let err = ensure_xaa_tokens(
        "enterprise-missing",
        "https://mcp-missing.example.test",
        Some(&oauth),
        home.path(),
    )
    .await
    .expect_err("missing idp token should fail before exchange");
    assert!(err.to_string().contains("oauth.xaa.idpIdToken"));
}
