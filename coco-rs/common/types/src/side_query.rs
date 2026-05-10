//! Side-query types — data types for LLM side-queries.
//!
//! TS: utils/sideQuery.ts (SideQueryOptions, response types)
//!
//! These are pure data types (no async). The async `SideQuery` trait
//! that uses these types lives in `coco-tool-runtime` (which has async-trait).
//! This split lets both `coco-permissions` and `coco-tool-runtime` share the
//! same request/response types without circular dependencies.

use crate::ModelRole;
use serde::Deserialize;
use serde::Serialize;

// ── Request ──

/// A side-query request to the LLM.
///
/// Deliberately matches the TS `SideQueryOptions` common denominator.
/// Provider-specific details (beta headers, cache control, attribution)
/// are handled by the implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryRequest {
    /// Model to use. If `None`, uses the implementation's default.
    pub model: Option<String>,

    /// Which role to resolve. Wins over `model` when set: the
    /// implementation looks up `ModelRoles::get(role)` and runs the
    /// query against that resolved provider+model. Lets memory recall
    /// say "use the Memory role" without ever hardcoding a model
    /// string. `None` falls back to `model` (then to the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_role: Option<ModelRole>,

    /// System prompt.
    pub system: String,

    /// Conversation messages.
    pub messages: Vec<SideQueryMessage>,

    /// Tool definitions for structured output.
    pub tools: Vec<SideQueryToolDef>,

    /// Force the LLM to call a specific tool (by name).
    /// Corresponds to `tool_choice: { type: "tool", name: "..." }`.
    pub forced_tool: Option<String>,

    /// Max output tokens (default: 1024).
    pub max_tokens: Option<i32>,

    /// Temperature override.
    pub temperature: Option<f64>,

    /// Thinking budget tokens. `None` = no thinking.
    pub thinking_budget: Option<i32>,

    /// Custom stop sequences.
    pub stop_sequences: Vec<String>,

    /// Skip the CLI system prompt prefix (for internal classifiers).
    pub skip_system_prefix: bool,

    /// Source label for telemetry (e.g. "permission_explainer", "auto_mode").
    pub query_source: String,
}

/// A message in a side-query conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryMessage {
    pub role: SideQueryRole,
    pub content: String,
}

/// Role in a side-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueryRole {
    User,
    Assistant,
}

/// A tool definition for structured output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// ── Response ──

/// Response from a side-query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryResponse {
    /// Text content blocks concatenated.
    pub text: Option<String>,

    /// Tool use blocks from the response.
    pub tool_uses: Vec<SideQueryToolUse>,

    /// Stop reason.
    pub stop_reason: SideQueryStopReason,

    /// Token usage.
    pub usage: SideQueryUsage,

    /// Which model actually served the request.
    pub model_used: String,
}

/// A tool use block in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideQueryToolUse {
    pub name: String,
    pub input: serde_json::Value,
}

/// Why the LLM stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueryStopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    /// Unknown or provider-specific reason.
    Other(String),
}

/// Token usage from a side-query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SideQueryUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

// ── Convenience constructors ──

impl SideQueryRequest {
    /// Simple single-turn text query.
    pub fn simple(system: &str, user_prompt: &str, query_source: &str) -> Self {
        Self {
            model: None,
            model_role: None,
            system: system.to_string(),
            messages: vec![SideQueryMessage {
                role: SideQueryRole::User,
                content: user_prompt.to_string(),
            }],
            tools: Vec::new(),
            forced_tool: None,
            max_tokens: None,
            temperature: None,
            thinking_budget: None,
            stop_sequences: Vec::new(),
            skip_system_prefix: false,
            query_source: query_source.to_string(),
        }
    }

    /// Query with forced tool use (structured output).
    pub fn with_forced_tool(
        system: &str,
        user_prompt: &str,
        tool: SideQueryToolDef,
        query_source: &str,
    ) -> Self {
        let tool_name = tool.name.clone();
        Self {
            model: None,
            model_role: None,
            system: system.to_string(),
            messages: vec![SideQueryMessage {
                role: SideQueryRole::User,
                content: user_prompt.to_string(),
            }],
            tools: vec![tool],
            forced_tool: Some(tool_name),
            max_tokens: None,
            temperature: None,
            thinking_budget: None,
            stop_sequences: Vec::new(),
            skip_system_prefix: false,
            query_source: query_source.to_string(),
        }
    }

    /// Builder: pin this side-query to a specific [`ModelRole`].
    /// Wins over any later `model =` setting at request time.
    #[must_use]
    pub fn with_model_role(mut self, role: ModelRole) -> Self {
        self.model_role = Some(role);
        self
    }
}

impl SideQueryResponse {
    /// Get the first tool use input, if any.
    pub fn first_tool_input(&self) -> Option<&serde_json::Value> {
        self.tool_uses.first().map(|tu| &tu.input)
    }
}

// ── Post-turn cache-safe params (D8) ──

/// Parameters that must be **byte-identical** between the parent
/// session's last turn and a post-turn fork's first request to share
/// the parent's prompt cache.
///
/// TS: `utils/forkedAgent.ts::CacheSafeParams` + module-level
/// `lastCacheSafeParams` slot, written by `handleStopHooks` after each
/// turn and read by `runForkedAgent` callers (`/btw`,
/// `promptSuggestion`, `postTurnSummary`).
///
/// **Coco-rs scope**: this is the cross-layer DTO. The slot itself
/// lives on `coco_query::QueryEngine` (`last_cache_safe_params:
/// Arc<RwLock<Option<CacheSafeParams>>>`) populated in
/// `finalize_turn_post_tools`. Cleared on `/clear`. Post-turn fork
/// features (none ship in coco-rs today — see
/// `docs/coco-rs/agentteam-architecture.md` "Deferred design
/// decisions") will read it via `engine.last_cache_safe_params()`.
///
/// **Cache-key fields included here**: rendered system prompt, model
/// id, parent message history. **Excluded**: the live `ToolUseContext`
/// (non-serializable) — fork callers reconstruct it; tool schema
/// changes invalidate the cache regardless. **Also excluded**:
/// thinking config — derived per-call from the inherited
/// `ThinkingLevel`; setting `max_output_tokens` on a fork can clamp
/// `budget_tokens` and silently break cache parity (TS callers must
/// avoid that combination, ditto for coco-rs).
///
/// All fields are owned strings / values so the slot can be safely
/// cloned without lifetime entanglement with the parent's per-turn
/// state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheSafeParams {
    /// Pre-rendered system prompt bytes — must match the parent's
    /// last request verbatim. Mirrors the same threading used by
    /// `SpawnMode::Fork`'s `rendered_system_prompt`.
    pub rendered_system_prompt: String,
    /// Resolved model id at the time of the parent turn. Cache keys
    /// are scoped per `(provider, model)` — a fork that targets a
    /// different model will simply miss the cache.
    pub model_id: String,
    /// Provider instance name that served the parent turn. Captured
    /// alongside `model_id` so post-turn forks can perform
    /// **fast-mode-aware** rate-limit selectivity:
    /// `prompt_suggestion::build_suggestion_context` reads
    /// `app_state.rate_limits.get(&cache.provider)` to decide whether
    /// to suppress, so a 429 on a *different* provider doesn't
    /// silence suggestions when the fork's actual provider is healthy.
    /// `#[serde(default)]` for backward compat with on-disk session
    /// formats that pre-date Phase 7 — empty string means "unknown
    /// provider" (selective check fails closed → no suppression).
    #[serde(default)]
    pub provider: String,
    /// Parent message history that should prefix the fork's prompt.
    /// Carried as serialized JSON so this DTO crosses layer
    /// boundaries without pulling `coco-messages` into `coco-types`.
    /// Same shape as `AgentQueryConfig.fork_context_messages`.
    #[serde(default)]
    pub fork_context_messages: Vec<serde_json::Value>,
}

#[cfg(test)]
#[path = "side_query.test.rs"]
mod tests;
