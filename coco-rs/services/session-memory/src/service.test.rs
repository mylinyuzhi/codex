use super::*;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

fn record_calls() -> (Arc<Mutex<Vec<String>>>, SummarizerFn) {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for_fn = log.clone();
    let summarizer: SummarizerFn = Arc::new(move |prompt: String| {
        let log = log_for_fn.clone();
        Box::pin(async move {
            log.lock().await.push(prompt.clone());
            Ok::<_, anyhow::Error>(format!("MEMORY:\n{}", &prompt[..prompt.len().min(40)]))
        })
    });
    (log, summarizer)
}

#[tokio::test]
async fn test_maybe_extract_disabled_when_no_summarizer() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    let outcome = svc.maybe_extract(20_000, 5, None, "hello").await;
    assert_eq!(outcome, ExtractionOutcome::Disabled);
}

#[tokio::test]
async fn test_maybe_extract_skipped_below_init_threshold() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    let (_log, summarizer) = record_calls();
    svc.set_summarizer(summarizer).await;
    let outcome = svc.maybe_extract(5_000, 0, None, "hi").await;
    assert_eq!(
        outcome,
        ExtractionOutcome::Skipped(ExtractionDecision::BelowInitThreshold),
    );
}

#[tokio::test]
async fn test_maybe_extract_writes_file_and_caches() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    let (log, summarizer) = record_calls();
    svc.set_summarizer(summarizer).await;

    let outcome = svc
        .maybe_extract(20_000, 0, Some(uuid::Uuid::new_v4()), "transcript content")
        .await;
    assert!(matches!(outcome, ExtractionOutcome::Extracted { .. }));

    let cached = svc.current_text().await;
    assert!(cached.starts_with("MEMORY:"));
    assert!(svc.path().exists());
    assert_eq!(log.lock().await.len(), 1);
}

#[tokio::test]
async fn test_maybe_extract_skipped_after_low_delta() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    let (_log, summarizer) = record_calls();
    svc.set_summarizer(summarizer).await;
    // First extract sets the baseline.
    let _ = svc.maybe_extract(10_000, 0, None, "first").await;
    // Same tokens → no delta → skip.
    let outcome = svc.maybe_extract(10_500, 5, None, "second").await;
    assert_eq!(
        outcome,
        ExtractionOutcome::Skipped(ExtractionDecision::InsufficientDelta),
    );
}

#[tokio::test]
async fn test_load_from_disk_populates_cache() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    // Pre-seed the file directly.
    let path = svc.path();
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&path, "preseeded body").await.unwrap();
    svc.load_from_disk().await.unwrap();
    assert_eq!(svc.current_text().await, "preseeded body");
}

#[tokio::test]
async fn test_clear_after_compact_resets_state() {
    let dir = TempDir::new().unwrap();
    let svc = SessionMemoryService::new(dir.path().to_path_buf(), "s1".into());
    let (_log, summarizer) = record_calls();
    svc.set_summarizer(summarizer).await;
    let id = uuid::Uuid::new_v4();
    let _ = svc.maybe_extract(20_000, 0, Some(id), "hello").await;
    assert_eq!(svc.last_summarized_message_id().await, Some(id));
    svc.clear_after_compact().await;
    assert_eq!(svc.last_summarized_message_id().await, None);
    assert!(svc.current_text().await.is_empty());
}

#[tokio::test]
async fn test_count_tool_calls_in_last_turn() {
    use coco_messages::AssistantContent;
    use coco_types::TokenUsage;

    let content = vec![
        AssistantContent::Text(coco_messages::TextContent {
            text: "hi".into(),
            provider_metadata: None,
        }),
        AssistantContent::ToolCall(coco_messages::ToolCallContent {
            tool_call_id: "1".into(),
            tool_name: "X".into(),
            input: serde_json::json!({}),
            provider_executed: None,
            provider_metadata: None,
        }),
        AssistantContent::ToolCall(coco_messages::ToolCallContent {
            tool_call_id: "2".into(),
            tool_name: "Y".into(),
            input: serde_json::json!({}),
            provider_executed: None,
            provider_metadata: None,
        }),
    ];
    let asst =
        coco_messages::create_assistant_message(content, "test-model", TokenUsage::default());
    assert_eq!(count_tool_calls_in_last_assistant_turn(&[asst]), 2);
}
