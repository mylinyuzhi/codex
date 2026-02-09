use super::*;

#[test]
fn test_register_and_unregister() {
    let agent_id = "test-agent-1".to_string();

    let _rx = register_backgroundable_agent(agent_id.clone());
    assert!(is_agent_backgroundable(&agent_id));

    unregister_backgroundable_agent(&agent_id);
    assert!(!is_agent_backgroundable(&agent_id));
}

#[test]
fn test_trigger_removes_from_map() {
    let agent_id = "test-agent-2".to_string();

    let _rx = register_backgroundable_agent(agent_id.clone());
    assert!(is_agent_backgroundable(&agent_id));

    let triggered = trigger_background_transition(&agent_id);
    assert!(triggered);

    // Should be removed after trigger
    assert!(!is_agent_backgroundable(&agent_id));
}

#[test]
fn test_trigger_nonexistent() {
    let triggered = trigger_background_transition("nonexistent");
    assert!(!triggered);
}

#[tokio::test]
async fn test_signal_received() {
    let agent_id = "test-agent-3".to_string();

    let rx = register_backgroundable_agent(agent_id.clone());

    // Trigger in another task
    let agent_id_clone = agent_id.clone();
    tokio::spawn(async move {
        trigger_background_transition(&agent_id_clone);
    });

    // Wait for the signal
    let result = rx.await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_receiver_dropped_before_trigger() {
    let agent_id = "test-agent-4".to_string();

    let rx = register_backgroundable_agent(agent_id.clone());
    drop(rx); // Drop the receiver

    // Triggering should still work (returns false since receiver is closed)
    let triggered = trigger_background_transition(&agent_id);
    assert!(!triggered);
}

#[test]
fn test_backgroundable_agent_ids() {
    let id1 = "bg-list-1".to_string();
    let id2 = "bg-list-2".to_string();

    let _rx1 = register_backgroundable_agent(id1.clone());
    let _rx2 = register_backgroundable_agent(id2.clone());

    let ids = backgroundable_agent_ids();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));

    unregister_backgroundable_agent(&id1);
    unregister_backgroundable_agent(&id2);
}