use super::*;

#[test]
fn test_event_stream_config_default() {
    let config = EventStreamConfig::default();
    assert_eq!(config.tick_interval, Duration::from_millis(250));
    assert_eq!(config.draw_interval, Duration::from_millis(16));
}

#[test]
fn test_broker_pause_state() {
    // Test broker pause/resume logic without creating EventStream
    // (EventStream requires a real terminal)
    let broker = Arc::new(EventBroker::new());

    assert!(!broker.is_paused());

    broker.pause();
    assert!(broker.is_paused());

    broker.resume();
    assert!(!broker.is_paused());
}
