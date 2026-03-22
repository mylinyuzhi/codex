use super::*;

#[test]
fn test_broker_pause_resume() {
    let broker = EventBroker::new();

    assert!(!broker.is_paused());
    assert_eq!(broker.pause_depth(), 0);

    broker.pause();
    assert!(broker.is_paused());
    assert_eq!(broker.pause_depth(), 1);

    broker.resume();
    assert!(!broker.is_paused());
    assert_eq!(broker.pause_depth(), 0);
}

#[test]
fn test_broker_nested_pause() {
    let broker = EventBroker::new();

    broker.pause();
    assert!(broker.is_paused());
    assert_eq!(broker.pause_depth(), 1);

    broker.pause();
    assert!(broker.is_paused());
    assert_eq!(broker.pause_depth(), 2);

    broker.resume();
    assert!(broker.is_paused()); // Still paused
    assert_eq!(broker.pause_depth(), 1);

    broker.resume();
    assert!(!broker.is_paused());
    assert_eq!(broker.pause_depth(), 0);
}

#[test]
fn test_broker_force_resume() {
    let broker = EventBroker::new();

    broker.pause();
    broker.pause();
    assert_eq!(broker.pause_depth(), 2);

    broker.force_resume();
    assert!(!broker.is_paused());
    assert_eq!(broker.pause_depth(), 0);
}

#[test]
fn test_broker_underflow_protection() {
    let broker = EventBroker::new();

    // Resume without pause should not underflow
    broker.resume();
    assert!(!broker.is_paused());
    assert_eq!(broker.pause_depth(), 0);

    broker.resume();
    assert_eq!(broker.pause_depth(), 0);
}

#[test]
fn test_pause_guard() {
    let broker = Arc::new(EventBroker::new());

    assert!(!broker.is_paused());

    {
        let _guard = PauseGuard::new(broker.clone());
        assert!(broker.is_paused());
    }

    // Guard dropped, should be resumed
    assert!(!broker.is_paused());
}
