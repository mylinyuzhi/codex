//! High-level API client wrapper with retry support.

use crate::LanguageModel;
use crate::LanguageModelCallOptions;
use crate::LanguageModelGenerateResult;
use crate::LanguageModelStreamPart;
use crate::error::ApiError;
use crate::error::Result;
use crate::provider_factory;
use crate::retry::RetryConfig;
use crate::retry::RetryContext;
use crate::retry::RetryDecision;
use crate::unified_stream::UnifiedStream;
use cocode_protocol::ProviderInfo;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClientConfig {
    /// Retry configuration.
    #[serde(default)]
    pub retry: RetryConfig,
    /// Stall detection timeout.
    #[serde(default = "default_stall_timeout", with = "humantime_serde")]
    pub stall_timeout: Duration,
    /// Enable stall detection.
    #[serde(default = "default_true")]
    pub stall_detection_enabled: bool,
    /// Fallback configuration for stream errors and context overflow.
    #[serde(default)]
    pub fallback: FallbackConfig,
}

fn default_stall_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_true() -> bool {
    true
}
fn default_fallback_max_tokens() -> Option<u64> {
    Some(21333)
}
fn default_min_output_tokens() -> u64 {
    3000
}
fn default_max_overflow_attempts() -> u32 {
    3
}

/// Configuration for fallback behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Enable automatic fallback from streaming to non-streaming on stream errors.
    #[serde(default = "default_true")]
    pub enable_stream_fallback: bool,

    /// Maximum tokens for fallback requests.
    #[serde(default = "default_fallback_max_tokens")]
    pub fallback_max_tokens: Option<u64>,

    /// Enable context overflow recovery (auto-reduce max_tokens).
    #[serde(default = "default_true")]
    pub enable_overflow_recovery: bool,

    /// Minimum output tokens to preserve during overflow recovery.
    #[serde(default = "default_min_output_tokens")]
    pub min_output_tokens: u64,

    /// Maximum overflow recovery attempts.
    #[serde(default = "default_max_overflow_attempts")]
    pub max_overflow_attempts: u32,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enable_stream_fallback: true,
            fallback_max_tokens: default_fallback_max_tokens(),
            enable_overflow_recovery: true,
            min_output_tokens: default_min_output_tokens(),
            max_overflow_attempts: default_max_overflow_attempts(),
        }
    }
}

impl FallbackConfig {
    /// Disable all fallback mechanisms.
    pub fn disabled() -> Self {
        Self {
            enable_stream_fallback: false,
            fallback_max_tokens: None,
            enable_overflow_recovery: false,
            min_output_tokens: default_min_output_tokens(),
            max_overflow_attempts: 0,
        }
    }

    pub fn with_stream_fallback(mut self, enabled: bool) -> Self {
        self.enable_stream_fallback = enabled;
        self
    }

    pub fn with_fallback_max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.fallback_max_tokens = max_tokens;
        self
    }

    pub fn with_overflow_recovery(mut self, enabled: bool) -> Self {
        self.enable_overflow_recovery = enabled;
        self
    }

    pub fn with_min_output_tokens(mut self, min_tokens: u64) -> Self {
        self.min_output_tokens = min_tokens;
        self
    }

    pub fn with_max_overflow_attempts(mut self, max_attempts: u32) -> Self {
        self.max_overflow_attempts = max_attempts;
        self
    }
}

impl Default for ApiClientConfig {
    fn default() -> Self {
        Self {
            retry: RetryConfig::default(),
            stall_timeout: default_stall_timeout(),
            stall_detection_enabled: default_true(),
            fallback: FallbackConfig::default(),
        }
    }
}

impl ApiClientConfig {
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
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

    pub fn with_fallback(mut self, fallback: FallbackConfig) -> Self {
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
    pub async fn stream_request(
        &self,
        model: &dyn LanguageModel,
        request: LanguageModelCallOptions,
        options: StreamOptions,
    ) -> Result<UnifiedStream> {
        let mut retry_ctx = RetryContext::new(self.config.retry.clone());
        let mut current_request = request;
        let mut use_streaming = options.streaming;
        let mut overflow_attempts: u32 = 0;

        loop {
            debug!(
                model = %model.model_id(),
                attempt = retry_ctx.current_attempt(),
                streaming = use_streaming,
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
                        && let Some(new_max) = self.try_overflow_recovery(&current_request)
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
                            current_request.max_output_tokens = Some(max);
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
    fn try_overflow_recovery(&self, request: &LanguageModelCallOptions) -> Option<u64> {
        let current_max = request.max_output_tokens.unwrap_or(8192);
        let min_tokens = self.config.fallback.min_output_tokens;
        let new_max = current_max * 3 / 4;

        if new_max >= min_tokens {
            Some(new_max)
        } else {
            None
        }
    }

    /// Make a non-streaming request with retry support.
    pub async fn generate(
        &self,
        model: &dyn LanguageModel,
        request: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult> {
        let mut retry_ctx = RetryContext::new(self.config.retry.clone());

        loop {
            debug!(
                model = %model.model_id(),
                attempt = retry_ctx.current_attempt(),
                "Making non-streaming API request"
            );

            let result = model
                .do_generate(request.clone())
                .await
                .map_err(ApiError::from);

            match result {
                Ok(response) => return Ok(response),
                Err(api_error) => {
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

/// Builder for creating an API client.
pub struct ApiClientBuilder {
    config: ApiClientConfig,
}

impl ApiClientBuilder {
    pub fn new() -> Self {
        Self {
            config: ApiClientConfig::default(),
        }
    }

    pub fn retry(mut self, retry: RetryConfig) -> Self {
        self.config.retry = retry;
        self
    }

    pub fn stall_timeout(mut self, timeout: Duration) -> Self {
        self.config.stall_timeout = timeout;
        self
    }

    pub fn stall_detection(mut self, enabled: bool) -> Self {
        self.config.stall_detection_enabled = enabled;
        self
    }

    pub fn fallback(mut self, fallback: FallbackConfig) -> Self {
        self.config.fallback = fallback;
        self
    }

    pub fn build(self) -> ApiClient {
        ApiClient::with_config(self.config)
    }
}

impl Default for ApiClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
