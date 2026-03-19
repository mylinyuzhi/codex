use super::*;

#[test]
fn test_as_str_roundtrip() {
    for tool in ToolName::ALL {
        let s = tool.as_str();
        let parsed = ToolName::from_str(s);
        assert_eq!(parsed, Some(*tool), "Failed to roundtrip {tool:?}");
    }
}

#[test]
fn test_unknown_returns_none() {
    assert_eq!(ToolName::from_str("unknown_tool"), None);
    assert_eq!(ToolName::from_str("mcp_some_tool"), None);
}

#[test]
fn test_special_names() {
    // Shell is lowercase
    assert_eq!(ToolName::Shell.as_str(), "shell");
    assert_eq!(ToolName::from_str("shell"), Some(ToolName::Shell));

    // ApplyPatch is lowercase with underscore
    assert_eq!(ToolName::ApplyPatch.as_str(), "apply_patch");
    assert_eq!(
        ToolName::from_str("apply_patch"),
        Some(ToolName::ApplyPatch)
    );

    // MCPSearch has uppercase MCP
    assert_eq!(ToolName::McpSearch.as_str(), "MCPSearch");
    assert_eq!(ToolName::from_str("MCPSearch"), Some(ToolName::McpSearch));
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", ToolName::Read), "Read");
    assert_eq!(format!("{}", ToolName::Shell), "shell");
    assert_eq!(format!("{}", ToolName::ApplyPatch), "apply_patch");
}

#[test]
fn test_serde() {
    let json = serde_json::to_string(&ToolName::Read).unwrap();
    assert_eq!(json, "\"Read\"");

    let parsed: ToolName = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ToolName::Read);
}

#[test]
fn test_all_count() {
    // Ensure ALL contains all variants (36 tools)
    assert_eq!(ToolName::ALL.len(), 36);
}
