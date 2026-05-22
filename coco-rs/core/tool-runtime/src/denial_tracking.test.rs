use super::*;

#[test]
fn test_circuit_breaker_trips_at_threshold() {
    let mut tracker = DenialTracker::new();
    assert!(!tracker.is_circuit_breaker_tripped());

    tracker.record_denial("Bash");
    tracker.record_denial("Bash");
    assert!(!tracker.is_circuit_breaker_tripped());

    tracker.record_denial("Bash");
    assert!(tracker.is_circuit_breaker_tripped());
    assert!(tracker.is_stuck());
}

#[test]
fn test_reset_consecutive_does_not_reset_circuit_breaker() {
    let mut tracker = DenialTracker::new();
    for _ in 0..3 {
        tracker.record_denial("Bash");
    }
    assert!(tracker.is_circuit_breaker_tripped());

    tracker.reset_consecutive();
    assert!(!tracker.is_stuck());
    // Circuit breaker stays tripped until explicitly reset.
    assert!(tracker.is_circuit_breaker_tripped());
}

#[test]
fn test_reset_circuit_breaker() {
    let mut tracker = DenialTracker::new();
    for _ in 0..3 {
        tracker.record_denial("Bash");
    }
    tracker.reset_circuit_breaker();
    assert!(!tracker.is_circuit_breaker_tripped());
    assert!(!tracker.is_stuck());
}

#[test]
fn test_per_tool_tracking() {
    let mut tracker = DenialTracker::new();
    tracker.record_denial("Bash");
    tracker.record_denial("Bash");
    tracker.record_denial("Write");

    assert_eq!(tracker.tool_denial_count("Bash"), 2);
    assert_eq!(tracker.tool_denial_count("Write"), 1);
    assert_eq!(tracker.most_denied_tool(), Some(("Bash", 2)));
}

#[test]
fn test_suggestion_message() {
    let mut tracker = DenialTracker::new();
    assert!(tracker.suggestion_message().is_none());

    for _ in 0..3 {
        tracker.record_denial("Bash");
    }
    let msg = tracker.suggestion_message().unwrap();
    assert!(msg.contains("consecutive"));
    assert!(msg.contains("Bash"));
}

#[test]
fn test_subagent_fork_isolation_no_parent_pollution() {
    // Regression: parent's denial count must not bleed into a fork.
    // TS parity: createSubagentContext always builds a fresh DenialTracker.
    let mut parent = DenialTracker::new();
    let mut fork = DenialTracker::new();

    // Fork hits 3 denies and trips its own breaker.
    for _ in 0..3 {
        fork.record_denial("Bash");
    }
    assert!(fork.is_circuit_breaker_tripped());

    // Parent stays clean.
    assert_eq!(parent.consecutive_denials, 0);
    assert!(!parent.is_circuit_breaker_tripped());

    // Parent denials still trip independently.
    for _ in 0..3 {
        parent.record_denial("Write");
    }
    assert!(parent.is_circuit_breaker_tripped());
    assert_eq!(parent.tool_denial_count("Bash"), 0);
}
