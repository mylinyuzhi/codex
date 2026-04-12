use coco_types::ThinkingLevel;
use coco_types::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use tracing::warn;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Prompt;

use crate::errors::InferenceError;
use crate::retry::RetryConfig;
use crate::usage::UsageAccumulator;

/// Parameters for a single query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParams {
    /// Messages to send (as LlmPrompt).
    pub prompt: LanguageModelV4Prompt,
    /// Maximum output tokens.
    #[serde(default)]
    pub max_tokens: Option<i64>,
    /// Thinking level for this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    /// Whether to use fast mode.
    #[serde(default)]
    pub fast_mode: bool,
    /// Tool definitions available for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<vercel_ai_provider::LanguageModelV4Tool>>,
}

/// Result of a query.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// The response content parts.
    pub content: Vec<AssistantContentPart>,
    /// Token usage for this call.
    pub usage: TokenUsage,
    /// Model that actually served the request.
    pub model: String,
    /// Stop reason.
    pub stop_reason: Option<String>,
    /// Request ID from provider.
    pub request_id: Option<String>,
    /// Number of retries attempted.
    pub retries: i32,
    /// Total duration including retries (milliseconds).
    pub total_duration_ms: i64,
}

/// LLM API client wrapping vercel-ai LanguageModelV4.
///
/// Takes any `Arc<dyn LanguageModelV4>` — real provider or mock.
/// Thread-safe. Accumulates usage across calls.
pub struct ApiClient {
    /// The underlying model (real or mock — ApiClient doesn't care).
    model: Arc<dyn LanguageModelV4>,
    /// Retry configuration.
    pub retry: RetryConfig,
    /// Accumulated usage.
    pub usage: Arc<Mutex<UsageAccumulator>>,
}

impl ApiClient {
    /// Create a new ApiClient wrapping any LanguageModelV4 implementation.
    pub fn new(model: Arc<dyn LanguageModelV4>, retry: RetryConfig) -> Self {
        Self {
            model,
            retry,
            usage: Arc::new(Mutex::new(UsageAccumulator::new())),
        }
    }

    /// The provider name.
    pub fn provider(&self) -> &str {
        self.model.provider()
    }

    /// The model ID.
    pub fn model_id(&self) -> &str {
        self.model.model_id()
    }

    /// Execute a query with retry logic.
    pub async fn query(&self, params: &QueryParams) -> Result<QueryResult, InferenceError> {
        let start = std::time::Instant::now();
        let mut attempt = 0;

        loop {
            match self.do_query(params).await {
                Ok(mut result) => {
                    result.retries = attempt;
                    result.total_duration_ms = start.elapsed().as_millis() as i64;

                    // Record usage
                    let mut usage = self.usage.lock().await;
                    usage.record(&result.model, result.usage);

                    return Ok(result);
                }
                Err(e) => {
                    if !self.retry.should_retry(attempt, &e) {
                        warn!(
                            error_class = e.error_class(),
                            attempt, "non-retryable error, giving up"
                        );
                        return Err(e);
                    }

                    let delay = self.retry.delay_for_attempt(attempt, &e);
                    info!(
                        error_class = e.error_class(),
                        attempt,
                        delay_ms = delay.as_millis() as i64,
                        "retrying after error"
                    );

                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Execute a single query attempt via LanguageModelV4::do_generate().
    async fn do_query(&self, params: &QueryParams) -> Result<QueryResult, InferenceError> {
        let mut options = LanguageModelV4CallOptions {
            prompt: params.prompt.clone(),
            max_output_tokens: params.max_tokens.map(|t| t as u64),
            ..Default::default()
        };
        if let Some(ref tools) = params.tools {
            options.tools = Some(tools.clone());
        }

        let result =
            self.model
                .do_generate(options)
                .await
                .map_err(|e| InferenceError::ProviderError {
                    status: 0,
                    message: e.to_string(),
                })?;

        // Convert vercel-ai Usage → coco TokenUsage
        let usage = TokenUsage {
            input_tokens: result.usage.input_tokens.total.unwrap_or(0) as i64,
            output_tokens: result.usage.output_tokens.total.unwrap_or(0) as i64,
            cache_read_input_tokens: result.usage.input_tokens.cache_read.unwrap_or(0) as i64,
            cache_creation_input_tokens: result.usage.input_tokens.cache_write.unwrap_or(0) as i64,
        };

        let model_id = result
            .response
            .as_ref()
            .and_then(|r| r.model_id.clone())
            .unwrap_or_else(|| self.model.model_id().to_string());

        let stop_reason = Some(result.finish_reason.unified.to_string());

        Ok(QueryResult {
            content: result.content,
            usage,
            model: model_id,
            stop_reason,
            request_id: None,
            retries: 0,
            total_duration_ms: 0,
        })
    }

    /// Execute a streaming query. Returns a channel receiver for stream events.
    ///
    /// Events are sent as they arrive from the model. The caller should
    /// consume events until the channel closes or a Finish/Error event is received.
    pub async fn query_stream(
        &self,
        params: &QueryParams,
    ) -> Result<tokio::sync::mpsc::Receiver<crate::stream::StreamEvent>, InferenceError> {
        let options = LanguageModelV4CallOptions {
            prompt: params.prompt.clone(),
            max_output_tokens: params.max_tokens.map(|t| t as u64),
            ..Default::default()
        };

        let result =
            self.model
                .do_stream(options)
                .await
                .map_err(|e| InferenceError::ProviderError {
                    status: 0,
                    message: e.to_string(),
                })?;

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(crate::stream::process_stream(result.stream, tx));

        Ok(rx)
    }

    /// Get accumulated usage across all calls.
    pub async fn accumulated_usage(&self) -> UsageAccumulator {
        self.usage.lock().await.clone()
    }
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
