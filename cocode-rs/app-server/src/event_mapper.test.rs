use super::*;

#[test]
fn test_text_delta_emits_item_started_and_delta() {
    let mut mapper = EventMapper::new("turn_1".into());
    let events = mapper.map(LoopEvent::TextDelta {
        delta: "hello".into(),
        turn_id: "turn_1".into(),
    });
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], ServerNotification::ItemStarted(_)));
    assert!(matches!(
        events[1],
        ServerNotification::AgentMessageDelta(_)
    ));
}

#[test]
fn test_flush_emits_completed_items() {
    let mut mapper = EventMapper::new("turn_1".into());
    mapper.map(LoopEvent::TextDelta {
        delta: "hello".into(),
        turn_id: "turn_1".into(),
    });
    let flushed = mapper.flush();
    assert_eq!(flushed.len(), 1);
    assert!(matches!(flushed[0], ServerNotification::ItemCompleted(_)));
}

#[test]
fn test_thinking_to_text_transition_closes_reasoning() {
    let mut mapper = EventMapper::new("turn_1".into());
    mapper.map(LoopEvent::ThinkingDelta {
        delta: "thinking...".into(),
        turn_id: "turn_1".into(),
    });
    let events = mapper.map(LoopEvent::TextDelta {
        delta: "answer".into(),
        turn_id: "turn_1".into(),
    });
    // Should close reasoning + start text item + text delta
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], ServerNotification::ItemCompleted(_)));
    assert!(matches!(events[1], ServerNotification::ItemStarted(_)));
    assert!(matches!(
        events[2],
        ServerNotification::AgentMessageDelta(_)
    ));
}
