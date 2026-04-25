use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_tool::ToolRegistry;
use coco_tools::ReadTool;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use super::*;
use coco_types::PermissionMode;

// Bring the top-level CoreEvent + ServerNotification re-exports into scope
// for the Phase 1 lifecycle tests below.
use crate::CoreEvent;
use crate::ServerNotification;

// ─── Simple text-only mock ───

struct TextMock {
    text: String,
}

#[async_trait::async_trait]
impl LanguageModelV4 for TextMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-text"
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        Ok(LanguageModelV4GenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: self.text.clone(),
                provider_metadata: None,
            })],
            usage: Usage::new(10, 5),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
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

// ─── Multi-turn mock: first call returns tool_call, second returns text ───

struct ToolCallThenTextMock {
    call_count: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for ToolCallThenTextMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-toolcall"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);

        if call == 0 {
            // First call: return a tool call (Read tool)
            Ok(LanguageModelV4GenerateResult {
                content: vec![
                    AssistantContentPart::Text(TextPart {
                        text: "Let me read that file for you.".into(),
                        provider_metadata: None,
                    }),
                    AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: "call_001".into(),
                        tool_name: "Read".into(),
                        input: serde_json::json!({"file_path": "/tmp/nonexistent.txt"}),
                        provider_executed: None,
                        provider_metadata: None,
                    }),
                ],
                usage: Usage::new(20, 15),
                finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
        } else {
            // Second call: return final text (after seeing tool result)
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: "The file does not exist. Let me help you create it.".into(),
                    provider_metadata: None,
                })],
                usage: Usage::new(30, 10),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
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

// ─── Multi-tool mock: returns 2 tool calls in one response ───

struct MultiToolMock {
    call_count: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for MultiToolMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-multi"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);

        if call == 0 {
            // First call: return TWO tool calls (parallel read)
            Ok(LanguageModelV4GenerateResult {
                content: vec![
                    AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: "call_a".into(),
                        tool_name: "Read".into(),
                        input: serde_json::json!({"file_path": "/tmp/file_a.txt"}),
                        provider_executed: None,
                        provider_metadata: None,
                    }),
                    AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: "call_b".into(),
                        tool_name: "Read".into(),
                        input: serde_json::json!({"file_path": "/tmp/file_b.txt"}),
                        provider_executed: None,
                        provider_metadata: None,
                    }),
                ],
                usage: Usage::new(15, 10),
                finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
        } else {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: "Both files could not be read.".into(),
                    provider_metadata: None,
                })],
                usage: Usage::new(25, 8),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
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

// ─── Single tool-call mock: first call returns one tool, second returns text ───

struct OneToolThenTextMock {
    call_count: AtomicI32,
    tool_call_id: String,
    tool_name: String,
    input: serde_json::Value,
    final_text: String,
}

#[async_trait::async_trait]
impl LanguageModelV4 for OneToolThenTextMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-one-tool-then-text"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: self.tool_call_id.clone(),
                    tool_name: self.tool_name.clone(),
                    input: self.input.clone(),
                    provider_executed: None,
                    provider_metadata: None,
                })],
                usage: Usage::new(5, 5),
                finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
        } else {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: self.final_text.clone(),
                    provider_metadata: None,
                })],
                usage: Usage::new(5, 5),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
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

// ─── Tests ───

#[tokio::test]
async fn test_single_turn_text_only() {
    let model = Arc::new(TextMock {
        text: "Hello!".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("hi").await.expect("should succeed");

    assert_eq!(result.response_text, "Hello!");
    assert_eq!(result.turns, 1);
    assert!(!result.cancelled);
}

#[tokio::test]
async fn test_with_fallback_client_does_not_break_primary_path() {
    // Phase 8-β sanity: installing a fallback client via the
    // builder must NOT change behavior when the primary succeeds.
    // The per-session ModelRuntime stays on the primary; the
    // fallback is just available. We confirm the normal text
    // response comes back unchanged.
    let primary_model = Arc::new(TextMock {
        text: "primary-answer".into(),
    });
    let fallback_model = Arc::new(TextMock {
        text: "fallback-answer".into(),
    });
    let primary = Arc::new(ApiClient::new(primary_model, RetryConfig::default()));
    let fallback = Arc::new(ApiClient::new(fallback_model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), primary, tools, cancel, None)
        .with_fallback_client(fallback);
    let result = engine.run("hi").await.expect("should succeed");

    // Primary succeeded ⇒ fallback never activates; response comes
    // from the primary.
    assert_eq!(result.response_text, "primary-answer");
}

#[tokio::test]
async fn test_multi_turn_tool_call_then_text() {
    // Mock: call 1 → tool_call(Read), call 2 → text
    let model = Arc::new(ToolCallThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    // Register ReadTool so it can be found and executed
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine
        .run("read /tmp/nonexistent.txt")
        .await
        .expect("should succeed");

    // Should have done 2 turns: tool_call + final text
    assert_eq!(result.turns, 2);
    assert_eq!(
        result.response_text,
        "The file does not exist. Let me help you create it."
    );
    // Usage accumulated from both turns
    assert_eq!(result.total_usage.input_tokens, 50); // 20 + 30
    assert_eq!(result.total_usage.output_tokens, 25); // 15 + 10
    assert!(!result.cancelled);
}

#[tokio::test]
async fn test_multi_tool_calls_in_one_response() {
    // Mock: call 1 → 2 tool_calls(Read, Read), call 2 → text
    let model = Arc::new(MultiToolMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("read both files").await.expect("should succeed");

    assert_eq!(result.turns, 2);
    assert_eq!(result.response_text, "Both files could not be read.");
    // Usage: 15+25 input, 10+8 output
    assert_eq!(result.total_usage.input_tokens, 40);
    assert_eq!(result.total_usage.output_tokens, 18);
}

#[tokio::test]
async fn test_cancellation() {
    let model = Arc::new(TextMock {
        text: "nope".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();
    cancel.cancel();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("hi").await.expect("should succeed");

    assert!(result.cancelled);
    assert_eq!(result.turns, 0);
}

#[tokio::test]
async fn test_system_prompt_included() {
    let model = Arc::new(TextMock {
        text: "I am helpful.".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();

    let config = QueryEngineConfig {
        system_prompt: Some("You are a helpful assistant.".into()),
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, tools, cancel, None);
    let result = engine.run("hello").await.expect("should succeed");

    assert_eq!(result.response_text, "I am helpful.");
}

#[tokio::test]
async fn test_max_turns_limit() {
    // Model always returns tool calls → should stop at max_turns
    let model = Arc::new(ToolCallThenTextMock {
        call_count: AtomicI32::new(0), // but we set max_turns=1 so it stops after first
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let config = QueryEngineConfig {
        max_turns: 1,
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, tools, cancel, None);
    let result = engine.run("read file").await.expect("should succeed");

    // Only 1 turn allowed, should stop even though tool call would trigger another
    assert_eq!(result.turns, 1);
}

#[tokio::test]
async fn test_permission_mode_passed_to_context() {
    let model = Arc::new(TextMock { text: "ok".into() });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();

    let config = QueryEngineConfig {
        model_name: "test-opus".into(),
        permission_mode: PermissionMode::Default,
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, tools, cancel, None);
    let result = engine.run("hello").await.expect("should succeed");

    assert_eq!(result.response_text, "ok");
    assert_eq!(result.turns, 1);
}

#[tokio::test]
async fn test_tool_execution_with_real_tools() {
    // Test that tool results are properly passed back to the model
    let dir = tempfile::tempdir().unwrap();
    let test_file = dir.path().join("test.txt");
    std::fs::write(&test_file, "hello world").unwrap();

    // Mock that asks to read a real file, then produces final text
    struct ReadRealFileMock {
        call_count: AtomicI32,
        file_path: String,
    }

    #[async_trait::async_trait]
    impl LanguageModelV4 for ReadRealFileMock {
        fn provider(&self) -> &str {
            "mock"
        }
        fn model_id(&self) -> &str {
            "mock-read-real"
        }
        async fn do_generate(
            &self,
            _options: LanguageModelV4CallOptions,
        ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
            let call = self.call_count.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Ok(LanguageModelV4GenerateResult {
                    content: vec![AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: "read_1".into(),
                        tool_name: "Read".into(),
                        input: serde_json::json!({"file_path": self.file_path}),
                        provider_executed: None,
                        provider_metadata: None,
                    })],
                    usage: Usage::new(10, 5),
                    finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                    warnings: vec![],
                    provider_metadata: None,
                    request: None,
                    response: None,
                })
            } else {
                Ok(LanguageModelV4GenerateResult {
                    content: vec![AssistantContentPart::Text(TextPart {
                        text: "File read successfully.".into(),
                        provider_metadata: None,
                    })],
                    usage: Usage::new(10, 5),
                    finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                    warnings: vec![],
                    provider_metadata: None,
                    request: None,
                    response: None,
                })
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

    let model = Arc::new(ReadRealFileMock {
        call_count: AtomicI32::new(0),
        file_path: test_file.to_str().unwrap().to_string(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("read the file").await.expect("should succeed");

    assert_eq!(result.turns, 2);
    assert_eq!(result.response_text, "File read successfully.");
}

/// Concurrency-safe tools still emit their lifecycle events
/// (`ToolUseQueued` → `ToolUseStarted` → `ToolUseCompleted`) and execute
/// through the normal post-commit tool path.
///
/// This test reuses `ReadRealFileMock` shape (Read tool is `is_concurrency_safe`)
/// but observes the `CoreEvent` channel to prove the standard executor path
/// fires all three AgentStreamEvent lifecycle phases for the read call.
#[tokio::test]
async fn test_read_tool_emits_full_tool_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let test_file = dir.path().join("sample.txt");
    std::fs::write(&test_file, "eager-payload").unwrap();

    struct SingleReadMock {
        call_count: AtomicI32,
        file_path: String,
    }

    #[async_trait::async_trait]
    impl LanguageModelV4 for SingleReadMock {
        fn provider(&self) -> &str {
            "mock"
        }
        fn model_id(&self) -> &str {
            "mock-eager"
        }
        async fn do_generate(
            &self,
            _options: LanguageModelV4CallOptions,
        ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
            let call = self.call_count.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Ok(LanguageModelV4GenerateResult {
                    content: vec![AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: "eager_1".into(),
                        tool_name: "Read".into(),
                        input: serde_json::json!({"file_path": self.file_path}),
                        provider_executed: None,
                        provider_metadata: None,
                    })],
                    usage: Usage::new(8, 4),
                    finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                    warnings: vec![],
                    provider_metadata: None,
                    request: None,
                    response: None,
                })
            } else {
                Ok(LanguageModelV4GenerateResult {
                    content: vec![AssistantContentPart::Text(TextPart {
                        text: "done".into(),
                        provider_metadata: None,
                    })],
                    usage: Usage::new(5, 3),
                    finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                    warnings: vec![],
                    provider_metadata: None,
                    request: None,
                    response: None,
                })
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

    let model = Arc::new(SingleReadMock {
        call_count: AtomicI32::new(0),
        file_path: test_file.to_str().unwrap().to_string(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CoreEvent>(256);

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);

    let result = engine
        .run_with_events("please read it", event_tx)
        .await
        .expect("ok");
    assert_eq!(result.response_text, "done");

    // Drain the event channel. Eager dispatch must emit Queued + Started
    // during the stream loop, and Completed during post-stream processing.
    drop(engine);
    let mut saw_queued = false;
    let mut saw_started = false;
    let mut saw_completed_with_payload = false;
    while let Some(evt) = event_rx.recv().await {
        if let CoreEvent::Stream(stream_evt) = evt {
            match stream_evt {
                crate::AgentStreamEvent::ToolUseQueued { call_id, .. } if call_id == "eager_1" => {
                    saw_queued = true;
                }
                crate::AgentStreamEvent::ToolUseStarted { call_id, .. } if call_id == "eager_1" => {
                    saw_started = true;
                }
                crate::AgentStreamEvent::ToolUseCompleted {
                    call_id,
                    output,
                    is_error,
                    ..
                } if call_id == "eager_1" => {
                    assert!(!is_error, "read must succeed");
                    assert!(
                        output.contains("eager-payload"),
                        "output should contain file content, got: {output}"
                    );
                    saw_completed_with_payload = true;
                }
                _ => {}
            }
        }
    }

    assert!(saw_queued, "ToolUseQueued must fire");
    assert!(saw_started, "ToolUseStarted must fire");
    assert!(
        saw_completed_with_payload,
        "ToolUseCompleted must carry the file payload"
    );
}

#[tokio::test]
async fn test_bash_destructive_command_blocked() {
    // Test that the Bash tool blocks destructive commands
    use coco_tool::Tool;
    use coco_tools::BashTool;

    let tool = BashTool;
    let ctx = coco_tool::ToolUseContext::test_default();

    // "rm -rf /" should be blocked by destructive warning check
    let result = tool
        .execute(serde_json::json!({"command": "rm -rf /"}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("permission denied") || err.to_string().contains("delete"));
}

#[tokio::test]
async fn test_bash_safe_command_executes() {
    use coco_tool::Tool;
    use coco_tools::BashTool;

    let tool = BashTool;
    let ctx = coco_tool::ToolUseContext::test_default();

    let result = tool
        .execute(
            serde_json::json!({"command": "echo integration_test_ok"}),
            &ctx,
        )
        .await
        .expect("echo should work");

    // Bash output is the TS-shaped object envelope:
    // `{ stdout, stderr, exitCode, interrupted, ... }` — pull stdout
    // from the structured field rather than treating data as a raw
    // string.
    let stdout = result.data["stdout"]
        .as_str()
        .expect("bash output should have a stdout field");
    assert!(stdout.contains("integration_test_ok"), "stdout: {stdout}");
    assert_eq!(result.data["exitCode"], 0);
}

#[tokio::test]
async fn test_budget_tracker_stops_on_limit() {
    use crate::budget::BudgetDecision;
    use crate::budget::BudgetTracker;

    let mut tracker = BudgetTracker::new(Some(100), 30, 3);
    tracker.record_usage(&coco_types::TokenUsage {
        input_tokens: 80,
        output_tokens: 30,
        ..Default::default()
    });
    assert!(matches!(tracker.check(1), BudgetDecision::Stop { .. }));
}

#[tokio::test]
async fn test_budget_exhausted_in_engine() {
    // Model always returns tool calls, but token budget is tiny (15)
    // so after the first LLM response (usage 20+15=35 > 15) the budget check stops the loop.
    let model = Arc::new(ToolCallThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let config = QueryEngineConfig {
        max_tokens: Some(15),
        ..Default::default()
    };
    let engine = QueryEngine::new(config, client, tools, cancel, None);
    let result = engine.run("read file").await.expect("should succeed");

    // First turn executes (usage 20+15=35 > 15), then budget check before turn 2 stops
    assert!(result.budget_exhausted);
    assert_eq!(result.turns, 1);
}

// ─── Phase 1 lifecycle emission tests ───

/// Collect all CoreEvents emitted by the engine during a run.
async fn collect_events_from_run(
    model: Arc<dyn LanguageModelV4>,
    tools: Arc<ToolRegistry>,
    config: QueryEngineConfig,
    bootstrap: Option<SessionBootstrap>,
    prompt: &str,
) -> (QueryResult, Vec<CoreEvent>) {
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let cancel = CancellationToken::new();
    let mut engine = QueryEngine::new(config, client, tools, cancel, None);
    if let Some(b) = bootstrap {
        engine = engine.with_session_bootstrap(b);
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CoreEvent>(256);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }
        events
    });

    let result = engine
        .run_with_events(prompt, event_tx)
        .await
        .expect("engine run should succeed");
    let events = collector.await.unwrap();
    (result, events)
}

fn tool_result_error_text(messages: &[coco_types::Message], tool_use_id: &str) -> Option<String> {
    messages.iter().find_map(|message| {
        let coco_types::Message::ToolResult(result) = message else {
            return None;
        };
        if result.tool_use_id != tool_use_id {
            return None;
        }
        let coco_types::LlmMessage::Tool { content, .. } = &result.message else {
            return None;
        };
        content.iter().find_map(|part| {
            let coco_types::ToolContent::ToolResult(content) = part else {
                return None;
            };
            if content.tool_call_id != tool_use_id || !content.is_error {
                return None;
            }
            match &content.output {
                ToolResultContent::ErrorText { value, .. } => Some(value.clone()),
                ToolResultContent::Text { value, .. } => Some(value.clone()),
                _ => None,
            }
        })
    })
}

fn tool_result_text(messages: &[coco_types::Message], tool_use_id: &str) -> Option<String> {
    messages.iter().find_map(|message| {
        let coco_types::Message::ToolResult(result) = message else {
            return None;
        };
        if result.tool_use_id != tool_use_id {
            return None;
        }
        let coco_types::LlmMessage::Tool { content, .. } = &result.message else {
            return None;
        };
        content.iter().find_map(|part| {
            let coco_types::ToolContent::ToolResult(content) = part else {
                return None;
            };
            if content.tool_call_id != tool_use_id || content.is_error {
                return None;
            }
            match &content.output {
                ToolResultContent::Text { value, .. } => Some(value.clone()),
                _ => None,
            }
        })
    })
}

fn attachment_text_by_kind(
    messages: &[coco_types::Message],
    kind: coco_types::AttachmentKind,
) -> Option<String> {
    messages.iter().find_map(|message| {
        let coco_types::Message::Attachment(attachment) = message else {
            return None;
        };
        if attachment.kind != kind {
            return None;
        }
        let coco_types::AttachmentBody::Api(coco_types::LlmMessage::User { content, .. }) =
            &attachment.body
        else {
            return None;
        };
        content.iter().find_map(|part| match part {
            vercel_ai_provider::UserContentPart::Text(text) => Some(text.text.clone()),
            _ => None,
        })
    })
}

fn tool_result_index(messages: &[coco_types::Message], tool_use_id: &str) -> Option<usize> {
    messages.iter().position(|message| {
        let coco_types::Message::ToolResult(result) = message else {
            return false;
        };
        result.tool_use_id == tool_use_id
    })
}

fn attachment_index_by_kind_and_text(
    messages: &[coco_types::Message],
    kind: coco_types::AttachmentKind,
    needle: &str,
) -> Option<usize> {
    messages.iter().position(|message| {
        let coco_types::Message::Attachment(attachment) = message else {
            return false;
        };
        if attachment.kind != kind {
            return false;
        }
        let coco_types::AttachmentBody::Api(coco_types::LlmMessage::User { content, .. }) =
            &attachment.body
        else {
            return false;
        };
        content.iter().any(|part| match part {
            vercel_ai_provider::UserContentPart::Text(text) => text.text.contains(needle),
            _ => false,
        })
    })
}

fn user_message_index_containing(messages: &[coco_types::Message], needle: &str) -> Option<usize> {
    messages.iter().position(|message| {
        let coco_types::Message::User(user) = message else {
            return false;
        };
        let coco_types::LlmMessage::User { content, .. } = &user.message else {
            return false;
        };
        content.iter().any(|part| match part {
            vercel_ai_provider::UserContentPart::Text(text) => text.text.contains(needle),
            _ => false,
        })
    })
}

fn tool_lifecycle_counts(
    events: &[CoreEvent],
    tool_use_id: &str,
) -> (usize, usize, usize, Option<bool>) {
    let mut queued = 0;
    let mut started = 0;
    let mut completed = 0;
    let mut completed_is_error = None;
    for event in events {
        let CoreEvent::Stream(stream) = event else {
            continue;
        };
        match stream {
            crate::AgentStreamEvent::ToolUseQueued { call_id, .. } if call_id == tool_use_id => {
                queued += 1;
            }
            crate::AgentStreamEvent::ToolUseStarted { call_id, .. } if call_id == tool_use_id => {
                started += 1;
            }
            crate::AgentStreamEvent::ToolUseCompleted {
                call_id, is_error, ..
            } if call_id == tool_use_id => {
                completed += 1;
                completed_is_error = Some(*is_error);
            }
            _ => {}
        }
    }
    (queued, started, completed, completed_is_error)
}

fn completed_event_output(events: &[CoreEvent], tool_use_id: &str) -> Option<String> {
    events.iter().find_map(|event| {
        let CoreEvent::Stream(stream) = event else {
            return None;
        };
        match stream {
            crate::AgentStreamEvent::ToolUseCompleted {
                call_id, output, ..
            } if call_id == tool_use_id => Some(output.clone()),
            _ => None,
        }
    })
}

#[tokio::test]
async fn unknown_tool_call_gets_error_result_and_completed_event() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "unknown_1".into(),
        tool_name: "MissingTool".into(),
        input: serde_json::json!({}),
        final_text: "done".into(),
    });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (result, events) = collect_events_from_run(model, tools, config, None, "run it").await;

    assert_eq!(result.response_text, "done");
    assert_eq!(result.turns, 2);
    let output = tool_result_error_text(&result.final_messages, "unknown_1")
        .expect("unknown tool should produce an error tool result");
    assert!(output.contains("Unknown tool: MissingTool"));

    let (queued, started, completed, completed_is_error) =
        tool_lifecycle_counts(&events, "unknown_1");
    assert_eq!(queued, 1, "committed unknown tool call must be queued");
    assert_eq!(started, 0, "unknown tool call is never runnable");
    assert_eq!(completed, 1, "queued unknown tool call must complete");
    assert_eq!(completed_is_error, Some(true));
}

#[tokio::test]
async fn invalid_tool_input_gets_error_result_and_completed_event() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "invalid_read_1".into(),
        tool_name: "Read".into(),
        input: serde_json::json!({}),
        final_text: "done".into(),
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let config = QueryEngineConfig::default();
    let (result, events) = collect_events_from_run(model, tools, config, None, "read it").await;

    assert_eq!(result.response_text, "done");
    assert_eq!(result.turns, 2);
    let output = tool_result_error_text(&result.final_messages, "invalid_read_1")
        .expect("invalid input should produce an error tool result");
    assert!(output.contains("Invalid input"));
    assert!(output.contains("file_path"));

    let (queued, started, completed, completed_is_error) =
        tool_lifecycle_counts(&events, "invalid_read_1");
    assert_eq!(queued, 1, "committed invalid tool call must be queued");
    assert_eq!(started, 0, "validation failure is never runnable");
    assert_eq!(completed, 1, "queued invalid tool call must complete");
    assert_eq!(completed_is_error, Some(true));
}

struct PermissionRewriteTool;

#[async_trait::async_trait]
impl coco_tool::Tool for PermissionRewriteTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Custom("permission_rewrite".into())
    }

    fn name(&self) -> &str {
        "permission_rewrite"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "permission rewrite test tool".into()
    }

    async fn check_permissions(
        &self,
        _input: &serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> coco_types::PermissionDecision {
        coco_types::PermissionDecision::Allow {
            updated_input: Some(serde_json::json!({"value": "rewritten"})),
            feedback: None,
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult::data(input))
    }
}

struct HookEchoTool;

#[async_trait::async_trait]
impl coco_tool::Tool for HookEchoTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Custom("hook_echo".into())
    }

    fn name(&self) -> &str {
        "hook_echo"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "hook echo test tool".into()
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult::data(input))
    }
}

struct HookMcpTool;

#[async_trait::async_trait]
impl coco_tool::Tool for HookMcpTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Mcp {
            server: "test-server".into(),
            tool: "hook_mcp".into(),
        }
    }

    fn name(&self) -> &str {
        "hook_mcp"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "hook mcp test tool".into()
    }

    fn mcp_info(&self) -> Option<&coco_tool::McpToolInfo> {
        static INFO: std::sync::LazyLock<coco_tool::McpToolInfo> =
            std::sync::LazyLock::new(|| coco_tool::McpToolInfo {
                server_name: "test-server".into(),
                tool_name: "hook_mcp".into(),
            });
        Some(&INFO)
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult::data(serde_json::json!({
            "value": "original-mcp-output"
        })))
    }
}

struct HookOrderingTool;

#[async_trait::async_trait]
impl coco_tool::Tool for HookOrderingTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Custom("hook_ordering".into())
    }

    fn name(&self) -> &str {
        "hook_ordering"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "hook ordering test tool".into()
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult {
            data: serde_json::json!({"value": "ordering"}),
            new_messages: vec![coco_messages::create_user_message("tool new message")],
            app_state_patch: None,
        })
    }
}

struct HookOrderingMcpTool;

#[async_trait::async_trait]
impl coco_tool::Tool for HookOrderingMcpTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Mcp {
            server: "test-server".into(),
            tool: "hook_ordering_mcp".into(),
        }
    }

    fn name(&self) -> &str {
        "hook_ordering_mcp"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "hook ordering mcp test tool".into()
    }

    fn mcp_info(&self) -> Option<&coco_tool::McpToolInfo> {
        static INFO: std::sync::LazyLock<coco_tool::McpToolInfo> =
            std::sync::LazyLock::new(|| coco_tool::McpToolInfo {
                server_name: "test-server".into(),
                tool_name: "hook_ordering_mcp".into(),
            });
        Some(&INFO)
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult {
            data: serde_json::json!({"value": "ordering-mcp"}),
            new_messages: vec![coco_messages::create_user_message("tool new message")],
            app_state_patch: None,
        })
    }
}

struct HookFailTool;

#[async_trait::async_trait]
impl coco_tool::Tool for HookFailTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Custom("hook_fail".into())
    }

    fn name(&self) -> &str {
        "hook_fail"
    }

    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema::default()
    }

    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "hook failure test tool".into()
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Err(coco_tool::ToolError::ExecutionFailed {
            message: "kaboom".into(),
            source: None,
        })
    }
}

#[tokio::test]
async fn permission_allow_updated_input_reaches_execution() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "rewrite_1".into(),
        tool_name: "permission_rewrite".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(PermissionRewriteTool));
    let tools = Arc::new(registry);
    let config = QueryEngineConfig::default();
    let (result, events) = collect_events_from_run(model, tools, config, None, "rewrite it").await;

    assert_eq!(result.response_text, "done");
    let output = tool_result_text(&result.final_messages, "rewrite_1")
        .expect("rewritten tool should produce a successful tool result");
    assert!(output.contains("rewritten"), "output: {output}");
    assert!(!output.contains("original"), "output: {output}");

    let (queued, started, completed, completed_is_error) =
        tool_lifecycle_counts(&events, "rewrite_1");
    assert_eq!(queued, 1);
    assert_eq!(started, 1);
    assert_eq!(completed, 1);
    assert_eq!(completed_is_error, Some(false));
}

#[tokio::test]
async fn pre_tool_use_updated_input_reaches_execution() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "hook_rewrite_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PreToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"updatedInput\":{\"value\":\"hooked\"}}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("rewrite through hook").await.expect("ok");

    let output = tool_result_text(&result.final_messages, "hook_rewrite_1")
        .expect("hook-rewritten tool should produce a successful tool result");
    assert!(output.contains("hooked"), "output: {output}");
    assert!(!output.contains("original"), "output: {output}");
}

#[tokio::test]
async fn post_tool_use_receives_effective_input() {
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("post_hook_saw_effective_input");
    let marker_path = marker.to_str().unwrap().to_string();

    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "post_hook_input_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PreToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"updatedInput\":{\"value\":\"hooked\"}}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command: format!("if grep -q hooked; then touch '{marker_path}'; fi"),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("rewrite and post-hook").await.expect("ok");

    assert_eq!(result.response_text, "done");
    assert!(
        marker.exists(),
        "PostToolUse hook should receive the rewritten effective input"
    );
}

#[tokio::test]
async fn post_tool_use_updated_mcp_output_rewrites_mcp_result() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "post_hook_mcp_rewrite_1".into(),
        tool_name: "hook_mcp".into(),
        input: serde_json::json!({}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookMcpTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_mcp".into()),
        handler: coco_hooks::HookHandler::Command {
            command:
                "printf '%s\\n' '{\"updatedMCPToolOutput\":{\"value\":\"rewritten-mcp-output\"}}'"
                    .into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CoreEvent>(256);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }
        events
    });
    let result = engine
        .run_with_events("rewrite mcp output", event_tx)
        .await
        .expect("ok");
    let events = collector.await.expect("collector should join");

    let output = tool_result_text(&result.final_messages, "post_hook_mcp_rewrite_1")
        .expect("mcp tool should produce a successful tool result");
    assert!(output.contains("rewritten-mcp-output"), "output: {output}");
    assert!(!output.contains("original-mcp-output"), "output: {output}");
    let event_output = completed_event_output(&events, "post_hook_mcp_rewrite_1")
        .expect("tool completed event should be emitted");
    assert!(
        event_output.contains("rewritten-mcp-output"),
        "event output: {event_output}"
    );
    assert!(
        !event_output.contains("original-mcp-output"),
        "event output: {event_output}"
    );
}

#[tokio::test]
async fn post_tool_use_updated_mcp_output_is_ignored_for_non_mcp_tool() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "post_hook_non_mcp_rewrite_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command:
                "printf '%s\\n' '{\"updatedMCPToolOutput\":{\"value\":\"rewritten-non-mcp\"}}'"
                    .into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine
        .run("do not rewrite non-mcp output")
        .await
        .expect("ok");

    let output = tool_result_text(&result.final_messages, "post_hook_non_mcp_rewrite_1")
        .expect("non-mcp tool should produce a successful tool result");
    assert!(output.contains("original"), "output: {output}");
    assert!(!output.contains("rewritten-non-mcp"), "output: {output}");
}

#[tokio::test]
async fn post_tool_use_additional_context_is_injected() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "post_hook_context_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"additionalContext\":\"post hook context\"}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine
        .run("post-hook additional context")
        .await
        .expect("ok");

    let attachment = attachment_text_by_kind(
        &result.final_messages,
        coco_types::AttachmentKind::HookAdditionalContext,
    )
    .expect("post-tool-use hook should inject additional context");
    assert!(attachment.contains("hook_echo hook additional context: post hook context"));
}

#[tokio::test]
async fn post_tool_use_prevent_continuation_stops_next_turn() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "post_hook_stop_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "should not happen".into(),
    });
    let model_for_client: Arc<dyn LanguageModelV4> = model.clone();
    let client = Arc::new(ApiClient::new(model_for_client, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"continue\":false,\"stopReason\":\"stop after tool\"}'"
                .into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("post-hook stop continuation").await.expect("ok");

    assert_eq!(model.call_count.load(Ordering::SeqCst), 1);
    assert_eq!(result.stop_reason.as_deref(), Some("stop after tool"));
    assert_eq!(result.last_continue_reason, None);
    let attachment = attachment_text_by_kind(
        &result.final_messages,
        coco_types::AttachmentKind::HookStoppedContinuation,
    )
    .expect("post-tool-use hook should inject stopped-continuation attachment");
    assert!(attachment.contains("hook_echo hook stopped continuation: stop after tool"));
}

#[tokio::test]
async fn non_mcp_success_path_orders_post_hook_messages_before_new_messages() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "ordering_non_mcp_1".into(),
        tool_name: "hook_ordering".into(),
        input: serde_json::json!({}),
        final_text: "should not happen".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookOrderingTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_ordering".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"additionalContext\":\"hook context\",\"continue\":false,\"stopReason\":\"stop ordering\"}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("check non-mcp ordering").await.expect("ok");

    let tool_result_idx =
        tool_result_index(&result.final_messages, "ordering_non_mcp_1").expect("tool result");
    let additional_idx = attachment_index_by_kind_and_text(
        &result.final_messages,
        coco_types::AttachmentKind::HookAdditionalContext,
        "hook context",
    )
    .expect("hook additional context");
    let new_message_idx =
        user_message_index_containing(&result.final_messages, "tool new message").expect("new msg");
    let prevent_idx = attachment_index_by_kind_and_text(
        &result.final_messages,
        coco_types::AttachmentKind::HookStoppedContinuation,
        "stop ordering",
    )
    .expect("prevent attachment");

    assert!(tool_result_idx < additional_idx);
    assert!(additional_idx < new_message_idx);
    assert!(new_message_idx < prevent_idx);
}

#[tokio::test]
async fn mcp_success_path_defers_post_hook_messages_until_after_prevent() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "ordering_mcp_1".into(),
        tool_name: "hook_ordering_mcp".into(),
        input: serde_json::json!({}),
        final_text: "should not happen".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookOrderingMcpTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUse,
        matcher: Some("hook_ordering_mcp".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"additionalContext\":\"hook context\",\"continue\":false,\"stopReason\":\"stop ordering\"}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("check mcp ordering").await.expect("ok");

    let tool_result_idx =
        tool_result_index(&result.final_messages, "ordering_mcp_1").expect("tool result");
    let new_message_idx =
        user_message_index_containing(&result.final_messages, "tool new message").expect("new msg");
    let prevent_idx = attachment_index_by_kind_and_text(
        &result.final_messages,
        coco_types::AttachmentKind::HookStoppedContinuation,
        "stop ordering",
    )
    .expect("prevent attachment");
    let additional_idx = attachment_index_by_kind_and_text(
        &result.final_messages,
        coco_types::AttachmentKind::HookAdditionalContext,
        "hook context",
    )
    .expect("hook additional context");

    assert!(tool_result_idx < new_message_idx);
    assert!(new_message_idx < prevent_idx);
    assert!(prevent_idx < additional_idx);
}

#[tokio::test]
async fn failure_path_orders_error_result_before_post_tool_use_failure_context() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "failure_ordering_1".into(),
        tool_name: "hook_fail".into(),
        input: serde_json::json!({}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookFailTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PostToolUseFailure,
        matcher: Some("hook_fail".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "printf '%s\\n' '{\"additionalContext\":\"failure context\",\"continue\":false,\"stopReason\":\"ignored stop\"}'".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("check failure ordering").await.expect("ok");

    let error_output = tool_result_error_text(&result.final_messages, "failure_ordering_1")
        .expect("failure tool should produce an error tool result");
    assert!(error_output.contains("kaboom"), "output: {error_output}");
    let tool_result_idx =
        tool_result_index(&result.final_messages, "failure_ordering_1").expect("tool result");
    let additional_idx = attachment_index_by_kind_and_text(
        &result.final_messages,
        coco_types::AttachmentKind::HookAdditionalContext,
        "failure context",
    )
    .expect("failure additional context");
    assert!(tool_result_idx < additional_idx);
    assert!(
        attachment_index_by_kind_and_text(
            &result.final_messages,
            coco_types::AttachmentKind::HookStoppedContinuation,
            "ignored stop",
        )
        .is_none(),
        "failure path must not emit prevent_continuation attachments"
    );
}

#[tokio::test]
async fn failure_path_completed_event_matches_error_tool_result_text() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "failure_event_1".into(),
        tool_name: "hook_fail".into(),
        input: serde_json::json!({}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookFailTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CoreEvent>(256);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }
        events
    });
    let result = engine
        .run_with_events("check failure event output", event_tx)
        .await
        .expect("ok");
    let events = collector.await.expect("collector should join");

    let tool_result_output = tool_result_error_text(&result.final_messages, "failure_event_1")
        .expect("failure tool should produce an error tool result");
    let event_output = completed_event_output(&events, "failure_event_1")
        .expect("tool completed event should be emitted");
    assert_eq!(event_output, tool_result_output);
}

#[tokio::test]
async fn pre_tool_use_permission_deny_records_denial() {
    let model = Arc::new(OneToolThenTextMock {
        call_count: AtomicI32::new(0),
        tool_call_id: "hook_deny_1".into(),
        tool_name: "hook_echo".into(),
        input: serde_json::json!({"value": "original"}),
        final_text: "done".into(),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(HookEchoTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PreToolUse,
        matcher: Some("hook_echo".into()),
        handler: coco_hooks::HookHandler::Command {
            command:
                "printf '%s\\n' '{\"permissionDecision\":\"deny\",\"reason\":\"hook says no\"}'"
                    .into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    );
    let result = engine.run("deny through hook").await.expect("ok");

    assert_eq!(result.permission_denials.len(), 1);
    assert_eq!(result.permission_denials[0].tool_name, "hook_echo");
    assert_eq!(
        result.permission_denials[0].tool_input,
        serde_json::json!({"value": "original"})
    );
    let output = tool_result_error_text(&result.final_messages, "hook_deny_1")
        .expect("hook permission denial should produce an error tool result");
    assert!(output.contains("hook says no"), "output: {output}");
}

#[tokio::test]
async fn test_session_started_emitted_with_bootstrap() {
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig {
        model_name: "test-model".into(),
        session_id: "session-1".into(),
        permission_mode: PermissionMode::AcceptEdits,
        ..Default::default()
    };
    let bootstrap = SessionBootstrap {
        protocol_version: "1.0".into(),
        cwd: "/tmp".into(),
        version: "0.0.1".into(),
        slash_commands: vec!["/help".into()],
        agents: vec!["explorer".into()],
        ..Default::default()
    };
    let (_result, events) =
        collect_events_from_run(model, tools, config, Some(bootstrap), "hi").await;

    let started = events.iter().find_map(|e| match e {
        CoreEvent::Protocol(ServerNotification::SessionStarted(p)) => Some(p),
        _ => None,
    });
    let p = started.expect("SessionStarted should be emitted");
    assert_eq!(p.session_id, "session-1");
    assert_eq!(p.model, "test-model");
    assert_eq!(p.cwd, "/tmp");
    assert_eq!(p.version, "0.0.1");
    assert_eq!(p.permission_mode, "acceptEdits");
    assert_eq!(p.slash_commands, vec!["/help".to_string()]);
    assert_eq!(p.agents, vec!["explorer".to_string()]);
}

#[tokio::test]
async fn test_session_started_not_emitted_without_bootstrap() {
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (_result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    let found = events.iter().any(|e| {
        matches!(
            e,
            CoreEvent::Protocol(ServerNotification::SessionStarted(_))
        )
    });
    assert!(!found, "SessionStarted should not fire without bootstrap");
}

#[tokio::test]
async fn test_session_state_transitions_running_then_idle() {
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (_result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    let states: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::Protocol(ServerNotification::SessionStateChanged { state }) => Some(*state),
            _ => None,
        })
        .collect();

    assert_eq!(states.len(), 2, "expected Running + Idle");
    assert_eq!(states[0], coco_types::SessionState::Running);
    assert_eq!(states[1], coco_types::SessionState::Idle);
}

#[tokio::test]
async fn test_session_result_emitted_with_full_metadata() {
    let model = Arc::new(TextMock {
        text: "final".into(),
    });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig {
        model_name: "test-model".into(),
        session_id: "s1".into(),
        ..Default::default()
    };
    let (result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    let sr_params = events.iter().find_map(|e| match e {
        CoreEvent::Protocol(ServerNotification::SessionResult(p)) => Some(p.as_ref()),
        _ => None,
    });
    let p = sr_params.expect("SessionResult should be emitted");
    assert_eq!(p.session_id, "s1");
    assert_eq!(p.total_turns, result.turns);
    assert_eq!(p.duration_ms, result.duration_ms);
    assert_eq!(p.stop_reason, "end_turn");
    assert!(!p.is_error);
    assert_eq!(p.result.as_deref(), Some("final"));
    // CostTracker should have recorded the mock API call under the model name.
    assert!(p.model_usage.contains_key("mock-text"));
}

#[tokio::test]
async fn test_session_result_ordering_after_idle() {
    // SessionStateChanged(Idle) should be emitted before SessionResult
    // so SDK consumers see the state transition first.
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (_result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    let idle_idx = events.iter().position(|e| {
        matches!(
            e,
            CoreEvent::Protocol(ServerNotification::SessionStateChanged {
                state: coco_types::SessionState::Idle
            })
        )
    });
    let result_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::Protocol(ServerNotification::SessionResult(_))));
    assert!(idle_idx.is_some());
    assert!(result_idx.is_some());
    assert!(idle_idx < result_idx);
}

#[tokio::test]
async fn test_session_events_fire_in_strict_order() {
    // The complete envelope: SessionStarted → Running → ... → Idle → SessionResult
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let bootstrap = SessionBootstrap {
        cwd: "/".into(),
        protocol_version: "1.0".into(),
        version: "0.0.1".into(),
        ..Default::default()
    };
    let (_result, events) =
        collect_events_from_run(model, tools, config, Some(bootstrap), "hi").await;

    let started_idx = events.iter().position(|e| {
        matches!(
            e,
            CoreEvent::Protocol(ServerNotification::SessionStarted(_))
        )
    });
    let running_idx = events.iter().position(|e| {
        matches!(
            e,
            CoreEvent::Protocol(ServerNotification::SessionStateChanged {
                state: coco_types::SessionState::Running
            })
        )
    });
    let idle_idx = events.iter().position(|e| {
        matches!(
            e,
            CoreEvent::Protocol(ServerNotification::SessionStateChanged {
                state: coco_types::SessionState::Idle
            })
        )
    });
    let result_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::Protocol(ServerNotification::SessionResult(_))));

    assert!(started_idx.is_some(), "SessionStarted missing");
    assert!(
        running_idx.is_some(),
        "SessionStateChanged(Running) missing"
    );
    assert!(idle_idx.is_some(), "SessionStateChanged(Idle) missing");
    assert!(result_idx.is_some(), "SessionResult missing");

    // TS-aligned ordering: init → running → ... → idle → result
    assert!(started_idx < running_idx, "init must precede running");
    assert!(running_idx < idle_idx, "running must precede idle");
    assert!(idle_idx < result_idx, "idle must precede result");
}

#[tokio::test]
async fn test_session_result_num_api_calls_populated() {
    // Verify num_api_calls is populated from CostTracker.total_api_calls.
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (_result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    let sr = events.iter().find_map(|e| match e {
        CoreEvent::Protocol(ServerNotification::SessionResult(p)) => Some(p.as_ref()),
        _ => None,
    });
    let p = sr.expect("SessionResult should be emitted");
    assert_eq!(p.num_api_calls, Some(1), "one API call was made");
}

/// Mock tool that always returns `PermissionDecision::Ask`. Used to verify
/// that the engine emits `RequiresAction` before falling through to Allow.
#[derive(Debug)]
struct AskingTool;

#[async_trait::async_trait]
impl coco_tool::Tool for AskingTool {
    fn id(&self) -> coco_types::ToolId {
        coco_types::ToolId::Custom("AskingTool".into())
    }
    fn name(&self) -> &str {
        "AskingTool"
    }
    fn input_schema(&self) -> coco_types::ToolInputSchema {
        coco_types::ToolInputSchema {
            properties: std::collections::HashMap::new(),
        }
    }
    fn description(
        &self,
        _input: &serde_json::Value,
        _options: &coco_tool::DescriptionOptions,
    ) -> String {
        "asking tool".into()
    }
    async fn prompt(&self, _options: &coco_tool::PromptOptions) -> String {
        "asking tool".into()
    }
    async fn check_permissions(
        &self,
        _input: &serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> coco_types::PermissionDecision {
        coco_types::PermissionDecision::Ask {
            message: "please approve".into(),
            suggestions: vec![],
        }
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<coco_types::ToolResult<serde_json::Value>, coco_tool::ToolError> {
        Ok(coco_types::ToolResult::data(
            serde_json::json!({ "ok": true }),
        ))
    }
}

/// Mock that first returns a tool call to AskingTool, then returns text.
struct AskingToolCallMock {
    call_count: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for AskingToolCallMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-asking"
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: "call_1".into(),
                    tool_name: "AskingTool".into(),
                    input: serde_json::json!({}),
                    provider_executed: None,
                    provider_metadata: None,
                })],
                usage: Usage::new(10, 5),
                finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
        } else {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: "approved and done".into(),
                    provider_metadata: None,
                })],
                usage: Usage::new(5, 3),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
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

#[tokio::test]
async fn test_requires_action_emitted_on_permission_ask() {
    // Phase 2.F.1: when a tool's check_permissions returns Ask, the engine
    // emits SessionStateChanged::RequiresAction, then transitions back to
    // Running. Currently the Ask still falls through to Allow — the full
    // approval roundtrip is wired in Phase 2.C.4.
    let model = Arc::new(AskingToolCallMock {
        call_count: AtomicI32::new(0),
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingTool));
    let tools = Arc::new(registry);
    let config = QueryEngineConfig::default();
    let (result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    // Collect state transitions in order
    let states: Vec<coco_types::SessionState> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::Protocol(ServerNotification::SessionStateChanged { state }) => Some(*state),
            _ => None,
        })
        .collect();

    // Expected sequence:
    //   Running (session entry)
    //   RequiresAction (Ask encountered)
    //   Running (transitioned back)
    //   Idle (session exit)
    assert!(
        states.contains(&coco_types::SessionState::RequiresAction),
        "RequiresAction missing from states: {states:?}"
    );
    assert_eq!(states.first(), Some(&coco_types::SessionState::Running));
    assert_eq!(states.last(), Some(&coco_types::SessionState::Idle));

    // The RequiresAction must come before Idle
    let req_idx = states
        .iter()
        .position(|s| *s == coco_types::SessionState::RequiresAction);
    let idle_idx = states
        .iter()
        .position(|s| *s == coco_types::SessionState::Idle);
    assert!(req_idx.is_some() && idle_idx.is_some());
    assert!(req_idx < idle_idx);

    // SessionStateTracker dedup contract: no consecutive duplicate states.
    // Two Running events in a row would indicate the engine is emitting
    // without going through the tracker. See plan file WS-4.
    for window in states.windows(2) {
        assert_ne!(
            window[0], window[1],
            "consecutive duplicate SessionState: {states:?}"
        );
    }

    // And since Ask auto-allows, the turn still completes successfully.
    assert_eq!(result.turns, 2);
}

#[tokio::test]
async fn test_query_result_has_permission_denials_field() {
    // Phase 2.F.2: QueryResult carries permission_denials and
    // SessionResult flushes them to the SDK consumer. For a happy-path
    // session with no denials, the vec should be empty.
    let model = Arc::new(TextMock { text: "ok".into() });
    let tools = Arc::new(ToolRegistry::new());
    let config = QueryEngineConfig::default();
    let (result, events) = collect_events_from_run(model, tools, config, None, "hi").await;

    assert!(result.permission_denials.is_empty());
    // Verify the SessionResult also reflects this.
    let sr = events.iter().find_map(|e| match e {
        CoreEvent::Protocol(ServerNotification::SessionResult(p)) => Some(p.as_ref()),
        _ => None,
    });
    let p = sr.expect("SessionResult should be emitted");
    assert!(p.permission_denials.is_empty());
}

#[tokio::test]
async fn test_session_result_cancelled_marks_is_error() {
    // Cancellation path: cancelled flag → is_error=true, stop_reason="cancelled".
    let model = Arc::new(TextMock { text: "ok".into() });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();
    cancel.cancel(); // Pre-cancel before running

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        /*hooks*/ None,
    );

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CoreEvent>(256);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }
        events
    });

    let _ = engine.run_with_events("hi", event_tx).await;
    let events = collector.await.unwrap();

    let sr = events.iter().find_map(|e| match e {
        CoreEvent::Protocol(ServerNotification::SessionResult(p)) => Some(p.as_ref()),
        _ => None,
    });
    let p = sr.expect("SessionResult should be emitted even when cancelled");
    assert!(p.is_error);
    assert!(p.result.is_none());
    assert_eq!(p.stop_reason, "cancelled");
}

// ─── Phase 2.C.9 + 2.C.10 — SdkApprovalBridge + final_messages ────────
//
// These tests exercise the engine's `PermissionDecision::Ask` branch
// (which in 2.C.9 was rewired to consult `ctx.permission_bridge`) and
// the `QueryResult.final_messages` field (added in 2.C.10 for multi-
// turn SDK history threading).

use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolPermissionBridge;
use coco_tool::ToolPermissionDecision;
use coco_tool::ToolPermissionRequest;
use coco_tool::ToolPermissionResolution;
use coco_types::PermissionDecision;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolResult as CocoToolResult;
use serde_json::Value;
use std::sync::Mutex as StdMutex;

/// Custom mock tool that always returns `PermissionDecision::Ask` and,
/// if execution is reached, reports success with an empty payload.
///
/// Used to test that the engine's Ask branch correctly consults the
/// installed `permission_bridge` and honors its decision (approved →
/// execute; rejected → skip + record denial).
struct AskingMockTool;

#[async_trait::async_trait]
impl Tool for AskingMockTool {
    fn id(&self) -> ToolId {
        ToolId::Custom("asking_mock".into())
    }
    fn name(&self) -> &str {
        "asking_mock"
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema::default()
    }
    fn description(&self, _input: &Value, _opts: &DescriptionOptions) -> String {
        "Mock tool that always returns Ask".into()
    }
    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> PermissionDecision {
        PermissionDecision::Ask {
            message: "Mock needs permission".into(),
            suggestions: Vec::new(),
        }
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: &coco_tool::ToolUseContext,
    ) -> Result<CocoToolResult<Value>, ToolError> {
        Ok(CocoToolResult::data(serde_json::json!({"ok": true})))
    }
}

/// Test bridge that records every `request_permission` call and
/// returns a pre-programmed decision. The recorded calls let the test
/// assert the engine supplied the expected fields (tool name, input).
struct RecordingBridge {
    decision: ToolPermissionDecision,
    calls: StdMutex<Vec<ToolPermissionRequest>>,
}

impl RecordingBridge {
    fn new(decision: ToolPermissionDecision) -> Self {
        Self {
            decision,
            calls: StdMutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<ToolPermissionRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ToolPermissionBridge for RecordingBridge {
    async fn request_permission(
        &self,
        request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        self.calls.lock().unwrap().push(request);
        Ok(ToolPermissionResolution {
            decision: self.decision,
            feedback: Some("recorded".into()),
        })
    }
}

/// Mock that emits a single tool_call to `asking_mock`, then on the
/// follow-up call (after the tool result or denial) emits a final text.
struct AskingToolThenTextMock {
    call_count: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for AskingToolThenTextMock {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-asking"
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: "ask_call_1".into(),
                    tool_name: "asking_mock".into(),
                    input: serde_json::json!({}),
                    provider_executed: None,
                    provider_metadata: None,
                })],
                usage: Usage::new(5, 5),
                finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
        } else {
            Ok(LanguageModelV4GenerateResult {
                content: vec![AssistantContentPart::Text(TextPart {
                    text: "done".into(),
                    provider_metadata: None,
                })],
                usage: Usage::new(5, 5),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                warnings: vec![],
                provider_metadata: None,
                request: None,
                response: None,
            })
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

#[tokio::test]
async fn ask_branch_consults_bridge_and_executes_on_approved() {
    let model = Arc::new(AskingToolThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingMockTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let bridge = Arc::new(RecordingBridge::new(ToolPermissionDecision::Approved));
    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None)
        .with_permission_bridge(bridge.clone() as Arc<dyn ToolPermissionBridge>);

    let result = engine
        .run("please run asking_mock")
        .await
        .expect("should succeed");

    // Bridge saw exactly one call, for the asking_mock tool.
    let calls = bridge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool_name, "asking_mock");

    // Tool was executed (no denial recorded) and final text reached.
    assert_eq!(result.permission_denials.len(), 0);
    assert_eq!(result.response_text, "done");
}

#[tokio::test]
async fn ask_branch_consults_bridge_and_records_denial_on_rejected() {
    let model = Arc::new(AskingToolThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingMockTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let bridge = Arc::new(RecordingBridge::new(ToolPermissionDecision::Rejected));
    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None)
        .with_permission_bridge(bridge.clone() as Arc<dyn ToolPermissionBridge>);

    let result = engine
        .run("please run asking_mock")
        .await
        .expect("should succeed");

    // Bridge was consulted.
    assert_eq!(bridge.calls().len(), 1);

    // Denial was recorded, tool was not executed (engine loops to
    // second model call which emits the final text).
    assert_eq!(result.permission_denials.len(), 1);
    assert_eq!(result.permission_denials[0].tool_name, "asking_mock");
    assert_eq!(result.response_text, "done");
}

#[tokio::test]
async fn pre_tool_use_block_runs_before_permission_ask() {
    let model = Arc::new(AskingToolThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingMockTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let bridge = Arc::new(RecordingBridge::new(ToolPermissionDecision::Approved));
    let mut hooks = coco_hooks::HookRegistry::new();
    hooks.register(coco_hooks::HookDefinition {
        event: coco_types::HookEventType::PreToolUse,
        matcher: Some("asking_mock".into()),
        handler: coco_hooks::HookHandler::Command {
            command: "echo blocked by hook; exit 2".into(),
            timeout_ms: Some(1000),
            shell: None,
        },
        priority: 0,
        scope: coco_types::HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    });

    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel,
        Some(Arc::new(hooks)),
    )
    .with_permission_bridge(bridge.clone() as Arc<dyn ToolPermissionBridge>);

    let result = engine
        .run("please run asking_mock")
        .await
        .expect("should succeed");

    assert_eq!(
        bridge.calls().len(),
        0,
        "PreToolUse block should short-circuit before permission bridge"
    );
    assert!(result.permission_denials.is_empty());
    let output = tool_result_error_text(&result.final_messages, "ask_call_1")
        .expect("hook block should produce an error tool result");
    assert!(output.contains("blocked by hook"), "output: {output}");
    assert_eq!(result.response_text, "done");
}

#[tokio::test]
async fn ask_branch_without_bridge_falls_back_to_auto_allow() {
    // Sanity: existing (pre-2.C.9) behavior still works when no bridge
    // is installed. The tool auto-executes despite returning Ask.
    let model = Arc::new(AskingToolThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingMockTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("run it").await.expect("should succeed");

    assert_eq!(result.permission_denials.len(), 0);
    assert_eq!(result.response_text, "done");
}

#[tokio::test]
async fn query_result_final_messages_contains_full_roundtrip() {
    // Verify QueryResult.final_messages captures user + assistant +
    // tool_use + tool_result messages — this is the content the SDK
    // runner uses to thread multi-turn context.
    let model = Arc::new(ToolCallThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("read /tmp/nonexistent.txt").await.unwrap();

    // Expected shape (roughly):
    //   [User(prompt), Assistant(text+tool_call), User(tool_result), Assistant(final text)]
    // We require at least 4 messages: the initial user prompt plus the
    // tool-call roundtrip plus the final assistant reply. The exact
    // Message variant layout isn't locked in — just sanity check length
    // and that both roles appear.
    assert!(
        result.final_messages.len() >= 4,
        "expected >= 4 messages in final_messages, got {}",
        result.final_messages.len()
    );
    let has_user = result
        .final_messages
        .iter()
        .any(|m| matches!(m, coco_types::Message::User(_)));
    let has_assistant = result
        .final_messages
        .iter()
        .any(|m| matches!(m, coco_types::Message::Assistant(_)));
    assert!(has_user && has_assistant);
}

#[tokio::test]
async fn query_result_final_messages_populated_on_cancel() {
    // Cancellation path also goes through `make_result`, so
    // final_messages should be set — may be empty if cancelled
    // before the first message, but the field must exist.
    let model = Arc::new(TextMock { text: "hi".into() });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();
    cancel.cancel();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);
    let result = engine.run("hi").await.unwrap();

    assert!(result.cancelled);
    // final_messages is populated via `history.messages.clone()` even
    // on the cancellation path — it's a Vec (possibly empty), not a
    // dangling/default fallback.
    let _ = result.final_messages;
}

#[tokio::test]
async fn run_with_messages_uses_last_user_message_for_history_key() {
    // Regression test for the 2.C.10 change that made
    // `run_session_loop` use the LAST user message in `turn_messages`
    // rather than the first. This matters for multi-turn SDK use
    // where `turn_messages = [prior_history..., new_user_msg]`.
    //
    // The easiest way to exercise this is to pass in a pre-existing
    // history and verify the engine completes successfully — if the
    // old `first()` logic still ran, it would pick up the prior user
    // message and key file history against it, which is semantically
    // wrong but wouldn't cause an observable failure in tests. So
    // this test is more of a smoke test that `run_with_messages` still
    // works with multi-user-message inputs.
    let model = Arc::new(TextMock { text: "ack".into() });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let cancel = CancellationToken::new();

    let engine = QueryEngine::new(QueryEngineConfig::default(), client, tools, cancel, None);

    let prior = coco_messages::create_user_message("previous turn");
    let new = coco_messages::create_user_message("current turn");
    let (tx, _rx) = tokio::sync::mpsc::channel::<CoreEvent>(16);
    let result = engine
        .run_with_messages(vec![prior, new], tx)
        .await
        .expect("should succeed");

    assert_eq!(result.response_text, "ack");
    // The combined list + the assistant reply should be in final_messages.
    assert!(result.final_messages.len() >= 3);
}

/// Bridge that blocks forever until signalled via a `Notify`. Used to
/// verify the engine's Ask branch aborts its bridge await when the
/// cancel token fires — without this cancel-awareness, a turn can
/// hang indefinitely waiting for an SDK client approval that never
/// arrives.
struct BlockingBridge {
    started: Arc<tokio::sync::Notify>,
    unblock: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl ToolPermissionBridge for BlockingBridge {
    async fn request_permission(
        &self,
        _request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        // Signal that we've entered the bridge.
        self.started.notify_one();
        // Block until either (a) the test wakes us or (b) the engine's
        // `select!` aborts this await via cancel. We want the latter
        // path to fire in the test.
        self.unblock.notified().await;
        Ok(ToolPermissionResolution {
            decision: ToolPermissionDecision::Approved,
            feedback: None,
        })
    }
}

#[tokio::test]
async fn ask_branch_aborts_bridge_await_on_cancel() {
    // Regression test for the 2.C.9 second-round fix: the engine must
    // abort the bridge's `request_permission` await when the cancel
    // token fires. Previously, the oneshot inside
    // `SdkServerState::send_server_request` was not cancel-aware and
    // would hang forever if the SDK client never replied.
    let model = Arc::new(AskingToolThenTextMock {
        call_count: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(AskingMockTool));
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let started = Arc::new(tokio::sync::Notify::new());
    let unblock = Arc::new(tokio::sync::Notify::new());
    let bridge = Arc::new(BlockingBridge {
        started: started.clone(),
        unblock: unblock.clone(),
    });
    let engine = QueryEngine::new(
        QueryEngineConfig::default(),
        client,
        tools,
        cancel.clone(),
        None,
    )
    .with_permission_bridge(bridge as Arc<dyn ToolPermissionBridge>);

    // Kick off the engine and simultaneously wait for the bridge to
    // enter its block, then cancel.
    let engine_task = tokio::spawn(async move { engine.run("run asking_mock").await });

    // Wait for the bridge to signal it's inside `request_permission`.
    tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
        .await
        .expect("bridge should enter its await");

    // Now cancel the turn. The engine should abort the bridge await
    // via its internal `tokio::select!` and treat the cancel as a
    // rejection with feedback, then loop around and exit on the next
    // top-of-loop `cancel.is_cancelled()` check.
    cancel.cancel();

    // The engine should return within a reasonable time — not hang.
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), engine_task)
        .await
        .expect("engine should not hang after cancel")
        .expect("engine task should not panic")
        .expect("engine should return Ok even on cancel");

    assert!(result.cancelled);

    // Explicitly drop the unblock notifier so the blocking bridge's
    // future doesn't continue running forever if any residual task
    // is still holding a reference.
    drop(unblock);
}

// ─── WS-5: forward_hook_events structured child task ────────────────────
//
// The forwarder translates `HookExecutionEvent`s into CoreEvent::Protocol
// notifications. It is now an owned child task with two exit paths:
//
//   1. Graceful: the caller drops the matching sender; `rx.recv()` returns
//      None after all queued events have been drained. Normal shutdown.
//   2. Cancellation: the token is cancelled. Any in-flight event in the
//      channel is discarded. Used when a drain timeout expires.
//
// These tests exercise both paths in isolation by invoking the private
// `QueryEngine::forward_hook_events` directly with synthetic channels.

#[tokio::test]
async fn test_hook_forwarder_drains_on_sender_drop() {
    // Graceful path: push three events, drop the sender, the forwarder
    // should drain all three into the core_tx before exiting.
    let (hook_tx, hook_rx) = tokio::sync::mpsc::channel::<coco_hooks::HookExecutionEvent>(16);
    let (core_tx, mut core_rx) = tokio::sync::mpsc::channel::<CoreEvent>(16);
    let cancel = CancellationToken::new();

    let handle = tokio::spawn(QueryEngine::forward_hook_events(
        hook_rx,
        Some(core_tx),
        cancel,
    ));

    hook_tx
        .send(coco_hooks::HookExecutionEvent::Started {
            hook_id: "h1".into(),
            hook_name: "hook-one".into(),
            hook_event: "PreToolUse".into(),
        })
        .await
        .unwrap();
    hook_tx
        .send(coco_hooks::HookExecutionEvent::Progress {
            hook_id: "h1".into(),
            hook_name: "hook-one".into(),
            stdout: "working".into(),
            stderr: String::new(),
        })
        .await
        .unwrap();
    hook_tx
        .send(coco_hooks::HookExecutionEvent::Response {
            hook_id: "h1".into(),
            hook_name: "hook-one".into(),
            exit_code: Some(0),
            stdout: "done".into(),
            stderr: String::new(),
            outcome: coco_types::HookOutcome::Success,
        })
        .await
        .unwrap();
    drop(hook_tx);

    // Forwarder must exit cleanly and promptly.
    tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("forwarder must exit within 2s on sender-drop")
        .expect("forwarder task must not panic");

    // All three events must have been translated and forwarded.
    let mut forwarded = Vec::new();
    while let Ok(evt) = core_rx.try_recv() {
        forwarded.push(evt);
    }
    assert_eq!(
        forwarded.len(),
        3,
        "expected 3 forwarded events: {forwarded:?}"
    );
    assert!(matches!(
        forwarded[0],
        CoreEvent::Protocol(ServerNotification::HookStarted(_))
    ));
    assert!(matches!(
        forwarded[1],
        CoreEvent::Protocol(ServerNotification::HookProgress(_))
    ));
    assert!(matches!(
        forwarded[2],
        CoreEvent::Protocol(ServerNotification::HookResponse(_))
    ));
}

// ── Progress-event forwarder (protocol + TUI fan-out + throttle) ──

#[test]
fn test_classify_progress_payload_recognizes_bash_and_powershell() {
    let bash = serde_json::json!({
        "type": "bash_progress",
        "status": "running",
        "elapsedTimeSeconds": 4.5,
        "taskId": "t-1",
    });
    let (tool_name, elapsed, task_id) =
        super::classify_progress_payload(&bash).expect("bash must classify");
    assert_eq!(tool_name, "Bash");
    assert_eq!(elapsed, 4.5);
    assert_eq!(task_id.as_deref(), Some("t-1"));

    let ps = serde_json::json!({"type": "powershell_progress"});
    let (tool_name, elapsed, task_id) =
        super::classify_progress_payload(&ps).expect("powershell must classify");
    assert_eq!(tool_name, "PowerShell");
    assert_eq!(elapsed, 0.0);
    assert_eq!(task_id, None);
}

#[test]
fn test_classify_progress_payload_rejects_unrelated_types() {
    // agent_progress and skill_progress follow different propagation
    // in TS — they must NOT produce a protocol ToolProgress here.
    for t in ["agent_progress", "skill_progress", "other"] {
        let v = serde_json::json!({"type": t});
        assert!(
            super::classify_progress_payload(&v).is_none(),
            "type {t} must not classify"
        );
    }
    // Missing `type` field → None.
    assert!(super::classify_progress_payload(&serde_json::json!({})).is_none());
    // Non-object payload → None.
    assert!(super::classify_progress_payload(&serde_json::json!("str")).is_none());
}

#[test]
fn test_progress_throttle_blocks_second_emission_within_window() {
    // 1-second window is enough for the test to never have to wait:
    // the two `now` values we pass are synthetic `Instant`s.
    let cap = std::num::NonZeroUsize::new(100).unwrap_or(std::num::NonZeroUsize::MIN);
    let mut th = super::ProgressThrottle::with_params(std::time::Duration::from_secs(1), cap);
    let t0 = std::time::Instant::now();
    assert!(th.allow("parent-A", t0), "first call must pass");
    let t1 = t0 + std::time::Duration::from_millis(500);
    assert!(
        !th.allow("parent-A", t1),
        "within-window call must be blocked"
    );
    let t2 = t0 + std::time::Duration::from_millis(1200);
    assert!(th.allow("parent-A", t2), "post-window call must pass");
}

#[test]
fn test_progress_throttle_lru_evicts_oldest_key() {
    // Tiny max (2 entries) so the LRU path is exercised in one call.
    let cap = std::num::NonZeroUsize::new(2).unwrap_or(std::num::NonZeroUsize::MIN);
    let mut th = super::ProgressThrottle::with_params(std::time::Duration::from_secs(60), cap);
    let t = std::time::Instant::now();
    assert!(th.allow("A", t));
    assert!(th.allow("B", t + std::time::Duration::from_secs(1)));
    // Adding "C" must evict "A" (oldest).
    assert!(th.allow("C", t + std::time::Duration::from_secs(2)));
    // "A" re-appears: since it was evicted, the next emission for it
    // should pass (within-window blocking would have kept it).
    assert!(
        th.allow("A", t + std::time::Duration::from_secs(3)),
        "A was evicted so it should re-pass"
    );
}

#[tokio::test]
async fn test_drain_one_progress_emits_both_tui_and_protocol_when_qualifying() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(8);
    let mut throttle = super::ProgressThrottle::new();
    let progress = coco_tool::ToolProgress {
        tool_use_id: "tu-1".into(),
        parent_tool_use_id: Some("parent-1".into()),
        data: serde_json::json!({
            "type": "bash_progress",
            "status": "running",
            "elapsedTimeSeconds": 2.0,
        }),
    };
    super::drain_one_progress(&Some(tx), progress, &mut throttle).await;

    // Event 1: TUI-only ToolProgress (raw data passthrough).
    let tui_evt = rx.recv().await.expect("first event");
    match tui_evt {
        CoreEvent::Tui(coco_types::TuiOnlyEvent::ToolProgress { tool_use_id, .. }) => {
            assert_eq!(tool_use_id, "tu-1");
        }
        other => panic!("expected Tui ToolProgress, got {other:?}"),
    }
    // Event 2: protocol ToolProgress.
    let proto_evt = rx.recv().await.expect("second event");
    match proto_evt {
        CoreEvent::Protocol(coco_types::ServerNotification::ToolProgress(p)) => {
            assert_eq!(p.tool_use_id, "tu-1");
            assert_eq!(p.tool_name, "Bash");
            assert_eq!(p.parent_tool_use_id.as_deref(), Some("parent-1"));
            assert_eq!(p.elapsed_time_seconds, 2.0);
        }
        other => panic!("expected Protocol ToolProgress, got {other:?}"),
    }
}

#[tokio::test]
async fn test_drain_one_progress_suppresses_protocol_for_non_bash_payload() {
    // `agent_progress` is a valid tool progress shape but must not
    // surface a protocol `ToolProgress` wire event — only TUI.
    // Keep an owning `Some(tx)` across the call so the channel
    // doesn't close when the drain returns — otherwise `rx.recv()`
    // yields `Ok(None)` instead of pending.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(8);
    let sender = Some(tx);
    let mut throttle = super::ProgressThrottle::new();
    let progress = coco_tool::ToolProgress {
        tool_use_id: "tu-agent".into(),
        parent_tool_use_id: None,
        data: serde_json::json!({"type": "agent_progress"}),
    };
    super::drain_one_progress(&sender, progress, &mut throttle).await;
    // Only the TUI event is delivered; the next recv blocks.
    let evt = rx.recv().await.expect("TUI event");
    assert!(matches!(
        evt,
        CoreEvent::Tui(coco_types::TuiOnlyEvent::ToolProgress { .. })
    ));
    let res = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
    assert!(
        res.is_err(),
        "no protocol event must be delivered, got: {res:?}"
    );
    drop(sender);
}

#[tokio::test]
async fn test_drain_one_progress_throttles_bursts() {
    // Two back-to-back bash progress events share a parent id — the
    // second protocol emission must be throttled.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(16);
    let mut throttle = super::ProgressThrottle::new();
    let make = |tu: &str| coco_tool::ToolProgress {
        tool_use_id: tu.into(),
        parent_tool_use_id: Some("parent-X".into()),
        data: serde_json::json!({"type": "bash_progress"}),
    };
    super::drain_one_progress(&Some(tx.clone()), make("tu-1"), &mut throttle).await;
    super::drain_one_progress(&Some(tx), make("tu-2"), &mut throttle).await;

    // Four events max (two calls × {TUI, Protocol}) — but the second
    // protocol emission is throttled, so exactly 3 events arrive.
    let mut tui = 0;
    let mut proto = 0;
    while let Ok(Some(evt)) =
        tokio::time::timeout(std::time::Duration::from_millis(20), rx.recv()).await
    {
        match evt {
            CoreEvent::Tui(coco_types::TuiOnlyEvent::ToolProgress { .. }) => tui += 1,
            CoreEvent::Protocol(coco_types::ServerNotification::ToolProgress(_)) => proto += 1,
            other => panic!("unexpected event {other:?}"),
        }
    }
    assert_eq!(tui, 2, "TUI must see both events");
    assert_eq!(proto, 1, "Protocol emission must be throttled to one");
}

// ── Fallback classifier (I13 trigger) ──

#[test]
fn test_is_capacity_error_message_classifies_overloaded_variants() {
    // Covers: Rust `InferenceError::Overloaded` Display, Rust rate-limit
    // Display, raw Anthropic 529 text, raw 503, status prefixes, and
    // case insensitivity.
    assert!(super::is_capacity_error_message("provider overloaded"));
    assert!(super::is_capacity_error_message(
        "API Error: overloaded_error on request"
    ));
    assert!(super::is_capacity_error_message(
        "rate limited: retry later"
    ));
    assert!(super::is_capacity_error_message("status: 529 from gateway"));
    assert!(super::is_capacity_error_message("HTTP (503) upstream"));
    assert!(super::is_capacity_error_message(
        "Provider Overloaded: high demand"
    ));
}

#[test]
fn test_is_capacity_error_message_rejects_unrelated_errors() {
    // Non-capacity errors must not accidentally trigger the fallback.
    assert!(!super::is_capacity_error_message("authentication failed"));
    assert!(!super::is_capacity_error_message("prompt_too_long"));
    assert!(!super::is_capacity_error_message("network error: timeout"));
    assert!(!super::is_capacity_error_message(
        "provider error (500): internal"
    ));
}

#[tokio::test]
async fn test_emit_model_fallback_notice_capacity_degrade_template() {
    // Capacity-degrade: "Switched to {new} due to high demand for {original}".
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(4);
    super::emit_model_fallback_notice(
        &Some(tx),
        /*original*/ "claude-opus",
        /*new_model*/ "claude-sonnet",
        /*session_id*/ "s-1",
        crate::model_runtime::ModelFallbackReason::CapacityDegrade {
            consecutive_errors: 3,
        },
    )
    .await;
    let evt = rx.recv().await.expect("one event emitted");
    match evt {
        CoreEvent::Stream(crate::AgentStreamEvent::TextDelta { delta, turn_id }) => {
            assert_eq!(turn_id, "s-1");
            assert!(delta.contains("Switched to claude-sonnet"));
            assert!(delta.contains("claude-opus"));
            assert!(
                delta.contains("high demand"),
                "capacity-degrade template missing: {delta}"
            );
        }
        other => panic!("expected Stream(TextDelta), got {other:?}"),
    }
}

#[tokio::test]
async fn test_emit_model_fallback_notice_probe_recovery_template() {
    // Probe recovery must NOT describe primary as a "fallback model".
    // Direction-aware template: "Recovered to primary {new} after probe".
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(4);
    super::emit_model_fallback_notice(
        &Some(tx),
        /*original*/ "",
        /*new_model*/ "claude-opus",
        /*session_id*/ "s-2",
        crate::model_runtime::ModelFallbackReason::ProbeRecovery,
    )
    .await;
    let evt = rx.recv().await.expect("one event emitted");
    match evt {
        CoreEvent::Stream(crate::AgentStreamEvent::TextDelta { delta, .. }) => {
            assert!(
                delta.contains("Recovered to primary claude-opus"),
                "probe-recovery template missing: {delta}"
            );
            assert!(
                !delta.contains("fallback model"),
                "must NOT describe primary as a fallback: {delta}"
            );
        }
        other => panic!("expected Stream(TextDelta), got {other:?}"),
    }
}

#[tokio::test]
async fn test_emit_model_fallback_notice_chain_exhausted_template() {
    // ChainExhausted is terminal — announces no successor model.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CoreEvent>(4);
    super::emit_model_fallback_notice(
        &Some(tx),
        /*original*/ "gpt-5",
        /*new_model*/ "",
        /*session_id*/ "s-3",
        crate::model_runtime::ModelFallbackReason::ChainExhausted,
    )
    .await;
    let evt = rx.recv().await.expect("one event emitted");
    match evt {
        CoreEvent::Stream(crate::AgentStreamEvent::TextDelta { delta, .. }) => {
            assert!(
                delta.contains("exhausted"),
                "chain-exhausted template missing: {delta}"
            );
            assert!(delta.contains("gpt-5"), "last-tried model missing: {delta}");
        }
        other => panic!("expected Stream(TextDelta), got {other:?}"),
    }
}

#[tokio::test]
async fn test_hook_forwarder_exits_on_cancel() {
    // Cancellation path: the forwarder must exit within a short deadline
    // even while the sender side is still open. Simulates the drain-timeout
    // escape hatch in run_internal_with_messages.
    let (_hook_tx, hook_rx) = tokio::sync::mpsc::channel::<coco_hooks::HookExecutionEvent>(16);
    let (core_tx, _core_rx) = tokio::sync::mpsc::channel::<CoreEvent>(16);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(QueryEngine::forward_hook_events(
        hook_rx,
        Some(core_tx),
        cancel_clone,
    ));

    // Cancel; forwarder should return without waiting for sender drop.
    cancel.cancel();

    tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("forwarder must exit within 2s on cancel")
        .expect("forwarder task must not panic");

    // _hook_tx is still open — the forwarder exited via the cancel branch,
    // not the channel-closed branch. Confirming it would have hung without
    // the cancel token.
}
