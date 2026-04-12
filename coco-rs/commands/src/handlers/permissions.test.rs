use super::*;

#[tokio::test]
async fn test_list_permissions_output() {
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Permission Rules"));
    assert!(output.contains("Session"));
    assert!(output.contains("Project"));
    assert!(output.contains("User"));
    assert!(output.contains("highest to lowest priority"));
}

#[tokio::test]
async fn test_allow_known_tool() {
    let output = handler("allow Bash".to_string()).await.unwrap();
    assert!(output.contains("allow rule"));
    assert!(output.contains("Bash"));
    assert!(!output.contains("Warning"));
}

#[tokio::test]
async fn test_allow_unknown_tool() {
    let output = handler("allow CustomTool".to_string()).await.unwrap();
    assert!(output.contains("allow rule"));
    assert!(output.contains("Warning"));
    assert!(output.contains("not a known built-in tool"));
}

#[tokio::test]
async fn test_deny_tool() {
    let output = handler("deny Write".to_string()).await.unwrap();
    assert!(output.contains("deny rule"));
    assert!(output.contains("Write"));
}

#[tokio::test]
async fn test_reset_permissions() {
    let output = handler("reset".to_string()).await.unwrap();
    assert!(output.contains("cleared"));
    assert!(output.contains("File-based rules"));
}

#[tokio::test]
async fn test_unknown_subcommand() {
    let output = handler("foobar".to_string()).await.unwrap();
    assert!(output.contains("Unknown permissions subcommand"));
    assert!(output.contains("Usage"));
}

#[tokio::test]
async fn test_read_permission_rules_from_json() {
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.json");
    let settings = serde_json::json!({
        "permissions": {
            "allow": ["Bash", "Read"],
            "deny": ["Write"]
        },
        "allowedTools": ["Glob", "Grep"],
    });
    tokio::fs::write(&settings_path, serde_json::to_string(&settings).unwrap())
        .await
        .unwrap();

    let rules = read_permission_rules_from_path(&settings_path).await;
    assert!(!rules.is_empty());
    // Should contain entries from both "permissions" and "allowedTools"
    assert!(rules.iter().any(|r| r.contains("Bash")));
    assert!(rules.iter().any(|r| r.contains("Glob")));
}

#[tokio::test]
async fn test_read_permission_rules_nonexistent() {
    let rules = read_permission_rules_from_path(Path::new("/tmp/nonexistent_perms.json")).await;
    assert!(rules.is_empty());
}

#[tokio::test]
async fn test_allow_mcp_tool() {
    let output = handler("allow mcp__myserver__tool".to_string())
        .await
        .unwrap();
    assert!(output.contains("allow rule"));
    assert!(
        !output.contains("Warning"),
        "MCP tools should be recognized as valid"
    );
}
