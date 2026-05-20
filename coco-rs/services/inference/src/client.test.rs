use std::sync::Arc;

use coco_llm_types::AssistantContentPart;
use coco_llm_types::FinishReason;
use coco_llm_types::LlmMessage;
use coco_llm_types::StopReason;
use coco_llm_types::TextPart;
use coco_llm_types::Usage;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;

use super::*;

/// Simple mock model for testing — returns a fixed text response.
struct MockModel {
    response_text: String,
}

impl MockModel {
    fn new(text: &str) -> Self {
        Self {
            response_text: text.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl LanguageModelV4 for MockModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }

    async fn do_generate(
        &self,
        _options: &LanguageModelV4CallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<LanguageModelV4GenerateResult, vercel_ai_provider::AISdkError> {
        Ok(LanguageModelV4GenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: self.response_text.clone(),
                provider_metadata: None,
            })],
            usage: Usage::new(10, 5),
            finish_reason: FinishReason::new(StopReason::EndTurn),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }

    async fn do_stream(
        &self,
        _options: &LanguageModelV4CallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<LanguageModelV4StreamResult, vercel_ai_provider::AISdkError> {
        Err(vercel_ai_provider::AISdkError::new(
            "mock does not support streaming",
        ))
    }
}

/// Mock model that always returns an error.
struct ErrorModel;

#[async_trait::async_trait]
impl LanguageModelV4 for ErrorModel {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "error-model"
    }
    async fn do_generate(
        &self,
        _options: &LanguageModelV4CallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<LanguageModelV4GenerateResult, vercel_ai_provider::AISdkError> {
        Err(vercel_ai_provider::AISdkError::new("simulated failure"))
    }
    async fn do_stream(
        &self,
        _options: &LanguageModelV4CallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<LanguageModelV4StreamResult, vercel_ai_provider::AISdkError> {
        Err(vercel_ai_provider::AISdkError::new("simulated failure"))
    }
}

fn mock_client(text: &str) -> ApiClient {
    ApiClient::with_default_fingerprint(Arc::new(MockModel::new(text)), RetryConfig::default())
}

#[tokio::test]
async fn test_client_returns_mock_text() {
    let client = mock_client("Hello from mock!");
    let params = QueryParams {
        prompt: vec![LlmMessage::user_text("hi")],
        max_tokens: Some(100),
        ..Default::default()
    };
    let result = client.query(&params).await.expect("query should succeed");

    // Verify we got the mock response
    assert!(!result.content.is_empty());
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 5);
    assert_eq!(result.model, "mock-model");
    assert_eq!(result.retries, 0);
    assert!(result.total_duration_ms >= 0);
}

#[tokio::test]
async fn test_client_model_id() {
    let client = mock_client("test");
    assert_eq!(client.model_id(), "mock-model");
    assert_eq!(client.provider(), "mock");
}

#[tokio::test]
async fn test_usage_accumulation() {
    let client = mock_client("test");
    let params = QueryParams {
        prompt: vec![LlmMessage::user_text("hi")],
        max_tokens: Some(100),
        ..Default::default()
    };

    client.query(&params).await.expect("query 1");
    client.query(&params).await.expect("query 2");

    let usage = client.accumulated_usage().await;
    assert_eq!(usage.call_count, 2);
    assert_eq!(usage.total.input_tokens, 20);
    assert_eq!(usage.total.output_tokens, 10);
}

#[tokio::test]
async fn test_error_model_fails() {
    let client = ApiClient::with_default_fingerprint(
        Arc::new(ErrorModel),
        RetryConfig {
            max_retries: 0,
            ..Default::default()
        },
    );
    let params = QueryParams {
        prompt: vec![LlmMessage::user_text("hi")],
        max_tokens: Some(100),
        ..Default::default()
    };
    let result = client.query(&params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_provider_error_includes_provider_and_model_attribution() {
    let client = ApiClient::with_default_fingerprint(
        Arc::new(ErrorModel),
        RetryConfig {
            max_retries: 0,
            ..Default::default()
        },
    );
    let params = QueryParams {
        prompt: vec![LlmMessage::user_text("hi")],
        max_tokens: Some(100),
        ..Default::default()
    };
    let err = client.query(&params).await.unwrap_err();
    let message = match err {
        InferenceError::ProviderError { message, .. } => message,
        other => panic!("expected ProviderError, got {other:?}"),
    };
    assert!(
        message.contains("Provider 'mock'"),
        "missing provider attribution: {message}"
    );
    assert!(
        message.contains("model 'error-model'"),
        "missing model attribution: {message}"
    );
    assert!(
        message.contains("simulated failure"),
        "missing original error: {message}"
    );
}

#[test]
fn stop_reason_is_normal_covers_happy_path() {
    for normal in [
        coco_llm_types::StopReason::EndTurn,
        coco_llm_types::StopReason::StopSequence,
        coco_llm_types::StopReason::ToolUse,
    ] {
        assert!(normal.is_normal(), "{normal:?} should be normal");
        assert!(!normal.is_abnormal());
    }
}

#[test]
fn stop_reason_flags_truncation_and_filter() {
    for abnormal in [
        coco_llm_types::StopReason::MaxTokens,
        coco_llm_types::StopReason::ContextWindowExceeded,
        coco_llm_types::StopReason::ContentFilter,
        coco_llm_types::StopReason::Error,
        coco_llm_types::StopReason::Other,
    ] {
        assert!(abnormal.is_abnormal(), "{abnormal:?} should be abnormal");
        assert!(!abnormal.is_normal());
    }
}
