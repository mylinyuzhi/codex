//! End-to-end integration tests for the multi-slot fallback +
//! half-open recovery wiring on `QueryEngine`.
//!
//! Unit coverage of the state machine lives in
//! `app/query/src/model_runtime.test.rs`; these tests exercise the
//! engine's integration points: constructing the runtime with a
//! chain, walking slots on capacity errors, transparently
//! recovering from probe failures, and emitting
//! `ModelFallbackReason::ChainExhausted` when all slots are spent.
//!
//! The mock model is deliberately minimal — it returns either a
//! capacity error or a text response based on a per-call counter.
//! We bypass the shared `mock_harness` because that harness
//! models only success paths.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::Mutex;

use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_tool_runtime::ToolRegistry;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

/// Each scripted call returns either a capacity error or a
/// successful text reply. Tracks call count for assertions.
struct ScriptedCapacityMock {
    id: &'static str,
    calls: Arc<Mutex<Vec<CallOutcome>>>,
    next: std::sync::atomic::AtomicUsize,
}

#[derive(Clone, Debug)]
enum CallOutcome {
    Capacity,
    Text(&'static str),
}

impl ScriptedCapacityMock {
    fn new(id: &'static str, outcomes: Vec<CallOutcome>) -> Arc<Self> {
        Arc::new(Self {
            id,
            calls: Arc::new(Mutex::new(outcomes)),
            next: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn call_count(&self) -> usize {
        self.next.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LanguageModelV4 for ScriptedCapacityMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        self.id
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let idx = self.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let outcome = self
            .calls
            .lock()
            .unwrap()
            .get(idx)
            .cloned()
            .unwrap_or(CallOutcome::Text("(exhausted)"));
        match outcome {
            CallOutcome::Capacity => {
                // "status: 529" matches the capacity classifier in engine.rs.
                Err(AISdkError::new("provider overloaded (status: 529)"))
            }
            CallOutcome::Text(s) => Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: s.to_string(),
                    provider_metadata: None,
                })],
                usage: Usage::new(0, 0),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            }),
        }
    }
    async fn do_stream(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        let result = self.do_generate(options).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

/// Build an ApiClient with NO internal retries — each engine-level
/// `query_stream` corresponds to exactly one mock `do_generate`
/// call, so the engine's capacity-streak counter ticks 1-to-1 with
/// scripted capacity outcomes. The default `max_retries=3` would
/// swallow 3 capacity outcomes per engine call and make the test
/// non-deterministic without more outcomes to burn.
fn api_client(model: Arc<dyn LanguageModelV4>) -> Arc<ApiClient> {
    let retry = RetryConfig {
        max_retries: 0,
        ..RetryConfig::default()
    };
    Arc::new(ApiClient::new(model, retry))
}

fn minimal_config(model_name: &str) -> QueryEngineConfig {
    // `max_turns` bounds retry iterations — the capacity streak can
    // use up to MAX_CONSECUTIVE_CAPACITY_ERRORS=3 iterations per
    // slot before advance fires, plus 1 successful call. 20 is
    // plenty to let a 2-slot chain walk through 3+3+1 iterations
    // without budget-exhausting.
    QueryEngineConfig {
        model_name: model_name.to_string(),
        max_turns: 20,
        max_tokens: Some(16_384),
        context_window: 200_000,
        max_output_tokens: 4_096,
        streaming_tool_execution: false,
        system_prompt: Some("you are a test assistant".into()),
        session_id: "s-fallback-test".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_engine_advances_to_fallback_after_capacity_streak() {
    // Primary returns 3 capacity errors, then a text response.
    // With `MAX_CONSECUTIVE_CAPACITY_ERRORS = 3`, the 3rd error
    // triggers `advance()` to the fallback, which succeeds.
    let primary = ScriptedCapacityMock::new(
        "primary-model",
        vec![
            CallOutcome::Capacity,
            CallOutcome::Capacity,
            CallOutcome::Capacity,
        ],
    );
    let fallback = ScriptedCapacityMock::new("fallback-model", vec![CallOutcome::Text("done")]);

    let primary_client = api_client(primary.clone());
    let fallback_client = api_client(fallback.clone());

    let engine = QueryEngine::new(
        minimal_config("primary-model"),
        primary_client,
        Arc::new(ToolRegistry::new()),
        CancellationToken::new(),
        None,
    )
    .with_fallback_clients(vec![fallback_client]);

    let result = engine.run("hello").await.expect("engine must recover");
    // Primary saw 3 capacity errors (no retry — engine short-
    // circuits after streak); fallback served 1 call.
    assert_eq!(primary.call_count(), 3, "primary must hit 3-strike streak");
    assert_eq!(fallback.call_count(), 1, "fallback must serve recovery");
    assert_eq!(result.response_text, "done");
}

#[tokio::test]
async fn test_engine_surfaces_error_when_chain_exhausted() {
    // Primary + fallback both return capacity errors indefinitely.
    // After 3 strikes on primary + advance + 3 strikes on
    // fallback, advance returns Exhausted and the engine surfaces
    // an error. A ChainExhausted notice must be emitted.
    let primary = ScriptedCapacityMock::new(
        "primary",
        vec![
            CallOutcome::Capacity,
            CallOutcome::Capacity,
            CallOutcome::Capacity,
        ],
    );
    let fallback = ScriptedCapacityMock::new(
        "fallback",
        vec![
            CallOutcome::Capacity,
            CallOutcome::Capacity,
            CallOutcome::Capacity,
        ],
    );
    let primary_client = api_client(primary.clone());
    let fallback_client = api_client(fallback.clone());

    let engine = QueryEngine::new(
        minimal_config("primary"),
        primary_client,
        Arc::new(ToolRegistry::new()),
        CancellationToken::new(),
        None,
    )
    .with_fallback_clients(vec![fallback_client]);

    // Capture emitted events to check for the ChainExhausted notice.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<coco_query::CoreEvent>(64);
    let events_handle = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(e) = rx.recv().await {
            collected.push(e);
        }
        collected
    });

    let result = engine
        .run_with_messages(vec![coco_messages::create_user_message("hello")], tx)
        .await;
    assert!(result.is_err(), "exhausted chain must surface error");

    let events = events_handle.await.unwrap();
    let saw_exhausted = events.iter().any(|e| {
        matches!(
            e,
            coco_query::CoreEvent::Stream(coco_query::AgentStreamEvent::TextDelta { delta, .. })
                if delta.contains("exhausted")
        )
    });
    assert!(
        saw_exhausted,
        "ChainExhausted notice must be emitted; events: {events:?}"
    );
}
