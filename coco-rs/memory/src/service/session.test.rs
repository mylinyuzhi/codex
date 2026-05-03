use super::*;
use crate::config::MemoryConfig;
use crate::service::test_support::RecordingHandle;
use tempfile::tempdir;

fn cfg() -> MemoryConfig {
    MemoryConfig::default()
}

#[tokio::test]
async fn skips_when_disabled() {
    let temp = tempdir().unwrap();
    let config = MemoryConfig {
        session_memory_enabled: false,
        ..cfg()
    };
    let svc = SessionMemoryService::new(
        "s1".into(),
        temp.path().into(),
        config,
        std::sync::Arc::new(RecordingHandle::default()),
    );
    let outcome = svc.maybe_extract(20_000, 5, true).await;
    assert_eq!(outcome, SessionMemoryOutcome::Skipped(SkipReason::Disabled));
}

#[tokio::test]
async fn skips_below_init_threshold() {
    let temp = tempdir().unwrap();
    let svc = SessionMemoryService::new(
        "s1".into(),
        temp.path().into(),
        cfg(),
        std::sync::Arc::new(RecordingHandle::default()),
    );
    let outcome = svc.maybe_extract(5_000, 5, true).await;
    assert_eq!(
        outcome,
        SessionMemoryOutcome::Skipped(SkipReason::BelowInitThreshold)
    );
}

#[tokio::test]
async fn fires_at_init_with_template_seeded() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    let outcome = svc.maybe_extract(15_000, 5, true).await;
    assert!(matches!(outcome, SessionMemoryOutcome::Completed { .. }));
    let calls = handle.calls();
    assert_eq!(calls.len(), 1);
    let constraints = calls[0].constraints.as_ref().unwrap();
    assert_eq!(constraints.max_turns, Some(3));
    // Seed file exists on disk with the 9-section template.
    let path = svc.file_path();
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("# Session Title"));
    assert!(content.contains("# Worklog"));
}

#[tokio::test]
async fn update_skips_until_token_growth_satisfies_gate() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    // Init.
    assert!(matches!(
        svc.maybe_extract(12_000, 5, true).await,
        SessionMemoryOutcome::Completed { .. },
    ));
    // Tiny growth — skipped.
    assert_eq!(
        svc.maybe_extract(13_000, 5, true).await,
        SessionMemoryOutcome::Skipped(SkipReason::BelowUpdateThreshold)
    );
    // Growth satisfies threshold + tool-call gate — fires.
    assert!(matches!(
        svc.maybe_extract(20_000, 5, true).await,
        SessionMemoryOutcome::Completed { .. },
    ));
}

#[tokio::test]
async fn natural_break_fires_when_no_tool_calls_last_turn() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    let _ = svc.maybe_extract(12_000, 5, true).await;
    // No tool calls last turn; growth satisfies → natural break.
    assert!(matches!(
        svc.maybe_extract(20_000, 0, false).await,
        SessionMemoryOutcome::Completed { .. },
    ));
}

#[tokio::test]
async fn current_content_returns_none_before_seed() {
    let temp = tempdir().unwrap();
    let svc = SessionMemoryService::new(
        "s1".into(),
        temp.path().into(),
        cfg(),
        std::sync::Arc::new(RecordingHandle::default()),
    );
    assert!(svc.current_content().await.is_none());
}
