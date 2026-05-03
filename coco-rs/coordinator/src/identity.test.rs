use super::*;

#[test]
fn test_create_teammate_context() {
    let ctx = create_teammate_context("researcher", "my-team", Some("blue".into()), true, "sess-1");
    assert_eq!(ctx.agent_id, "researcher@my-team");
    assert_eq!(ctx.agent_name, "researcher");
    assert_eq!(ctx.team_name, "my-team");
    assert_eq!(ctx.color.as_deref(), Some("blue"));
    assert!(ctx.plan_mode_required);
    assert_eq!(ctx.parent_session_id, "sess-1");
}

#[test]
fn test_dynamic_context_set_clear() {
    clear_dynamic_team_context();
    assert!(get_dynamic_team_context().is_none());

    set_dynamic_team_context(DynamicTeamContext {
        agent_id: "worker@team".into(),
        agent_name: "worker".into(),
        team_name: "team".into(),
        color: None,
        plan_mode_required: false,
        parent_session_id: None,
    });

    let ctx = get_dynamic_team_context().unwrap();
    assert_eq!(ctx.agent_id, "worker@team");

    clear_dynamic_team_context();
    assert!(get_dynamic_team_context().is_none());
}

#[test]
fn test_is_team_lead() {
    let tc = TeamContext {
        team_name: "test".into(),
        team_file_path: String::new(),
        lead_agent_id: "leader@test".into(),
        self_agent_id: Some("leader@test".into()),
        self_agent_name: Some("team-lead".into()),
        is_leader: true,
        self_agent_color: None,
        teammates: Default::default(),
    };
    assert!(is_team_lead(Some(&tc)));

    let tc2 = TeamContext {
        is_leader: false,
        ..tc
    };
    assert!(!is_team_lead(Some(&tc2)));
    assert!(!is_team_lead(None));
}

#[tokio::test]
async fn test_task_local_context() {
    let ctx = create_teammate_context("tester", "t", None, false, "s");

    let result = run_with_teammate_context(ctx, async {
        let inner = get_teammate_context().unwrap();
        assert_eq!(inner.agent_name, "tester");
        assert!(is_in_process_teammate());
        42
    })
    .await;
    assert_eq!(result, 42);

    // Outside the scope, should not have context
    assert!(!is_in_process_teammate());
}
