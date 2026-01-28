//! High-level API client wrapper with retry and fallback support.
//!
//! This module provides [`ApiClient`] which wraps a hyper-sdk [`Model`]
//! with additional features needed for the agent loop:
//! - Retry with exponential backoff
//! - Model fallback on overload
//! - Stall detection
//! - Prompt caching support

use crate::cache::PromptCacheConfig;
use crate::error::{ApiError, Result};
use crate::retry::{RetryConfig, RetryContext, RetryDecision};
use crate::unified_stream::UnifiedStream;
use hyper_sdk::{GenerateRequest, GenerateResponse, Model};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Options for a streaming request.
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    /// Enable streaming (default: true).
    pub streaming: bool,
    /// Event sender for UI updates.
    pub event_tx: Option<mpsc::Sender<hyper_sdk::StreamUpdate>>,
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
    pub fn with_event_tx(mut self, tx: mpsc::Sender<hyper_sdk::StreamUpdate>) -> Self {
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
    /// Prompt caching configuration.
    #[serde(default)]
    pub cache: PromptCacheConfig,
    /// Stall detection timeout.
    #[serde(default = "default_stall_timeout", with = "humantime_serde")]
    pub stall_timeout: Duration,
    /// Enable stall detection.
    #[serde(default = "default_stall_enabled")]
    pub stall_detection_enabled: bool,
}

fn default_stall_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_stall_enabled() -> bool {
    true
}

impl Default for ApiClientConfig {
    fn default() -> Self {
        Self {
            retry: RetryConfig::default(),
            cache: PromptCacheConfig::default(),
            stall_timeout: default_stall_timeout(),
            stall_detection_enabled: default_stall_enabled(),
        }
    }
}

impl ApiClientConfig {
    /// Set the retry configuration.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    /// Set the cache configuration.
    pub fn with_cache(mut self, cache: PromptCacheConfig) -> Self {
        self.cache = cache;
        self
    }

    /// Set the stall timeout.
    pub fn with_stall_timeout(mut self, timeout: Duration) -> Self {
        self.stall_timeout = timeout;
        self
    }

    /// Enable or disable stall detection.
    pub fn with_stall_detection(mut self, enabled: bool) -> Self {
        self.stall_detection_enabled = enabled;
        self
    }
}

/// High-level API client with retry, fallback, and caching.
///
/// This wraps a hyper-sdk [`Model`] with additional features
/// needed for the agent loop.
///
/// # Example
///
/// ```ignore
/// use cocode_api::{ApiClient, ApiClientConfig, StreamOptions};
/// use hyper_sdk::{OpenAIProvider, Provider, GenerateRequest, Message};
///
/// let provider = OpenAIProvider::from_env()?;
/// let model = provider.model("gpt-4o")?;
///
/// let client = ApiClient::new(model);
/// let request = GenerateRequest::new(vec![
///     Message::user("Hello!"),
/// ]);
///
/// let stream = client.stream_request(request, StreamOptions::streaming()).await?;
/// ```
pub struct ApiClient {
    model: Arc<dyn Model>,
    fallback_model: Option<Arc<dyn Model>>,
    config: ApiClientConfig,
}

impl ApiClient {
    /// Create a new API client from a hyper-sdk model.
    pub fn new(model: Arc<dyn Model>) -> Self {
        Self {
            model,
            fallback_model: None,
            config: ApiClientConfig::default(),
        }
    }

    /// Create a new API client with custom configuration.
    pub fn with_config(model: Arc<dyn Model>, config: ApiClientConfig) -> Self {
        Self {
            model,
            fallback_model: None,
            config,
        }
    }

    /// Set a fallback model for overload situations.
    pub fn with_fallback(mut self, fallback: Arc<dyn Model>) -> Self {
        self.fallback_model = Some(fallback);
        self
    }

    /// Get a reference to the primary model.
    pub fn model(&self) -> &Arc<dyn Model> {
        &self.model
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ApiClientConfig {
        &self.config
    }

    /// Make a streaming request with retry and fallback support.
    ///
    /// Returns a [`UnifiedStream`] that can be used to consume the response.
    pub async fn stream_request(
        &self,
        request: GenerateRequest,
        options: StreamOptions,
    ) -> Result<UnifiedStream> {
        let mut retry_ctx = RetryContext::new(self.config.retry.clone());
        let mut use_fallback = false;

        loop {
            let current_model = if use_fallback {
                self.fallback_model.as_ref().unwrap_or(&self.model)
            } else {
                &self.model
            };

            debug!(
                model = %current_model.model_id(),
                attempt = retry_ctx.current_attempt(),
                "Making API request"
            );

            let result = if options.streaming {
                self.do_streaming_request(current_model, &request).await
            } else {
                self.do_non_streaming_request(current_model, &request).await
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
                        RetryDecision::Fallback => {
                            if self.fallback_model.is_some() && !use_fallback {
                                warn!(
                                    from = %self.model.model_id(),
                                    to = %self.fallback_model.as_ref().unwrap().model_id(),
                                    "Falling back to alternative model"
                                );
                                use_fallback = true;
                                retry_ctx.reset();
                            } else {
                                return Err(api_error);
                            }
                        }
                        RetryDecision::GiveUp => {
                            return Err(api_error);
                        }
                    }
                }
            }
        }
    }

    /// Make a non-streaming request with retry support.
    pub async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let stream = self
            .stream_request(request, StreamOptions::non_streaming())
            .await?;
        let collected = stream.collect().await?;

        Ok(GenerateResponse {
            id: String::new(),
            content: collected.content,
            finish_reason: collected.finish_reason,
            usage: collected.usage.map(|u| hyper_sdk::TokenUsage {
                prompt_tokens: i64::from(u.input_tokens),
                completion_tokens: i64::from(u.output_tokens),
                total_tokens: i64::from(u.input_tokens) + i64::from(u.output_tokens),
                cache_read_tokens: u.cache_read_tokens.map(i64::from),
                cache_creation_tokens: u.cache_creation_tokens.map(i64::from),
                reasoning_tokens: None,
            }),
            model: self.model.model_id().to_string(),
        })
    }

    /// Internal: make a streaming request.
    async fn do_streaming_request(
        &self,
        model: &Arc<dyn Model>,
        request: &GenerateRequest,
    ) -> Result<UnifiedStream> {
        let stream_response = model
            .stream(request.clone())
            .await
            .map_err(ApiError::from)?;

        let processor = stream_response.into_processor();

        // Apply stall timeout if configured
        let processor = if self.config.stall_detection_enabled {
            processor.idle_timeout(self.config.stall_timeout)
        } else {
            processor
        };

        Ok(UnifiedStream::from_stream(processor))
    }

    /// Internal: make a non-streaming request.
    async fn do_non_streaming_request(
        &self,
        model: &Arc<dyn Model>,
        request: &GenerateRequest,
    ) -> Result<UnifiedStream> {
        let response = model
            .generate(request.clone())
            .await
            .map_err(ApiError::from)?;

        Ok(UnifiedStream::from_response(response))
    }
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("model", &self.model.model_id())
            .field(
                "fallback",
                &self.fallback_model.as_ref().map(|m| m.model_id()),
            )
            .field("config", &self.config)
            .finish()
    }
}

/// Builder for creating an API client.
pub struct ApiClientBuilder {
    config: ApiClientConfig,
    fallback: Option<Arc<dyn Model>>,
}

impl ApiClientBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: ApiClientConfig::default(),
            fallback: None,
        }
    }

    /// Set the retry configuration.
    pub fn retry(mut self, retry: RetryConfig) -> Self {
        self.config.retry = retry;
        self
    }

    /// Set the cache configuration.
    pub fn cache(mut self, cache: PromptCacheConfig) -> Self {
        self.config.cache = cache;
        self
    }

    /// Set the fallback model.
    pub fn fallback_model(mut self, model: Arc<dyn Model>) -> Self {
        self.fallback = Some(model);
        self
    }

    /// Set the stall timeout.
    pub fn stall_timeout(mut self, timeout: Duration) -> Self {
        self.config.stall_timeout = timeout;
        self
    }

    /// Enable or disable stall detection.
    pub fn stall_detection(mut self, enabled: bool) -> Self {
        self.config.stall_detection_enabled = enabled;
        self
    }

    /// Build the API client with the given model.
    pub fn build(self, model: Arc<dyn Model>) -> ApiClient {
        let mut client = ApiClient::with_config(model, self.config);
        if let Some(fallback) = self.fallback {
            client.fallback_model = Some(fallback);
        }
        client
    }
}

impl Default for ApiClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_defaults() {
        let config = ApiClientConfig::default();
        assert!(config.cache.enabled);
        assert!(config.stall_detection_enabled);
        assert_eq!(config.stall_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_client_config_builder() {
        let config = ApiClientConfig::default()
            .with_stall_timeout(Duration::from_secs(60))
            .with_stall_detection(false);

        assert_eq!(config.stall_timeout, Duration::from_secs(60));
        assert!(!config.stall_detection_enabled);
    }

    #[test]
    fn test_stream_options() {
        let opts = StreamOptions::streaming();
        assert!(opts.streaming);

        let opts = StreamOptions::non_streaming();
        assert!(!opts.streaming);
    }

    #[test]
    fn test_builder() {
        let builder = ApiClientBuilder::new()
            .stall_timeout(Duration::from_secs(45))
            .stall_detection(false);

        assert_eq!(builder.config.stall_timeout, Duration::from_secs(45));
        assert!(!builder.config.stall_detection_enabled);
    }
}
