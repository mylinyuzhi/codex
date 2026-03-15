use super::*;

#[tokio::test]
async fn test_speculation_lifecycle() {
    let tracker = SpeculationTracker::new();

    // Start speculation
    let spec_id = tracker
        .start_speculation(vec!["call-1".to_string(), "call-2".to_string()])
        .await;
    assert!(spec_id.starts_with("spec-"));

    // Record results
    tracker
        .record_result(
            &spec_id,
            "call-1",
            "Read",
            SpeculativeResult {
                content: "file contents".to_string(),
                is_error: false,
            },
        )
        .await;

    // Check state
    assert_eq!(
        tracker.get_state(&spec_id).await,
        Some(SpeculationState::Pending)
    );
    assert!(tracker.is_speculative("call-1").await);
    assert!(tracker.is_speculative("call-2").await);
    assert!(!tracker.is_speculative("call-3").await);

    // Commit
    let results = tracker.commit(&spec_id).await;
    assert!(results.is_some());
    assert_eq!(results.unwrap().len(), 1); // Only one result recorded

    // Check committed state
    assert_eq!(
        tracker.get_state(&spec_id).await,
        Some(SpeculationState::Committed)
    );
}

#[tokio::test]
async fn test_speculation_rollback() {
    let tracker = SpeculationTracker::new();

    let spec_id = tracker.start_speculation(vec!["call-1".to_string()]).await;

    tracker
        .record_result(
            &spec_id,
            "call-1",
            "Read",
            SpeculativeResult {
                content: "data".to_string(),
                is_error: false,
            },
        )
        .await;

    // Rollback
    let rolled_back = tracker.rollback(&spec_id, "Model reconsideration").await;
    assert_eq!(rolled_back.len(), 1);
    assert_eq!(rolled_back[0], "call-1");

    // Check state
    assert_eq!(
        tracker.get_state(&spec_id).await,
        Some(SpeculationState::RolledBack)
    );
}

#[tokio::test]
async fn test_speculation_stats() {
    let tracker = SpeculationTracker::new();

    let spec_id1 = tracker.start_speculation(vec!["call-1".to_string()]).await;
    let spec_id2 = tracker.start_speculation(vec!["call-2".to_string()]).await;

    let stats = tracker.stats().await;
    assert_eq!(stats.pending, 2);
    assert_eq!(stats.committed, 0);
    assert_eq!(stats.total, 2);

    tracker.commit(&spec_id1).await;

    let stats = tracker.stats().await;
    assert_eq!(stats.pending, 1);
    assert_eq!(stats.committed, 1);

    tracker.rollback(&spec_id2, "test").await;

    let stats = tracker.stats().await;
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.committed, 1);
    assert_eq!(stats.rolled_back, 1);
}

#[tokio::test]
async fn test_commit_all() {
    let tracker = SpeculationTracker::new();

    tracker.start_speculation(vec!["call-1".to_string()]).await;
    tracker.start_speculation(vec!["call-2".to_string()]).await;

    let committed = tracker.commit_all().await;
    assert_eq!(committed, 2);

    let stats = tracker.stats().await;
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.committed, 2);
}

#[tokio::test]
async fn test_rollback_all() {
    let tracker = SpeculationTracker::new();

    tracker.start_speculation(vec!["call-1".to_string()]).await;
    tracker.start_speculation(vec!["call-2".to_string()]).await;

    let rolled_back = tracker.rollback_all("stream error").await;
    assert_eq!(rolled_back, 2);

    let stats = tracker.stats().await;
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.rolled_back, 2);
}

#[tokio::test]
async fn test_cleanup_completed() {
    let tracker = SpeculationTracker::new();

    let spec_id = tracker.start_speculation(vec!["call-1".to_string()]).await;
    tracker.commit(&spec_id).await;

    assert_eq!(tracker.stats().await.total, 1);

    tracker.cleanup_completed().await;

    assert_eq!(tracker.stats().await.total, 0);
}

#[test]
fn test_speculation_state_display() {
    assert_eq!(SpeculationState::Pending.as_str(), "pending");
    assert_eq!(SpeculationState::Committed.as_str(), "committed");
    assert_eq!(SpeculationState::RolledBack.as_str(), "rolled_back");
}
