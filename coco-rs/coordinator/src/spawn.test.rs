use super::*;

#[test]
fn test_get_teammate_command() {
    let cmd = get_teammate_command();
    // Should return something (either env var or current exe or "claude")
    assert!(!cmd.is_empty());
}

#[test]
fn test_build_teammate_command_basic() {
    let config = TeammateSpawnConfig {
        name: "researcher".into(),
        team_name: "my-team".into(),
        color: Some(crate::constants::AgentColorName::Blue),
        plan_mode_required: false,
        prompt: "Find the bug".into(),
        cwd: "/project".into(),
        model: Some("anthropic/claude-opus-4-7".into()),
        system_prompt: None,
        system_prompt_mode: crate::pane::SystemPromptMode::Default,
        worktree_path: None,
        parent_session_id: "session-1".into(),
        permissions: Vec::new(),
        allow_permission_prompts: false,
        ..Default::default()
    };

    let cmd = build_teammate_command(&config);
    assert!(cmd.contains("cd /project &&"));
    assert!(cmd.contains("--agent-id=researcher@my-team"));
    assert!(cmd.contains("--agent-name=researcher"));
    assert!(cmd.contains("--team-name=my-team"));
    assert!(cmd.contains("--agent-color=blue"));
    assert!(cmd.contains("--model=anthropic/claude-opus-4-7"));
    assert!(cmd.contains("--parent-session-id=session-1"));
    assert!(!cmd.contains("--plan-mode-required"));
}

#[test]
fn test_build_teammate_command_plan_mode() {
    let config = TeammateSpawnConfig {
        name: "coder".into(),
        team_name: "team".into(),
        color: None,
        plan_mode_required: true,
        prompt: "Fix it".into(),
        cwd: "/tmp".into(),
        model: None,
        system_prompt: None,
        system_prompt_mode: crate::pane::SystemPromptMode::Default,
        worktree_path: None,
        parent_session_id: "sess".into(),
        permissions: Vec::new(),
        allow_permission_prompts: false,
        ..Default::default()
    };

    let cmd = build_teammate_command(&config);
    assert!(cmd.contains("--plan-mode-required"));
    assert!(!cmd.contains("--agent-color"));
}

#[test]
fn test_build_inherited_env_vars() {
    let config = TeammateSpawnConfig {
        name: "worker".into(),
        team_name: "t".into(),
        color: Some(crate::constants::AgentColorName::Red),
        plan_mode_required: true,
        prompt: String::new(),
        cwd: String::new(),
        model: None,
        system_prompt: None,
        system_prompt_mode: crate::pane::SystemPromptMode::Default,
        worktree_path: None,
        parent_session_id: "s".into(),
        permissions: Vec::new(),
        allow_permission_prompts: false,
        ..Default::default()
    };

    let env = build_inherited_env_vars(&config);
    // Feature::AgentTeams gate is propagated via the COCO_FEATURE_*
    // namespace so spawned children re-resolve it the same way the
    // parent did.
    assert!(env.contains("COCO_FEATURE_AGENT_TEAMS=1"));
    assert!(env.contains("COCO_AGENT_COLOR=red"));
    assert!(env.contains("COCO_PLAN_MODE_REQUIRED=1"));
    // T10: worker identity must be in env (mirrors TS AsyncLocalStorage
    // context). Without these the child can't read its own identity
    // when callers go through `crate::identity::*` helpers.
    assert!(env.contains("COCO_AGENT_ID=worker@t"));
    assert!(env.contains("COCO_AGENT_NAME=worker"));
    assert!(env.contains("COCO_TEAM_NAME=t"));
}

#[test]
fn test_build_inherited_cli_flags() {
    let config = TeammateSpawnConfig {
        name: "worker".into(),
        team_name: "t".into(),
        color: Some(crate::constants::AgentColorName::Green),
        plan_mode_required: false,
        prompt: String::new(),
        cwd: String::new(),
        model: Some("anthropic/claude-sonnet-4-6".into()),
        system_prompt: None,
        system_prompt_mode: crate::pane::SystemPromptMode::Default,
        worktree_path: None,
        parent_session_id: "sess".into(),
        permissions: vec!["Edit".into()],
        allow_permission_prompts: false,
        ..Default::default()
    };

    let flags = build_inherited_cli_flags(&config);
    assert!(flags.contains(&"--agent-id=worker@t".to_string()));
    assert!(flags.contains(&"--model=anthropic/claude-sonnet-4-6".to_string()));
    assert!(flags.contains(&"--agent-color=green".to_string()));
    assert!(flags.contains(&"--permission=Edit".to_string()));
}
