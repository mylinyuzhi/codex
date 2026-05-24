use super::*;
use pretty_assertions::assert_eq;

#[test]
fn note_width_initializes_without_change() {
    let mut state = HistoryReflowState::default();

    assert_eq!(
        state.note_width(80),
        HistoryWidthChange {
            initialized: true,
            changed: false,
        }
    );
    assert!(!state.replay_needed_for_width(80));
}

#[test]
fn note_width_reports_changed_after_initial_width() {
    let mut state = HistoryReflowState::default();
    state.note_width(80);

    assert_eq!(
        state.note_width(100),
        HistoryWidthChange {
            initialized: false,
            changed: true,
        }
    );
    assert!(state.replay_needed_for_width(100));
}

#[test]
fn note_viewport_reports_changed_after_height_change() {
    let mut state = HistoryReflowState::default();
    state.note_viewport(80, 10);

    assert_eq!(
        state.note_viewport(80, 12),
        HistoryViewportChange {
            initialized: false,
            changed: true,
        }
    );
    assert!(state.replay_needed_for_viewport(80, 12));
}

#[test]
fn scheduled_viewport_is_not_reported_as_needed_twice() {
    let mut state = HistoryReflowState::default();
    state.note_viewport(80, 10);
    state.note_viewport(100, 12);

    state.schedule_viewport_replay(100, 12, false);

    assert_eq!(state.pending_viewport(), Some((100, 12)));
    assert!(!state.replay_needed_for_viewport(100, 12));
}

#[test]
fn scheduled_width_is_not_reported_as_needed_twice() {
    let mut state = HistoryReflowState::default();
    state.note_width(80);
    state.note_width(100);

    state.schedule_resize_replay(100, false);

    assert_eq!(state.pending_width(), Some(100));
    assert!(!state.replay_needed_for_width(100));
}

#[test]
fn pending_replay_becomes_due_after_deadline() {
    let mut state = HistoryReflowState::default();
    state.schedule_resize_replay(100, false);
    assert!(!state.pending_is_due(Instant::now()));

    state.force_due_for_test();

    assert!(state.pending_is_due(Instant::now()));
}

#[test]
fn mark_replayed_width_clears_pending_state() {
    let mut state = HistoryReflowState::default();
    state.note_width(80);
    state.schedule_resize_replay(100, false);

    state.mark_replayed_width(100, false);

    assert_eq!(state.pending_width(), None);
    assert!(!state.replay_needed_for_width(100));
}

#[test]
fn mark_replayed_viewport_clears_pending_state() {
    let mut state = HistoryReflowState::default();
    state.note_viewport(80, 10);
    state.schedule_viewport_replay(100, 12, false);

    state.mark_replayed_viewport(100, 12, false);

    assert_eq!(state.pending_viewport(), None);
    assert!(!state.replay_needed_for_viewport(100, 12));
}

#[test]
fn stream_finish_replay_needed_when_resize_was_requested_during_stream() {
    let mut state = HistoryReflowState::default();
    state.schedule_resize_replay(120, true);

    assert!(state.take_stream_finish_replay_needed());
    assert!(!state.take_stream_finish_replay_needed());
}

#[test]
fn stream_finish_replay_needed_when_replay_ran_during_stream() {
    let mut state = HistoryReflowState::default();
    state.mark_replayed_width(120, true);

    assert!(state.take_stream_finish_replay_needed());
}

#[test]
fn clear_resets_reflow_state() {
    let mut state = HistoryReflowState::default();
    state.note_width(80);
    state.schedule_resize_replay(100, true);

    state.clear();

    assert_eq!(state.pending_width(), None);
    assert!(!state.take_stream_finish_replay_needed());
    assert_eq!(
        state.note_width(100),
        HistoryWidthChange {
            initialized: true,
            changed: false,
        }
    );
}
