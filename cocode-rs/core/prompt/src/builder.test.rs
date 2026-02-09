use cocode_context::ContextInjection;
use cocode_context::EnvironmentInfo;
use cocode_context::InjectionPosition;
use cocode_context::MemoryFile;
use cocode_protocol::PermissionMode;

use super::*;

fn test_env() -> EnvironmentInfo {
    EnvironmentInfo::builder()
        .platform("darwin")
        .os_version("Darwin 24.0.0")
        .cwd("/home/user/project")
        .is_git_repo(true)
        .git_branch("main")
        .date("2025-01-29")
        .model("claude-3-opus")
        .context_window(200000)
        .max_output_tokens(16384)
        .build()
        .unwrap()
}

#[test]
fn test_build_minimal() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);

    // Should contain identity and environment
    assert!(prompt.contains("Identity"));
    assert!(prompt.contains("darwin"));
    assert!(prompt.contains("2025-01-29"));
    // Should NOT contain tool policy (no tools)
    assert!(!prompt.contains("Tool Usage Policy"));
    // Should NOT contain MCP instructions (no MCP servers)
    assert!(!prompt.contains("MCP Server Instructions"));
}

#[test]
fn test_build_with_tools() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .tool_names(vec!["Read".to_string(), "Write".to_string()])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("Tool Usage Policy"));
}

#[test]
fn test_build_with_mcp() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .mcp_server_names(vec!["github".to_string()])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("MCP Server Instructions"));
}

#[test]
fn test_build_with_memory_files() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .memory_files(vec![MemoryFile {
            path: "CLAUDE.md".to_string(),
            content: "Use Rust conventions".to_string(),
            priority: 0,
        }])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("CLAUDE.md"));
    assert!(prompt.contains("Use Rust conventions"));
}

#[test]
fn test_build_permission_modes() {
    for mode in &[
        PermissionMode::Default,
        PermissionMode::Plan,
        PermissionMode::AcceptEdits,
        PermissionMode::Bypass,
    ] {
        let ctx = ConversationContext::builder()
            .environment(test_env())
            .permission_mode(*mode)
            .build()
            .unwrap();

        let prompt = SystemPromptBuilder::build(&ctx);
        assert!(prompt.contains("Permission Mode"));
    }
}

#[test]
fn test_build_with_injections() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .injections(vec![ContextInjection {
            label: "custom-hook".to_string(),
            content: "Hook output here".to_string(),
            position: InjectionPosition::EndOfPrompt,
        }])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("Hook output here"));
}

#[test]
fn test_build_for_subagent_explore() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build_for_subagent(&ctx, SubagentType::Explore);
    assert!(prompt.contains("Explore Subagent"));
    assert!(prompt.contains("darwin"));
    assert!(!prompt.contains("Plan Subagent"));
}

#[test]
fn test_build_for_subagent_plan() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build_for_subagent(&ctx, SubagentType::Plan);
    assert!(prompt.contains("Plan Subagent"));
    assert!(!prompt.contains("Explore Subagent"));
}

#[test]
fn test_build_summarization() {
    let (system, user) = SystemPromptBuilder::build_summarization("conversation content", None);
    assert!(!system.is_empty());
    assert!(user.contains("conversation content"));
}

#[test]
fn test_build_brief_summarization() {
    let (system, user) = SystemPromptBuilder::build_brief_summarization("brief content");
    assert!(!system.is_empty());
    assert!(user.contains("brief content"));
}

#[test]
fn test_build_with_tools_includes_dynamic_policy() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .tool_names(vec![
            "Read".to_string(),
            "Edit".to_string(),
            "LS".to_string(),
        ])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("Use Read for reading files"));
    assert!(prompt.contains("Use Edit for modifying files"));
    assert!(prompt.contains("Use LS for directory listing"));
    assert!(!prompt.contains("Use Grep"));
}

#[test]
fn test_build_with_tools_excludes_ls_when_not_registered() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .tool_names(vec!["Read".to_string(), "Edit".to_string()])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);
    assert!(prompt.contains("Use Read for reading files"));
    assert!(prompt.contains("Use Edit for modifying files"));
    assert!(!prompt.contains("Use LS for directory listing"));
}

#[test]
fn test_section_ordering() {
    let ctx = ConversationContext::builder()
        .environment(test_env())
        .tool_names(vec!["Read".to_string()])
        .mcp_server_names(vec!["github".to_string()])
        .memory_files(vec![MemoryFile {
            path: "CLAUDE.md".to_string(),
            content: "rules".to_string(),
            priority: 0,
        }])
        .build()
        .unwrap();

    let prompt = SystemPromptBuilder::build(&ctx);

    // Verify ordering: Identity before ToolPolicy before Security before Environment
    let identity_pos = prompt.find("# Identity").unwrap();
    let tool_pos = prompt.find("# Tool Usage Policy").unwrap();
    let security_pos = prompt.find("# Security Guidelines").unwrap();
    let env_pos = prompt.find("# Environment").unwrap();
    let permission_pos = prompt.find("# Permission Mode").unwrap();
    let memory_pos = prompt.find("# Memory Files").unwrap();

    assert!(identity_pos < tool_pos);
    assert!(tool_pos < security_pos);
    assert!(security_pos < env_pos);
    assert!(env_pos < permission_pos);
    assert!(permission_pos < memory_pos);
}
