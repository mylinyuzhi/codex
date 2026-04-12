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
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use super::*;
use coco_types::PermissionMode;

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
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        Err(AISdkError::new("not supported"))
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
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        Err(AISdkError::new("not supported"))
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
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        Err(AISdkError::new("not supported"))
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
            _: LanguageModelV4CallOptions,
        ) -> Result<LanguageModelV4StreamResult, AISdkError> {
            Err(AISdkError::new("not supported"))
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

    let text = result.data.as_str().unwrap();
    assert!(text.contains("integration_test_ok"));
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
