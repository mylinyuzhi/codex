use std::sync::Arc;
use std::sync::Mutex;

use coco_inference::LanguageModelMessage;
use coco_inference::UserContentPart;
use coco_types::CacheSafeParams;
use coco_types::ForkLabel;
use coco_types::HookEventType;
use coco_types::HookScope;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

use crate::config::QueryEngineConfig;
use crate::engine::QueryEngine;
use crate::forked_agent::ForkDispatcher;
use crate::forked_agent::ForkedAgentOptions;
use crate::forked_agent::ForkedAgentResult;

#[derive(Default)]
struct CapturingModel {
    options: Mutex<Option<coco_inference::LanguageModelCallOptions>>,
}

#[async_trait::async_trait]
impl coco_inference::LanguageModel for CapturingModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }

    async fn do_generate(
        &self,
        options: coco_inference::LanguageModelCallOptions,
    ) -> Result<coco_inference::LanguageModelGenerateResult, coco_inference::AISdkError> {
        *self.options.lock().expect("model options lock poisoned") = Some(options);
        Ok(coco_inference::LanguageModelGenerateResult {
            content: vec![coco_inference::AssistantContentPart::Text(
                coco_inference::TextPart {
                    text: "direct summary".into(),
                    provider_metadata: None,
                },
            )],
            usage: coco_inference::Usage::new(0, 0),
            finish_reason: coco_inference::FinishReason::new(
                coco_inference::UnifiedFinishReason::EndTurn,
            ),
            warnings: Vec::new(),
            provider_metadata: None,
            request: None,
            response: None,
        })
    }

    async fn do_stream(
        &self,
        options: coco_inference::LanguageModelCallOptions,
    ) -> Result<coco_inference::LanguageModelStreamResult, coco_inference::AISdkError> {
        let result = self.do_generate(options).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

#[derive(Default)]
struct CapturingCompactDispatcher {
    cache: Mutex<Option<CacheSafeParams>>,
    options: Mutex<Option<ForkedAgentOptions>>,
    prompt: Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl ForkDispatcher for CapturingCompactDispatcher {
    async fn dispatch(
        &self,
        cache: &CacheSafeParams,
        options: &ForkedAgentOptions,
        prompt: &str,
        _system_prompt_override: Option<String>,
    ) -> Result<ForkedAgentResult, coco_error::BoxedError> {
        *self.cache.lock().expect("cache lock poisoned") = Some(cache.clone());
        *self.options.lock().expect("options lock poisoned") = Some(options.clone());
        *self.prompt.lock().expect("prompt lock poisoned") = Some(prompt.to_string());
        Ok(ForkedAgentResult {
            messages: vec![assistant_msg("fork summary")],
            ..Default::default()
        })
    }
}

struct FailingDispatcher;

#[async_trait::async_trait]
impl ForkDispatcher for FailingDispatcher {
    async fn dispatch(
        &self,
        _cache: &CacheSafeParams,
        _options: &ForkedAgentOptions,
        _prompt: &str,
        _system_prompt_override: Option<String>,
    ) -> Result<ForkedAgentResult, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "fork failed",
            coco_error::StatusCode::Internal,
        )))
    }
}

#[derive(Default)]
struct CapturingSessionStartSink {
    effects: Mutex<Vec<crate::session_start_hooks::SessionStartHookSideEffects>>,
}

#[async_trait::async_trait]
impl crate::session_start_hooks::SessionStartHookSideEffectSink for CapturingSessionStartSink {
    async fn handle_session_start_hook_side_effects(
        &self,
        effects: crate::session_start_hooks::SessionStartHookSideEffects,
    ) {
        self.effects
            .lock()
            .expect("side-effect sink lock poisoned")
            .push(effects);
    }
}

fn new_engine(
    model: Arc<CapturingModel>,
    dispatcher: Option<Arc<dyn ForkDispatcher>>,
) -> QueryEngine {
    let client = Arc::new(coco_inference::ApiClient::with_default_fingerprint(
        model,
        coco_inference::RetryConfig::default(),
    ));
    let tools = Arc::new(coco_tool_runtime::ToolRegistry::new());
    let mut engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        CancellationToken::new(),
        None,
    );
    if let Some(dispatcher) = dispatcher {
        engine = engine.with_fork_dispatcher(dispatcher);
    }
    engine
}

fn new_engine_with_hooks(
    model: Arc<CapturingModel>,
    hooks: Arc<coco_hooks::HookRegistry>,
    sync_hook_buffer: coco_hooks::SyncHookEventBuffer,
    side_effect_sink: Option<crate::session_start_hooks::SessionStartHookSideEffectSinkRef>,
) -> QueryEngine {
    let client = Arc::new(coco_inference::ApiClient::with_default_fingerprint(
        model,
        coco_inference::RetryConfig::default(),
    ));
    let tools = Arc::new(coco_tool_runtime::ToolRegistry::new());
    let mut engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        CancellationToken::new(),
        Some(hooks),
    )
    .with_sync_hook_buffer(sync_hook_buffer);
    if let Some(sink) = side_effect_sink {
        engine = engine.with_session_start_hook_side_effect_sink(sink);
    }
    engine
}

fn hook(event: HookEventType, command: &str) -> coco_hooks::HookDefinition {
    coco_hooks::HookDefinition {
        event,
        matcher: None,
        handler: coco_hooks::HookHandler::Command {
            command: command.to_string(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        status_message: None,
    }
}

fn compact_attempt(summary_request: &str) -> coco_compact::CompactSummaryAttempt {
    coco_compact::CompactSummaryAttempt {
        messages: vec![coco_messages::create_user_message(
            "conversation slice only",
        )],
        context_messages: vec![coco_messages::create_user_message(
            "conversation context for api",
        )],
        summary_request: summary_request.to_string(),
        prompt_kind: coco_compact::CompactSummaryKind::Full,
        pre_compact_tokens: 42,
        max_summary_tokens: 20_000,
    }
}

fn assistant_msg(text: &str) -> coco_messages::Message {
    coco_messages::Message::Assistant(coco_messages::AssistantMessage {
        message: coco_messages::LlmMessage::Assistant {
            content: vec![coco_messages::AssistantContent::Text(
                coco_messages::TextContent {
                    text: text.into(),
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        uuid: uuid::Uuid::new_v4(),
        model: "test-model".into(),
        stop_reason: Some(coco_messages::StopReason::EndTurn),
        usage: Some(coco_types::TokenUsage::default()),
        cost_usd: None,
        request_id: Some("req-compact".into()),
        api_error: None,
    })
}

fn empty_cache() -> CacheSafeParams {
    CacheSafeParams {
        rendered_system_prompt: "system".into(),
        model_id: "mock-model".into(),
        provider: "mock".into(),
        prompt_cache: None,
        fork_context_messages: vec![serde_json::json!({"old":"parent cache"})],
    }
}

#[tokio::test]
async fn compact_summary_uses_cache_safe_fork_with_deny_all_tools() {
    let model = Arc::new(CapturingModel::default());
    let dispatcher = Arc::new(CapturingCompactDispatcher::default());
    let engine = new_engine(model, Some(dispatcher.clone()));
    engine.save_cache_safe_params(empty_cache()).await;

    let response = engine
        .run_compact_summary_attempt(compact_attempt("summarize now"))
        .await
        .expect("fork summary should succeed");

    assert_eq!(response.summary, "fork summary");
    assert_eq!(
        dispatcher
            .prompt
            .lock()
            .expect("prompt lock poisoned")
            .as_deref(),
        Some("summarize now")
    );

    let options = dispatcher
        .options
        .lock()
        .expect("options lock poisoned")
        .clone()
        .expect("dispatcher should capture options");
    assert_eq!(options.fork_label, ForkLabel::Compact);
    assert_eq!(options.max_turns, Some(1));
    assert!(options.skip_cache_write);
    assert!(options.can_use_tool.is_some());
    assert!(options.require_can_use_tool);

    let cache = dispatcher
        .cache
        .lock()
        .expect("cache lock poisoned")
        .clone()
        .expect("dispatcher should capture cache");
    let serialized =
        serde_json::to_string(&cache.fork_context_messages).expect("fork context should serialize");
    assert!(serialized.contains("conversation context for api"));
    assert!(!serialized.contains("conversation slice only"));
    assert!(!serialized.contains("parent cache"));
}

#[tokio::test]
async fn compact_summary_falls_back_to_direct_no_tools_call() {
    let model = Arc::new(CapturingModel::default());
    let engine = new_engine(model.clone(), Some(Arc::new(FailingDispatcher)));
    engine.save_cache_safe_params(empty_cache()).await;

    let mut attempt = compact_attempt("direct request");
    attempt.max_summary_tokens = 123;
    let response = engine
        .run_compact_summary_attempt(attempt)
        .await
        .expect("direct fallback should succeed");

    assert_eq!(response.summary, "direct summary");
    let options = model
        .options
        .lock()
        .expect("model options lock poisoned")
        .clone()
        .expect("direct model should be called");
    assert!(
        options.tools.is_none(),
        "direct compact must not expose tools"
    );
    assert!(options.tool_choice.is_none());
    assert_eq!(options.max_output_tokens, Some(123));
    assert_eq!(options.prompt.len(), 2);
    assert!(format!("{:?}", options.prompt[0]).contains("conversation context for api"));
    assert!(!format!("{:?}", options.prompt[0]).contains("conversation slice only"));
    match options.prompt.last() {
        Some(LanguageModelMessage::User { content, .. }) => {
            assert!(content.iter().any(|part| matches!(
                part,
                UserContentPart::Text(text) if text.text == "direct request"
            )));
        }
        other => panic!("summary request should be appended as a user message, got {other:?}"),
    }
}

#[tokio::test]
async fn partial_compact_runs_hooks_and_inlines_session_start_results() {
    let model = Arc::new(CapturingModel::default());
    let hooks = Arc::new(coco_hooks::HookRegistry::new());
    hooks.register(hook(HookEventType::PreCompact, "echo pre-hook-instruction"));
    hooks.register(hook(HookEventType::PostCompact, "echo post-hook-output"));
    hooks.register(hook(
        HookEventType::SessionStart,
        "echo session-hook-output",
    ));
    let sync = coco_hooks::SyncHookEventBuffer::new();
    let engine = new_engine_with_hooks(model.clone(), hooks, sync.clone(), None);

    let mut history = coco_messages::MessageHistory::new();
    history
        .messages
        .push(coco_messages::create_user_message("kept prefix"));
    history.messages.push(assistant_msg("kept assistant"));
    history
        .messages
        .push(coco_messages::create_user_message("summarize tail"));
    history.messages.push(assistant_msg("tail assistant"));

    let outcome = engine
        .run_partial_compact(
            &mut history,
            &None,
            2,
            coco_messages::PartialCompactDirection::Newest,
            Some("focus user feedback".to_string()),
            Some("user compact instruction".to_string()),
        )
        .await;

    assert_eq!(outcome, coco_compact::CompactOutcome::Applied);
    let options = model
        .options
        .lock()
        .expect("model options lock poisoned")
        .clone()
        .expect("direct model should be called");
    let rendered_prompt = format!("{:?}", options.prompt);
    assert!(rendered_prompt.contains("user compact instruction"));
    assert!(rendered_prompt.contains("pre-hook-instruction"));
    assert!(rendered_prompt.contains("focus user feedback"));

    let rendered_history = format!("{:?}", history.messages);
    assert!(rendered_history.contains("session-hook-output"));
    assert!(
        sync.drain().await.is_empty(),
        "partial compact should not duplicate SessionStart output into next-turn hook buffer"
    );
}

#[tokio::test]
async fn partial_compact_applies_session_start_aggregate_side_effects() {
    let model = Arc::new(CapturingModel::default());
    let hooks = Arc::new(coco_hooks::HookRegistry::new());
    hooks.register(hook(
        HookEventType::SessionStart,
        "printf '{\"initialUserMessage\":\"hook initial turn\",\"watchPaths\":[\"/tmp/coco-watch\"]}'",
    ));
    let sync = coco_hooks::SyncHookEventBuffer::new();
    let sink = Arc::new(CapturingSessionStartSink::default());
    let engine = new_engine_with_hooks(model, hooks, sync.clone(), Some(sink.clone()));

    let mut history = coco_messages::MessageHistory::new();
    history
        .messages
        .push(coco_messages::create_user_message("kept prefix"));
    history.messages.push(assistant_msg("kept assistant"));
    history
        .messages
        .push(coco_messages::create_user_message("summarize tail"));
    history.messages.push(assistant_msg("tail assistant"));

    let outcome = engine
        .run_partial_compact(
            &mut history,
            &None,
            2,
            coco_messages::PartialCompactDirection::Newest,
            None,
            None,
        )
        .await;

    assert_eq!(outcome, coco_compact::CompactOutcome::Applied);
    let rendered_history = format!("{:?}", history.messages);
    assert!(rendered_history.contains("hook initial turn"));
    let effects = sink
        .effects
        .lock()
        .expect("side-effect sink lock poisoned")
        .clone();
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].initial_user_message.as_deref(),
        Some("hook initial turn")
    );
    assert_eq!(effects[0].watch_paths, vec!["/tmp/coco-watch"]);
    assert!(
        sync.drain().await.is_empty(),
        "partial compact should not duplicate SessionStart output into next-turn hook buffer"
    );
}
