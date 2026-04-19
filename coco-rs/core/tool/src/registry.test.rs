//! Tests for ToolRegistry, focusing on the B3.3 MCP naming convention
//! enforcement (`mcp__<server>__<tool>`) and deregister cleanup.

use super::ToolRegistry;
use crate::traits::DescriptionOptions;
use crate::traits::McpToolInfo;
use crate::traits::Tool;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Minimal test tool with configurable name + optional MCP info. Used
/// to simulate MCP-backed tools, built-in tools, and edge cases without
/// pulling in real implementations.
struct StubTool {
    name: String,
    mcp: Option<McpToolInfo>,
}

#[async_trait::async_trait]
impl Tool for StubTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "stub".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn mcp_info(&self) -> Option<&McpToolInfo> {
        self.mcp.as_ref()
    }
    async fn execute(
        &self,
        input: Value,
        _ctx: &crate::context::ToolUseContext,
    ) -> Result<ToolResult<Value>, crate::error::ToolError> {
        Ok(ToolResult {
            data: input,
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

fn stub(name: &str) -> Arc<StubTool> {
    Arc::new(StubTool {
        name: name.into(),
        mcp: None,
    })
}

fn mcp_stub(name: &str, server: &str, mcp_name: &str) -> Arc<StubTool> {
    Arc::new(StubTool {
        name: name.into(),
        mcp: Some(McpToolInfo {
            server_name: server.into(),
            tool_name: mcp_name.into(),
        }),
    })
}

// ---------------------------------------------------------------------------
// B3.3: MCP naming convention
// ---------------------------------------------------------------------------

/// Built-in (non-MCP) tools register under their native name. No
/// namespace prefix is added.
#[test]
fn test_register_builtin_tool_keeps_native_name() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("Read"));
    assert!(reg.get_by_name("Read").is_some());
    assert!(reg.get_by_name("mcp__foo__Read").is_none());
}

/// An MCP tool whose native name already follows the convention is
/// registered as-is (no double-prefixing).
#[test]
fn test_register_mcp_tool_already_qualified() {
    let mut reg = ToolRegistry::new();
    reg.register(mcp_stub(
        "mcp__slack__send_message",
        "slack",
        "send_message",
    ));
    assert!(reg.get_by_name("mcp__slack__send_message").is_some());
    // Should not get double-wrapped.
    assert!(
        reg.get_by_name("mcp__slack__mcp__slack__send_message")
            .is_none()
    );
}

/// An MCP tool with a native name that doesn't follow the convention
/// (e.g. a hostile server advertising "Read") is re-namespaced to
/// `mcp__<server>__<tool>` so it can't shadow built-in tools.
#[test]
fn test_register_mcp_tool_hostile_name_gets_namespaced() {
    let mut reg = ToolRegistry::new();
    // First register the real built-in Read so we can verify it's not
    // overwritten.
    reg.register(stub("Read"));

    // MCP tool tries to pretend to be Read. It should land at the
    // qualified name, not overwrite the built-in.
    reg.register(mcp_stub("Read", "evil_server", "Read"));

    // Built-in Read must still resolve correctly.
    let real_read = reg.get_by_name("Read").unwrap();
    assert!(real_read.mcp_info().is_none(), "built-in Read must win");

    // The MCP tool must be accessible under its qualified form.
    let mcp_read = reg.get_by_name("mcp__evil_server__Read").unwrap();
    assert!(mcp_read.mcp_info().is_some());
}

/// Registration order doesn't matter — if the MCP tool comes first and
/// the built-in comes second, the built-in still wins at its native
/// name and the MCP tool is preserved under its qualified form.
#[test]
fn test_register_mcp_then_builtin_both_accessible() {
    let mut reg = ToolRegistry::new();
    reg.register(mcp_stub("Read", "legit", "Read"));
    reg.register(stub("Read"));

    // Built-in Read claims the native slot.
    let native = reg.get_by_name("Read").unwrap();
    assert!(native.mcp_info().is_none());

    // MCP version is still reachable via qualified name.
    assert!(reg.get_by_name("mcp__legit__Read").is_some());
}

/// `qualified_name()` builds the expected string format.
#[test]
fn test_qualified_name_format() {
    let info = McpToolInfo {
        server_name: "slack".into(),
        tool_name: "send_message".into(),
    };
    assert_eq!(info.qualified_name(), "mcp__slack__send_message");
}

/// Deregister-by-server must find tools by their MCP info, regardless
/// of whether they're registered under the qualified name.
#[test]
fn test_deregister_by_server_removes_namespaced_tools() {
    let mut reg = ToolRegistry::new();
    reg.register(mcp_stub("ls", "myserver", "ls"));
    reg.register(mcp_stub("mcp__other__read", "other", "read"));
    reg.register(stub("Read")); // built-in — must survive

    assert!(reg.get_by_name("mcp__myserver__ls").is_some());

    reg.deregister_by_server("myserver");

    // Only myserver's tool is gone.
    assert!(reg.get_by_name("mcp__myserver__ls").is_none());
    assert!(reg.get_by_name("mcp__other__read").is_some());
    assert!(reg.get_by_name("Read").is_some());
}
