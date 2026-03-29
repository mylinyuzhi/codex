//! High-level API client wrapper with retry support.

use crate::LanguageModel;
use crate::LanguageModelCallOptions;
use crate::LanguageModelGenerateResult;
use crate::LanguageModelStreamPart;
use crate::error::ApiError;
use crate::error::Result;
use crate::provider_factory;
use crate::retry::RetryContext;
use crate::retry::RetryDecision;
use crate::unified_stream::UnifiedStream;
use cocode_protocol::ProviderApi;
use cocode_protocol::ProviderInfo;

pub use cocode_protocol::ApiFallbackConfig;
pub use cocode_protocol::ApiRetryConfig;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use vercel_ai::stream::StreamProcessor;

/// Options for a streaming request.
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    /// Enable streaming (default: true).
    pub streaming: bool,
    /// Event sender for UI updates.
    pub event_tx: Option<mpsc::Sender<LanguageModelStreamPart>>,
}

impl StreamOptions {
    /// Create options for streaming.
    pub fn streaming() -> Self {
        Self {
            streaming: true,
            event_tx: None,
        }
    }

    /// Create options for non-streaming.
    pub fn non_streaming() -> Self {
        Self {
            streaming: false,
            event_tx: None,
        }
    }

    /// Set the event sender.
    pub fn with_event_tx(mut self, tx: mpsc::Sender<LanguageModelStreamPart>) -> Self {
        self.event_tx = Some(tx);
        self
    }
}

/// Configuration for the API client.
///
/// Uses `ApiRetryConfig` and `ApiFallbackConfig` from the protocol crate
/// as the canonical source of truth for retry and fallback settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiClientConfig {
    /// Retry configuration.
    #[serde(default)]
    pub retry: ApiRetryConfig,
    /// Stall detection timeout.
    #[serde(default = "default_stall_timeout", with = "humantime_serde")]
    pub stall_timeout: Duration,
    /// Enable stall detection.
    #[serde(default = "cocode_protocol::default_true")]
    pub stall_detection_enabled: bool,
    /// Fallback configuration for stream errors and context overflow.
    #[serde(default)]
    pub fallback: ApiFallbackConfig,
}

fn default_stall_timeout() -> Duration {
    Duration::from_secs(cocode_protocol::api_config::DEFAULT_STALL_TIMEOUT_SECS as u64)
}

impl Default for ApiClientConfig {
    fn default() -> Self {
        Self {
            retry: ApiRetryConfig::default(),
            stall_timeout: default_stall_timeout(),
            stall_detection_enabled: true,
            fallback: ApiFallbackConfig::default(),
        }
    }
}

impl ApiClientConfig {
    pub fn with_retry(mut self, retry: ApiRetryConfig) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_stall_timeout(mut self, timeout: Duration) -> Self {
        self.stall_timeout = timeout;
        self
    }

    pub fn with_stall_detection(mut self, enabled: bool) -> Self {
        self.stall_detection_enabled = enabled;
        self
    }

    pub fn with_fallback(mut self, fallback: ApiFallbackConfig) -> Self {
        self.fallback = fallback;
        self
    }
}

/// High-level API client with retry and caching.
///
/// Model-agnostic: each request receives the model as a parameter.
#[derive(Clone)]
pub struct ApiClient {
    config: ApiClientConfig,
}

impl ApiClient {
    pub fn new() -> Self {
        Self {
            config: ApiClientConfig::default(),
        }
    }

    pub fn with_config(config: ApiClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ApiClientConfig {
        &self.config
    }

    /// Create an ApiClient with a model from ProviderInfo.
    pub fn from_provider_info(
        info: &ProviderInfo,
        model_slug: &str,
        config: ApiClientConfig,
    ) -> Result<(Self, Arc<dyn LanguageModel>)> {
        let model = provider_factory::create_model(info, model_slug)?;
        Ok((Self::with_config(config), model))
    }

    /// Make a streaming request with retry support.
    #[tracing::instrument(skip_all, fields(model = %model.model_id(), streaming = options.streaming))]
    pub async fn stream_request(
        &self,
        model: &dyn LanguageModel,
        request: LanguageModelCallOptions,
        options: StreamOptions,
    ) -> Result<UnifiedStream> {
        let mut retry_ctx = RetryContext::new(self.config.retry.clone());
        let mut current_request = request;
        let mut use_streaming = options.streaming;
        let mut overflow_attempts: i32 = 0;

        loop {
            debug!(
                model = %model.model_id(),
                attempt = retry_ctx.current_attempt(),
                streaming = use_streaming,
                max_tokens = ?current_request.max_output_tokens,
                "Making API request"
            );

            let result = if use_streaming {
                self.do_streaming_request(model, &current_request).await
            } else {
                self.do_non_streaming_request(model, &current_request).await
            };

            match result {
                Ok(stream) => {
                    let stream = if let Some(tx) = options.event_tx.clone() {
                        stream.with_event_sender(tx)
                    } else {
                        stream
                    };
                    return Ok(stream);
                }
                Err(api_error) => {
                    // 1. Context overflow recovery
                    if api_error.is_context_overflow()
                        && self.config.fallback.enable_overflow_recovery
                        && overflow_attempts < self.config.fallback.max_overflow_attempts
                        && let Some(new_max) =
                            self.try_overflow_recovery(&current_request, &api_error)
                    {
                        info!(
                            old = ?current_request.max_output_tokens,
                            new = new_max,
                            attempt = overflow_attempts + 1,
                            max_attempts = self.config.fallback.max_overflow_attempts,
                            "Recovering from context overflow by reducing max_tokens"
                        );
                        current_request.max_output_tokens = Some(new_max);
                        overflow_attempts += 1;
                        continue;
                    }

                    // 2. Stream fallback
                    if use_streaming
                        && self.config.fallback.enable_stream_fallback
                        && api_error.is_stream_error()
                    {
                        info!(
                            error = %api_error,
                            "Falling back to non-streaming due to stream error"
                        );
                        use_streaming = false;
                        if let Some(max) = self.config.fallback.fallback_max_tokens {
                            current_request.max_output_tokens = Some(max as u64);
                        }
                        continue;
                    }

                    // 3. Standard retry
                    let decision = retry_ctx.decide(&api_error);

                    match decision {
                        RetryDecision::Retry { delay } => {
                            info!(
                                attempt = retry_ctx.current_attempt(),
                                max = retry_ctx.max_retries(),
                                delay_ms = delay.as_millis() as i64,
                                error = %api_error,
                                "Retrying after error"
                            );
                            tokio::time::sleep(delay).await;
                        }
                        RetryDecision::GiveUp => {
                            if retry_ctx.current_attempt() > 0 {
                                tracing::warn!(
                                    diagnostics = ?retry_ctx.diagnostics(),
                                    "All retries exhausted"
                                );
                            }
                            return Err(api_error);
                        }
                    }
                }
            }
        }
    }

    /// Attempt to recover from context overflow by reducing max_tokens.
    ///
    /// Two strategies (matching Claude Code's `withApiRetry`):
    /// 1. **Smart recovery**: Parse the error message for `inputTokens` and
    ///    `contextLimit`, then calculate the exact available output space.
    /// 2. **Blind fallback**: Reduce `max_tokens` by 25% when parsing fails.
    ///
    /// The thinking budget is accounted for so the model can still reason.
    fn try_overflow_recovery(
        &self,
        request: &LanguageModelCallOptions,
        error: &ApiError,
    ) -> Option<u64> {
        let fallback = &self.config.fallback;
        let floor = fallback.floor_output_tokens;

        let new_max = if let Some(info) = error.overflow_info() {
            // Smart recovery: calculate exact available space
            let input = info.input_tokens.unwrap_or(0);
            let limit = info.context_limit.unwrap_or(0);
            let available = limit - input - fallback.buffer_tokens;
            available.max(floor)
        } else {
            // Blind fallback: reduce by 25%
            let current_max = request.max_output_tokens.unwrap_or(8192) as i64;
            (current_max * 3 / 4).max(floor)
        };

        // Account for thinking budget: ensure output space can fit it
        let thinking_budget = extract_thinking_budget(request);
        let new_max = if let Some(budget) = thinking_budget {
            new_max.max(budget + 1)
        } else {
            new_max
        };

        if new_max >= fallback.min_output_tokens {
            Some(new_max as u64)
        } else {
            None
        }
    }

    /// Make a non-streaming request with retry and overflow recovery.
    ///
    /// Applies the same context overflow recovery as `stream_request()`:
    /// if the provider reports `context_length_exceeded`, `max_tokens` is
    /// reduced and the request is retried.
    pub async fn generate(
        &self,
        model: &dyn LanguageModel,
        request: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult> {
        let mut retry_ctx = RetryContext::new(self.config.retry.clone());
        let mut current_request = request;
        let mut overflow_attempts: i32 = 0;

        loop {
            debug!(
                model = %model.model_id(),
                attempt = retry_ctx.current_attempt(),
                "Making non-streaming API request"
            );

            let result = model
                .do_generate(current_request.clone())
                .await
                .map_err(ApiError::from);

            match result {
                Ok(response) => return Ok(response),
                Err(api_error) => {
                    // 1. Context overflow recovery (same as stream_request)
                    if api_error.is_context_overflow()
                        && self.config.fallback.enable_overflow_recovery
                        && overflow_attempts < self.config.fallback.max_overflow_attempts
                        && let Some(new_max) =
                            self.try_overflow_recovery(&current_request, &api_error)
                    {
                        info!(
                            old = ?current_request.max_output_tokens,
                            new = new_max,
                            attempt = overflow_attempts + 1,
                            max_attempts = self.config.fallback.max_overflow_attempts,
                            "Recovering from context overflow in generate()"
                        );
                        current_request.max_output_tokens = Some(new_max);
                        overflow_attempts += 1;
                        continue;
                    }

                    // 2. Standard retry
                    let decision = retry_ctx.decide(&api_error);

                    match decision {
                        RetryDecision::Retry { delay } => {
                            info!(
                                attempt = retry_ctx.current_attempt(),
                                max = retry_ctx.max_retries(),
                                delay_ms = delay.as_millis() as i64,
                                error = %api_error,
                                "Retrying after error"
                            );
                            tokio::time::sleep(delay).await;
                        }
                        RetryDecision::GiveUp => {
                            if retry_ctx.current_attempt() > 0 {
                                tracing::warn!(
                                    diagnostics = ?retry_ctx.diagnostics(),
                                    "All retries exhausted"
                                );
                            }
                            return Err(api_error);
                        }
                    }
                }
            }
        }
    }

    /// Internal: make a streaming request.
    async fn do_streaming_request(
        &self,
        model: &dyn LanguageModel,
        request: &LanguageModelCallOptions,
    ) -> Result<UnifiedStream> {
        let stream_result = model
            .do_stream(request.clone())
            .await
            .map_err(ApiError::from)?;

        let mut processor = StreamProcessor::new(stream_result);

        if self.config.stall_detection_enabled {
            processor = processor.idle_timeout(self.config.stall_timeout);
        }

        Ok(UnifiedStream::from_stream(processor))
    }

    /// Make a streaming request with provider-level failover.
    pub async fn stream_request_with_fallback(
        &self,
        models: &[&dyn LanguageModel],
        request: LanguageModelCallOptions,
        options: StreamOptions,
    ) -> Result<UnifiedStream> {
        if models.is_empty() {
            return Err(crate::error::api_error::InvalidRequestSnafu {
                message: "no models provided for fallback".to_string(),
            }
            .build());
        }

        if models.len() == 1 {
            return self.stream_request(models[0], request, options).await;
        }

        let mut all_failures: Vec<String> = Vec::new();
        let mut last_error: Option<ApiError> = None;

        for (i, model) in models.iter().enumerate() {
            info!(
                model = %model.model_id(),
                provider = %model.provider(),
                index = i,
                total = models.len(),
                "Trying model for failover"
            );

            match self
                .stream_request(*model, request.clone(), options.clone())
                .await
            {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    all_failures.push(format!(
                        "[{}:{}] {}",
                        model.provider(),
                        model.model_id(),
                        err
                    ));

                    info!(
                        model = %model.model_id(),
                        provider = %model.provider(),
                        error = %err,
                        remaining = models.len() - i - 1,
                        "Model failed, trying next fallback"
                    );

                    last_error = Some(err);
                }
            }
        }

        tracing::warn!(
            diagnostics = ?all_failures,
            "All fallback models exhausted"
        );
        Err(crate::error::api_error::RetriesExhaustedSnafu {
            attempts: models.len() as i32,
            message: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "all models failed".to_string()),
            diagnostics: all_failures,
        }
        .build())
    }

    /// Internal: make a non-streaming request.
    async fn do_non_streaming_request(
        &self,
        model: &dyn LanguageModel,
        request: &LanguageModelCallOptions,
    ) -> Result<UnifiedStream> {
        let response = model
            .do_generate(request.clone())
            .await
            .map_err(ApiError::from)?;

        Ok(UnifiedStream::from_response(response))
    }
}

/// Extract the thinking budget from provider options, if present.
///
/// Inspects the Anthropic, Volcengine, and Z.AI provider option paths
/// for `thinking.budgetTokens` or `thinking.budget_tokens`.
fn extract_thinking_budget(request: &LanguageModelCallOptions) -> Option<i64> {
    let opts = request.provider_options.as_ref()?;

    // Providers that support thinking.budgetTokens
    const THINKING_BUDGET_PROVIDERS: &[ProviderApi] = &[
        ProviderApi::Anthropic,
        ProviderApi::Volcengine,
        ProviderApi::Zai,
    ];

    for provider_type in THINKING_BUDGET_PROVIDERS {
        let key = crate::request_options_merge::provider_name_for_type(*provider_type);
        if let Some(thinking) = opts
            .get(key)
            .and_then(|provider_opts| provider_opts.get("thinking"))
        {
            // Try "budgetTokens" (camelCase, Anthropic/Volcengine wire format)
            if let Some(n) = thinking
                .get("budgetTokens")
                .or_else(|| thinking.get("budget_tokens"))
                .and_then(serde_json::Value::as_i64)
            {
                return Some(n);
            }
        }
    }
    None
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
