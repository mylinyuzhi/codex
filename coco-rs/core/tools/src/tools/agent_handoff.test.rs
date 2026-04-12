use std::sync::Arc;

use coco_tool::SideQuery;
use coco_tool::SideQueryHandle;
use coco_types::SideQueryRequest;
use coco_types::SideQueryResponse;

use super::*;

// ── Sync tests ──

#[test]
fn test_is_read_only_agent() {
    assert!(is_read_only_agent("Explore"));
    assert!(is_read_only_agent("Plan"));
    assert!(is_read_only_agent("claude-code-guide"));
    assert!(!is_read_only_agent("general-purpose"));
    assert!(!is_read_only_agent("custom-agent"));
}

#[test]
fn test_build_transcript_summary_text_content() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "Find files"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "text", "text": "I'll search for files"},
            {"type": "tool_use", "name": "Glob", "id": "tu_1", "input": {}}
        ]}),
        serde_json::json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "file1.rs"}
        ]}),
    ];

    let summary = build_transcript_summary(&messages);
    assert!(summary.contains("[user] Find files"));
    assert!(summary.contains("[assistant] I'll search for files"));
    assert!(summary.contains("[assistant] tool_use: Glob"));
    assert!(summary.contains("[user] tool_result"));
}

#[test]
fn test_build_transcript_summary_empty() {
    let messages: Vec<serde_json::Value> = vec![];
    let summary = build_transcript_summary(&messages);
    assert!(summary.is_empty());
}

// ── Mock SideQuery ──

struct MockSideQuery {
    responses: tokio::sync::Mutex<Vec<Result<SideQueryResponse, anyhow::Error>>>,
}

impl MockSideQuery {
    fn with_responses(responses: Vec<&str>) -> Self {
        Self {
            responses: tokio::sync::Mutex::new(
                responses
                    .into_iter()
                    .map(|text| {
                        Ok(SideQueryResponse {
                            text: Some(text.to_string()),
                            tool_uses: Vec::new(),
                            stop_reason: coco_types::SideQueryStopReason::EndTurn,
                            usage: coco_types::SideQueryUsage {
                                input_tokens: 10,
                                output_tokens: 5,
                            },
                            model_used: "mock-model".to_string(),
                        })
                    })
                    .collect(),
            ),
        }
    }

    fn with_error() -> Self {
        Self {
            responses: tokio::sync::Mutex::new(vec![Err(anyhow::anyhow!("LLM unavailable"))]),
        }
    }
}

#[async_trait::async_trait]
impl SideQuery for MockSideQuery {
    async fn query(&self, _request: SideQueryRequest) -> anyhow::Result<SideQueryResponse> {
        let mut responses = self.responses.lock().await;
        if responses.is_empty() {
            anyhow::bail!("no more mock responses")
        }
        responses.remove(0)
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }
}

// ── Async handoff tests ──

#[tokio::test]
async fn test_classify_read_only_agent_skips() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_error());
    let result = classify_handoff("transcript", "Explore", /*tool_count*/ 5, &handle).await;
    assert_eq!(result, HandoffClassification::Safe);
}

#[tokio::test]
async fn test_classify_zero_tool_uses_skips() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_error());
    let result = classify_handoff(
        "transcript",
        "general-purpose",
        /*tool_count*/ 0,
        &handle,
    )
    .await;
    assert_eq!(result, HandoffClassification::Safe);
}

#[tokio::test]
async fn test_classify_stage1_safe() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_responses(vec!["SAFE"]));
    let result = classify_handoff(
        "transcript",
        "general-purpose",
        /*tool_count*/ 5,
        &handle,
    )
    .await;
    assert_eq!(result, HandoffClassification::Safe);
}

#[tokio::test]
async fn test_classify_stage1_blocked_stage2_confirms() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_responses(vec![
        "BLOCKED: suspicious file deletion",
        "BLOCKED: confirmed file deletion risk",
    ]));
    let result = classify_handoff(
        "transcript",
        "general-purpose",
        /*tool_count*/ 5,
        &handle,
    )
    .await;
    assert!(matches!(result, HandoffClassification::Blocked { .. }));
}

#[tokio::test]
async fn test_classify_stage1_blocked_stage2_safe_false_positive() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_responses(vec![
        "BLOCKED: maybe suspicious",
        "SAFE — false positive",
    ]));
    let result = classify_handoff(
        "transcript",
        "general-purpose",
        /*tool_count*/ 5,
        &handle,
    )
    .await;
    assert_eq!(result, HandoffClassification::Safe);
}

#[tokio::test]
async fn test_classify_llm_error_fails_open() {
    let handle: SideQueryHandle = Arc::new(MockSideQuery::with_error());
    let result = classify_handoff(
        "transcript",
        "general-purpose",
        /*tool_count*/ 5,
        &handle,
    )
    .await;
    assert_eq!(result, HandoffClassification::Safe);
}
