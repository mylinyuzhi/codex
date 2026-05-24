use super::*;
use crate::McpServerRef;

fn test_definition(name: &str) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        description: format!("{name} agent"),
        agent_type: name.to_string(),
        ..Default::default()
    }
}

fn test_spawn_input(agent_type: &str, prompt: &str) -> SpawnInput {
    SpawnInput {
        agent_type: agent_type.to_string(),
        prompt: prompt.to_string(),
        identity: None,
        max_turns: None,
        run_in_background: Some(false),
        allowed_tools: None,
        resume_from: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
        isolation_override: None,
        description: None,
    }
}

#[test]
fn test_new_manager() {
    let mgr = SubagentManager::new();
    assert!(mgr.agents.is_empty());
    assert!(mgr.definitions.is_empty());
}

#[test]
fn test_register_agent_type() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    assert_eq!(mgr.definitions.len(), 1);
    assert_eq!(mgr.definitions[0].agent_type, "bash");
}

#[tokio::test]
async fn test_spawn_agent() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id = mgr.spawn("bash", "run ls").await.expect("spawn");
    assert!(!id.is_empty());
    // Without an execute_fn, the stub completes immediately
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Completed));
}

#[tokio::test]
async fn test_spawn_unknown_type() {
    let mut mgr = SubagentManager::new();
    let result = mgr.spawn("nonexistent", "test").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_spawn_full_with_stub() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let input = test_spawn_input("bash", "test");
    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(!result.agent_id.is_empty());
    assert!(result.output.is_some()); // Stub returns output
    assert!(result.background.is_none());
}

#[tokio::test]
async fn test_spawn_full_background() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let mut input = test_spawn_input("bash", "test");
    input.run_in_background = Some(true);

    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(!result.agent_id.is_empty());
    assert!(result.output.is_none()); // Background has no immediate output
    assert!(result.background.is_some());
    assert_eq!(
        mgr.get_status(&result.agent_id),
        Some(AgentStatus::Backgrounded)
    );
}

#[tokio::test]
async fn test_resume_non_backgrounded() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");
    let result = mgr.resume(&id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resume_backgrounded() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Manually set to backgrounded for test.
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Backgrounded;

    let resumed_id = mgr.resume(&id).await.expect("resume");
    assert_eq!(resumed_id, id);
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Running));
}

#[test]
fn test_get_status_missing() {
    let mgr = SubagentManager::new();
    assert!(mgr.get_status("nonexistent").is_none());
}

#[tokio::test]
async fn test_critical_reminder_as_system_prompt_suffix() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let captured = Arc::new(Mutex::new((String::new(), None::<String>)));
    let captured_clone = captured.clone();

    let execute_fn: AgentExecuteFn = Box::new(move |params: AgentExecuteParams| {
        let captured = captured_clone.clone();
        Box::pin(async move {
            *captured.lock().await = (params.prompt, params.system_prompt_suffix);
            Ok("done".to_string())
        })
    });

    let mut def = test_definition("explore");
    def.critical_reminder = Some("CRITICAL: You are read-only.".to_string());

    let mut mgr = SubagentManager::new().with_execute_fn(execute_fn);
    mgr.register_agent_type(def);

    let input = test_spawn_input("explore", "find the config file");
    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(result.output.is_some());

    let (actual_prompt, actual_suffix) = captured.lock().await.clone();
    // Prompt should be unchanged (not prefixed with reminder)
    assert_eq!(
        actual_prompt, "find the config file",
        "Prompt should not contain critical_reminder"
    );
    // Suffix should carry the critical_reminder
    assert_eq!(
        actual_suffix.as_deref(),
        Some("CRITICAL: You are read-only."),
        "system_prompt_suffix should contain critical_reminder"
    );
}

#[tokio::test]
async fn test_no_critical_reminder_passes_prompt_unchanged() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let captured_prompt = Arc::new(Mutex::new(String::new()));
    let captured_prompt_clone = captured_prompt.clone();

    let execute_fn: AgentExecuteFn = Box::new(move |params: AgentExecuteParams| {
        let captured = captured_prompt_clone.clone();
        Box::pin(async move {
            *captured.lock().await = params.prompt;
            Ok("done".to_string())
        })
    });

    let def = test_definition("bash"); // no critical_reminder

    let mut mgr = SubagentManager::new().with_execute_fn(execute_fn);
    mgr.register_agent_type(def);

    let input = test_spawn_input("bash", "run ls -la");
    mgr.spawn_full(input).await.expect("spawn_full");

    let actual_prompt = captured_prompt.lock().await;
    assert_eq!(*actual_prompt, "run ls -la");
}

#[tokio::test]
async fn test_background_limit_exceeded() {
    let mut mgr = SubagentManager::new().with_max_background_agents(2);
    mgr.register_agent_type(test_definition("bash"));

    // Spawn two background agents (should succeed)
    for i in 0..2 {
        let mut input = test_spawn_input("bash", &format!("task {i}"));
        input.run_in_background = Some(true);
        mgr.spawn_full(input).await.expect("spawn should succeed");
    }

    // Third background agent should fail with BackgroundLimit
    let mut input = test_spawn_input("bash", "one too many");
    input.run_in_background = Some(true);
    let err = mgr.spawn_full(input).await.unwrap_err();
    assert!(
        matches!(
            err,
            crate::error::SubagentError::BackgroundLimit { limit: 2, .. }
        ),
        "Expected BackgroundLimit error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_foreground_does_not_count_toward_background_limit() {
    let mut mgr = SubagentManager::new().with_max_background_agents(1);
    mgr.register_agent_type(test_definition("bash"));

    // Spawn a foreground agent (should not count)
    let fg_input = test_spawn_input("bash", "foreground task");
    mgr.spawn_full(fg_input).await.expect("foreground spawn");

    // Background agent should still succeed (limit is 1, no bg agents yet)
    let mut bg_input = test_spawn_input("bash", "background task");
    bg_input.run_in_background = Some(true);
    mgr.spawn_full(bg_input).await.expect("background spawn");
}

// ── MCP server availability validation tests ──

#[test]
fn test_validate_mcp_servers_no_requirements() {
    let def = test_definition("bash");
    let empty = std::collections::HashSet::new();
    assert!(SubagentManager::validate_mcp_servers(&def, &empty));
}

#[test]
fn test_validate_mcp_servers_available() {
    let available: std::collections::HashSet<String> = ["slack".to_string()].into_iter().collect();
    let mut def = test_definition("slack-bot");
    def.mcp_servers = Some(vec![McpServerRef {
        name: "slack".to_string(),
        transport: None,
    }]);
    assert!(SubagentManager::validate_mcp_servers(&def, &available));
}

#[test]
fn test_validate_mcp_servers_unavailable() {
    let mut def = test_definition("slack-bot");
    def.mcp_servers = Some(vec![McpServerRef {
        name: "slack".to_string(),
        transport: None,
    }]);
    let empty = std::collections::HashSet::new();
    assert!(!SubagentManager::validate_mcp_servers(&def, &empty));
}

#[test]
fn test_available_definitions_filters() {
    let mcp_github_tool = format!(
        "{prefix}github{sep}search",
        prefix = cocode_protocol::MCP_TOOL_PREFIX,
        sep = cocode_protocol::MCP_TOOL_SEPARATOR,
    );
    let mut mgr = SubagentManager::new().with_tools(vec![
        cocode_protocol::ToolName::Read.as_str().to_string(),
        mcp_github_tool,
    ]);
    // Agent without MCP requirements — always available
    mgr.register_agent_type(test_definition("bash"));
    // Agent requiring github — available
    let mut github_def = test_definition("github-bot");
    github_def.mcp_servers = Some(vec![McpServerRef {
        name: "github".to_string(),
        transport: None,
    }]);
    mgr.register_agent_type(github_def);
    // Agent requiring slack — NOT available
    let mut slack_def = test_definition("slack-bot");
    slack_def.mcp_servers = Some(vec![McpServerRef {
        name: "slack".to_string(),
        transport: None,
    }]);
    mgr.register_agent_type(slack_def);

    let available = mgr.available_definitions();
    let names: Vec<&str> = available.iter().map(|d| d.agent_type.as_str()).collect();
    assert_eq!(names, vec!["bash", "github-bot"]);
}

// ── Lifecycle method tests ──

#[tokio::test]
async fn test_remove_completed_agent() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");
    // Agent should be completed (stub)
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Completed));

    let removed = mgr.remove_agent(&id);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().id, id);
    assert!(mgr.get_status(&id).is_none());
}

#[tokio::test]
async fn test_remove_running_agent_returns_none() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Manually set to running
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Running;

    let removed = mgr.remove_agent(&id);
    assert!(removed.is_none());
    // Agent should still be tracked
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Running));
}

#[tokio::test]
async fn test_gc_completed() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    // Spawn 3 agents (all complete as stubs)
    let id1 = mgr.spawn("bash", "task1").await.expect("spawn");
    let id2 = mgr.spawn("bash", "task2").await.expect("spawn");
    let id3 = mgr.spawn("bash", "task3").await.expect("spawn");
    assert_eq!(mgr.agent_count(), 3);

    // Set one to running
    mgr.agents.get_mut(&id2).expect("agent").status = AgentStatus::Running;

    let removed = mgr.gc_completed();
    assert_eq!(removed, 2); // id1 and id3 removed
    assert_eq!(mgr.agent_count(), 1);
    assert_eq!(mgr.get_status(&id2), Some(AgentStatus::Running));
    assert!(mgr.get_status(&id1).is_none());
    assert!(mgr.get_status(&id3).is_none());
}

#[tokio::test]
async fn test_status_counts() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id1 = mgr.spawn("bash", "t1").await.expect("spawn");
    let id2 = mgr.spawn("bash", "t2").await.expect("spawn");
    let _id3 = mgr.spawn("bash", "t3").await.expect("spawn");

    // Mix statuses
    mgr.agents.get_mut(&id1).expect("a").status = AgentStatus::Running;
    mgr.agents.get_mut(&id2).expect("a").status = AgentStatus::Backgrounded;
    // id3 stays Completed

    let counts = mgr.status_counts();
    assert_eq!(counts.get(&AgentStatus::Running), Some(&1));
    assert_eq!(counts.get(&AgentStatus::Backgrounded), Some(&1));
    assert_eq!(counts.get(&AgentStatus::Completed), Some(&1));
    assert_eq!(counts.get(&AgentStatus::Failed), None);
}

#[tokio::test]
async fn test_spawn_input_description() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let captured_type = Arc::new(Mutex::new(String::new()));
    let captured_clone = captured_type.clone();

    let execute_fn: AgentExecuteFn = Box::new(move |params: AgentExecuteParams| {
        let captured = captured_clone.clone();
        Box::pin(async move {
            *captured.lock().await = params.agent_type;
            Ok("done".to_string())
        })
    });

    let mut mgr = SubagentManager::new().with_execute_fn(execute_fn);
    mgr.register_agent_type(test_definition("explore"));

    let mut input = test_spawn_input("explore", "find stuff");
    input.description = Some("Search the codebase".to_string());

    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(result.output.is_some());
}

// ── agent_infos tests ──

#[tokio::test]
async fn test_agent_infos_returns_snapshots() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    mgr.register_agent_type(test_definition("explore"));

    let id1 = mgr.spawn("bash", "task1").await.expect("spawn");
    let id2 = mgr.spawn("explore", "task2").await.expect("spawn");

    // Set different statuses
    mgr.agents.get_mut(&id1).expect("a").status = AgentStatus::Running;
    // id2 stays Completed (stub)

    let infos = mgr.agent_infos();
    assert_eq!(infos.len(), 2);

    let info1 = infos.iter().find(|i| i.id == id1).expect("info1");
    assert_eq!(info1.agent_type, "bash");
    assert_eq!(info1.status, AgentStatus::Running);

    let info2 = infos.iter().find(|i| i.id == id2).expect("info2");
    assert_eq!(info2.agent_type, "explore");
    assert_eq!(info2.status, AgentStatus::Completed);
}

#[tokio::test]
async fn test_agent_infos_includes_name() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let mut input = test_spawn_input("bash", "task1");
    input.name = Some("my-agent".to_string());
    let result = mgr.spawn_full(input).await.expect("spawn_full");

    let infos = mgr.agent_infos();
    let info = infos
        .iter()
        .find(|i| i.id == result.agent_id)
        .expect("info");
    assert_eq!(info.name, Some("my-agent".to_string()));
}

// ── Killed status tests ──

#[tokio::test]
async fn test_killed_status_in_remove_agent() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Manually set to Killed
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Killed;

    // remove_agent should accept Killed status
    let removed = mgr.remove_agent(&id);
    assert!(removed.is_some());
    assert!(mgr.get_status(&id).is_none());
}

#[tokio::test]
async fn test_killed_status_in_gc_completed() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id1 = mgr.spawn("bash", "t1").await.expect("spawn");
    let _id2 = mgr.spawn("bash", "t2").await.expect("spawn");

    // Set one to Killed, other stays Completed
    mgr.agents.get_mut(&id1).expect("a").status = AgentStatus::Killed;

    let removed = mgr.gc_completed();
    assert_eq!(removed, 2); // Both Killed and Completed removed
    assert_eq!(mgr.agent_count(), 0);
}

// ── promote_killed tests ──

#[tokio::test]
async fn test_promote_killed_upgrades_failed_to_killed() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Simulate cancellation: handler marks agent as Failed
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Failed;

    let mut killed_set = std::collections::HashSet::new();
    killed_set.insert(id.clone());
    mgr.promote_killed(&killed_set);

    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Killed));
}

#[tokio::test]
async fn test_promote_killed_ignores_non_failed() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Manually set to Running (no-execute stub completes immediately)
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Running;

    // Agent is Running — should NOT be promoted
    let mut killed_set = std::collections::HashSet::new();
    killed_set.insert(id.clone());
    mgr.promote_killed(&killed_set);

    // Still Running, not Killed
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Running));
}

#[tokio::test]
async fn test_promote_killed_ignores_unknown_ids() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Failed;

    // killed set contains a different ID
    let mut killed_set = std::collections::HashSet::new();
    killed_set.insert("unknown-id".to_string());
    mgr.promote_killed(&killed_set);

    // Still Failed, not upgraded
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Failed));
}

// ── Auto-background timeout default tests ──

#[test]
fn test_auto_background_timeout_defaults_to_none() {
    let mgr = SubagentManager::new();
    assert!(mgr.auto_background_timeout.is_none());
}

#[test]
fn test_auto_background_timeout_builder() {
    let mgr = SubagentManager::new()
        .with_auto_background_timeout(Some(std::time::Duration::from_secs(60)));
    assert_eq!(
        mgr.auto_background_timeout,
        Some(std::time::Duration::from_secs(60))
    );
}

// ── Prefixed ID tests ──

#[tokio::test]
async fn test_agent_id_has_prefix() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");
    assert!(
        id.starts_with('a'),
        "Agent ID should start with 'a' prefix, got: {id}"
    );
    assert_eq!(id.len(), 9, "Agent ID should be a{{8hex}}, got: {id}");
}

// ── BackgroundOrigin tests ──

#[tokio::test]
async fn test_background_origin_explicit() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let mut input = test_spawn_input("bash", "test");
    input.run_in_background = Some(true);
    let result = mgr.spawn_full(input).await.expect("spawn_full");

    let infos = mgr.agent_infos();
    let info = infos
        .iter()
        .find(|i| i.id == result.agent_id)
        .expect("info");
    assert_eq!(info.background_origin, Some(BackgroundOrigin::Explicit));
}

#[tokio::test]
async fn test_foreground_has_no_background_origin() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let input = test_spawn_input("bash", "test");
    let result = mgr.spawn_full(input).await.expect("spawn_full");

    let infos = mgr.agent_infos();
    let info = infos
        .iter()
        .find(|i| i.id == result.agent_id)
        .expect("info");
    assert_eq!(info.background_origin, None);
}

// ── gc_stale tests ──

#[tokio::test]
async fn test_gc_stale_removes_old_agents() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id1 = mgr.spawn("bash", "t1").await.expect("spawn");
    let id2 = mgr.spawn("bash", "t2").await.expect("spawn");

    // Both completed (stub). Set id1 completed_at to long ago.
    let a1 = mgr.agents.get_mut(&id1).expect("a");
    a1.completed_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(600));
    a1.parent_notified = true; // parent has seen it, eligible for GC
    // id2 completed just now (already set by stub)

    let removed = mgr.gc_stale(std::time::Duration::from_secs(300));
    assert_eq!(removed, 1, "Should remove only the old agent");
    assert!(mgr.get_status(&id1).is_none(), "Old agent should be gone");
    assert!(mgr.get_status(&id2).is_some(), "Recent agent should remain");
}

#[tokio::test]
async fn test_gc_stale_keeps_running_agents() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Set to running (GC should not touch it)
    mgr.agents.get_mut(&id).expect("a").status = AgentStatus::Running;

    let removed = mgr.gc_stale(std::time::Duration::from_secs(0));
    assert_eq!(removed, 0);
    assert!(mgr.get_status(&id).is_some());
}

// ── kill_all_running tests ──

#[tokio::test]
async fn test_kill_all_running() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id1 = mgr.spawn("bash", "t1").await.expect("spawn");
    let id2 = mgr.spawn("bash", "t2").await.expect("spawn");
    let id3 = mgr.spawn("bash", "t3").await.expect("spawn");

    // Set some to running/backgrounded
    mgr.agents.get_mut(&id1).expect("a").status = AgentStatus::Running;
    mgr.agents.get_mut(&id2).expect("a").status = AgentStatus::Backgrounded;
    // id3 stays Completed

    let killed = mgr.kill_all_running();
    assert_eq!(killed.len(), 2);
    assert!(killed.contains(&id1));
    assert!(killed.contains(&id2));

    assert_eq!(mgr.get_status(&id1), Some(AgentStatus::Killed));
    assert_eq!(mgr.get_status(&id2), Some(AgentStatus::Killed));
    assert_eq!(mgr.get_status(&id3), Some(AgentStatus::Completed));
}

#[tokio::test]
async fn test_kill_all_running_empty() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let _id = mgr.spawn("bash", "test").await.expect("spawn"); // Completed

    let killed = mgr.kill_all_running();
    assert!(killed.is_empty());
}

// ── mark_notified tests ──

#[tokio::test]
async fn test_mark_notified_sets_flag() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    assert!(!mgr.agents.get(&id).expect("a").parent_notified);
    assert!(mgr.mark_notified(&id));
    assert!(mgr.agents.get(&id).expect("a").parent_notified);
}

#[test]
fn test_mark_notified_unknown_returns_false() {
    let mut mgr = SubagentManager::new();
    assert!(!mgr.mark_notified("nonexistent"));
}

#[tokio::test]
async fn test_gc_stale_keeps_unnotified_agents() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Completed long ago but parent not notified — should be kept.
    let a = mgr.agents.get_mut(&id).expect("a");
    a.completed_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(600));
    // parent_notified defaults to false

    let removed = mgr.gc_stale(std::time::Duration::from_secs(300));
    assert_eq!(removed, 0, "Unnotified agents should not be GC'd");
    assert!(mgr.get_status(&id).is_some());
}

#[tokio::test]
async fn test_agent_infos_includes_new_fields() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let mut input = test_spawn_input("bash", "test");
    input.run_in_background = Some(true);
    let result = mgr.spawn_full(input).await.expect("spawn_full");

    let infos = mgr.agent_infos();
    let info = infos
        .iter()
        .find(|i| i.id == result.agent_id)
        .expect("info");

    assert!(
        info.output_file.is_some(),
        "Background agent should have output_file"
    );
    assert!(!info.parent_notified, "New agent should not be notified");
}

#[tokio::test]
async fn test_read_deltas_returns_empty_when_no_output_files() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    // Foreground agent has no output file — read_deltas should be empty.
    let _id = mgr.spawn("bash", "test").await.expect("spawn");
    let deltas = mgr.read_deltas().await;
    assert!(deltas.is_empty());
}

#[tokio::test]
async fn test_read_deltas_reads_from_transcript() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let mut input = test_spawn_input("bash", "test");
    input.run_in_background = Some(true);
    let result = mgr.spawn_full(input).await.expect("spawn_full");
    let agent_id = result.agent_id.clone();

    // Write a transcript entry to the output file.
    let output_file = mgr
        .agents
        .get(&agent_id)
        .expect("a")
        .output_file
        .clone()
        .expect("file");
    // Ensure parent directory
    if let Some(parent) = output_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = serde_json::json!({"type": "progress", "message": "doing work"});
    std::fs::write(
        &output_file,
        format!("{}\n", serde_json::to_string(&entry).expect("json")),
    )
    .expect("write");

    let deltas = mgr.read_deltas().await;
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].0, agent_id);
    assert!(deltas[0].1.contains("doing work"));

    // Second read should return nothing (offset advanced).
    let deltas2 = mgr.read_deltas().await;
    assert!(deltas2.is_empty());

    drop(tmp);
}
