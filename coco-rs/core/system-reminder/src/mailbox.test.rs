use std::sync::Arc;

use super::*;

#[test]
fn drain_returns_empty_state_initially() {
    let mb = ReminderMailbox::new();
    let drained = mb.drain();
    assert_eq!(drained, ReminderMailboxState::default());
}

#[test]
fn put_then_drain_returns_value_and_clears() {
    let mb = ReminderMailbox::new();
    let handle: Arc<dyn ReminderMailboxRef> = mb.clone().handle();

    handle.put_structured_output("hello".to_string());
    handle.put_command_permissions("perm-snapshot".to_string());

    // Peek shows the values in flight.
    let state = mb.peek();
    assert_eq!(state.structured_output.as_deref(), Some("hello"));
    assert_eq!(state.command_permissions.as_deref(), Some("perm-snapshot"));
    assert!(state.dynamic_skill.is_none());

    // Drain returns + clears.
    let drained = mb.drain();
    assert_eq!(drained.structured_output.as_deref(), Some("hello"));
    assert_eq!(
        drained.command_permissions.as_deref(),
        Some("perm-snapshot")
    );

    let after = mb.drain();
    assert_eq!(after, ReminderMailboxState::default());
}

#[test]
fn latest_snapshot_wins() {
    let mb = ReminderMailbox::new();
    let handle = mb.clone().handle();
    handle.put_structured_output("first".to_string());
    handle.put_structured_output("second".to_string());
    handle.put_structured_output("third".to_string());
    let drained = mb.drain();
    assert_eq!(drained.structured_output.as_deref(), Some("third"));
}

#[test]
fn noop_mailbox_swallows_puts() {
    let handle: Arc<dyn ReminderMailboxRef> = Arc::new(NoOpReminderMailbox);
    handle.put_structured_output("ignored".to_string());
    handle.put_command_permissions("ignored".to_string());
    handle.put_dynamic_skill("ignored".to_string());
    handle.put_teammate_shutdown_batch("ignored".to_string());
    // No assertion on output — NoOp can't be drained, just verifying the
    // call doesn't panic and that the trait object compiles.
}

#[test]
fn handle_round_trip_keeps_arc_count_correct() {
    let mb = ReminderMailbox::new();
    assert_eq!(Arc::strong_count(&mb), 1);
    let handle = mb.clone().handle();
    assert_eq!(Arc::strong_count(&mb), 2);
    drop(handle);
    assert_eq!(Arc::strong_count(&mb), 1);
}

#[test]
fn drain_after_partial_writes_returns_only_set_fields() {
    let mb = ReminderMailbox::new();
    mb.put_dynamic_skill("skill-summary".to_string());
    let drained = mb.drain();
    assert_eq!(drained.dynamic_skill.as_deref(), Some("skill-summary"));
    assert!(drained.structured_output.is_none());
    assert!(drained.command_permissions.is_none());
    assert!(drained.teammate_shutdown_batch.is_none());
}
