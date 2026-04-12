use std::str::FromStr;

use coco_types::AgentTypeId;
use coco_types::SubagentType;
use pretty_assertions::assert_eq;

use super::*;

// ── AgentColorManager tests ──

#[tokio::test]
async fn test_color_assignment_unique() {
    let mgr = AgentColorManager::new();
    let c1 = mgr.get_or_assign("researcher").await;
    let c2 = mgr.get_or_assign("writer").await;
    let c3 = mgr.get_or_assign("coder").await;

    assert!(c1.is_some());
    assert!(c2.is_some());
    assert!(c3.is_some());
    assert_ne!(c1, c2);
    assert_ne!(c2, c3);
    assert_ne!(c1, c3);
}

#[tokio::test]
async fn test_color_assignment_stable() {
    let mgr = AgentColorManager::new();
    let c1 = mgr.get_or_assign("researcher").await;
    let c2 = mgr.get_or_assign("researcher").await;
    assert_eq!(c1, c2);
}

#[tokio::test]
async fn test_general_purpose_no_color() {
    let mgr = AgentColorManager::new();
    assert!(mgr.get_or_assign("general-purpose").await.is_none());
}

#[tokio::test]
async fn test_color_removal() {
    let mgr = AgentColorManager::new();
    let _ = mgr.get_or_assign("researcher").await;
    mgr.remove("researcher").await;
    let c2 = mgr.get_or_assign("researcher").await;
    assert!(c2.is_some());
}

#[tokio::test]
async fn test_color_wraps_around() {
    let mgr = AgentColorManager::new();
    for i in 0..AgentColor::ALL.len() + 2 {
        let color = mgr.get_or_assign(&format!("agent-{i}")).await;
        assert!(color.is_some());
    }
}

// ── filter_tools_for_agent tests ──

#[test]
fn test_filter_removes_disallowed() {
    let tools = vec![
        "Bash".to_string(),
        "Read".to_string(),
        "TeamCreate".to_string(),
        "SendMessage".to_string(),
    ];
    let filtered =
        filter_tools_for_agent(&tools, /*is_builtin*/ true, /*is_async*/ false);
    assert!(filtered.contains(&"Bash".to_string()));
    assert!(filtered.contains(&"Read".to_string()));
    assert!(!filtered.contains(&"TeamCreate".to_string()));
    assert!(!filtered.contains(&"SendMessage".to_string()));
}

#[test]
fn test_filter_custom_agent_extra_restrictions() {
    let tools = vec!["Bash".to_string(), "AskUserQuestion".to_string()];
    let builtin_filtered =
        filter_tools_for_agent(&tools, /*is_builtin*/ true, /*is_async*/ false);
    assert!(builtin_filtered.contains(&"AskUserQuestion".to_string()));

    let custom_filtered =
        filter_tools_for_agent(&tools, /*is_builtin*/ false, /*is_async*/ false);
    assert!(!custom_filtered.contains(&"AskUserQuestion".to_string()));
}

#[test]
fn test_filter_async_allowlist() {
    let tools = vec!["Bash".to_string(), "Read".to_string(), "Config".to_string()];
    let filtered = filter_tools_for_agent(&tools, /*is_builtin*/ true, /*is_async*/ true);
    assert!(filtered.contains(&"Bash".to_string()));
    assert!(filtered.contains(&"Read".to_string()));
    assert!(!filtered.contains(&"Config".to_string()));
}

#[test]
fn test_filter_mcp_always_allowed() {
    let tools = vec!["mcp__slack_post".to_string()];
    let filtered =
        filter_tools_for_agent(&tools, /*is_builtin*/ false, /*is_async*/ true);
    assert!(filtered.contains(&"mcp__slack_post".to_string()));
}

// ── resolve_agent_tools tests ──

fn test_agent(allowed: Vec<String>, disallowed: Vec<String>, builtin: bool) -> AgentDefinition {
    AgentDefinition {
        agent_type: if builtin {
            AgentTypeId::Builtin(SubagentType::Explore)
        } else {
            AgentTypeId::Custom("custom".into())
        },
        name: "test".into(),
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        ..Default::default()
    }
}

#[test]
fn test_resolve_wildcard() {
    let agent = test_agent(Vec::new(), Vec::new(), /*builtin*/ true);
    let available = vec!["Bash".to_string(), "Read".to_string(), "Write".to_string()];
    let result = resolve_agent_tools(&agent, &available, /*is_async*/ false);
    assert!(result.has_wildcard);
    assert!(result.resolved_tool_names.contains(&"Bash".to_string()));
}

#[test]
fn test_resolve_specific_tools() {
    let agent = test_agent(
        vec!["Bash".to_string(), "NonExistent".to_string()],
        Vec::new(),
        /*builtin*/ true,
    );
    let available = vec!["Bash".to_string(), "Read".to_string()];
    let result = resolve_agent_tools(&agent, &available, /*is_async*/ false);
    assert!(!result.has_wildcard);
    assert_eq!(result.valid_tools, vec!["Bash".to_string()]);
    assert_eq!(result.invalid_tools, vec!["NonExistent".to_string()]);
}

#[test]
fn test_resolve_with_disallowed() {
    let agent = test_agent(Vec::new(), vec!["Write".to_string()], /*builtin*/ true);
    let available = vec!["Bash".to_string(), "Read".to_string(), "Write".to_string()];
    let result = resolve_agent_tools(&agent, &available, /*is_async*/ false);
    assert!(!result.resolved_tool_names.contains(&"Write".to_string()));
    assert!(result.resolved_tool_names.contains(&"Bash".to_string()));
}

// ── enhance_agent_prompt tests ──

#[test]
fn test_enhance_prompt_background() {
    let enhanced = enhance_agent_prompt(
        "You are a code reviewer.",
        Path::new("/workspace/project"),
        "reviewer",
        /*is_background*/ true,
    );
    assert!(enhanced.contains("background agent"));
    assert!(enhanced.contains("/workspace/project"));
    assert!(enhanced.contains("reviewer"));
}

#[test]
fn test_enhance_prompt_foreground_general() {
    let enhanced = enhance_agent_prompt(
        "Base prompt",
        Path::new("/tmp"),
        "general-purpose",
        /*is_background*/ false,
    );
    assert!(enhanced.contains("/tmp"));
    assert!(!enhanced.contains("specialized"));
}

// ── count_tool_uses tests ──

#[test]
fn test_count_tool_uses() {
    let messages = vec![
        serde_json::json!({
            "type": "assistant",
            "content": [
                {"type": "text", "text": "Let me search"},
                {"type": "tool_use", "name": "Bash"},
                {"type": "tool_use", "name": "Read"},
            ]
        }),
        serde_json::json!({"type": "user", "content": "hello"}),
        serde_json::json!({
            "type": "assistant",
            "content": [
                {"type": "tool_use", "name": "Write"},
            ]
        }),
    ];
    assert_eq!(count_tool_uses(&messages), 3);
}

#[test]
fn test_count_tool_uses_empty() {
    let messages: Vec<serde_json::Value> = vec![];
    assert_eq!(count_tool_uses(&messages), 0);
}

// ── discover_agents tests ──

#[test]
fn test_discover_agents_from_dir() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("researcher.md"),
        "---\nname: researcher\ndescription: Research agent\n---\nYou research topics.",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("agents.json"),
        r#"{"writer": {"description": "Writing agent", "prompt": "You write docs."}}"#,
    )
    .unwrap();

    let agents = discover_agents(&[dir.path().to_path_buf()], &[]);
    let names: HashSet<String> = agents.iter().map(|a| a.name.clone()).collect();
    assert!(names.contains("researcher"));
    assert!(names.contains("writer"));
}

#[test]
fn test_discover_agents_builtins_override() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("Explore.md"), "Custom Explore prompt.").unwrap();

    let builtins = vec![AgentDefinition {
        agent_type: AgentTypeId::Builtin(SubagentType::Explore),
        name: "Explore".into(),
        description: Some("Built-in explore".into()),
        initial_prompt: Some("Built-in prompt.".into()),
        ..Default::default()
    }];

    let agents = discover_agents(&[dir.path().to_path_buf()], &builtins);
    let explore = agents.iter().find(|a| a.name == "Explore").unwrap();
    assert_eq!(
        explore.initial_prompt.as_deref(),
        Some("Custom Explore prompt.")
    );
}

// ── summarize_agent_result tests ──

#[test]
fn test_summarize_agent_result() {
    let summary = summarize_agent_result(
        "agent-123",
        "researcher",
        "Found 5 relevant files.",
        /*tool_use_count*/ 12,
        /*duration_ms*/ 5000,
        /*tokens*/ 15000,
    );
    assert_eq!(summary.agent_id, "agent-123");
    assert_eq!(summary.agent_type, "researcher");
    assert_eq!(summary.total_tool_use_count, 12);
    assert_eq!(summary.total_duration_ms, 5000);
}

// ── ToolName constant verification ──

#[test]
fn test_disallowed_tools_use_canonical_names() {
    for &tool in ALL_AGENT_DISALLOWED_TOOLS {
        assert!(
            coco_types::ToolName::from_str(tool).is_ok(),
            "ALL_AGENT_DISALLOWED_TOOLS entry '{tool}' is not a valid ToolName"
        );
    }
    for &tool in CUSTOM_AGENT_DISALLOWED_TOOLS {
        assert!(
            coco_types::ToolName::from_str(tool).is_ok(),
            "CUSTOM_AGENT_DISALLOWED_TOOLS entry '{tool}' is not a valid ToolName"
        );
    }
    for &tool in ASYNC_AGENT_ALLOWED_TOOLS {
        assert!(
            coco_types::ToolName::from_str(tool).is_ok(),
            "ASYNC_AGENT_ALLOWED_TOOLS entry '{tool}' is not a valid ToolName"
        );
    }
}
