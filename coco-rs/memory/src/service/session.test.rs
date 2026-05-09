use super::*;
use crate::config::MemoryConfig;
use crate::service::test_support::RecordingHandle;
use tempfile::tempdir;

fn cfg() -> MemoryConfig {
    MemoryConfig::default()
}

fn msg_id(s: &str) -> Option<String> {
    Some(s.to_string())
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
    let outcome = svc.maybe_extract(20_000, 5, true, msg_id("u1")).await;
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
    let outcome = svc.maybe_extract(5_000, 5, true, msg_id("u1")).await;
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
    let outcome = svc.maybe_extract(15_000, 5, true, msg_id("u1")).await;
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
    // Cursor advanced — TS parity with `lastMemoryMessageUuid`.
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u1")
    );
}

#[tokio::test]
async fn update_skips_until_token_growth_satisfies_gate() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    // Init.
    assert!(matches!(
        svc.maybe_extract(12_000, 5, true, msg_id("u1")).await,
        SessionMemoryOutcome::Completed { .. },
    ));
    // Tiny growth — skipped.
    assert_eq!(
        svc.maybe_extract(13_000, 5, true, msg_id("u2")).await,
        SessionMemoryOutcome::Skipped(SkipReason::BelowUpdateThreshold)
    );
    // Growth satisfies threshold + cumulative tool-call gate — fires.
    assert!(matches!(
        svc.maybe_extract(20_000, 5, true, msg_id("u3")).await,
        SessionMemoryOutcome::Completed { .. },
    ));
    // Cursor advances on each successful gate pass.
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u3")
    );
}

#[tokio::test]
async fn natural_break_fires_when_no_tool_calls_last_turn() {
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    let _ = svc.maybe_extract(12_000, 5, true, msg_id("u1")).await;
    // No tool calls last turn; growth satisfies → natural break.
    assert!(matches!(
        svc.maybe_extract(20_000, 0, false, msg_id("u2")).await,
        SessionMemoryOutcome::Completed { .. },
    ));
}

#[tokio::test]
async fn cumulative_tool_gate_skips_when_below_threshold() {
    // TS parity (`sessionMemory.ts:150-156`): cumulative tool-call count
    // since last extraction, NOT just the most-recent turn. With the
    // default threshold of 3, a turn that brings the cumulative to 2 (2
    // calls in last turn after a 0-call last extraction baseline) should
    // be gated unless natural-break also fires.
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());
    // Init at threshold.
    let _ = svc.maybe_extract(12_000, 5, true, msg_id("u1")).await;
    assert_eq!(handle.calls().len(), 1);
    // Cumulative=2 < 3, last turn HAD tool calls → both gates fail.
    let outcome = svc.maybe_extract(20_000, 2, true, msg_id("u2")).await;
    assert_eq!(
        outcome,
        SessionMemoryOutcome::Skipped(SkipReason::NeitherToolCallsNorBreak)
    );
    assert_eq!(handle.calls().len(), 1, "extraction must not fire");
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

#[tokio::test]
async fn summarized_cursor_only_advances_when_prior_turn_has_no_tool_calls() {
    // TS parity (`sessionMemory.ts:488-494
    // updateLastSummarizedMessageIdIfSafe`): the safely-summarized
    // cursor should NOT advance when the last assistant turn used
    // tools — preserving compact's ability to orphan-safely splice
    // SM into a downstream summary.
    let temp = tempdir().unwrap();
    let handle = std::sync::Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new("s1".into(), temp.path().into(), cfg(), handle.clone());

    // Init turn used tools — extraction cursor advances, summarized cursor does NOT.
    let _ = svc
        .maybe_extract(15_000, 5, /*had_tool_calls=*/ true, msg_id("u1"))
        .await;
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u1")
    );
    assert!(
        svc.last_summarized_message_id().await.is_none(),
        "summarized cursor must stay None when prior turn had tool calls"
    );

    // Next gate-passing turn with no tool calls: both cursors advance.
    let _ = svc
        .maybe_extract(25_000, 0, /*had_tool_calls=*/ false, msg_id("u2"))
        .await;
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u2")
    );
    assert_eq!(
        svc.last_summarized_message_id().await.as_deref(),
        Some("u2"),
        "summarized cursor must advance when prior turn had no tool calls"
    );
}

#[tokio::test]
async fn is_empty_true_until_real_content_written() {
    // TS `isSessionMemoryEmpty` (`prompts.ts:220-224`): returns true
    // when the file matches the seed template byte-for-byte (after
    // trim). compact uses this to fall back to LLM summarization when
    // the SM file hasn't accumulated real content yet.
    let temp = tempdir().unwrap();
    let svc = SessionMemoryService::new(
        "s1".into(),
        temp.path().into(),
        cfg(),
        std::sync::Arc::new(RecordingHandle::default()),
    );
    // No file yet → empty (compact fallback should fire).
    assert!(svc.is_empty().await);

    // Seed with the verbatim template → still "empty" (no real content).
    let template = crate::prompt::build_session_memory_template();
    std::fs::create_dir_all(svc.file_path().parent().unwrap()).unwrap();
    std::fs::write(svc.file_path(), template).unwrap();
    assert!(svc.is_empty().await);

    // Append real content → no longer empty.
    let mut full = template.to_string();
    full.push_str("\nReal extracted content here.\n");
    std::fs::write(svc.file_path(), &full).unwrap();
    assert!(!svc.is_empty().await);
}

#[tokio::test]
async fn custom_template_override_replaces_seed() {
    // TS `loadSessionMemoryTemplate` (`prompts.ts:86-104`): user-provided
    // template at `<session_memory_dir>/config/template.md` overrides
    // the static default for the seed write.
    let temp = tempdir().unwrap();
    let custom = "# Custom Title\n_custom hint_\n\n# Custom Section\n_custom_\n";
    let config_dir = temp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("template.md"), custom).unwrap();

    let svc = SessionMemoryService::new(
        "s1".into(),
        temp.path().into(),
        cfg(),
        std::sync::Arc::new(RecordingHandle::default()),
    );
    let _ = svc.maybe_extract(15_000, 5, true, msg_id("u1")).await;
    let on_disk = std::fs::read_to_string(svc.file_path()).unwrap();
    assert!(
        on_disk.contains("# Custom Title"),
        "expected custom template override to land on disk, got: {on_disk}"
    );
    assert!(
        !on_disk.contains("# Worklog"),
        "default 9-section template must not appear when override is in place"
    );
}
