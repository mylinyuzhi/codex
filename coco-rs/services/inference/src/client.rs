use crate::build_call_options::PerCallOverrides;
use crate::build_call_options::build_call_options;
use crate::errors::InferenceError;
use crate::fingerprint::ProviderClientFingerprint;
use crate::retry::RetryConfig;
use crate::usage::UsageAccumulator;
use coco_config::ModelInfo;
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

/// Parameters for a single query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParams {
    /// Messages to send (as LlmPrompt).
    pub prompt: LanguageModelV4Prompt,
    /// Maximum output tokens. Use [`coco_config::PositiveTokens`] when
    /// validation is required at the JSON boundary; this field stays
    /// `Option<i64>` because callers (TUI, CLI overrides) supply it
    /// raw. Conversion to `u64` happens here without an `as` cast —
    /// negative values are clamped to `None`.
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
    pub content: Vec<AssistantContentPart>,
    pub usage: TokenUsage,
    pub model: String,
    pub stop_reason: Option<String>,
    pub request_id: Option<String>,
    pub retries: i32,
    pub total_duration_ms: i64,
}

/// LLM API client wrapping vercel-ai LanguageModelV4.
///
/// Carries a [`ProviderClientFingerprint`] so a turn-boundary
/// coherence check can detect a stale `Arc<dyn LanguageModelV4>`
/// after `RuntimeConfig` hot-reload and rebuild without restarting
/// the process.
///
/// **Layer-2 plumbing.** When `model_info` is `Some`,
/// [`Self::query`] / [`Self::query_stream`] route through
/// [`build_call_options`] — this is the path that wraps
/// `info.extra_body` under `provider_options[<namespace>]`, applies
/// `info.default_thinking()` / `temperature` / `top_p` / `top_k`, and
/// per-call thinking overrides. When `model_info` is `None` (test /
/// mock constructor) the legacy direct construction is used.
pub struct ApiClient {
    /// The underlying model (real or mock — ApiClient doesn't care).
    model: Arc<dyn LanguageModelV4>,
    /// Identity of the underlying client. Updated when
    /// `with_fingerprint` is called; matched against
    /// [`ProviderClientFingerprint::compute`] at turn start.
    fingerprint: ProviderClientFingerprint,
    /// Resolved `ModelInfo` for the (provider, model) pair. Drives
    /// Layer-2 typed sampling + `extra_body` namespace wrap. `None`
    /// for test/mock paths that bypass the runtime registry.
    model_info: Option<ModelInfo>,
    pub retry: RetryConfig,
    pub usage: Arc<Mutex<UsageAccumulator>>,
}

impl ApiClient {
    /// Production constructor. The `fingerprint` should be computed
    /// from the resolved `ProviderConfig` via
    /// [`ProviderClientFingerprint::compute`] so the turn-boundary
    /// coherence check can detect a stale `Arc<dyn LanguageModelV4>`
    /// after hot-reload.
    ///
    /// `model_info` carries the resolved [`ModelInfo`] for the
    /// (provider, model_id) pair so [`Self::query`] / [`Self::query_stream`]
    /// route through [`build_call_options`] — without this, the
    /// `extra_body` / typed-sampling / thinking machinery is inert.
    pub fn new(
        model: Arc<dyn LanguageModelV4>,
        fingerprint: ProviderClientFingerprint,
        model_info: Option<ModelInfo>,
        retry: RetryConfig,
    ) -> Self {
        Self {
            model,
            fingerprint,
            model_info,
            retry,
            usage: Arc::new(Mutex::new(UsageAccumulator::new())),
        }
    }

    /// Test / mock constructor. Builds a placeholder fingerprint with
    /// empty digests — adequate for mock-backed tests but **not for
    /// production hot-reload coherence**: the all-zero digests will
    /// match any rebuild and skip the swap. `model_info` is `None`,
    /// so Layer-2 `build_call_options` is skipped (no `extra_body`,
    /// no thinking translation, no typed sampling).
    pub fn with_default_fingerprint(model: Arc<dyn LanguageModelV4>, retry: RetryConfig) -> Self {
        let fingerprint = ProviderClientFingerprint {
            provider: model.provider().to_string(),
            // Mock implements the OpenAI-compat wire shape; the field
            // is inert when digests are zero.
            api: coco_types::ProviderApi::OpenaiCompat,
            api_model_name: model.model_id().to_string(),
            base_url: String::new(),
            wire_api: None,
            client_options_digest: [0u8; 32],
            timeout_secs: 0,
            api_key_origin_digest: [0u8; 32],
        };
        Self::new(model, fingerprint, /*model_info*/ None, retry)
    }

    /// Identity of the underlying client.
    pub fn fingerprint(&self) -> &ProviderClientFingerprint {
        &self.fingerprint
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
    ///
    /// **Layer-2 caveat.** This path constructs
    /// `LanguageModelV4CallOptions` directly from `params` — it does
    /// **not** route through [`crate::build_call_options`], so
    /// `ModelInfo.extra_body` and per-call thinking translation are
    /// not applied. Callers that need Layer-2 namespace wrapping
    /// (`provider_options[<provider_name>]`) should construct
    /// `LanguageModelV4CallOptions` themselves via
    /// `build_call_options(...)` and call the underlying
    /// `LanguageModelV4` directly. `ApiClient::query` is the
    /// retry/usage-accumulating shim for already-built call options.
    pub async fn query(&self, params: &QueryParams) -> Result<QueryResult, InferenceError> {
        let start = std::time::Instant::now();
        let mut attempt = 0;

        loop {
            match self.do_query(params).await {
                Ok(mut result) => {
                    result.retries = attempt;
                    result.total_duration_ms =
                        i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX);

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
                        delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX),
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
        let options = self.build_options(params);

        let result =
            self.model
                .do_generate(options)
                .await
                .map_err(|e| InferenceError::ProviderError {
                    status: 0,
                    message: e.to_string(),
                })?;

        let usage = TokenUsage {
            input_tokens: result
                .usage
                .input_tokens
                .total
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0),
            output_tokens: result
                .usage
                .output_tokens
                .total
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0),
            cache_read_input_tokens: result
                .usage
                .input_tokens
                .cache_read
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0),
            cache_creation_input_tokens: result
                .usage
                .input_tokens
                .cache_write
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0),
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
    pub async fn query_stream(
        &self,
        params: &QueryParams,
    ) -> Result<tokio::sync::mpsc::Receiver<crate::stream::StreamEvent>, InferenceError> {
        let options = self.build_options(params);

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

    /// Build [`LanguageModelV4CallOptions`] for a query. When
    /// `model_info` is set (production path) routes through
    /// [`build_call_options`] so `extra_body` is wrapped under
    /// `provider_options[<namespace>]`, `info.default_thinking()`
    /// applies, and per-call `thinking_level` / `max_tokens`
    /// overrides are honored. Without `model_info` (mock / test path)
    /// falls back to the prompt + max_output_tokens + tools shape.
    fn build_options(&self, params: &QueryParams) -> LanguageModelV4CallOptions {
        let Some(info) = self.model_info.as_ref() else {
            // Legacy mock path — direct construction.
            let mut options = LanguageModelV4CallOptions {
                prompt: params.prompt.clone(),
                max_output_tokens: max_tokens_to_u64(params.max_tokens),
                ..Default::default()
            };
            if let Some(ref tools) = params.tools {
                options.tools = Some(tools.clone());
            }
            return options;
        };

        let max_output_tokens = params.max_tokens.and_then(|v| {
            // Drop invalid values (negative, zero, or > u32::MAX). A silent
            // drop leaves the user wondering why their explicit
            // `max_tokens` had no effect; warn once at the boundary.
            match u32::try_from(v) {
                Ok(u) if u > 0 => Some(coco_config::PositiveTokens::new(u)),
                _ => {
                    warn!(
                        max_tokens = v,
                        "QueryParams.max_tokens is non-positive or > u32::MAX; falling back to model default"
                    );
                    None
                }
            }
        });
        let per_call = PerCallOverrides {
            thinking_level: params.thinking_level.clone(),
            max_output_tokens,
            ..Default::default()
        };
        build_call_options(
            info,
            self.fingerprint.api,
            &self.fingerprint.provider,
            &per_call,
            params.prompt.clone(),
            params.tools.clone(),
        )
    }
}

/// Convert a caller-supplied `Option<i64>` `max_tokens` to the wire
/// `Option<u64>`. Negative / zero is dropped (== "let provider
/// default") with a single WARN trace so a user debugging an unhonored
/// override can see what happened. No `as u64` cast — checked.
fn max_tokens_to_u64(value: Option<i64>) -> Option<u64> {
    let v = value?;
    match u64::try_from(v) {
        Ok(u) if u > 0 => Some(u),
        _ => {
            warn!(
                max_tokens = v,
                "QueryParams.max_tokens is non-positive; falling back to model default"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
