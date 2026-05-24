use super::*;
use coco_types::ExpandedView;

use crate::state::AppState;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;

fn surface(view: TurnActivityView) -> ActivitySurfaceView {
    match view {
        TurnActivityView::Surface(surface) => surface,
        TurnActivityView::None => panic!("expected activity surface"),
    }
}

fn subagent() -> SubagentInstance {
    SubagentInstance {
        kind: crate::state::session::SubagentKind::Subagent,
        agent_id: "agent-1".into(),
        agent_type: "explore".into(),
        description: "scan".into(),
        status: SubagentStatus::Running,
        color: None,
        team_name: None,
        tool_use_id: None,
        started_at_ms: None,
        last_tool_name: None,
        tool_count: 0,
        total_tokens: 0,
        is_backgrounded: false,
        recent_activities: Vec::new(),
        final_message: None,
    }
}

#[test]
fn turn_activity_view_renders_agents_when_narrow() {
    let mut state = AppState::default();
    state.session.subagents.push(subagent());
    state.session.expanded_view = ExpandedView::Teammates;

    let view = surface(turn_activity_view(&state, 20));

    assert_eq!(view.title, ActivityTitle::Agents);
}

#[test]
fn turn_activity_view_prefers_expanded_plan_activity() {
    let mut state = AppState::default();
    state.session.expanded_view = ExpandedView::Tasks;
    state
        .session
        .todos_by_agent
        .insert("main".into(), Vec::new());
    state.session.subagents.push(subagent());

    let view = surface(turn_activity_view(&state, 160));

    assert_eq!(view.title, ActivityTitle::Tasks);
    assert_eq!(view.border, ActivityBorder::Plan);
}

#[test]
fn turn_activity_view_uses_agents_for_teammates_view() {
    let mut state = AppState::default();
    state.session.expanded_view = ExpandedView::Teammates;
    state.session.subagents.push(subagent());

    let view = surface(turn_activity_view(&state, 160));

    assert_eq!(view.title, ActivityTitle::Agents);
    assert_eq!(view.border, ActivityBorder::Agents);
}

#[test]
fn turn_activity_view_falls_back_to_agents_when_present() {
    let mut state = AppState::default();
    state.session.subagents.push(subagent());

    let view = surface(turn_activity_view(&state, 160));

    assert_eq!(view.title, ActivityTitle::Agents);
}

#[test]
fn turn_activity_view_renders_stream_status_without_agents() {
    let _locale = crate::i18n::locale_test_guard("en");
    let mut state = AppState::default();
    state.session.stream_stall = true;

    let view = surface(turn_activity_view(&state, 160));

    assert_eq!(view.title, ActivityTitle::Activity);
    assert!(
        view.lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.text.contains("Stream stall"))
    );
}

#[test]
fn turn_activity_view_keeps_stale_teammates_view_empty_without_agents() {
    let mut state = AppState::default();
    state.session.expanded_view = ExpandedView::Teammates;
    state
        .session
        .todos_by_agent
        .insert("main".into(), Vec::new());

    assert!(matches!(
        turn_activity_view(&state, 160),
        TurnActivityView::None
    ));
}

#[test]
fn inline_activity_height_caps_narrow_rows() {
    let view = TurnActivityView::Surface(ActivitySurfaceView {
        title: ActivityTitle::Activity,
        border: ActivityBorder::Activity,
        lines: vec![
            ActivityLine::text("one", ActivityTone::Text),
            ActivityLine::text("two", ActivityTone::Text),
            ActivityLine::text("three", ActivityTone::Text),
            ActivityLine::text("four", ActivityTone::Text),
        ],
    });

    assert_eq!(inline_activity_height(&view, 20, 60), 4);
}

#[test]
fn inline_activity_height_respects_available_height() {
    let view = TurnActivityView::Surface(ActivitySurfaceView {
        title: ActivityTitle::Activity,
        border: ActivityBorder::Activity,
        lines: vec![
            ActivityLine::text("one", ActivityTone::Text),
            ActivityLine::text("two", ActivityTone::Text),
        ],
    });

    assert_eq!(inline_activity_height(&view, 2, 120), 2);
}
