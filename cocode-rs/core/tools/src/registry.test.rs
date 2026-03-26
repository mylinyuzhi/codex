use super::*;
use crate::context::ToolContext;
use crate::error::Result;
use async_trait::async_trait;
use cocode_protocol::ToolName;
use cocode_protocol::ToolOutput;

struct TestTool {
    name: String,
}

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Test tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &mut ToolContext,
    ) -> Result<ToolOutput> {
        Ok(ToolOutput {
            content: cocode_protocol::ToolResultContent::Text("ok".to_string()),
            is_error: false,
            modifiers: Vec::new(),
            images: Vec::new(),
        })
    }
}

struct GatedTool {
    name: String,
    gate: cocode_protocol::Feature,
}

#[async_trait]
impl Tool for GatedTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Gated tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(self.gate)
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &mut ToolContext,
    ) -> Result<ToolOutput> {
        Ok(ToolOutput {
            content: cocode_protocol::ToolResultContent::Text("ok".to_string()),
            is_error: false,
            modifiers: Vec::new(),
            images: Vec::new(),
        })
    }
}

#[test]
fn test_register_and_get() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "test".to_string(),
    });

    assert!(registry.has("test"));
    assert!(registry.get("test").is_some());
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn test_alias() {
    let mut registry = ToolRegistry::new();
    registry.register_with_alias(
        TestTool {
            name: "read_file".to_string(),
        },
        ToolName::Read.as_str(),
    );

    assert!(registry.has("read_file"));
    assert!(registry.has(ToolName::Read.as_str()));
    assert!(registry.get(ToolName::Read.as_str()).is_some());
}

#[test]
fn test_mcp_tools() {
    let mut registry = ToolRegistry::new();

    let tools = vec![
        McpToolInfo {
            server: "".to_string(),
            name: "tool1".to_string(),
            description: Some("Tool 1".to_string()),
            input_schema: serde_json::json!({}),
        },
        McpToolInfo {
            server: "".to_string(),
            name: "tool2".to_string(),
            description: None,
            input_schema: serde_json::json!({}),
        },
    ];

    registry.register_mcp_server("myserver", tools);

    assert!(registry.is_mcp_tool("mcp__myserver__tool1"));
    assert!(registry.is_mcp_tool("mcp__myserver__tool2"));
    assert!(!registry.is_mcp_tool("tool1"));

    // Unregister
    registry.unregister_mcp_server("myserver");
    assert!(!registry.is_mcp_tool("mcp__myserver__tool1"));
}

#[test]
fn test_all_definitions() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "tool1".to_string(),
    });
    registry.register(TestTool {
        name: "tool2".to_string(),
    });

    let defs = registry.all_definitions();
    assert_eq!(defs.len(), 2);
}

#[test]
fn test_tool_names() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "beta".to_string(),
    });
    registry.register(TestTool {
        name: "alpha".to_string(),
    });

    let names = registry.tool_names();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn test_mcp_description_chars() {
    let mut registry = ToolRegistry::new();

    // Empty registry should return 0
    assert_eq!(registry.mcp_description_chars(), 0);

    let tools = vec![McpToolInfo {
        server: "".to_string(),
        name: "tool1".to_string(),
        description: Some("A test tool".to_string()),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    registry.register_mcp_server("srv", tools);

    let chars = registry.mcp_description_chars();
    assert!(chars > 0);
}

#[test]
fn test_should_enable_auto_search() {
    let mut registry = ToolRegistry::new();
    let config = cocode_protocol::McpAutoSearchConfig::default();

    // No MCP tools => should not enable
    assert!(!registry.should_enable_auto_search(200_000, &config));

    // Add many MCP tools with large descriptions to exceed threshold
    // Threshold for 200k context: 0.1 * 200000 * 2.5 = 50000 chars
    let large_desc = "x".repeat(5000);
    let tools: Vec<McpToolInfo> = (0..15)
        .map(|i| McpToolInfo {
            server: "".to_string(),
            name: format!("tool_{i}"),
            description: Some(large_desc.clone()),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        })
        .collect();
    registry.register_mcp_server("big_server", tools);

    // Should exceed 50k chars threshold
    assert!(registry.mcp_description_chars() >= 50000);
    assert!(registry.should_enable_auto_search(200_000, &config));
}

#[test]
fn test_mcp_tool_snapshot() {
    let mut registry = ToolRegistry::new();

    let tools = vec![
        McpToolInfo {
            server: "".to_string(),
            name: "tool_a".to_string(),
            description: Some("Tool A".to_string()),
            input_schema: serde_json::json!({}),
        },
        McpToolInfo {
            server: "".to_string(),
            name: "tool_b".to_string(),
            description: Some("Tool B".to_string()),
            input_schema: serde_json::json!({}),
        },
    ];
    registry.register_mcp_server("srv", tools);

    let snapshot = registry.mcp_tool_snapshot();
    assert_eq!(snapshot.len(), 2);
    // All entries should have server set to "srv"
    for info in &snapshot {
        assert_eq!(info.server, "srv");
    }
}

#[test]
fn test_defer_mcp_tool_definitions() {
    let mut registry = ToolRegistry::new();

    // Register a builtin tool
    registry.register(TestTool {
        name: "builtin".to_string(),
    });

    // Register MCP tools (info only)
    let tools = vec![McpToolInfo {
        server: "".to_string(),
        name: "mcp_tool".to_string(),
        description: Some("An MCP tool".to_string()),
        input_schema: serde_json::json!({}),
    }];
    registry.register_mcp_server("srv", tools);

    // Also put a matching entry in the tools map to simulate executable registration
    registry.register(TestTool {
        name: "mcp__srv__mcp_tool".to_string(),
    });

    assert!(registry.get("mcp__srv__mcp_tool").is_some());

    let deferred = registry.defer_mcp_tool_definitions();
    assert!(deferred.contains(&"mcp__srv__mcp_tool".to_string()));

    // Tool should be removed from executable set
    assert!(registry.get("mcp__srv__mcp_tool").is_none());

    // But metadata should still be available
    assert!(registry.is_mcp_tool("mcp__srv__mcp_tool"));

    // Builtin tool should not be affected
    assert!(registry.get("builtin").is_some());
}

// =========================================================================
// P22: Case-insensitive tool name fallback
// =========================================================================

#[test]
fn test_case_insensitive_tool_lookup() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "bash".to_string(),
    });

    // Exact match
    assert!(registry.get("bash").is_some());
    // Wrong case from model
    assert!(registry.get(ToolName::Bash.as_str()).is_some());
    assert!(registry.get("BASH").is_some());
    assert!(registry.get("bAsH").is_some());
    // Non-existent
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn test_case_insensitive_alias_lookup() {
    let mut registry = ToolRegistry::new();
    registry.register_with_alias(
        TestTool {
            name: "read_file".to_string(),
        },
        ToolName::Read.as_str(),
    );

    // Exact alias match
    assert!(registry.get(ToolName::Read.as_str()).is_some());
    // Case-insensitive alias
    assert!(registry.get("read").is_some());
    assert!(registry.get("READ").is_some());
}

#[test]
fn test_exact_match_preferred_over_case_insensitive() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: ToolName::Bash.as_str().to_string(),
    });
    registry.register(TestTool {
        name: "bash".to_string(),
    });

    // Exact match should be preferred
    let tool = registry.get("bash").unwrap();
    assert_eq!(tool.name(), "bash");

    let tool = registry.get(ToolName::Bash.as_str()).unwrap();
    assert_eq!(tool.name(), ToolName::Bash.as_str());
}

#[test]
fn test_restore_deferred_tools() {
    let mut registry = ToolRegistry::new();

    // Register a builtin tool
    registry.register(TestTool {
        name: "builtin".to_string(),
    });

    // Register MCP tools (info only) + executable entry
    let tools = vec![
        McpToolInfo {
            server: "".to_string(),
            name: "tool_a".to_string(),
            description: Some("Tool A".to_string()),
            input_schema: serde_json::json!({}),
        },
        McpToolInfo {
            server: "".to_string(),
            name: "tool_b".to_string(),
            description: Some("Tool B".to_string()),
            input_schema: serde_json::json!({}),
        },
    ];
    registry.register_mcp_server("srv", tools);

    // Simulate executable registration
    registry.register(TestTool {
        name: "mcp__srv__tool_a".to_string(),
    });
    registry.register(TestTool {
        name: "mcp__srv__tool_b".to_string(),
    });

    // Defer all MCP tools
    let deferred = registry.defer_mcp_tool_definitions();
    assert_eq!(deferred.len(), 2);
    assert!(registry.get("mcp__srv__tool_a").is_none());
    assert!(registry.get("mcp__srv__tool_b").is_none());

    // Restore only tool_a
    let restored = registry.restore_deferred_tools(&["mcp__srv__tool_a".to_string()]);
    assert_eq!(restored, vec!["mcp__srv__tool_a"]);
    assert!(registry.get("mcp__srv__tool_a").is_some());
    assert!(registry.get("mcp__srv__tool_b").is_none());

    // Restore tool_b
    let restored = registry.restore_deferred_tools(&["mcp__srv__tool_b".to_string()]);
    assert_eq!(restored, vec!["mcp__srv__tool_b"]);
    assert!(registry.get("mcp__srv__tool_b").is_some());

    // Restoring already-restored tool is a no-op
    let restored = registry.restore_deferred_tools(&["mcp__srv__tool_a".to_string()]);
    assert!(restored.is_empty());

    // Builtin not affected
    assert!(registry.get("builtin").is_some());
}

#[test]
fn test_restore_deferred_tools_nonexistent() {
    let mut registry = ToolRegistry::new();

    // Restoring from an empty deferred set returns nothing
    let restored = registry.restore_deferred_tools(&["mcp__srv__no_such_tool".to_string()]);
    assert!(restored.is_empty());
}

#[test]
fn test_definitions_filtered_excludes_disabled_gate() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "always_on".to_string(),
    });
    registry.register(GatedTool {
        name: "ls_tool".to_string(),
        gate: cocode_protocol::Feature::Ls,
    });

    // Ls disabled → gated tool excluded
    let mut features = cocode_protocol::Features::with_defaults();
    features.disable(cocode_protocol::Feature::Ls);
    let defs = registry.definitions_filtered(&features);
    assert!(defs.iter().any(|d| d.name == "always_on"));
    assert!(defs.iter().all(|d| d.name != "ls_tool"));
}

#[test]
fn test_no_duplicate_mcp_definitions() {
    let mut registry = ToolRegistry::new();

    // Register MCP tools (info only)
    let tools = vec![McpToolInfo {
        server: "".to_string(),
        name: "mcp_tool".to_string(),
        description: Some("An MCP tool".to_string()),
        input_schema: serde_json::json!({}),
    }];
    registry.register_mcp_server("srv", tools);

    // Also put a matching executable entry in self.tools (simulates register_mcp_tools_executable)
    registry.register(TestTool {
        name: "mcp__srv__mcp_tool".to_string(),
    });

    // definitions_filtered should not duplicate: one from tools, zero from mcp_tools
    let features = cocode_protocol::Features::with_defaults();
    let defs = registry.definitions_filtered(&features);
    let mcp_count = defs
        .iter()
        .filter(|d| d.name == "mcp__srv__mcp_tool")
        .count();
    assert_eq!(mcp_count, 1, "MCP tool should appear exactly once");

    // all_definitions should also not duplicate
    let all_defs = registry.all_definitions();
    let mcp_count = all_defs
        .iter()
        .filter(|d| d.name == "mcp__srv__mcp_tool")
        .count();
    assert_eq!(
        mcp_count, 1,
        "MCP tool should appear exactly once in all_definitions"
    );

    // len() should count it once
    assert_eq!(registry.len(), 1);

    // tool_names() should list it once
    let names = registry.tool_names();
    assert_eq!(
        names.iter().filter(|n| *n == "mcp__srv__mcp_tool").count(),
        1,
    );
}

#[test]
fn test_definitions_filtered_includes_enabled_gate() {
    let mut registry = ToolRegistry::new();
    registry.register(TestTool {
        name: "always_on".to_string(),
    });
    registry.register(GatedTool {
        name: "ls_tool".to_string(),
        gate: cocode_protocol::Feature::Ls,
    });

    // Ls enabled → gated tool included
    let features = cocode_protocol::Features::with_defaults(); // Ls is default enabled
    let defs = registry.definitions_filtered(&features);
    assert!(defs.iter().any(|d| d.name == "always_on"));
    assert!(defs.iter().any(|d| d.name == "ls_tool"));
}

#[test]
fn test_mcp_server_registration_populates_metadata() {
    let mut registry = ToolRegistry::new();

    let tools = vec![McpToolInfo {
        server: "".to_string(),
        name: "search".to_string(),
        description: Some("Search for items".to_string()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        }),
    }];
    registry.register_mcp_server("my_server", tools);

    let info = registry
        .get_mcp("mcp__my_server__search")
        .expect("MCP tool should be registered");
    assert_eq!(info.server, "my_server");
    assert_eq!(info.name, "search");
    assert_eq!(info.description, Some("Search for items".to_string()),);
    assert_eq!(info.input_schema["properties"]["query"]["type"], "string",);
    assert_eq!(info.input_schema["required"][0], "query");
}
