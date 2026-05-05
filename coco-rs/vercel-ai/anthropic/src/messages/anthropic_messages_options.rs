use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use vercel_ai_provider::ProviderOptions;

/// Anthropic-specific thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    /// For Sonnet 4.6, Opus 4.6, and newer models.
    #[serde(rename = "adaptive")]
    Adaptive,
    /// For models before Opus 4.6 (except Sonnet 4.6 which still supports it).
    #[serde(rename = "enabled")]
    Enabled {
        #[serde(rename = "budgetTokens")]
        budget_tokens: Option<u64>,
    },
    /// Disable thinking.
    #[serde(rename = "disabled")]
    Disabled,
}

/// Structured output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StructuredOutputMode {
    OutputFormat,
    JsonTool,
    Auto,
}

/// Effort level.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Max,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Speed mode (Opus 4.6 only).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    Fast,
    Standard,
}

impl Speed {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Standard => "standard",
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub name: String,
    pub url: String,
    pub authorization_token: Option<String>,
    pub tool_configuration: Option<McpToolConfiguration>,
}

/// MCP tool configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolConfiguration {
    pub enabled: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
}

/// Container configuration for agent skills.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfig {
    pub id: Option<String>,
    pub skills: Option<Vec<ContainerSkill>>,
}

/// A skill in a container.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSkill {
    #[serde(rename = "type")]
    pub skill_type: String,
    pub skill_id: String,
    pub version: Option<String>,
}

/// Cache control configuration.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct CacheControlConfig {
    #[serde(rename = "type")]
    pub cache_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// Adapter-side mirror of `coco_types::PromptCacheMode`. Same wire shape;
/// no shared type (`vercel-ai-anthropic` cannot import `coco-types` per
/// the L0 layer rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCacheMode {
    Disabled,
    Auto,
    Manual,
}

/// Adapter-side mirror of `coco_types::CacheTtl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCacheTtl {
    FiveMinutes,
    OneHour,
}

/// Adapter-side mirror of `coco_types::CacheScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCacheScope {
    Org,
    Global,
}

/// Adapter-side mirror of `coco_types::BetaCapability`.
///
/// Wire boundary: snake_case (matches what
/// `services/inference::cache_convert` writes). The adapter's
/// `beta_capabilities::map_capability` then translates each enum variant
/// into the actual Anthropic header string (kebab-case + date suffix,
/// e.g. `"context-1m-2025-08-07"`). Two distinct hops:
/// JSON-snake → Rust enum → Anthropic-kebab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterBetaCapability {
    /// Wire name forced to `"context_1m"` to match the upstream coco-types
    /// rename — serde's snake_case treats digits as part of the preceding
    /// word and would emit `"context1m"`.
    #[serde(rename = "context_1m")]
    Context1m,
    InterleavedThinking,
    ContextManagement,
    StructuredOutputs,
    TokenEfficientTools,
    FastMode,
    PromptCachingScope,
    RedactThinking,
    Advisor,
}

/// Per-call cache strategy directive. Mirror of `coco_types::PromptCacheConfig`
/// without the `requested_betas` field (which lives at the
/// `AnthropicProviderOptions` top level).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStrategy {
    pub mode: AdapterCacheMode,
    /// Caller-requested TTL. Adapter may downgrade based on eligibility.
    pub ttl: AdapterCacheTtl,
    #[serde(default)]
    pub scope: Option<AdapterCacheScope>,
    #[serde(default)]
    pub skip_cache_write: bool,
}

/// Internal-only keys that `services/inference::cache_convert` emits
/// into `provider_options["anthropic"]`. They MUST be stripped from
/// the raw map before the shallow-merge into the wire body, otherwise
/// they'd be sent verbatim to `api.anthropic.com/v1/messages` (which
/// would either reject the body or silently ignore the keys).
///
/// Design §10.1.5 / Finding 2.
const INTERNAL_ANTHROPIC_OPTION_KEYS: &[&str] = &[
    "cacheStrategy",
    "requestedBetas",
    "agenticQuery",
    "querySource",
];

/// Anthropic-specific provider options.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicProviderOptions {
    /// Whether to send reasoning to the model.
    pub send_reasoning: Option<bool>,
    /// Structured output mode.
    pub structured_output_mode: Option<StructuredOutputMode>,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Disable parallel tool use.
    pub disable_parallel_tool_use: Option<bool>,
    /// Cache control settings (low-level user override; composes
    /// additively with auto-placed markers).
    pub cache_control: Option<CacheControlConfig>,
    /// MCP servers.
    pub mcp_servers: Option<Vec<McpServerConfig>>,
    /// Container/agent skills configuration.
    pub container: Option<ContainerConfig>,
    /// Enable/disable tool streaming.
    pub tool_streaming: Option<bool>,
    /// Effort level.
    pub effort: Option<Effort>,
    /// Speed mode (Opus 4.6 only).
    pub speed: Option<Speed>,
    /// Extra beta features.
    pub anthropic_beta: Option<Vec<String>>,
    /// Context management configuration.
    pub context_management: Option<Value>,
    /// Inference geography constraint (`"us"` or `"global"`).
    pub inference_geo: Option<String>,

    // ─── Prompt-cache per-call fields (design §10.1) ─────────────────
    /// High-level cache strategy: auto-place markers, choose TTL/scope.
    /// Adapter resolves to wire `cache_control` blocks via
    /// `cache_placement::compute_marker_index_post_group`.
    pub cache_strategy: Option<CacheStrategy>,
    /// User-requested beta top-up (TS `getSdkBetas` equivalent).
    /// Adapter applies a TS-mirror allowlist (`Context1m` only on the
    /// typed channel) — design §10.4 / Finding F4.
    pub requested_betas: Option<Vec<AdapterBetaCapability>>,
    /// Per-call agentic flag — gates the `claude-code-20250219`
    /// baseline beta. Helper calls (compaction, title generation)
    /// pass `false`; main agent loop passes `true`.
    pub agentic_query: Option<bool>,
    /// Query source — matched against the 1h-TTL allowlist per call.
    pub query_source: Option<String>,
    // **Round-3 Finding 3:** `account_kind` and `in_overage` are NOT
    // per-call fields. They live on `AnthropicConfig` (session-stable)
    // because `cache_policy::resolve_ttl` latches eligibility on the
    // first call — a missing first-call value would default-corrupt
    // the latch for the whole session.
}

/// Extract Anthropic-specific options from generic provider options.
///
/// Returns `(typed, raw)`:
///
/// - `typed` — parsed `AnthropicProviderOptions`, used for header /
///   beta / validation side-effects (e.g. `interleaved-thinking-*`
///   beta, MCP tool validation, `code-execution-*` beta gates).
/// - `raw` — verbatim user-supplied map, shallow-merged into the wire
///   body root at the end of `get_args`. **Opaque to coco-rs** — every
///   key (typed-known or not) is patched as-is. Users are responsible
///   for the correctness of their keys and shapes.
///
/// Parses from both the canonical `"anthropic"` key and any custom
/// provider name key (for renamed instances like `"my-proxy"`),
/// merging them per-key with custom winning. The provider name prefix
/// is extracted from the full provider string
/// (`"my-proxy.messages"` → `"my-proxy"`).
///
/// `BTreeMap` keeps wire-body field order stable in tests / insta
/// snapshots.
pub fn extract_anthropic_options(
    provider_options: &Option<ProviderOptions>,
    provider: &str,
) -> (AnthropicProviderOptions, BTreeMap<String, Value>) {
    let opts = match provider_options.as_ref() {
        Some(opts) => opts,
        None => return (AnthropicProviderOptions::default(), BTreeMap::new()),
    };

    // Parse canonical "anthropic" key
    let canonical_value = opts
        .0
        .get("anthropic")
        .and_then(|v| serde_json::to_value(v).ok());
    let canonical: AnthropicProviderOptions = canonical_value
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Extract custom provider name prefix (e.g., "my-proxy.messages" → "my-proxy")
    let provider_name = match provider.find('.') {
        Some(idx) => &provider[..idx],
        None => provider,
    };

    let (typed, custom_value): (AnthropicProviderOptions, Option<Value>) =
        if provider_name == "anthropic" {
            (canonical, None)
        } else {
            let custom_value = opts
                .0
                .get(provider_name)
                .and_then(|v| serde_json::to_value(v).ok());
            let custom_typed: Option<AnthropicProviderOptions> = custom_value
                .clone()
                .and_then(|v| serde_json::from_value(v).ok());
            let merged = match custom_typed {
                Some(custom) => merge_anthropic_options(canonical, custom),
                None => canonical,
            };
            (merged, custom_value)
        };

    // Verbatim raw map: every key from canonical + custom (custom wins
    // per-key). NOT filtered by typed schema — user owns correctness.
    let mut raw = BTreeMap::new();
    if let Some(Value::Object(map)) = canonical_value {
        for (k, v) in map {
            raw.insert(k, v);
        }
    }
    if let Some(Value::Object(map)) = custom_value {
        for (k, v) in map {
            raw.insert(k, v);
        }
    }

    // Strip internal coco-rs-only signals so they never reach
    // `api.anthropic.com/v1/messages` (design §10.1.5 / Finding 2).
    for key in INTERNAL_ANTHROPIC_OPTION_KEYS {
        raw.remove(*key);
    }

    (typed, raw)
}

/// Merge two option structs: custom values override canonical.
fn merge_anthropic_options(
    canonical: AnthropicProviderOptions,
    custom: AnthropicProviderOptions,
) -> AnthropicProviderOptions {
    AnthropicProviderOptions {
        send_reasoning: custom.send_reasoning.or(canonical.send_reasoning),
        structured_output_mode: custom
            .structured_output_mode
            .or(canonical.structured_output_mode),
        thinking: custom.thinking.or(canonical.thinking),
        disable_parallel_tool_use: custom
            .disable_parallel_tool_use
            .or(canonical.disable_parallel_tool_use),
        cache_control: custom.cache_control.or(canonical.cache_control),
        mcp_servers: custom.mcp_servers.or(canonical.mcp_servers),
        container: custom.container.or(canonical.container),
        tool_streaming: custom.tool_streaming.or(canonical.tool_streaming),
        effort: custom.effort.or(canonical.effort),
        speed: custom.speed.or(canonical.speed),
        anthropic_beta: custom.anthropic_beta.or(canonical.anthropic_beta),
        context_management: custom.context_management.or(canonical.context_management),
        inference_geo: custom.inference_geo.or(canonical.inference_geo),
        cache_strategy: custom.cache_strategy.or(canonical.cache_strategy),
        requested_betas: custom.requested_betas.or(canonical.requested_betas),
        agentic_query: custom.agentic_query.or(canonical.agentic_query),
        query_source: custom.query_source.or(canonical.query_source),
    }
}

#[cfg(test)]
#[path = "anthropic_messages_options.test.rs"]
mod tests;
