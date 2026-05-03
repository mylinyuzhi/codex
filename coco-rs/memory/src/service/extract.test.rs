use super::*;
use crate::config::MemoryConfig;
use crate::service::test_support::RecordingHandle;
use tempfile::tempdir;

fn config() -> MemoryConfig {
    MemoryConfig::default()
}

fn turn_input(message_count: i32, has_writes: bool) -> TurnInput {
    TurnInput {
        fork_messages: Box::new(|| vec![serde_json::json!({"role": "user", "content": "hi"})]),
        message_count,
        last_message_id: Some("uuid".into()),
        has_memory_writes: has_writes,
    }
}

#[tokio::test]
async fn skips_when_extraction_disabled() {
    let temp = tempdir().unwrap();
    let cfg = MemoryConfig {
        extraction_enabled: false,
        ..config()
    };
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = ExtractService::new(temp.path().into(), cfg, handle.clone());
    let outcome = svc.maybe_extract(turn_input(20, false)).await;
    assert_eq!(outcome, ExtractOutcome::Skipped(SkipReason::Disabled));
    assert!(handle.calls().is_empty());
}

#[tokio::test]
async fn skips_when_main_agent_wrote_memory() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = ExtractService::new(temp.path().into(), config(), handle.clone());
    let outcome = svc.maybe_extract(turn_input(20, true)).await;
    assert_eq!(outcome, ExtractOutcome::Skipped(SkipReason::DirectWrite));
    assert!(handle.calls().is_empty());
}

#[tokio::test]
async fn throttle_skips_until_threshold() {
    let temp = tempdir().unwrap();
    let cfg = MemoryConfig {
        extraction_throttle: 3,
        ..config()
    };
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = ExtractService::new(temp.path().into(), cfg, handle.clone());

    let r1 = svc.maybe_extract(turn_input(10, false)).await;
    let r2 = svc.maybe_extract(turn_input(10, false)).await;
    let r3 = svc.maybe_extract(turn_input(10, false)).await;
    assert_eq!(r1, ExtractOutcome::Skipped(SkipReason::Throttled));
    assert_eq!(r2, ExtractOutcome::Skipped(SkipReason::Throttled));
    assert!(matches!(r3, ExtractOutcome::Completed { .. }));
    assert_eq!(handle.calls().len(), 1);
}

#[tokio::test]
async fn fires_with_constraints_and_fork_messages() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = ExtractService::new(temp.path().into(), config(), handle.clone());
    let outcome = svc.force(turn_input(20, false)).await;
    assert!(matches!(outcome, ExtractOutcome::Completed { .. }));
    let calls = handle.calls();
    assert_eq!(calls.len(), 1);
    let constraints = calls[0].constraints.as_ref().expect("constraints");
    assert_eq!(constraints.max_turns, Some(5));
    assert_eq!(
        constraints.allowed_write_roots,
        vec![temp.path().to_path_buf()]
    );
    // Fork mode + parent context propagation.
    assert_eq!(calls[0].isolation.as_deref(), Some("fork"));
    assert_eq!(calls[0].fork_context_messages.len(), 1);
}

#[tokio::test]
async fn drain_returns_true_when_idle() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = ExtractService::new(temp.path().into(), config(), handle);
    assert!(svc.drain(std::time::Duration::from_millis(20)).await);
}
