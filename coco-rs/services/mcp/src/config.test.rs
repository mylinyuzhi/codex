use super::*;

#[test]
fn test_parse_stdio_config() {
    let json = serde_json::json!({
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem"],
        "env": {"HOME": "/tmp"}
    });
    let config = parse_server_config(&json).unwrap();
    assert!(matches!(config, McpServerConfig::Stdio(_)));
    if let McpServerConfig::Stdio(stdio) = config {
        assert_eq!(stdio.command, "npx");
        assert_eq!(stdio.args.len(), 2);
        assert_eq!(stdio.env.get("HOME").unwrap(), "/tmp");
    }
}

#[test]
fn test_parse_stdio_with_cwd() {
    let json = serde_json::json!({
        "command": "node",
        "args": ["server.js"],
        "cwd": "/opt/mcp-server"
    });
    let config = parse_server_config(&json).unwrap();
    if let McpServerConfig::Stdio(stdio) = config {
        assert_eq!(stdio.cwd, Some(PathBuf::from("/opt/mcp-server")));
    }
}

#[test]
fn test_parse_sse_config() {
    let json = serde_json::json!({
        "url": "https://mcp.example.com/sse",
        "headers": {"Authorization": "Bearer token"}
    });
    let config = parse_server_config(&json).unwrap();
    assert!(matches!(config, McpServerConfig::Sse(_)));
}

#[test]
fn test_parse_http_config() {
    let json = serde_json::json!({
        "url": "https://mcp.example.com/api",
        "transport": "http",
        "headers": {"X-Api-Key": "key123"}
    });
    let config = parse_server_config(&json).unwrap();
    assert!(matches!(config, McpServerConfig::Http(_)));
    if let McpServerConfig::Http(http) = config {
        assert_eq!(http.url, "https://mcp.example.com/api");
        assert_eq!(http.headers.get("X-Api-Key").unwrap(), "key123");
    }
}

#[test]
fn test_parse_invalid_config() {
    let json = serde_json::json!({"invalid": true});
    assert!(parse_server_config(&json).is_none());
}

#[test]
fn test_parse_disabled_server_returns_none() {
    let json = serde_json::json!({
        "command": "npx",
        "args": ["server"],
        "disabled": true
    });
    assert!(parse_server_config(&json).is_none());
}

#[test]
fn test_load_deduplicates_by_name() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    // Write project .mcp.json
    std::fs::write(
        project_dir.join(".mcp.json"),
        serde_json::json!({
            "mcpServers": {
                "server1": {"command": "project-server", "args": []}
            }
        })
        .to_string(),
    )
    .unwrap();

    // Write user mcp.json (in config_home)
    let config_home = tmp.path().join("config");
    std::fs::create_dir_all(&config_home).unwrap();
    std::fs::write(
        config_home.join("mcp.json"),
        serde_json::json!({
            "mcpServers": {
                "server1": {"command": "user-server", "args": []}
            }
        })
        .to_string(),
    )
    .unwrap();

    let configs = McpConfigLoader::load(&project_dir, &config_home);
    // User scope loads after project, so user wins (later overrides earlier)
    assert_eq!(configs.len(), 1);
    let server = &configs[0];
    assert_eq!(server.name, "server1");
    assert_eq!(server.scope, ConfigScope::User);
    if let McpServerConfig::Stdio(stdio) = &server.config {
        assert_eq!(stdio.command, "user-server");
    }
}
