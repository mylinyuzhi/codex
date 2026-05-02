use crate::build_call_options::PerCallOverrides;
use crate::build_call_options::build_call_options;
use crate::cache_detection::CacheBreakDetector;
use crate::cache_detection::CacheState;
use crate::cache_detection::PromptStateInput;
use crate::cache_detection::canonical_extra_body_hash;
use crate::cache_detection::canonical_extra_body_serialize;
use crate::cache_detection::djb2_hash;
use crate::errors::InferenceError;
use crate::fingerprint::ProviderClientFingerprint;
use crate::retry::RetryConfig;
use crate::usage::UsageAccumulator;
use coco_config::ModelInfo;
use coco_types::ThinkingLevel;
use coco_types::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use tracing::warn;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Message;
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
    /// Anthropic `context_management` payload (camelCase shape) attached
    /// to this request. Producers should call
    /// [`crate::ApiClient::supports_server_side_context_edits`] before
    /// populating; non-Anthropic providers ignore the field but the
    /// inference layer still preserves the value through
    /// `PerCallOverrides`. Built by
    /// `coco_compact::encode_anthropic_context_management`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_management: Option<serde_json::Value>,
    /// Source of this query for cache-break tracking. Common values:
    /// `"repl_main_thread"`, `"sdk"`, `"agent:<type>"`, `"compact"`.
    /// `None` disables cache-break detection for this call (matches the
    /// untracked-source behavior in TS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_source: Option<String>,
    /// Optional agent id for per-instance subagent tracking. When
    /// concurrent subagents of the same type would otherwise share a
    /// `query_source` key, the agent id keeps their detector state
    /// isolated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Wall-clock millis since the last assistant message. Used by the
    /// cache-break detector to attribute drops to TTL expiry (5min /
    /// 1h) when no client-side change is to blame. `None` skips TTL
    /// attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_since_last_assistant_ms: Option<i64>,
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
    /// Optional prompt cache-break detector. When present,
    /// [`Self::query`] / [`Self::query_stream`] record the pre-call
    /// prompt state and check the post-call response usage for
    /// significant cache_read drops. Suppression APIs
    /// (`notify_compaction` / `notify_cache_deletion` /
    /// `cleanup_agent` / `cache_break_reset`) are exposed as
    /// passthroughs so the call sites that mutate the conversation
    /// (compact paths, subagent finalize, `/clear caches`) can
    /// declare expected drops.
    cache_break_detector: Option<Arc<Mutex<CacheBreakDetector>>>,
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
            cache_break_detector: None,
        }
    }

    /// Install a shared `CacheBreakDetector`. When present, every
    /// `query` invocation snapshots the pre-call prompt state and
    /// checks the post-call response for cache breaks. Multiple
    /// `ApiClient` instances on the same conversation can share one
    /// detector to keep tracking continuous across fallback model
    /// swaps.
    #[must_use]
    pub fn with_cache_break_detector(mut self, detector: Arc<Mutex<CacheBreakDetector>>) -> Self {
        self.cache_break_detector = Some(detector);
        self
    }

    /// Whether this client has a cache-break detector installed.
    #[must_use]
    pub fn has_cache_break_detector(&self) -> bool {
        self.cache_break_detector.is_some()
    }

    /// Notify the detector that compaction occurred — resets the
    /// previous-cache-read baseline so the next call doesn't trigger
    /// a false-positive cache break.
    pub async fn notify_compaction(&self, query_source: &str, agent_id: Option<&str>) {
        if let Some(d) = &self.cache_break_detector {
            d.lock().await.notify_compaction(query_source, agent_id);
        }
    }

    /// Notify the detector that a cache_edits deletion was issued —
    /// the next response's cache_read drop is expected, not a break.
    pub async fn notify_cache_deletion(&self, query_source: &str, agent_id: Option<&str>) {
        if let Some(d) = &self.cache_break_detector {
            d.lock().await.notify_cache_deletion(query_source, agent_id);
        }
    }

    /// Drop the detector's tracking entry for an agent that has
    /// finished. Called by the AgentTool finalize path so a long-lived
    /// session doesn't accumulate stale subagent state.
    pub async fn cache_break_cleanup_agent(&self, agent_id: &str) {
        if let Some(d) = &self.cache_break_detector {
            d.lock().await.cleanup_agent(agent_id);
        }
    }

    /// Reset all cache-break detector state. Wired to `/clear caches`.
    pub async fn cache_break_reset(&self) {
        if let Some(d) = &self.cache_break_detector {
            d.lock().await.reset();
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

    /// Whether this provider applies `context_management` server-side
    /// (preserves prompt cache while clearing old tool results /
    /// thinking blocks). Today only Anthropic — other providers ignore
    /// the field, so callers should fall back to client-side
    /// `coco_compact::micro_compact` instead.
    ///
    /// Used by `coco-query` to gate whether to encode and attach
    /// `coco_compact::ContextEditStrategy` to outgoing requests.
    #[must_use]
    pub fn supports_server_side_context_edits(&self) -> bool {
        matches!(self.fingerprint.api, coco_types::ProviderApi::Anthropic)
    }

    /// Provider-options namespace key for this client (e.g. `"anthropic"`,
    /// `"openai"`). Equivalent to the lookup `build_call_options` does
    /// internally; exposed so callers building their own
    /// `LanguageModelV4CallOptions` can place provider-specific blobs
    /// under the right key.
    #[must_use]
    pub fn provider_options_namespace(&self) -> &'static str {
        match self.fingerprint.api {
            coco_types::ProviderApi::Anthropic => "anthropic",
            coco_types::ProviderApi::Openai => "openai",
            coco_types::ProviderApi::Gemini => "google",
            coco_types::ProviderApi::Volcengine => "volcengine",
            coco_types::ProviderApi::Zai => "zai",
            coco_types::ProviderApi::OpenaiCompat => "openai-compatible",
        }
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

        // Phase 1: snapshot prompt state for cache-break detection.
        // Skip the (non-trivial) hashing work when no detector is installed.
        if let Some(detector) = &self.cache_break_detector
            && let Some(query_source) = params.query_source.as_deref()
        {
            let input = build_prompt_state_input(self, params, query_source);
            detector.lock().await.record_prompt_state(input);
        }

        loop {
            match self.do_query(params).await {
                Ok(mut result) => {
                    result.retries = attempt;
                    result.total_duration_ms =
                        i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX);

                    let mut usage = self.usage.lock().await;
                    usage.record(&result.model, result.usage);
                    drop(usage);

                    // Phase 2: post-call cache-break check.
                    if let Some(detector) = &self.cache_break_detector
                        && let Some(query_source) = params.query_source.as_deref()
                    {
                        let res = detector.lock().await.check_response_for_cache_break(
                            query_source,
                            result.usage.cache_read_input_tokens,
                            result.usage.cache_creation_input_tokens,
                            params.time_since_last_assistant_ms,
                            params.agent_id.as_deref(),
                        );
                        if matches!(res.state, CacheState::Broken) {
                            warn!(
                                target: "coco::cache_break",
                                source = %query_source,
                                agent_id = ?params.agent_id,
                                reason = %res.reason,
                                prev_cache_read = ?res.prev_cache_read_tokens,
                                cache_read = res.cache_read_tokens,
                                cache_creation = res.cache_creation_tokens,
                                "prompt cache break detected"
                            );
                            // OTel counter for dashboards / alerts. Reason
                            // is a free-form string; collapse to a small
                            // bucket here so cardinality stays bounded.
                            let bucket = cache_break_reason_bucket(&res.reason);
                            coco_otel::metrics::record_counter(
                                "coco_cache_break_total",
                                1,
                                &[("source", query_source), ("reason", bucket)],
                            );
                        }
                    }

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
            context_management: params.context_management.clone(),
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

/// Collapse a cache-break reason string into a small bucket label for
/// OTel cardinality. The reason itself can be a free-form join of many
/// `PendingChanges::explain()` parts; bucketing keeps the metric tractable.
fn cache_break_reason_bucket(reason: &str) -> &'static str {
    if reason.contains("model changed") {
        "model"
    } else if reason.contains("system prompt") {
        "system_prompt"
    } else if reason.contains("tools changed") {
        "tools"
    } else if reason.contains("provider options") {
        "provider_options"
    } else if reason.contains("betas") {
        "betas"
    } else if reason.contains("cache_control") {
        "cache_control"
    } else if reason.contains("effort") {
        "effort"
    } else if reason.contains("1h TTL") {
        "ttl_1h"
    } else if reason.contains("5min TTL") {
        "ttl_5min"
    } else if reason.contains("server-side") {
        "server_side"
    } else {
        "other"
    }
}

/// Build a [`PromptStateInput`] from `(client, params, query_source)` for
/// the cache-break detector's phase 1.
///
/// Provider-agnostic by design: collapses anything provider-specific into
/// a single `extra_body_hash` over `params.context_management` so adding
/// new providers requires no detector code changes. Anthropic-specific
/// fields (`betas`, `is_using_overage`, `cached_mc_enabled`) stay at
/// defaults until a future provider shim populates them via
/// [`QueryParams`] extensions.
fn build_prompt_state_input(
    client: &ApiClient,
    params: &QueryParams,
    query_source: &str,
) -> PromptStateInput {
    let (system_text, system_char_count) = extract_system_text(&params.prompt);
    let system_hash = djb2_hash(system_text.as_bytes());

    let (tool_names, per_tool_hashes, tools_hash) = hash_tools(params.tools.as_deref());

    let extra_body_hash = params
        .context_management
        .as_ref()
        .map(canonical_extra_body_hash)
        .unwrap_or(0);
    let extra_body_serialized = params
        .context_management
        .as_ref()
        .map(canonical_extra_body_serialize);

    let effort_value = params
        .thinking_level
        .as_ref()
        .map(|t| {
            // Stable serde-derived string. `format!("{:?}")` debug
            // format is not a stability commitment; serde rename-rules
            // are. The value is opaque to the detector — only
            // equality matters.
            serde_json::to_string(&t.effort).unwrap_or_default()
        })
        .unwrap_or_default();

    PromptStateInput {
        system_hash,
        tools_hash,
        cache_control_hash: 0, // Provider-specific; tracked via extra_body_hash for now.
        tool_names,
        per_tool_hashes,
        system_char_count,
        model: client.model_id().to_string(),
        query_source: query_source.to_string(),
        agent_id: params.agent_id.clone(),
        fast_mode: params.fast_mode,
        betas: Vec::new(),
        extra_body_hash,
        extra_body_serialized,
        effort_value,
        global_cache_strategy: String::new(),
        auto_mode_active: false,
        is_using_overage: false,
        cached_mc_enabled: false,
    }
}

/// Extract the concatenated system message text + char count. Returns
/// `("", 0)` when no system message is present.
fn extract_system_text(prompt: &LanguageModelV4Prompt) -> (String, i64) {
    let mut text = String::new();
    for msg in prompt {
        if let LanguageModelV4Message::System { content, .. } = msg {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(content);
        }
    }
    let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
    (text, chars)
}

/// Canonical-hash the tool list. Returns `(tool_names_in_order,
/// per_tool_hashes, aggregate_hash)`.
///
/// The aggregate is computed by walking `names` in declaration order and
/// folding each per-tool hash through djb2 — iterating `per_tool.values()`
/// would be HashMap-order-random and produce non-deterministic aggregates
/// for the same logical input.
fn hash_tools(
    tools: Option<&[vercel_ai_provider::LanguageModelV4Tool]>,
) -> (Vec<String>, HashMap<String, u64>, u64) {
    let Some(tools) = tools else {
        return (Vec::new(), HashMap::new(), 0);
    };
    let mut names = Vec::with_capacity(tools.len());
    let mut per_tool = HashMap::with_capacity(tools.len());
    for (idx, tool) in tools.iter().enumerate() {
        let raw_name = match tool {
            vercel_ai_provider::LanguageModelV4Tool::Function(f) => f.name.clone(),
            vercel_ai_provider::LanguageModelV4Tool::Provider(p) => p.name.clone(),
        };
        let key = if raw_name.is_empty() {
            format!("__idx_{idx}")
        } else {
            raw_name
        };
        let value = serde_json::to_value(tool).unwrap_or(serde_json::Value::Null);
        per_tool.insert(key.clone(), canonical_extra_body_hash(&value));
        names.push(key);
    }
    // Walk `names` (declaration order) so the aggregate is deterministic
    // for the same logical input, regardless of HashMap iteration order.
    let mut agg: u64 = 0;
    for name in &names {
        if let Some(h) = per_tool.get(name) {
            agg = agg.wrapping_mul(33).wrapping_add(*h);
        }
    }
    (names, per_tool, agg)
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
