use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_tool_name_as_str_roundtrip() {
    assert_eq!(ToolName::Bash.as_str(), "Bash");
    assert_eq!(ToolName::Lsp.as_str(), "LSP");
    assert_eq!(ToolName::Repl.as_str(), "REPL");
    assert_eq!(
        ToolName::VerifyPlanExecution.as_str(),
        "VerifyPlanExecution"
    );
    assert_eq!(ToolName::from_str("Bash").unwrap(), ToolName::Bash);
    assert_eq!(ToolName::from_str("LSP").unwrap(), ToolName::Lsp);
    assert_eq!(
        ToolName::from_str("VerifyPlanExecution").unwrap(),
        ToolName::VerifyPlanExecution
    );
}

#[test]
fn test_tool_name_ts_wire_names() {
    // Wire names that mirror claude-code TS (`*_TOOL_NAME` consts).
    assert_eq!(ToolName::SendUserMessage.as_str(), "SendUserMessage");
    assert_eq!(ToolName::ListMcpResources.as_str(), "ListMcpResourcesTool");
    assert_eq!(ToolName::ReadMcpResource.as_str(), "ReadMcpResourceTool");
}

#[test]
fn test_tool_name_renamed_wire_names_parse() {
    assert_eq!(
        ToolName::from_str("SendUserMessage").unwrap(),
        ToolName::SendUserMessage
    );
    assert_eq!(
        ToolName::from_str("ListMcpResourcesTool").unwrap(),
        ToolName::ListMcpResources
    );
    assert_eq!(
        ToolName::from_str("ReadMcpResourceTool").unwrap(),
        ToolName::ReadMcpResource
    );
    // The old names are gone — no legacy alias parsing.
    assert!(ToolName::from_str("Brief").is_err());
    assert!(ToolName::from_str("ListMcpResources").is_err());
    assert!(ToolName::from_str("ReadMcpResource").is_err());
}

#[test]
fn test_tool_name_from_str_unknown() {
    assert!(ToolName::from_str("Unknown").is_err());
}

#[test]
fn test_tool_id_builtin() {
    let id: ToolId = "Read".parse().unwrap();
    assert_eq!(id, ToolId::Builtin(ToolName::Read));
    assert!(id.is_builtin());
    assert!(!id.is_mcp());
    assert_eq!(id.to_string(), "Read");
}

#[test]
fn test_tool_id_mcp() {
    let id: ToolId = "mcp__slack__send".parse().unwrap();
    assert_eq!(
        id,
        ToolId::Mcp {
            server: "slack".into(),
            tool: "send".into()
        }
    );
    assert!(id.is_mcp());
    assert_eq!(id.mcp_server(), Some("slack"));
    assert_eq!(id.to_string(), "mcp__slack__send");
}

#[test]
fn test_tool_id_custom() {
    let id: ToolId = "my_plugin_tool".parse().unwrap();
    assert_eq!(id, ToolId::Custom("my_plugin_tool".into()));
    assert!(!id.is_builtin());
    assert!(!id.is_mcp());
}

#[test]
fn test_tool_id_serde_roundtrip() {
    let ids = vec![
        ToolId::Builtin(ToolName::Bash),
        ToolId::Mcp {
            server: "github".into(),
            tool: "create_pr".into(),
        },
        ToolId::Custom("custom_tool".into()),
    ];
    for id in ids {
        let json = serde_json::to_string(&id).unwrap();
        let parsed: ToolId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }
}

#[test]
fn test_tool_id_from_tool_name() {
    let id = ToolId::from(ToolName::Edit);
    assert_eq!(id, ToolId::Builtin(ToolName::Edit));
}
