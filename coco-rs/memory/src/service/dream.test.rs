use super::*;
use crate::config::MemoryConfig;
use crate::service::test_support::RecordingHandle;
use tempfile::tempdir;

#[tokio::test]
async fn skips_when_disabled() {
    let temp = tempdir().unwrap();
    let cfg = MemoryConfig {
        dream_enabled: false,
        ..MemoryConfig::default()
    };
    let svc = DreamService::new(
        temp.path().into(),
        cfg,
        std::sync::Arc::new(RecordingHandle::default()),
    );
    let outcome = svc.maybe_consolidate(temp.path(), &[], 0).await;
    assert_eq!(outcome, DreamOutcome::Skipped(SkipReason::Disabled));
}

#[tokio::test]
async fn skips_in_kairos_mode() {
    let temp = tempdir().unwrap();
    let cfg = MemoryConfig {
        kairos_mode: true,
        ..MemoryConfig::default()
    };
    let svc = DreamService::new(
        temp.path().into(),
        cfg,
        std::sync::Arc::new(RecordingHandle::default()),
    );
    let outcome = svc.maybe_consolidate(temp.path(), &[], 0).await;
    assert_eq!(outcome, DreamOutcome::Skipped(SkipReason::KairosMode));
}

#[tokio::test]
async fn skips_on_session_gate() {
    let temp = tempdir().unwrap();
    let svc = DreamService::new(
        temp.path().into(),
        MemoryConfig::default(),
        std::sync::Arc::new(RecordingHandle::default()),
    );
    // No prior consolidation → time gate passes (no last mtime).
    let outcome = svc
        .maybe_consolidate(temp.path(), &["s1".into(), "s2".into()], 0)
        .await;
    match outcome {
        DreamOutcome::Skipped(SkipReason::SessionGate { sessions_seen }) => {
            assert_eq!(sessions_seen, 2);
        }
        other => panic!("expected SessionGate, got {other:?}"),
    }
}

#[tokio::test]
async fn fires_with_dream_constraints() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = DreamService::new(temp.path().into(), MemoryConfig::default(), handle.clone());
    let sessions = vec![
        "s1".into(),
        "s2".into(),
        "s3".into(),
        "s4".into(),
        "s5".into(),
    ];
    let outcome = svc
        .maybe_consolidate(temp.path(), &sessions, DreamService::now_ms())
        .await;
    assert!(matches!(outcome, DreamOutcome::Completed { .. }));
    let calls = handle.calls();
    assert_eq!(calls.len(), 1);
    let constraints = calls[0].constraints.as_ref().expect("constraints");
    assert_eq!(constraints.max_turns, Some(20));
    assert_eq!(
        constraints.allowed_write_roots,
        vec![temp.path().to_path_buf()]
    );
}

#[tokio::test]
async fn second_call_within_throttle_skips() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = DreamService::new(temp.path().into(), MemoryConfig::default(), handle.clone());
    let sessions = vec![
        "s1".into(),
        "s2".into(),
        "s3".into(),
        "s4".into(),
        "s5".into(),
    ];
    let _first = svc
        .maybe_consolidate(temp.path(), &sessions, DreamService::now_ms())
        .await;
    let second = svc
        .maybe_consolidate(temp.path(), &sessions, DreamService::now_ms())
        .await;
    assert_eq!(second, DreamOutcome::Skipped(SkipReason::ScanThrottled));
}
