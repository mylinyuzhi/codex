//! Generic post-turn / side-channel forked-agent helper.
//!
//! TS source: `utils/forkedAgent.ts::runForkedAgent` + the 8 callers
//! that consume it (`utils/sideQuestion.ts`, `services/{compact,
//! PromptSuggestion/{promptSuggestion,speculation}, extractMemories,
//! SessionMemory, AgentSummary, autoDream}`). All callers funnel
//! through the same `query()` engine via `runForkedAgent`; coco-rs
//! routes them through this trait + dispatcher.
//!
//! ## The 9 fork variants (per [`coco_types::ForkLabel`])
//!
//! | Label | Purpose | canUseTool policy |
//! |---|---|---|
//! | `PromptSuggestion` | predict next user prompt | deny-all |
//! | `SideQuestion` | `/btw` answer-once side query | deny-all |
//! | `Compact` | `/compact` summarizer | deny-all |
//! | `ExtractMemories` | post-turn memory write | auto-mem (Read/Glob/Grep + read-only Bash + Edit/Write within memory_dir) |
//! | `SessionMemoryAuto` | auto session-memory rebuild | session-mem (Edit only on exact path) |
//! | `SessionMemoryManual` | `/summary` slash | session-mem |
//! | `AgentSummary` | 30s subagent progress snapshot | deny-all |
//! | `AutoDream` | KAIROS long-term consolidation | auto-mem with broader memory_root |
//! | `Speculation` | pre-execute prompt suggestion | 3-boundary (overlay) |
//!
//! ## Cache-parity contract
//!
//! [`ForkedAgentOptions::for_label`] returns the conservative shape that
//! preserves the parent's prompt cache:
//!
//! - `max_turns: Some(1)` — single round-trip
//! - `skip_transcript: true` — no sidechain noise in the parent's
//!   transcript
//! - `skip_cache_write: true` — fire-and-forget; don't pollute the
//!   shared cache with this branch
//! - `effort: None` — leaves thinking config untouched (TS PR #18143
//!   incident: setting `effort: 'low'` on prompt-suggestion forks
//!   collapsed cache hit rate from 92.7% → 61% by changing
//!   `budget_tokens` and busting the cache key)
//! - `max_output_tokens: None` — same cache-bust risk; only set when
//!   parity is *not* a goal (e.g. compact's distinct model)
//!
//! Override these only when cache parity isn't a goal.

use std::sync::Arc;

use coco_messages::Message;
use coco_tool_runtime::AgentQueryConfig;
use coco_types::CacheSafeParams;
use coco_types::ForkLabel;
use coco_types::TokenUsage;
use tokio_util::sync::CancellationToken;

// Re-export the canUseTool primitives from coco-tool-runtime so fork
// callers can build options without importing the executor crate
// directly.
pub use coco_tool_runtime::CanUseToolCallContext;
pub use coco_tool_runtime::CanUseToolDecision;
pub use coco_tool_runtime::CanUseToolHandle;
pub use coco_tool_runtime::CanUseToolHandleRef;
pub use coco_tool_runtime::DecisionReason;
pub use coco_tool_runtime::NoOpCanUseToolHandle;
pub use coco_tool_runtime::deny_all_handle;

/// Streaming hook fired for each [`Message`] the fork emits during
/// its agent loop. Used by speculation + auto-dream to update the
/// progress UI / append to a per-task ledger live (instead of waiting
/// for the fork to finish).
///
/// TS: `utils/forkedAgent.ts::runForkedAgent({onMessage})`.
pub type OnMessageCallback = Arc<dyn Fn(&Message) + Send + Sync>;

/// Caller-supplied isolation overrides — see TS
/// `utils/forkedAgent.ts::createSubagentContext::overrides`.
///
/// All fields are optional; `None` means "use the engine's default".
#[derive(Default, Clone)]
pub struct ForkedAgentOverrides {
    /// Cancellation token to install on the fork's engine. When
    /// `None`, dispatcher allocates a fresh token. Speculation /
    /// compact thread their own so user `Esc` aborts the fork.
    pub abort: Option<CancellationToken>,
    /// Pre-cloned `FileReadState` to install on the fork's
    /// `ToolUseContext`. When `None`, dispatcher clones from the
    /// parent at fork time. Pre-supplied is faster when the caller
    /// already has a clone in scope (sessionMemory does).
    pub file_read_state: Option<Arc<tokio::sync::RwLock<coco_context::FileReadState>>>,
}

impl std::fmt::Debug for ForkedAgentOverrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForkedAgentOverrides")
            .field("abort", &self.abort.is_some())
            .field("file_read_state", &self.file_read_state.is_some())
            .finish()
    }
}

/// Runtime knobs for a forked-agent invocation.
///
/// **No `Default` impl** — every fork must declare its [`ForkLabel`]
/// explicitly so telemetry never sees an unattributed `"fork"` string.
/// Use [`ForkedAgentOptions::for_label`] to start from the cache-safe
/// defaults and customize from there.
#[derive(Clone)]
pub struct ForkedAgentOptions {
    /// Hard cap on turns. `Some(1)` is the standard "one-shot" shape.
    pub max_turns: Option<i32>,
    /// `true` ⇒ fork's history doesn't enter the parent's transcript
    /// store. Default for ephemeral / fire-and-forget side queries.
    pub skip_transcript: bool,
    /// `true` ⇒ the fork's API request asks the provider not to
    /// write a fresh prompt-cache entry on the last message.
    pub skip_cache_write: bool,
    /// Optional reasoning-effort override. **Setting this busts cache
    /// parity** for older models that don't have adaptive thinking.
    /// Default `None` preserves the parent's cache key.
    pub effort: Option<String>,
    /// Hard cap on output tokens. **WARNING**: setting this clamps
    /// `budget_tokens`, breaking parent prompt-cache parity. PR #18143
    /// incident: 92.7% → 61% hit-rate. Only set when cache parity is
    /// not a goal (e.g. compact's distinct model). The inference
    /// layer logs `tracing::warn!` when this is `Some`.
    pub max_output_tokens: Option<i64>,
    /// Telemetry-only string for cache-break attribution. Defaults to
    /// `fork_label.as_str()` via [`Self::for_label`].
    pub query_source: String,
    /// Typed fork discriminator. Required — tells telemetry / log
    /// readers which fork variant fired without grepping callsites.
    pub fork_label: ForkLabel,
    /// Per-fork tool-execution gate. `Some` installs the callback at
    /// `ToolUseContext.can_use_tool` so the app/query preparer runs it
    /// before the static permission evaluator consults the tool's
    /// built-in `check_permissions`. The six policies (deny-all /
    /// auto-mem / session-mem / speculation 3-boundary) live in their
    /// respective subsystems; this module just threads the handle
    /// through. TS: `runForkedAgent({canUseTool})`.
    pub can_use_tool: Option<CanUseToolHandleRef>,
    /// When `true`, hook auto-approve cannot bypass [`Self::can_use_tool`].
    /// Speculation needs this so overlay path-rewrites always run
    /// regardless of hook config. TS: `requireCanUseTool`.
    pub require_can_use_tool: bool,
    /// Streaming message callback. Fires once per message the fork
    /// emits — live progress for speculation + auto-dream UIs.
    pub on_message: Option<OnMessageCallback>,
    /// Optional state overrides — see [`ForkedAgentOverrides`].
    pub overrides: ForkedAgentOverrides,
}

impl std::fmt::Debug for ForkedAgentOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForkedAgentOptions")
            .field("max_turns", &self.max_turns)
            .field("skip_transcript", &self.skip_transcript)
            .field("skip_cache_write", &self.skip_cache_write)
            .field("effort", &self.effort)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("query_source", &self.query_source)
            .field("fork_label", &self.fork_label)
            .field("can_use_tool_set", &self.can_use_tool.is_some())
            .field("require_can_use_tool", &self.require_can_use_tool)
            .field("on_message_set", &self.on_message.is_some())
            .field("overrides", &self.overrides)
            .finish()
    }
}

impl ForkedAgentOptions {
    /// Build options with the cache-parity-safe defaults for `label`.
    ///
    /// Defaults: `max_turns=Some(1)`, `skip_transcript=true`,
    /// `skip_cache_write=true`, `effort=None`, `max_output_tokens=None`,
    /// `can_use_tool=None`, `require_can_use_tool=false`.
    /// `query_source` defaults to `label.as_str()` so telemetry
    /// strings stay aligned with the typed enum.
    pub fn for_label(label: ForkLabel) -> Self {
        Self {
            max_turns: Some(1),
            skip_transcript: true,
            skip_cache_write: true,
            effort: None,
            max_output_tokens: None,
            query_source: label.as_str().to_string(),
            fork_label: label,
            can_use_tool: None,
            require_can_use_tool: false,
            on_message: None,
            overrides: ForkedAgentOverrides::default(),
        }
    }
}

/// Build a one-shot [`AgentQueryConfig`] from cached parent params +
/// caller-provided options.
///
/// The returned config's `model` matches the parent's, `system_prompt`
/// matches the parent's pre-rendered bytes, `fork_context_messages`
/// carries the parent's serialized post-turn history, and the
/// canUseTool / fork_label / max_output_tokens fields are threaded
/// through so the engine sees the per-fork policy.
pub fn build_query_config(
    cache: &CacheSafeParams,
    options: &ForkedAgentOptions,
) -> AgentQueryConfig {
    let mut prompt_cache = cache.prompt_cache.clone();
    if let Some(cfg) = prompt_cache.as_mut() {
        cfg.skip_cache_write = options.skip_cache_write;
    }
    AgentQueryConfig {
        system_prompt: cache.rendered_system_prompt.clone(),
        model: cache.model_id.clone(),
        max_turns: options.max_turns,
        prompt_cache,
        // Inherit the parent's history verbatim so the API request's
        // prefix bytes match — this is what enables cache sharing.
        fork_context_messages: cache.fork_context_messages.clone(),
        allowed_tools: Vec::new(),
        disallowed_tools: Vec::new(),
        extra_allow_rules: Vec::new(),
        effort: options.effort.clone(),
        // Per-fork policy: thread can_use_tool / fork_label /
        // max_output_tokens onto the child engine config so the
        // engine builder reflects them on QueryEngineConfig and
        // ToolUseContext. Empty when not set — preserves
        // pre-canUseTool behavior for callers that haven't migrated.
        can_use_tool: options.can_use_tool.clone(),
        require_can_use_tool: options.require_can_use_tool,
        fork_label: Some(options.fork_label),
        max_output_tokens_override: options.max_output_tokens,
        ..Default::default()
    }
}

/// Result of a [`ForkDispatcher::dispatch`] call.
///
/// Carries the full message list (TS parity: TS callers walk
/// `result.messages` to find the first non-empty assistant text
/// block — model may go "tool→denied→text" across two turns when
/// canUseTool denies). Numeric usage fields are surfaced for
/// telemetry callers; callers that only want the answer can ignore
/// them.
///
/// TS: `utils/forkedAgent.ts::ForkedAgentResult`.
#[derive(Debug, Clone, Default)]
pub struct ForkedAgentResult {
    /// Every assistant + user message produced during the fork (in
    /// emission order). Empty when the fork errored before
    /// producing any output.
    pub messages: Vec<Message>,
    /// Accumulated token usage across the fork's turns.
    pub total_usage: TokenUsage,
    /// Stop reason from the model on the last turn (e.g. `end_turn`,
    /// `tool_use`, `max_tokens`). `None` when the fork errored or
    /// was cancelled.
    pub stop_reason: Option<String>,
}

/// Async trait for dispatching a one-shot forked query.
///
/// Implementations capture whatever they need to build a fresh
/// [`coco_query::QueryEngine`] (typically `Arc<SessionRuntime>` in
/// the CLI) and drive a single turn against it. The parent engine's
/// history is *not* mutated — that's the whole point of forking.
///
/// TS reference: `utils/forkedAgent.ts::runForkedAgent`. 8 callers
/// route through the same TS function; the trait gives Rust callers
/// the same single seam.
#[async_trait::async_trait]
pub trait ForkDispatcher: Send + Sync {
    /// Run a forked query.
    ///
    /// `cache` is the parent's [`CacheSafeParams`] (typically read
    /// from `QueryEngine::last_cache_safe_params`). `prompt` is the
    /// new user message to append after the cached parent history.
    /// `system_prompt_override` lets non-cache-sharing callers
    /// substitute a different system prompt; cache-sharing callers
    /// should pass `None` so `cache.rendered_system_prompt` is used.
    async fn dispatch(
        &self,
        cache: &CacheSafeParams,
        options: &ForkedAgentOptions,
        prompt: &str,
        system_prompt_override: Option<String>,
    ) -> Result<ForkedAgentResult, coco_error::BoxedError>;
}

/// Convenience reference type matching the rest of the engine's
/// trait-object slots.
pub type ForkDispatcherRef = Arc<dyn ForkDispatcher>;

#[cfg(test)]
#[path = "forked_agent.test.rs"]
mod tests;
