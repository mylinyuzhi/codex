use super::*;
use crate::server_notification::*;

/// Helper to create a simple protocol CoreEvent for testing.
fn interrupted_event() -> CoreEvent {
    CoreEvent::Protocol(ServerNotification::TurnInterrupted(TurnInterruptedParams {
        turn_id: None,
    }))
}

/// Helper to check if a CoreEvent is a TurnInterrupted notification.
fn is_interrupted(event: &CoreEvent) -> bool {
    matches!(
        event,
        CoreEvent::Protocol(ServerNotification::TurnInterrupted(_))
    )
}

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
    let event = interrupted_event();
    let correlated = CorrelatedEvent::uncorrelated(event);

    assert!(!correlated.has_correlation());
    assert!(correlated.correlation_id().is_none());
    assert!(is_interrupted(correlated.event()));
}

#[test]
fn test_correlated_event_with_id() {
    let event = interrupted_event();
    let id = SubmissionId::from_string("sub-123");
    let correlated = CorrelatedEvent::correlated(event, id);

    assert!(correlated.has_correlation());
    assert_eq!(correlated.correlation_id().unwrap().as_str(), "sub-123");
}

#[test]
fn test_correlated_event_into_parts() {
    let event = interrupted_event();
    let id = SubmissionId::from_string("sub-123");
    let correlated = CorrelatedEvent::correlated(event, id);

    let (correlation, event) = correlated.into_parts();
    assert!(correlation.is_some());
    assert!(is_interrupted(&event));
}

#[test]
fn test_correlated_event_from_conversions() {
    // From CoreEvent
    let event = interrupted_event();
    let correlated: CorrelatedEvent = event.into();
    assert!(!correlated.has_correlation());

    // From (CoreEvent, SubmissionId)
    let event = interrupted_event();
    let id = SubmissionId::from_string("id");
    let correlated: CorrelatedEvent = (event, id).into();
    assert!(correlated.has_correlation());

    // From (CoreEvent, Option<SubmissionId>)
    let event = interrupted_event();
    let correlated: CorrelatedEvent = (event, None).into();
    assert!(!correlated.has_correlation());
}

#[test]
fn test_submission_id_serde() {
    let id = SubmissionId::from_string("test-id");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"test-id\"");

    let parsed: SubmissionId = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_str(), "test-id");
}
