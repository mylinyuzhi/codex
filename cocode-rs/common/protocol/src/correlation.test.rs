use super::*;

#[test]
fn test_submission_id_new() {
    let id1 = SubmissionId::new();
    let id2 = SubmissionId::new();
    // UUIDs should be unique
    assert_ne!(id1, id2);
    // Should be valid UUID format (36 chars with hyphens)
    assert_eq!(id1.as_str().len(), 36);
}

#[test]
fn test_submission_id_from_string() {
    let id = SubmissionId::from_string("test-id");
    assert_eq!(id.as_str(), "test-id");
    assert_eq!(id.to_string(), "test-id");
}

#[test]
fn test_submission_id_conversions() {
    let id: SubmissionId = "test".into();
    assert_eq!(id.as_str(), "test");

    let id: SubmissionId = String::from("test2").into();
    assert_eq!(id.as_str(), "test2");

    let inner = id.into_inner();
    assert_eq!(inner, "test2");
}

#[test]
fn test_correlated_event_uncorrelated() {
    let event = LoopEvent::StreamRequestStart;
    let correlated = CorrelatedEvent::uncorrelated(event);

    assert!(!correlated.has_correlation());
    assert!(correlated.correlation_id().is_none());
    assert!(matches!(correlated.event(), LoopEvent::StreamRequestStart));
}

#[test]
fn test_correlated_event_with_id() {
    let event = LoopEvent::StreamRequestStart;
    let id = SubmissionId::from_string("sub-123");
    let correlated = CorrelatedEvent::correlated(event, id);

    assert!(correlated.has_correlation());
    assert_eq!(correlated.correlation_id().unwrap().as_str(), "sub-123");
}

#[test]
fn test_correlated_event_into_parts() {
    let event = LoopEvent::StreamRequestStart;
    let id = SubmissionId::from_string("sub-123");
    let correlated = CorrelatedEvent::correlated(event, id);

    let (correlation, event) = correlated.into_parts();
    assert!(correlation.is_some());
    assert!(matches!(event, LoopEvent::StreamRequestStart));
}

#[test]
fn test_correlated_event_from_conversions() {
    // From LoopEvent
    let event = LoopEvent::StreamRequestStart;
    let correlated: CorrelatedEvent = event.into();
    assert!(!correlated.has_correlation());

    // From (LoopEvent, SubmissionId)
    let event = LoopEvent::StreamRequestStart;
    let id = SubmissionId::from_string("id");
    let correlated: CorrelatedEvent = (event, id).into();
    assert!(correlated.has_correlation());

    // From (LoopEvent, Option<SubmissionId>)
    let event = LoopEvent::StreamRequestStart;
    let correlated: CorrelatedEvent = (event, None).into();
    assert!(!correlated.has_correlation());
}

#[test]
fn test_correlated_event_serde() {
    let event = LoopEvent::TurnStarted {
        turn_id: "turn-1".to_string(),
        turn_number: 1,
    };
    let id = SubmissionId::from_string("sub-123");
    let correlated = CorrelatedEvent::correlated(event, id);

    let json = serde_json::to_string(&correlated).unwrap();
    assert!(json.contains("sub-123"));
    assert!(json.contains("turn_started"));

    let parsed: CorrelatedEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.correlation_id().unwrap().as_str(), "sub-123");
}

#[test]
fn test_submission_id_serde() {
    let id = SubmissionId::from_string("test-id");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"test-id\"");

    let parsed: SubmissionId = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_str(), "test-id");
}
