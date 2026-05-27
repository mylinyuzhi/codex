use crate::build_call_options::PerCallOverrides;
use crate::build_call_options::build_call_options_with_extra;
use crate::cache_detection::CacheBreakDetector;
use crate::cache_detection::CacheState;
use crate::cache_detection::PromptStateInput;
use crate::cache_detection::canonical_extra_body_hash;
use crate::cache_detection::canonical_extra_body_serialize;
use crate::cache_detection::djb2_hash;
use crate::errors::InferenceError;
use crate::fingerprint::ProviderClientFingerprint;
use crate::prompt_layout::build_prompt_layout_from_prompt;
use crate::prompt_layout::put_layout_options;
use crate::retry::RetryConfig;
use crate::usage::UsageAccumulator;
use coco_config::ModelInfo;
use coco_llm_types::AssistantContentPart;
use coco_llm_types::LlmMessage;
use coco_llm_types::LlmPrompt;
use coco_llm_types::UserContentPart;
use coco_types::Capability;
use coco_types::PromptCacheConfig;
use coco_types::ProviderModelSelection;
use coco_types::ThinkingLevel;
use coco_types::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;

/// Parameters for a single query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryParams {
    /// Messages to send (as LlmPrompt).
    pub prompt: LlmPrompt,
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
    /// Provider-agnostic tool selection directive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<vercel_ai_provider::LanguageModelV4ToolChoice>,
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
    /// Per-call prompt-cache directive. Forwarded by `services/inference`
    /// as opaque pass-through to `provider_options[<namespace>]` via
    /// [`crate::cache_convert::to_extra_body`]; non-Anthropic providers
    /// see no caching keys. Adapter (`vercel-ai-anthropic`) owns all
    /// policy interpretation.
    ///
    /// **Session-stable** account / overage state is NOT carried here —
    /// it lives on the provider's `AnthropicConfig`, set by
    /// `build_anthropic` from `RuntimeConfig.account.*`. See
    /// `docs/coco-rs/prompt-cache-design.md` §9.5 / R3-F3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<PromptCacheConfig>,
    /// Per-call agentic-loop flag. Helper calls (compaction, title
    /// generation, classification) pass `false`; main agent loop
    /// passes `true`. Gates the `claude-code-20250219` baseline beta in
    /// the Anthropic adapter.
    #[serde(default)]
    pub agentic: bool,
    /// Generation stop sequences forwarded to the provider. Used by the
    /// auto-mode classifier's stage-1 early termination (`</block>`)
    /// and any helper call that wants the model to halt on a marker.
    /// Mapping per provider (handled by [`build_call_options`] →
    /// `LanguageModelV4CallOptions.stop_sequences`):
    ///   * Anthropic → `stop_sequences`
    ///   * OpenAI Chat / OpenAI-Compatible → `stop`
    ///   * Gemini → `stopSequences`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Native structured-output spec. Threaded into
    /// [`vercel_ai_provider::LanguageModelV4CallOptions::response_format`]
    /// **only** when the resolved model declares
    /// [`Capability::StructuredOutput`]; otherwise dropped with a
    /// `debug!` log so the caller's
    /// `forced_tool` / `tools` path (if any) becomes the multi-LLM
    /// wire format.
    ///
    /// Per-provider wire shape is owned by the respective
    /// `vercel-ai-*` adapter (OpenAI `response_format.json_schema`,
    /// Gemini `responseSchema`, Anthropic `output_format` or synthetic
    /// json tool fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<vercel_ai_provider::ResponseFormat>,
}

/// Result of a query.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub content: Vec<AssistantContentPart>,
    pub usage: TokenUsage,
    pub model: String,
    /// Typed stop reason — the canonical 8-variant
    /// [`StopReason`] (re-exported as [`coco_llm_types::StopReason`])
    /// from the vercel-ai-provider seam. Higher layers match on this
    /// enum directly; no wire-string parsing anywhere above this
    /// boundary.
    pub stop_reason: Option<coco_llm_types::StopReason>,
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
    /// Logical provider/model identity from config, before any
    /// provider-specific `api_model_name` rewrite.
    model_identity: ProviderModelSelection,
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
        model_identity: ProviderModelSelection,
        retry: RetryConfig,
    ) -> Self {
        Self {
            model,
            fingerprint,
            model_info,
            model_identity,
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
            runtime_state_digest: [0u8; 32],
        };
        let model_identity = ProviderModelSelection {
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
        };
        Self::new(
            model,
            fingerprint,
            /*model_info*/ None,
            model_identity,
            retry,
        )
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

    /// Logical provider/model identity from config.
    pub fn model_identity(&self) -> &ProviderModelSelection {
        &self.model_identity
    }

    /// Resolved [`ModelInfo`] for the underlying client. `None` for
    /// test/mock clients built through the lightweight constructors
    /// that bypass the registry resolution path.
    ///
    /// Callers that need capability gates (e.g. `engine_prompt`
    /// branching on [`coco_types::Capability::ServerSideToolReference`])
    /// look up through this accessor rather than reaching into the
    /// configuration tree, so post-fallback model swaps surface
    /// immediately on the next turn.
    #[must_use]
    pub fn model_info(&self) -> Option<&ModelInfo> {
        self.model_info.as_ref()
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

    /// Whether this provider/model pair supports Anthropic-style prompt
    /// cache markers.
    ///
    /// Mirrors TS's two-axis gate: provider family must support the wire
    /// shape, and the resolved model must declare prompt-cache capability.
    /// `None` model info is reserved for tests/mocks, where we stay
    /// permissive so call-shape tests can exercise the path.
    #[must_use]
    pub fn supports_prompt_cache(&self) -> bool {
        if !matches!(self.fingerprint.api, coco_types::ProviderApi::Anthropic) {
            return false;
        }
        self.model_info
            .as_ref()
            .map(|m| {
                m.capabilities
                    .as_ref()
                    .is_some_and(|caps| caps.contains(&Capability::PromptCache))
            })
            .unwrap_or(true)
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
    /// **Layer-2 plumbing.** Call options are built **once** before the
    /// retry loop and reused across attempts. The detector hash and the
    /// retry body cannot drift because they share the same merged
    /// extra-body map.
    ///
    /// Mock paths that bypass `build_options_with_extra` (legacy
    /// constructor, `with_default_fingerprint`) skip the merged-extra
    /// snapshot and feed an empty `BTreeMap` to the detector — this
    /// preserves existing detection behavior on the mock path.
    #[tracing::instrument(
        skip_all,
        name = "api_call",
        fields(
            provider = %self.model.provider(),
            model_id = %self.model.model_id(),
            mode = "blocking",
            query_source = ?params.query_source,
            agent_id = ?params.agent_id,
            agentic = params.agentic,
            tool_count = params.tools.as_ref().map(Vec::len).unwrap_or(0),
            prompt_messages = params.prompt.len(),
        ),
    )]
    pub async fn query(&self, params: &QueryParams) -> Result<QueryResult, InferenceError> {
        let start = std::time::Instant::now();
        let mut attempt = 0;
        debug!("api_call begin");

        // Build call options exactly once. Same options reused across
        // retries and used as the input fed to detector hashing — no
        // drift possible (design §9.7.3 / Finding 5 fix).
        let (call_options, merged_extra) = self.build_options_with_extra(params);

        // Phase 1: snapshot prompt state for cache-break detection.
        // Skip the (non-trivial) hashing work when no detector is installed.
        if let Some(detector) = &self.cache_break_detector
            && let Some(query_source) = params.query_source.as_deref()
        {
            // Mirror the layout adapter that runs in `build_options`
            // so the detector reads the same hashes the wire body was
            // built from.
            let layout = if self.model_info.is_some() {
                Some(crate::prompt_layout::build_prompt_layout_from_prompt(
                    &params.prompt,
                    self.fingerprint.api,
                    params.tools.as_deref(),
                ))
            } else {
                None
            };
            let layout_hashes = layout.as_ref().and_then(|l| l.prompt_hash_inputs.as_ref());
            let input =
                build_prompt_state_input(self, params, query_source, layout_hashes, &merged_extra);
            detector.lock().await.record_prompt_state(input);
        }

        loop {
            // `call_options` is borrowed across attempts — no per-attempt
            // clone of the prompt vector. With N retries the savings are
            // N-1 × `Vec<LlmMessage>::clone` (which can be 100s of KB).
            match self.do_query_with_options(&call_options).await {
                Ok(mut result) => {
                    result.retries = attempt;
                    result.total_duration_ms =
                        i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX);

                    info!(
                        attempt,
                        duration_ms = result.total_duration_ms,
                        tokens_in = result.usage.input_tokens.total,
                        tokens_out = result.usage.output_tokens.total,
                        cache_read = result.usage.input_tokens.cache_read,
                        cache_creation = result.usage.input_tokens.cache_write,
                        stop_reason = ?result.stop_reason,
                        model_id = %result.model,
                        "api_call ok"
                    );

                    // Abnormal stop_reason ≠ error, but warrants a
                    // warn so ops can spot truncation / content-filter
                    // events without scraping every info-level line.
                    // Happy-path set: `EndTurn` / `StopSequence` /
                    // `ToolUse` (see [`StopReason::is_normal`]).
                    if let Some(reason) = result.stop_reason
                        && reason.is_abnormal()
                    {
                        warn!(
                            stop_reason = %reason,
                            tokens_out = result.usage.output_tokens.total,
                            max_tokens = ?params.max_tokens,
                            query_source = ?params.query_source,
                            model_id = %result.model,
                            "api_call ended on non-normal stop_reason"
                        );
                    }

                    let mut usage = self.usage.lock().await;
                    // Aggregate per the (provider, model_id) identity
                    // cached on ApiClient — not the raw `result.model`
                    // string, which lacks the provider qualifier and
                    // would conflate cross-provider models of the same
                    // name. See `UsageAccumulator` doc for wire format.
                    usage.record(&self.model_identity, result.usage);
                    drop(usage);

                    // Phase 2: post-call cache-break check.
                    if let Some(detector) = &self.cache_break_detector
                        && let Some(query_source) = params.query_source.as_deref()
                    {
                        let res = detector.lock().await.check_response_for_cache_break(
                            query_source,
                            result.usage.input_tokens.cache_read,
                            result.usage.input_tokens.cache_write,
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

    /// Execute a single query attempt via LanguageModelV4::do_generate()
    /// with pre-built options.
    ///
    /// `options` is borrowed across retries; per-attempt clones are
    /// avoided. `abort_signal` is `None` for now — coco-inference doesn't
    /// thread a cancellation token through `QueryParams` yet (TS parity
    /// also doesn't carry one at this seam). Adding it later means
    /// adding a `QueryParams.abort_signal: Option<CancellationToken>`
    /// field and forwarding here.
    async fn do_query_with_options(
        &self,
        options: &LanguageModelV4CallOptions,
    ) -> Result<QueryResult, InferenceError> {
        let result = self
            .model
            .do_generate(options, None)
            .await
            .map_err(|e| self.wrap_provider_error(e))?;

        let usage = crate::stream::token_usage_from_provider_usage(&result.usage);

        let model_id = result
            .response
            .as_ref()
            .and_then(|r| r.model_id.clone())
            .unwrap_or_else(|| self.model.model_id().to_string());

        // Typed unified reason — single source of truth set by the
        // provider-adapter seam (see `vercel-ai-anthropic` etc).
        let stop_reason = Some(result.finish_reason.unified);

        // Provider response.id (Anthropic message.id / OpenAI response.id
        // / OpenAI-compatible response.id) flows through to QueryResult so
        // the engine can stamp it onto the committed AssistantMessage.
        // Google adapter leaves this None — see plan §P1.5.
        let request_id = result.response.as_ref().and_then(|r| r.id.clone());

        Ok(QueryResult {
            content: result.content,
            usage,
            model: model_id,
            stop_reason,
            request_id,
            retries: 0,
            total_duration_ms: 0,
        })
    }

    /// Execute a streaming query. Returns a channel receiver for stream events.
    pub async fn query_stream(
        &self,
        params: &QueryParams,
    ) -> Result<tokio::sync::mpsc::Receiver<crate::stream::StreamEvent>, InferenceError> {
        self.query_stream_with_config(params, crate::stream::default_process_stream_config())
            .await
    }

    /// Execute a streaming query with explicit stream processor config.
    #[tracing::instrument(
        skip_all,
        name = "api_call",
        fields(
            provider = %self.model.provider(),
            model_id = %self.model.model_id(),
            mode = "stream",
            query_source = ?params.query_source,
            agent_id = ?params.agent_id,
            agentic = params.agentic,
            tool_count = params.tools.as_ref().map(Vec::len).unwrap_or(0),
            prompt_messages = params.prompt.len(),
        ),
    )]
    pub async fn query_stream_with_config(
        &self,
        params: &QueryParams,
        stream_config: crate::stream::StreamProcessorConfig,
    ) -> Result<tokio::sync::mpsc::Receiver<crate::stream::StreamEvent>, InferenceError> {
        debug!("api_call stream begin");
        let options = self.build_options(params);

        // `abort_signal: None` — see `do_query_with_options` rationale.
        let result = self.model.do_stream(&options, None).await.map_err(|e| {
            let err = self.wrap_provider_error(e);
            warn!(error = %err, "api_call stream open failed");
            err
        })?;

        debug!("api_call stream opened");
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(crate::stream::process_stream_with_config(
            result.stream,
            tx,
            stream_config,
        ));

        Ok(rx)
    }

    /// Get accumulated usage across all calls.
    pub async fn accumulated_usage(&self) -> UsageAccumulator {
        self.usage.lock().await.clone()
    }

    /// Wrap an opaque provider `AISdkError` into [`InferenceError::ProviderError`]
    /// with `(provider, model_id)` attribution prefixed onto the message.
    /// Mirrors `vercel_ai::wrap_gateway_error` so error logs name the
    /// failing provider/model instead of just the raw vendor message.
    /// Status defaults to `0` because the underlying SDK error type
    /// doesn't carry HTTP status — typed status codes are recovered at
    /// the next layer up via [`crate::errors`] classification.
    fn wrap_provider_error(&self, e: vercel_ai_provider::AISdkError) -> InferenceError {
        crate::errors::ProviderSnafu {
            status: 0_i32,
            message: format!(
                "Provider '{}' error for model '{}': {}",
                self.model.provider(),
                self.model.model_id(),
                e
            ),
        }
        .build()
    }

    /// Build [`LanguageModelV4CallOptions`] for a query, returning the
    /// merged flat extra-body map alongside the call options. The
    /// merged map is the canonical input for cache-break detection so
    /// the detector hash and the actual retry body cannot drift.
    ///
    /// Mock / test path (`model_info == None`) returns
    /// `(direct_construction, BTreeMap::new())` — the empty map
    /// preserves existing behavior for callers that bypass Layer-2.
    fn build_options_with_extra(
        &self,
        params: &QueryParams,
    ) -> (
        LanguageModelV4CallOptions,
        BTreeMap<String, serde_json::Value>,
    ) {
        let Some(info) = self.model_info.as_ref() else {
            // Legacy mock path — direct construction; empty merged map.
            let mut options = LanguageModelV4CallOptions {
                prompt: params.prompt.clone(),
                max_output_tokens: max_tokens_to_u64(params.max_tokens),
                ..Default::default()
            };
            if let Some(ref tools) = params.tools {
                options.tools = Some(tools.clone());
            }
            options.tool_choice = params.tool_choice.clone();
            if let Some(stops) = params.stop_sequences.as_ref()
                && !stops.is_empty()
            {
                options.stop_sequences = Some(stops.clone());
            }
            // Mock path has no `ModelInfo` to query capabilities — accept
            // `response_format` as-is so test doubles can exercise the
            // structured-output codepath without registering a capability.
            if let Some(fmt) = params.response_format.clone() {
                options.response_format = Some(fmt);
            }
            return (options, BTreeMap::new());
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
            cache_strategy: params.cache.clone(),
            agentic_query: params.agentic,
            query_source: params.query_source.clone(),
            stop_sequences: params.stop_sequences.clone(),
            ..Default::default()
        };
        let (mut call, merged_extra) = build_call_options_with_extra(
            info,
            self.fingerprint.api,
            &self.fingerprint.provider,
            &per_call,
            params.prompt.clone(),
            params.tools.clone(),
        );
        call.tool_choice = params.tool_choice.clone();

        // Layout adapter: route the System / Developer text into the
        // provider's native top-level slot and stash provider-agnostic
        // hash inputs for the cache-break detector. Provider crates
        // parse `provider_options["prompt_layout"]` via a local serde
        // mirror struct (no `coco-inference` dependency).
        let layout = build_prompt_layout_from_prompt(
            &call.prompt,
            self.fingerprint.api,
            call.tools.as_deref(),
        );
        let mut po = call.provider_options.unwrap_or_default();
        put_layout_options(&mut po, &layout);
        call.provider_options = Some(po);

        // Native structured-output gate. The resolved provider adapter
        // handles its own wire shape (OpenAI `response_format.json_schema`,
        // Gemini `responseSchema`, Anthropic `output_format` with
        // `structured-outputs-2025-11-13` beta or synthetic-tool
        // fallback). We forward `response_format` only when the
        // model declares [`Capability::StructuredOutput`] —
        // OpenAI-compatible endpoints without this capability
        // (Volcengine, ZAI, …) historically 400 on
        // `response_format: json_schema`. Caller's `forced_tool` /
        // `tools` path stays untouched and runs as the multi-LLM
        // fallback when the capability isn't declared.
        if let Some(fmt) = params.response_format.clone() {
            let supports = info
                .capabilities
                .as_deref()
                .unwrap_or(&[])
                .contains(&Capability::StructuredOutput);
            if supports {
                call.response_format = Some(fmt);
            } else {
                debug!(
                    target: "coco_inference::client",
                    model_id = self.model.model_id(),
                    "response_format requested but model lacks Capability::StructuredOutput; dropping (falling back to caller's tool path if any)"
                );
            }
        }

        (call, merged_extra)
    }

    /// Convenience shim around [`Self::build_options_with_extra`] that
    /// drops the merged-extra snapshot. Used by `query_stream` paths
    /// where no detector hash is needed.
    fn build_options(&self, params: &QueryParams) -> LanguageModelV4CallOptions {
        self.build_options_with_extra(params).0
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

/// Build a [`PromptStateInput`] from `(client, params, query_source,
/// merged_extra)` for the cache-break detector's phase 1.
///
/// Provider-agnostic by design: hashes the **post-merge, pre-namespace-wrap**
/// flat extra map so any new key (cache_strategy, requestedBetas,
/// agenticQuery, querySource, …) is automatically tracked without
/// per-feature plumbing. Anthropic-specific fields (`betas`,
/// `is_using_overage`, `cached_mc_enabled`) stay at defaults — those
/// session-stable bits are caught by [`ProviderClientFingerprint`]
/// changes, not by the per-call detector hash (design §12.2).
///
/// **Layout integration.** When the call options already carry a
/// `provider_options["prompt_layout"].prompt_hash_inputs` payload (set by
/// [`crate::prompt_layout::build_prompt_layout_from_prompt`] in
/// `build_options`), prefer those hashes — they're the single source of
/// truth for prompt-content cache inputs. Otherwise fall back to walking
/// `params.prompt` directly so callers that bypass `build_options` (mock
/// path, integration tests) still get cache detection.
fn build_prompt_state_input(
    client: &ApiClient,
    params: &QueryParams,
    query_source: &str,
    layout_hashes: Option<&crate::prompt_layout::PromptHashInputs>,
    merged_extra: &BTreeMap<String, serde_json::Value>,
) -> PromptStateInput {
    let (system_hash, tool_names, per_tool_hashes, tools_hash, system_char_count) =
        if let Some(hashes) = layout_hashes {
            let names: Vec<String> = hashes
                .per_tool_hashes
                .iter()
                .map(|(n, _)| n.clone())
                .collect();
            let per_tool: HashMap<String, u64> = hashes.per_tool_hashes.iter().cloned().collect();
            (
                hashes.system_text_hash,
                names,
                per_tool,
                hashes.tools_hash,
                hashes.contextual_user_char_count,
            )
        } else {
            let (system_text, system_char_count) = extract_system_text(&params.prompt);
            let system_hash = djb2_hash(system_text.as_bytes());
            let (tool_names, per_tool, tools_hash) = hash_tools(params.tools.as_deref());
            (
                system_hash,
                tool_names,
                per_tool,
                tools_hash,
                system_char_count,
            )
        };

    // Hash the full merged extra-body map (pre-namespace-wrap). Empty
    // map → hash 0 / serialized None, preserving mock-path behavior.
    let (extra_body_hash, extra_body_serialized) = if merged_extra.is_empty() {
        (0, None)
    } else {
        let v = serde_json::to_value(merged_extra).unwrap_or(serde_json::Value::Null);
        (
            canonical_extra_body_hash(&v),
            Some(canonical_extra_body_serialize(&v)),
        )
    };

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
fn extract_system_text(prompt: &LlmPrompt) -> (String, i64) {
    let mut text = String::new();
    for msg in prompt {
        if let LlmMessage::System { content, .. } = msg {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&collapse_text_parts(content));
        }
    }
    let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
    (text, chars)
}

fn collapse_text_parts(parts: &[UserContentPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            UserContentPart::Text(text_part) => Some(text_part.text.as_str()),
            UserContentPart::File(_) => None,
        })
        .collect::<Vec<_>>()
        .join("")
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
