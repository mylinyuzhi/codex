use super::*;

fn test_env() -> EnvironmentInfo {
    EnvironmentInfo::builder()
        .cwd("/tmp/test")
        .model("test-model")
        .context_window(200000)
        .max_output_tokens(16384)
        .build()
        .unwrap()
}

#[test]
fn test_builder_minimal() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .build()
        .unwrap();

    assert_eq!(ctx.environment.model, "test-model");
    assert!(!ctx.has_tools());
    assert!(!ctx.has_mcp_servers());
    assert!(!ctx.is_subagent());
    assert_eq!(ctx.permission_mode, PermissionMode::Default);
}

#[test]
fn test_builder_full() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .tool_names(vec!["Read".to_string(), "Write".to_string()])
        .mcp_server_names(vec!["github".to_string()])
        .memory_files(vec![MemoryFile {
            path: "CLAUDE.md".to_string(),
            content: "instructions".to_string(),
            priority: 0,
        }])
        .permission_mode(PermissionMode::AcceptEdits)
        .subagent_type(SubagentType::Explore)
        .build()
        .unwrap();

    assert!(ctx.has_tools());
    assert!(ctx.has_mcp_servers());
    assert!(ctx.is_subagent());
    assert_eq!(ctx.subagent_type, Some(SubagentType::Explore));
    assert_eq!(ctx.permission_mode, PermissionMode::AcceptEdits);
    assert_eq!(ctx.memory_files.len(), 1);
}

#[test]
fn test_builder_missing_environment() {
    let result = ConversationContext::builder().build();
    assert!(result.is_err());
}

#[test]
fn test_subagent_type_display() {
    assert_eq!(SubagentType::Explore.to_string(), "explore");
    assert_eq!(SubagentType::Plan.to_string(), "plan");
}

#[test]
fn test_injection_position_serde() {
    let json = r#""before_tools""#;
    let pos: InjectionPosition = serde_json::from_str(json).unwrap();
    assert_eq!(pos, InjectionPosition::BeforeTools);
}
