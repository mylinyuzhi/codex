//! Tests for the `QueryEngineRunner`.
//!
//! These are compile-level smoke tests: they verify the runner can be
//! constructed, is `Send + Sync` (required for `Arc<dyn TurnRunner>`),
//! and can be installed on an `SdkServer`. End-to-end behavior is
//! exercised via the CLI integration path once a mock model is plumbed
//! through; until then, `ScriptedRunner` in `dispatcher.test.rs` is the
//! unit-level stand-in for the `TurnRunner` trait contract.

use std::sync::Arc;
use std::sync::atomic::AtomicI32;

use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_tool_runtime::ToolRegistry;
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

use super::*;

struct SilentModel {
    _calls: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for SilentModel {
    fn provider(&self) -> &str {
        "silent"
    }
    fn model_id(&self) -> &str {
        "silent-test"
    }
    async fn do_generate(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        Ok(LanguageModelV4GenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: "ok".into(),
                provider_metadata: None,
            })],
            usage: Usage::new(1, 1),
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
        Err(AISdkError::new(
            "streaming not supported in silent test model",
        ))
    }
}

#[test]
fn runner_is_send_sync() {
    // This is a compile-time assertion via function signatures that
    // `QueryEngineRunner: Send + Sync`. Required because SdkServerState
    // holds an `Arc<dyn TurnRunner>` across await points.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<QueryEngineRunner>();
}

#[test]
fn runner_can_be_constructed_and_boxed_as_trait_object() {
    let model = Arc::new(SilentModel {
        _calls: AtomicI32::new(0),
    });
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));
    let tools = Arc::new(ToolRegistry::new());
    let runner = QueryEngineRunner::new(
        client, tools, /*max_output_tokens*/ 16_384, /*max_turns*/ 10,
        /*system_prompt*/ None,
    );
    let _as_trait_obj: Arc<dyn crate::sdk_server::handlers::TurnRunner> = Arc::new(runner);
}
