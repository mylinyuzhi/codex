use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

/// Which LLM provider implementation to use.
/// Consumed by coco-config (ProviderInfo) and coco-inference (ProviderFactory).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApi {
    Anthropic,
    Openai,
    Gemini,
    Volcengine,
    Zai,
    OpenaiCompat,
}

impl ProviderApi {
    /// Canonical display name used in banners, config labels, and wire strings.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
            Self::Gemini => "google",
            Self::Volcengine => "volcengine",
            Self::Zai => "zai",
            Self::OpenaiCompat => "openai-compat",
        }
    }
}

impl fmt::Display for ProviderApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which purpose a model serves. Multiple roles can map to different models.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Main,
    /// Plan-mode model. Used in two distinct contexts:
    ///
    /// 1. **Subagent role** — when the built-in Plan subagent spawns
    ///    (`SubagentType::Plan → ModelRole::Plan` in
    ///    `core/subagent/src/subagent_role.rs:34`), the spawn factory
    ///    resolves the client via `client_for_role(Plan)`.
    /// 2. **Main-session plan-mode swap** — when the leader enters
    ///    `PermissionMode::Plan`, the engine swaps the active client
    ///    to `client_for_role(Plan)` for the duration of plan mode.
    ///    TS parity behavioral analogue: `getRuntimeMainLoopModel`'s
    ///    `opusplan` → Opus alias swap (`utils/model/model.ts:145-167`).
    ///    coco-rs encodes this as a generic role slot so it works for
    ///    any provider, not just Anthropic.
    ///
    /// Unconfigured `models.plan` falls back to Main's spec via the
    /// chain in `runtime.rs:507`, and `client_for_role` short-circuits
    /// to the cached Main `Arc` — both call sites degrade cleanly to
    /// "no swap" without spurious cache breaks.
    Plan,
    Fast,
    Explore,
    Review,
    /// Forked-agent spawn (AgentTool / SkillTool). Distinct from
    /// `Explore` — Explore is an investigative subagent type that
    /// happens to often be a "small fast" model; Subagent is the
    /// generic spawn role used by tools/Agent and the swarm runtime.
    Subagent,
    Memory,
    HookAgent,
}

impl ModelRole {
    /// Canonical snake_case spelling. Matches the serde wire form so
    /// `ModelRole::Subagent.as_str() == serde_json::to_string(&Subagent)?`
    /// modulo the surrounding quotes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Fast => "fast",
            Self::Plan => "plan",
            Self::Explore => "explore",
            Self::Review => "review",
            Self::HookAgent => "hook_agent",
            Self::Memory => "memory",
            Self::Subagent => "subagent",
        }
    }
}

impl fmt::Display for ModelRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ModelRole {
    type Err = String;

    /// Accept the canonical snake_case spelling plus the camelCase form
    /// `hookAgent` for symmetry with TS-flavored config files. Trim and
    /// lowercase first so YAML scalars like `Explore` / ` plan ` parse.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "main" => Ok(Self::Main),
            "fast" => Ok(Self::Fast),
            "plan" => Ok(Self::Plan),
            "explore" => Ok(Self::Explore),
            "review" => Ok(Self::Review),
            "hook_agent" | "hookagent" => Ok(Self::HookAgent),
            "memory" => Ok(Self::Memory),
            "subagent" => Ok(Self::Subagent),
            _ => Err(format!("unknown model role: {s}")),
        }
    }
}

/// A resolved model identity: provider + model ID.
/// Produced by coco-config, consumed by coco-inference.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Sub-provider routing string (e.g., "bedrock", "vertex").
    pub provider: String,
    /// Resolved ProviderApi for dispatch.
    pub api: ProviderApi,
    /// Model identifier (e.g., "claude-opus-4-6", "gpt-5").
    pub model_id: String,
    /// Human-readable display name.
    pub display_name: String,
}

impl PartialEq for ModelSpec {
    fn eq(&self, other: &Self) -> bool {
        self.provider == other.provider && self.api == other.api && self.model_id == other.model_id
    }
}

impl Eq for ModelSpec {}

impl std::hash::Hash for ModelSpec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.provider.hash(state);
        self.api.hash(state);
        self.model_id.hash(state);
    }
}

/// Model capabilities (checked at request time).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    TextGeneration,
    Streaming,
    Vision,
    Audio,
    ToolCalling,
    Embedding,
    ExtendedThinking,
    StructuredOutput,
    ReasoningSummaries,
    ParallelToolCalls,
    FastMode,
    /// Wire supports Anthropic-style `cache_control` blocks.
    PromptCache,
    /// `context-1m-2025-08-07` beta — 1M token context window.
    /// Wire name forced to `"context_1m"` (serde's snake_case treats digits
    /// as part of the preceding word and would emit `"context1m"`).
    #[serde(rename = "context_1m")]
    Context1m,
    /// `interleaved-thinking-2025-05-14` beta. Also gates
    /// `redact-thinking-2026-02-12` (TS `betas.ts:272` reuses the
    /// same `modelSupportsISP` predicate for both).
    InterleavedThinking,
    /// `context-management-2025-06-27` beta.
    ContextManagement,
    /// Anthropic adaptive thinking — server picks effort dynamically.
    /// Gates the convert layer's emission of
    /// `thinking: {"type":"adaptive"}` for `ReasoningEffort::Auto`.
    /// Without this capability, the convert layer omits the field
    /// entirely so the server-side default applies (avoids 400 from
    /// non-adaptive models that reject the value).
    ///
    /// Known supporting models (Anthropic family): Claude Sonnet 4.6,
    /// Claude Opus 4.6+, DeepSeek V4 (anthropic-compat).
    AdaptiveThinking,
    /// `token-efficient-tools-2026-03-28` beta. Mutually
    /// exclusive with structured outputs.
    TokenEfficientTools,
    /// Server-side `tool_reference` expansion (Anthropic beta
    /// `tool-search-tool-2025-10-19`). When set, the provider's API
    /// server expands `{type:"tool_reference",tool_name:X}` content
    /// blocks into inline `<functions>...</functions>` markup before
    /// the prompt reaches the model. Lets the client keep the `tools`
    /// array constant across turns (delayed tools carry
    /// `defer_loading: true`) and emit references inside
    /// `tool_result.content` instead of growing the tools list —
    /// preserving prompt cache prefix across `ToolSearch` discoveries.
    ///
    /// Provider-specific (Anthropic-only). The multi-provider default
    /// path is client-side promotion through
    /// `ToolAppState::discovered_tool_names`, which costs a cache
    /// break on the tools array but works on every provider.
    ///
    /// Known supporting models: Claude Sonnet 4.5+, Opus 4+, GPT-5
    /// (anthropic-compat). NOT supported on Haiku 4.5 / 3.5 /
    /// older 3-series.
    ServerSideToolReference,
    /// Provider/model is known to work correctly with coco-rs's
    /// client-side `ToolSearch` promotion path (`discovered_tool_names`
    /// `AppStatePatch` + tools-array growth on the next turn).
    ///
    /// **Per-model opt-in**, default **off** for unknown models.
    /// Rationale: a model that doesn't tolerate the growing tools
    /// array (legacy proxies, local quantized models with strict
    /// schema cache, …) shouldn't be silently subjected to ToolSearch
    /// — eager-loading every tool's full schema on turn 1 is the
    /// safe degradation. Set this capability in the registry
    /// (`builtin_models_partial`) once a model has been validated.
    ///
    /// The runtime activation predicate is:
    /// ```text
    /// tool_search_active =
    ///     Feature::ToolSearch
    ///     && (ServerSideToolReference || ClientSideToolSearch)
    /// ```
    /// When **both** capabilities are absent, the model lands in the
    /// "eager-load every tool, hide ToolSearch" state regardless of
    /// the user's `Feature::ToolSearch` setting.
    ///
    /// No TS analogue — TS only ships the server-side path and
    /// blacklists incompatible models via
    /// `DEFAULT_UNSUPPORTED_MODEL_PATTERNS`. coco-rs needs a positive
    /// capability for the client-side path because it works on every
    /// Provider, so "default-on" would mis-fire on local / custom
    /// model deployments that nobody has vetted.
    ClientSideToolSearch,
}

/// How a model handles file editing / apply_patch tool.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    /// String-schema function tool (GPT-5.2+, codex models).
    #[default]
    Freeform,
    /// JSON function tool (gpt-oss).
    Function,
    /// Shell-based, prompt instructions only (GPT-5, o3, o4-mini).
    Shell,
}

/// Communication protocol (OpenAI has two APIs).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    Chat,
    Responses,
}

/// A set of capabilities for convenience.
pub type CapabilitySet = HashSet<Capability>;

#[cfg(test)]
#[path = "provider.test.rs"]
mod tests;
