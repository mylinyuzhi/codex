use super::*;
use std::collections::HashMap;

#[test]
fn test_mcp_server_stdio() {
    let config = McpServerConfig {
        name: "file-server".to_string(),
        description: Some("File system MCP server".to_string()),
        transport: McpTransport::Stdio {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@anthropic/file-server".to_string()],
        },
        env: HashMap::new(),
        auto_start: true,
    };

    assert_eq!(config.name, "file-server");
    assert!(config.auto_start);

    if let McpTransport::Stdio { command, args } = &config.transport {
        assert_eq!(command, "npx");
        assert_eq!(args.len(), 2);
    } else {
        panic!("Expected Stdio transport");
    }
}

#[test]
fn test_mcp_server_http() {
    let config = McpServerConfig {
        name: "remote-server".to_string(),
        description: None,
        transport: McpTransport::Http {
            url: "http://localhost:3000".to_string(),
        },
        env: HashMap::new(),
        auto_start: false,
    };

    if let McpTransport::Http { url } = &config.transport {
        assert_eq!(url, "http://localhost:3000");
    } else {
        panic!("Expected Http transport");
    }
}

#[test]
fn test_mcp_server_serialize_deserialize() {
    let config = McpServerConfig {
        name: "test-server".to_string(),
        description: Some("A test server".to_string()),
        transport: McpTransport::Stdio {
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
        },
        env: {
            let mut map = HashMap::new();
            map.insert("NODE_ENV".to_string(), "production".to_string());
            map
        },
        auto_start: true,
    };

    let json_str = serde_json::to_string(&config).expect("serialize");
    let back: McpServerConfig = serde_json::from_str(&json_str).expect("deserialize");

    assert_eq!(back.name, "test-server");
    assert_eq!(back.env.get("NODE_ENV"), Some(&"production".to_string()));
}

#[test]
fn test_mcp_server_from_json() {
    let json_str = r#"{
  "name": "filesystem",
  "description": "Provides file system access",
  "auto_start": true,
  "transport": {
    "type": "stdio",
    "command": "npx",
    "args": ["-y", "@anthropic/mcp-server-filesystem"]
  },
  "env": {
    "MCP_DEBUG": "true"
  }
}"#;

    let config: McpServerConfig = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(config.name, "filesystem");
    assert_eq!(
        config.description,
        Some("Provides file system access".to_string())
    );
    assert!(config.auto_start);
    assert_eq!(config.env.get("MCP_DEBUG"), Some(&"true".to_string()));

    if let McpTransport::Stdio { command, args } = &config.transport {
        assert_eq!(command, "npx");
        assert_eq!(args.len(), 2);
    } else {
        panic!("Expected Stdio transport");
    }
}

#[test]
fn test_resolve_variables_plugin_root() {
    let mut config = McpServerConfig {
        name: "test".to_string(),
        description: None,
        transport: McpTransport::Stdio {
            command: "${COCODE_PLUGIN_ROOT}/bin/server".to_string(),
            args: vec![
                "--config".to_string(),
                "${COCODE_PLUGIN_ROOT}/config.json".to_string(),
            ],
        },
        env: {
            let mut map = HashMap::new();
            map.insert(
                "PLUGIN_DIR".to_string(),
                "${COCODE_PLUGIN_ROOT}".to_string(),
            );
            map
        },
        auto_start: true,
    };

    config.resolve_variables(std::path::Path::new("/plugins/my-plugin"), None);

    if let McpTransport::Stdio { command, args } = &config.transport {
        assert_eq!(command, "/plugins/my-plugin/bin/server");
        assert_eq!(args[1], "/plugins/my-plugin/config.json");
    } else {
        panic!("Expected Stdio transport");
    }
    assert_eq!(
        config.env.get("PLUGIN_DIR"),
        Some(&"/plugins/my-plugin".to_string())
    );
}

#[test]
fn test_resolve_variables_env() {
    // SAFETY: test-only, single-threaded test environment.
    unsafe { std::env::set_var("COCODE_TEST_VAR_12345", "hello-world") };

    let mut config = McpServerConfig {
        name: "test".to_string(),
        description: None,
        transport: McpTransport::Stdio {
            command: "node".to_string(),
            args: vec!["--token=${env.COCODE_TEST_VAR_12345}".to_string()],
        },
        env: HashMap::new(),
        auto_start: true,
    };

    config.resolve_variables(std::path::Path::new("/tmp"), None);

    if let McpTransport::Stdio { args, .. } = &config.transport {
        assert_eq!(args[0], "--token=hello-world");
    }

    // SAFETY: test-only, single-threaded test environment.
    unsafe { std::env::remove_var("COCODE_TEST_VAR_12345") };
}

#[test]
fn test_resolve_variables_http_url() {
    let mut config = McpServerConfig {
        name: "test".to_string(),
        description: None,
        transport: McpTransport::Http {
            url: "http://${env.MCP_HOST_ABSENT}:8080".to_string(),
        },
        env: HashMap::new(),
        auto_start: true,
    };

    config.resolve_variables(std::path::Path::new("/tmp"), None);

    if let McpTransport::Http { url } = &config.transport {
        // Missing env var resolves to empty string
        assert_eq!(url, "http://:8080");
    }
}

#[test]
fn test_resolve_variables_user_config() {
    let mut config = McpServerConfig {
        name: "test".to_string(),
        description: None,
        transport: McpTransport::Stdio {
            command: "server".to_string(),
            args: vec!["--api-key=${user_config.api_key}".to_string()],
        },
        env: HashMap::new(),
        auto_start: true,
    };

    let mut user_config = HashMap::new();
    user_config.insert(
        "api_key".to_string(),
        serde_json::Value::String("sk-123".to_string()),
    );

    config.resolve_variables(std::path::Path::new("/tmp"), Some(&user_config));

    if let McpTransport::Stdio { args, .. } = &config.transport {
        assert_eq!(args[0], "--api-key=sk-123");
    }
}

#[test]
fn test_resolve_variables_no_patterns() {
    let mut config = McpServerConfig {
        name: "test".to_string(),
        description: None,
        transport: McpTransport::Stdio {
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
        },
        env: HashMap::new(),
        auto_start: true,
    };

    config.resolve_variables(std::path::Path::new("/tmp"), None);

    if let McpTransport::Stdio { command, args } = &config.transport {
        assert_eq!(command, "node");
        assert_eq!(args[0], "server.js");
    }
}

#[test]
fn test_mcp_server_defaults() {
    let json_str = r#"{
  "name": "minimal",
  "transport": {
    "type": "http",
    "url": "http://localhost:8080"
  }
}"#;

    let config: McpServerConfig = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(config.name, "minimal");
    assert!(config.description.is_none());
    assert!(config.auto_start); // Default is true
    assert!(config.env.is_empty());
}
