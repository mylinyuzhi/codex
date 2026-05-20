use super::*;
use crate::config::MemoryConfig;
use crate::service::test_support::RecordingHandle;
use coco_paths::ProjectPaths;
use std::sync::Arc;
use tempfile::tempdir;

fn cfg() -> MemoryConfig {
    MemoryConfig::default()
}

fn msg_id(s: &str) -> Option<String> {
    Some(s.to_string())
}

/// Build a `ProjectPaths` rooted at `base`, with the project root also
/// at `base` so the slug is deterministic for tests.
fn pp(base: &std::path::Path) -> Arc<ProjectPaths> {
    Arc::new(ProjectPaths::new(base.to_path_buf(), base))
}

#[tokio::test]
async fn skips_when_disabled() {
    let temp = tempdir().unwrap();
    let config = MemoryConfig {
        session_memory_enabled: false,
        ..cfg()
    };
    let svc = SessionMemoryService::new(
        pp(temp.path()),
        "s1".into(),
        config,
        Arc::new(RecordingHandle::default()),
    );
    let outcome = svc.maybe_extract(20_000, 5, true, msg_id("u1")).await;
    assert_eq!(outcome, SessionMemoryOutcome::Skipped(SkipReason::Disabled));
}

#[tokio::test]
async fn skips_below_init_threshold() {
    let temp = tempdir().unwrap();
    let svc = SessionMemoryService::new(
        pp(temp.path()),
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
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
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());
    let outcome = svc.maybe_extract(15_000, 5, true, msg_id("u1")).await;
    assert!(matches!(outcome, SessionMemoryOutcome::Completed { .. }));
    let calls = handle.calls();
    assert_eq!(calls.len(), 1);
    let constraints = calls[0].constraints.as_ref().unwrap();
    // `extraction_max_turns.max(5)` — the old `Some(3)` cap silently
    // truncated SM updates for models that prefer one-section-per-turn
    // pacing. Now matches `extraction_max_turns` (default 5).
    assert_eq!(constraints.max_turns, Some(5));
    // Seed file exists on disk with the 9-section template.
    let path = svc.file_path();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("# Session Title"));
    assert!(content.contains("# Worklog"));
    // Path layout mirrors TS: `<projectDir>/<sid>/session-memory/summary.md`.
    assert!(
        path.ends_with("session-memory/summary.md"),
        "expected TS summary.md filename, got: {}",
        path.display()
    );
    // Cursor advanced — TS parity with `lastMemoryMessageUuid`.
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u1")
    );
}

#[tokio::test]
async fn update_skips_until_token_growth_satisfies_gate() {
    let temp = tempdir().unwrap();
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());
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
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());
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
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());
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
        pp(temp.path()),
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
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
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());

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
        pp(temp.path()),
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
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
    // User-provided template at `<session-memory-dir>/config/template.md`
    // overrides the static default for the seed write.
    let temp = tempdir().unwrap();
    let project_paths = pp(temp.path());
    // The override lives under the per-session SM dir (file_path.parent()).
    let sm_dir = project_paths.session_memory_dir("s1");
    let config_dir = sm_dir.join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let custom = "# Custom Title\n_custom hint_\n\n# Custom Section\n_custom_\n";
    std::fs::write(config_dir.join("template.md"), custom).unwrap();

    let svc = SessionMemoryService::new(
        project_paths,
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
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

#[tokio::test]
async fn set_session_id_repaths_writes_and_wipes_state() {
    // After /clear regen, subsequent reads/writes must land in the
    // new session's directory, and stale in-memory state must be
    // wiped so the next gate starts from a clean baseline.
    let temp = tempdir().unwrap();
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "old".into(), cfg(), handle.clone());
    // Initial extract under "old" session.
    let _ = svc.maybe_extract(15_000, 5, true, msg_id("u1")).await;
    assert!(svc.file_path().to_string_lossy().contains("/old/"));
    assert_eq!(
        svc.last_extraction_message_id().await.as_deref(),
        Some("u1")
    );

    svc.set_session_id("new".into()).await;
    assert!(svc.file_path().to_string_lossy().contains("/new/"));
    assert!(svc.last_extraction_message_id().await.is_none());
    assert!(svc.current_text().await.is_empty());
}

#[tokio::test]
async fn current_text_caches_after_extract_and_clears_on_compact() {
    let temp = tempdir().unwrap();
    let handle = Arc::new(RecordingHandle::default());
    let svc = SessionMemoryService::new(pp(temp.path()), "s1".into(), cfg(), handle.clone());
    assert!(svc.current_text().await.is_empty());
    let _ = svc.maybe_extract(15_000, 5, true, msg_id("u1")).await;
    let cached = svc.current_text().await;
    assert!(
        cached.contains("# Session Title"),
        "post-extract cache must mirror seeded file body"
    );

    svc.clear_after_compact().await;
    assert!(
        svc.current_text().await.is_empty(),
        "clear_after_compact must wipe the text cache"
    );
    assert!(
        svc.last_extraction_message_id().await.is_none(),
        "clear_after_compact must wipe the extraction cursor"
    );
}

#[tokio::test]
async fn load_from_disk_warms_cache_from_existing_file() {
    let temp = tempdir().unwrap();
    let project_paths = pp(temp.path());
    let svc = SessionMemoryService::new(
        project_paths.clone(),
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
    );
    // Pre-seed the on-disk SM file before calling load_from_disk.
    let path = svc.file_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "preseeded body").unwrap();
    svc.load_from_disk().await;
    assert_eq!(svc.current_text().await, "preseeded body");
}

#[tokio::test]
async fn last_summarized_message_uuid_accessor_parses_string_cursor() {
    let temp = tempdir().unwrap();
    let svc = SessionMemoryService::new(
        pp(temp.path()),
        "s1".into(),
        cfg(),
        Arc::new(RecordingHandle::default()),
    );
    let uuid = uuid::Uuid::new_v4();
    svc.set_last_summarized_message_id(Some(uuid)).await;
    assert_eq!(svc.last_summarized_message_uuid().await, Some(uuid));
    svc.set_last_summarized_message_id(None).await;
    assert!(svc.last_summarized_message_uuid().await.is_none());
}
