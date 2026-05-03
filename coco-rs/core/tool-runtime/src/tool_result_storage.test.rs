use super::*;
use std::sync::Arc;
use tokio::sync::RwLock;

#[test]
fn test_resolve_persistence_threshold_opt_out() {
    assert_eq!(resolve_persistence_threshold(i64::MAX), i64::MAX);
}

#[test]
fn test_resolve_persistence_threshold_clamps_to_default() {
    assert_eq!(
        resolve_persistence_threshold(100_000),
        DEFAULT_MAX_RESULT_SIZE_CHARS
    );
    assert_eq!(resolve_persistence_threshold(10_000), 10_000);
}

#[tokio::test]
async fn test_persist_to_disk_writes_file_and_preview() {
    let tmp = tempfile::TempDir::new().unwrap();
    let big = "x".repeat(8_000);
    let result = persist_to_disk(tmp.path(), "abc", &big, false)
        .await
        .unwrap();
    assert_eq!(result.original_size, 8_000);
    assert!(result.has_more);
    assert_eq!(result.preview.len(), PREVIEW_SIZE_BYTES);
    assert_eq!(result.filepath, tool_result_path(tmp.path(), "abc", false));
    let on_disk = std::fs::read_to_string(&result.filepath).unwrap();
    assert_eq!(on_disk, big);
}

#[tokio::test]
async fn test_apply_tool_result_budget_inert_when_disabled() {
    let state: ContentReplacementStateRef =
        Arc::new(RwLock::new(ContentReplacementState::new(i64::MAX)));
    let candidates = vec![ToolResultCandidate {
        tool_use_id: "id1".into(),
        content_chars: 1_000_000,
        tool_name: Some("Bash".into()),
        persistence_opted_out: false,
    }];
    let outcome = apply_tool_result_budget(&candidates, &state).await;
    assert!(outcome.newly_replaced.is_empty());
    assert_eq!(outcome.freed_chars, 0);
    assert!(state.read().await.replacements.is_empty());
}

#[tokio::test]
async fn test_apply_tool_result_budget_evicts_oldest_until_under_cap() {
    let state: ContentReplacementStateRef =
        Arc::new(RwLock::new(ContentReplacementState::new(100)));
    // Three candidates, total 240 chars. Budget = 100. Most recent
    // (id3) is preserved. id1 (oldest) is evicted, id2 evicted next
    // → still 80 ≤ 100 → done.
    let candidates = vec![
        ToolResultCandidate {
            tool_use_id: "id1".into(),
            content_chars: 80,
            tool_name: Some("Bash".into()),
            persistence_opted_out: false,
        },
        ToolResultCandidate {
            tool_use_id: "id2".into(),
            content_chars: 80,
            tool_name: Some("Bash".into()),
            persistence_opted_out: false,
        },
        ToolResultCandidate {
            tool_use_id: "id3".into(),
            content_chars: 80,
            tool_name: Some("Bash".into()),
            persistence_opted_out: false,
        },
    ];
    let outcome = apply_tool_result_budget(&candidates, &state).await;
    assert_eq!(
        outcome.newly_replaced,
        vec!["id1".to_string(), "id2".into()]
    );
    assert_eq!(outcome.freed_chars, 160);
    let s = state.read().await;
    assert!(s.replacements.contains_key("id1"));
    assert!(s.replacements.contains_key("id2"));
    assert!(!s.replacements.contains_key("id3"));
    // All three must be marked seen.
    assert!(s.seen_ids.contains("id1"));
    assert!(s.seen_ids.contains("id2"));
    assert!(s.seen_ids.contains("id3"));
}

#[tokio::test]
async fn test_apply_tool_result_budget_skips_opted_out() {
    let state: ContentReplacementStateRef = Arc::new(RwLock::new(ContentReplacementState::new(50)));
    let candidates = vec![
        ToolResultCandidate {
            tool_use_id: "id1".into(),
            content_chars: 100,
            tool_name: Some("Read".into()),
            persistence_opted_out: true, // Read on canonical file — opt out
        },
        ToolResultCandidate {
            tool_use_id: "id2".into(),
            content_chars: 100,
            tool_name: Some("Bash".into()),
            persistence_opted_out: false,
        },
    ];
    let outcome = apply_tool_result_budget(&candidates, &state).await;
    // id1 is over cap but opted out → cannot evict; id2 is the most
    // recent so always preserved. Net: nothing replaced; aggregate
    // stays over cap (TS behaviour: no further action possible).
    assert!(outcome.newly_replaced.is_empty());
}

#[test]
fn test_render_persisted_reference_includes_filepath_and_preview() {
    let p = PersistedToolResult {
        filepath: PathBuf::from("/sess/tool-results/abc.txt"),
        original_size: 12_345,
        is_json: false,
        preview: "first chars...".into(),
        has_more: true,
    };
    let rendered = render_persisted_reference(&p);
    assert!(rendered.starts_with(PERSISTED_OUTPUT_TAG));
    assert!(rendered.ends_with(PERSISTED_OUTPUT_CLOSING_TAG));
    assert!(rendered.contains("/sess/tool-results/abc.txt"));
    assert!(rendered.contains("12345 bytes"));
    assert!(rendered.contains("first chars..."));
}
