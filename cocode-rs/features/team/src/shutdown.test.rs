use super::*;

#[tokio::test]
async fn request_and_check_pending() {
    let tracker = ShutdownTracker::new();
    assert!(!tracker.is_pending("a1").await);

    tracker.request("a1", "lead").await.unwrap();
    assert!(tracker.is_pending("a1").await);
}

#[tokio::test]
async fn acknowledge_still_pending() {
    let tracker = ShutdownTracker::new();
    tracker.request("a1", "lead").await.unwrap();
    tracker.acknowledge("a1").await.unwrap();
    // Acknowledged but not completed
    assert!(tracker.is_pending("a1").await);
}

#[tokio::test]
async fn complete_no_longer_pending() {
    let tracker = ShutdownTracker::new();
    tracker.request("a1", "lead").await.unwrap();
    tracker.acknowledge("a1").await.unwrap();
    tracker.complete("a1").await.unwrap();
    assert!(!tracker.is_pending("a1").await);
}

#[tokio::test]
async fn all_complete() {
    let tracker = ShutdownTracker::new();
    let members = vec!["a1".to_string(), "a2".to_string()];

    tracker.request("a1", "lead").await.unwrap();
    tracker.request("a2", "lead").await.unwrap();
    assert!(!tracker.all_complete(&members).await);

    tracker.complete("a1").await.unwrap();
    assert!(!tracker.all_complete(&members).await);

    tracker.complete("a2").await.unwrap();
    assert!(tracker.all_complete(&members).await);
}

#[tokio::test]
async fn get_state() {
    let tracker = ShutdownTracker::new();
    assert!(tracker.get_state("a1").await.is_none());

    tracker.request("a1", "lead").await.unwrap();
    let state = tracker.get_state("a1").await.unwrap();
    assert!(matches!(state, ShutdownState::Requested { from, .. } if from == "lead"));
}

#[tokio::test]
async fn remove_tracking() {
    let tracker = ShutdownTracker::new();
    tracker.request("a1", "lead").await.unwrap();
    tracker.remove("a1").await;
    assert!(tracker.get_state("a1").await.is_none());
}
