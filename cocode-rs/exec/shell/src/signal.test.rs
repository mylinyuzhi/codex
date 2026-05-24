use super::*;

#[test]
fn test_register_and_unregister() {
    let id = "test-bash-1".to_string();

    let _rx = register_backgroundable_bash(id.clone());
    assert!(backgroundable_bash_ids().contains(&id));

    unregister_backgroundable_bash(&id);
    assert!(!backgroundable_bash_ids().contains(&id));
}

#[test]
fn test_trigger_removes_from_map() {
    let id = "test-bash-2".to_string();

    let _rx = register_backgroundable_bash(id.clone());
    assert!(backgroundable_bash_ids().contains(&id));

    let triggered = trigger_bash_background(&id);
    assert!(triggered);

    // Should be removed after trigger
    assert!(!backgroundable_bash_ids().contains(&id));
}

#[test]
fn test_trigger_nonexistent() {
    let triggered = trigger_bash_background("nonexistent-bash");
    assert!(!triggered);
}

#[tokio::test]
async fn test_signal_received() {
    let id = "test-bash-3".to_string();

    let rx = register_backgroundable_bash(id.clone());

    let id_clone = id.clone();
    tokio::spawn(async move {
        trigger_bash_background(&id_clone);
    });

    let result = rx.await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_receiver_dropped_before_trigger() {
    let id = "test-bash-4".to_string();

    let rx = register_backgroundable_bash(id.clone());
    drop(rx);

    let triggered = trigger_bash_background(&id);
    assert!(!triggered);
}

#[test]
fn test_backgroundable_bash_ids() {
    let id1 = "bg-bash-list-1".to_string();
    let id2 = "bg-bash-list-2".to_string();

    let _rx1 = register_backgroundable_bash(id1.clone());
    let _rx2 = register_backgroundable_bash(id2.clone());

    let ids = backgroundable_bash_ids();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));

    unregister_backgroundable_bash(&id1);
    unregister_backgroundable_bash(&id2);
}
