# coco-inference — Crate Plan

TS source: `src/services/api/`, `src/utils/auth.ts`, `src/services/oauth/`, `src/services/analytics/`, `src/services/tokenEstimation.ts`, `src/services/claudeAiLimits.ts`, `src/services/rateLimitMessages.ts`

## Dependencies

```
coco-inference depends on:
  - coco-types   (TokenUsage, ModelUsage, ProviderApi, ModelSpec, Capability)
  - coco-config  (ProviderInfo, ModelInfo, ModelRoles — for provider factory + request building)
  - coco-error   (ApiError)
  - vercel-ai-provider (LanguageModelV4 trait)
  - vercel-ai-anthropic, vercel-ai-openai, vercel-ai-google, etc. (provider impls)
  - reqwest, tokio (HTTP, async)

coco-inference does NOT depend on:
  - coco-tool, coco-tools (no tool knowledge)
  - coco-messages (no message history)
  - any app/ crate
```

## Modules

```
coco-inference/src/
  model_hub.rs      # ModelHub: caches providers + models, resolves ModelRole -> ModelSpec
  provider_factory.rs # ProviderFactory: ProviderApi -> Arc<dyn ProviderV4>
  request_builder.rs  # 5-step pipeline: normalize, cache, thinking, options, headers
  client.rs         # ApiClient wrapper around LanguageModelV4
  query.rs          # queryWithStreaming(), queryWithoutStreaming()
  retry.rs          # Exponential backoff, fallback, 529 handling
  errors.rs         # Error classification (retryable, auth, prompt-too-long)
  usage.rs          # TokenUsage accumulation (ModelUsage from coco-types)
  auth.rs           # API key, OAuth, Bedrock/Vertex/Foundry auth
  logging.rs        # Per-request logging (usage, TTFT, latency)
  rate_limit.rs     # Rate limit enforcement, cooldown, messaging
  token_estimation.rs  # Token counting for budget decisions
  analytics.rs      # Telemetry events, feature flags
```

## Data Definitions

### Client (vercel-ai based, unlike TS's @anthropic-ai/sdk)

```rust
/// Unlike TS which uses @anthropic-ai/sdk directly, Rust uses vercel-ai trait
/// for multi-provider support (Anthropic, OpenAI, Google, etc.)
pub struct ApiClient {
    model: Arc<dyn LanguageModelV4>,  // from vercel-ai-provider
    config: ApiClientConfig,
}

pub struct ApiClientConfig {
    pub provider: ProviderApi,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_retries: i32,            // default: 10
    pub custom_headers: HashMap<String, String>,
    pub proxy: Option<ProxyConfig>,
}
```

### Query Parameters

```rust
pub struct QueryParams {
    pub messages: Vec<MessageParam>,
    pub model: String,
    pub max_tokens: i64,
    pub system: Option<SystemPrompt>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<f64>,
    pub thinking: Option<ThinkingConfig>,
    pub effort: Option<EffortLevel>,
    pub custom_headers: HashMap<String, String>,  // beta headers
}

pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: Option<i64>,
}
```

### Retry Configuration (from `withRetry.ts`)

```rust
pub struct RetryConfig {
    pub max_retries: i32,           // default: 10
    pub base_delay_ms: i64,         // 500ms exponential base
    pub max_529_retries: i32,       // 3 consecutive before fallback
    pub persistent_max_backoff_ms: i64,  // 5 min
    pub persistent_reset_cap_ms: i64,    // 6 hours
}

/// Retryable conditions:
/// - 429: rate limit (always retry)
/// - 529: overloaded (only for foreground queries)
/// - APIConnectionError: timeouts, ECONNRESET, EPIPE
pub fn should_retry(error: &ApiError, config: &RetryConfig) -> bool;
pub fn calculate_backoff(attempt: i32, base_delay_ms: i64) -> Duration;
```

### Error Classification (from `errors.ts`)

```rust
pub enum ApiErrorKind {
    RateLimit,         // 429
    Overloaded,        // 529
    PromptTooLong,     // 400 with specific message
    MediaSizeError,    // 400 with media size message
    AuthError,         // 401, 403
    BadRequest,        // 400 (other)
    ServerError,       // 500, 502, 503
    ConnectionError,   // timeouts, ECONNRESET
}

pub fn classify_error(status: u16, body: &str) -> ApiErrorKind;
pub fn is_retryable(kind: &ApiErrorKind) -> bool;
pub fn parse_prompt_too_long_tokens(msg: &str) -> Option<(i64, i64)>;  // (actual, limit)
```

### Token Usage (from `usage.ts`)

```rust
// TokenUsage, ModelUsage — imported from coco-types (not redefined here)

pub fn accumulate_usage(total: &mut ModelUsage, delta: &TokenUsage);
pub fn detect_gateway(headers: &HeaderMap, base_url: &str) -> Option<KnownGateway>;
// Known gateways: litellm, helicone, portkey, cloudflare, kong, braintrust, databricks
```

### ModelHub (from cocode-rs `model_hub.rs`)

```rust
/// Central model management. Caches providers and models.
/// Resolves ModelRole -> ModelSpec using ModelRoles config.
pub struct ModelHub {
    config: Arc<ResolvedConfig>,
    providers: RwLock<HashMap<String, Arc<dyn ProviderV4>>>,
    models: RwLock<HashMap<ModelSpec, Arc<dyn LanguageModelV4>>>,
    factory: ProviderFactory,
}

impl ModelHub {
    pub async fn get_model(&self, spec: &ModelSpec) -> Result<Arc<dyn LanguageModelV4>, ApiError>;
    pub fn resolve_role(&self, role: ModelRole) -> ModelSpec;
    pub async fn get_or_create_provider(&self, api: &ProviderApi) -> Result<Arc<dyn ProviderV4>, ApiError>;
}
```

### ProviderFactory (from cocode-rs `provider_factory.rs`)

```rust
/// Routes ProviderApi enum to concrete provider implementation.
pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create_provider(&self, info: &ProviderInfo) -> Result<Arc<dyn ProviderV4>, ApiError>;
}
```

## Core Logic

### Streaming Query

```rust
/// Main entry point for LLM queries.
/// Uses vercel-ai LanguageModelV4 trait (not Anthropic SDK directly).
///
/// Input: QueryParams.prompt is LanguageModelV4Prompt (from coco-messages::to_language_model_prompt).
/// Output: Stream of vercel-ai StreamPart events.
pub async fn query_streaming(
    client: &ApiClient,
    params: QueryParams,
    cancel: CancellationToken,
) -> Result<impl Stream<Item = StreamPart>, ApiError>;

pub async fn query_non_streaming(
    client: &ApiClient,
    params: QueryParams,
    cancel: CancellationToken,
) -> Result<AssistantMessage, ApiError>;

/// Reverse path: collect vercel-ai stream into internal AssistantMessage.
/// Called by QueryEngine.process_stream().
///
/// Assembly:
///   StreamPart::TextDelta        → accumulate into AssistantContentBlock::Text
///   StreamPart::ToolCallDelta    → accumulate into AssistantContentBlock::ToolUse (by id)
///   StreamPart::ReasoningDelta   → accumulate into AssistantContentBlock::Thinking
///   StreamPart::FinishReason     → set stop_reason
///   StreamPart::Usage            → extract TokenUsage
pub fn collect_stream_to_message(
    stream: impl Stream<Item = StreamPart>,
    event_tx: &mpsc::Sender<QueryEvent>,
) -> (AssistantMessage, TokenUsage);
```

### Retry with Fallback

```rust
/// Retry loop with exponential backoff.
/// On 529 overloaded (3 consecutive), triggers model fallback.
/// On persistent retry (unattended), caps at 5min backoff, 6hr total.
pub async fn with_retry<F, T>(
    f: F,
    config: RetryConfig,
    cancel: CancellationToken,
) -> Result<T, CannotRetryError>
where
    F: Fn() -> Future<Output = Result<T, ApiError>>;
```

### Rate Limiting

```rust
pub struct RateLimitState {
    pub cooldown_until: Option<Instant>,
    pub consecutive_429s: i32,
}

pub fn check_rate_limit(state: &RateLimitState) -> Result<(), RateLimitMessage>;
pub fn format_rate_limit_message(retry_after: Duration) -> String;
```
