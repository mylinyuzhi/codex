use super::cycle;
use super::next;
use coco_types::ExpandedView;

use crate::state::AppState;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;

fn running_subagent() -> SubagentInstance {
    SubagentInstance {
        agent_id: "t-1".into(),
        agent_type: "explore".into(),
        description: "scan".into(),
        status: SubagentStatus::Running,
        color: None,
        started_at_ms: None,
        token_usage: None,
    }
}

#[test]
fn cycle_with_no_teammates_toggles_none_and_tasks() {
    assert_eq!(next(ExpandedView::None, false), ExpandedView::Tasks);
    assert_eq!(next(ExpandedView::Tasks, false), ExpandedView::None);
    // Stale Teammates collapses to Tasks on next press, then to None.
    assert_eq!(next(ExpandedView::Teammates, false), ExpandedView::Tasks);
}

#[test]
fn cycle_with_running_teammates_does_three_step_cycle() {
    assert_eq!(next(ExpandedView::None, true), ExpandedView::Tasks);
    assert_eq!(next(ExpandedView::Tasks, true), ExpandedView::Teammates);
    assert_eq!(next(ExpandedView::Teammates, true), ExpandedView::None);
}

#[test]
fn cycle_via_state_uses_running_subagents_to_pick_branch() {
    let mut state = AppState::new();
    state.session.expanded_view = ExpandedView::Tasks;
    cycle(&mut state);
    // No subagents → Tasks → None
    assert_eq!(state.session.expanded_view, ExpandedView::None);

    state.session.subagents.push(running_subagent());
    state.session.expanded_view = ExpandedView::Tasks;
    cycle(&mut state);
    // Has running subagents → Tasks → Teammates
    assert_eq!(state.session.expanded_view, ExpandedView::Teammates);
}
