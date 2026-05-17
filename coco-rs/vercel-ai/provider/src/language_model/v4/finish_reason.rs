//! Finish reason types for model responses.
//!
//! # coco-rs extension (deviates from `@ai-sdk/provider` v4)
//!
//! The upstream TS spec defines six unified values
//! (`"stop" | "length" | "content-filter" | "tool-calls" | "error" |
//! "other"`) with provider-specific refinements (e.g. Anthropic
//! `model_context_window_exceeded`, `stop_sequence`) flowing through
//! [`FinishReason::raw`].
//!
//! coco-rs extends the enum with **two refinements as first-class
//! variants** — [`UnifiedFinishReason::ContextWindowExceeded`] and
//! [`UnifiedFinishReason::StopSequence`] — and **uses snake_case wire
//! format** (`"end_turn"`, `"max_tokens"`, `"tool_use"`,
//! `"stop_sequence"`, `"model_context_window_exceeded"`,
//! `"content_filter"`, `"error"`, `"other"`). The rename to TS-style
//! `end_turn` / `max_tokens` / `tool_use` (vs the spec's bare `stop`
//! / `length` / `tool-calls`) is intentional: coco-rs's SDK protocol
//! and transcript JSON have always used those names, and folding the
//! refinements into one enum means we have a single typed
//! `StopReason` type instead of one wrapper per layer (`vercel-ai`,
//! `coco-inference`, `coco-messages`). [`FinishReason::raw`] is still
//! preserved verbatim from the provider for diagnostics.
//!
//! `coco_inference::StopReason` and `coco_messages::StopReason` are
//! re-exports of this enum so callers across the workspace see one
//! typed value, set once at the provider-adapter seam.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Unified finish reason for a completed LLM turn.
///
/// Multi-LLM-stable: each `vercel-ai-<provider>` adapter maps its raw
/// stop_reason into one of these variants. See module docs for the
/// per-provider mapping table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFinishReason {
    /// Model finished a normal assistant turn (no tool calls, no
    /// stop sequence match). Covers Anthropic `end_turn`/`pause_turn`,
    /// OpenAI `stop` (when no stop_sequence matched), Google `STOP`
    /// (no tool calls).
    #[default]
    EndTurn,
    /// Stop sequence matched. Coco-rs refinement of the spec's
    /// `Stop` bucket — provider raw is `stop_sequence` /
    /// `"stop-sequence"`.
    StopSequence,
    /// Model invoked one or more tools and is awaiting results.
    /// Anthropic `tool_use`, OpenAI `tool_calls`, Google `STOP +
    /// has_tool_calls`.
    ToolUse,
    /// Output-token budget exhausted. Anthropic `max_tokens`, OpenAI
    /// `length`, Google `MAX_TOKENS`. Engine drives 64k escalate +
    /// multi-turn recovery (`app/query/src/engine.rs`).
    MaxTokens,
    /// Context-window limit hit. Coco-rs refinement of the
    /// `MaxTokens` bucket — provider raw is
    /// `model_context_window_exceeded`. Recovery path is shared with
    /// `MaxTokens`; the variant exists so the user-facing wording
    /// can distinguish "context window" vs "output token maximum".
    /// Wire string keeps the Anthropic-original
    /// `model_context_window_exceeded` form so transcripts that
    /// captured the raw value round-trip via the typed enum.
    #[serde(rename = "model_context_window_exceeded")]
    ContextWindowExceeded,
    /// Provider blocked / refused / safety-filtered the response.
    /// Multi-LLM unified bucket — Anthropic `refusal`, OpenAI
    /// `content_filter`, Google `SAFETY` / `RECITATION` / `IMAGE_SAFETY`.
    ContentFilter,
    /// Provider reported an error during generation. Distinct from
    /// network / HTTP / auth errors which surface via separate
    /// error channels.
    Error,
    /// Unspecified / unknown termination. Raw wire string is
    /// preserved on [`FinishReason::raw`] for diagnostics.
    Other,
}

impl UnifiedFinishReason {
    /// Normal (happy-path) termination — engine treats as
    /// end-of-turn without escalation, recovery, or synthetic
    /// api_error emission. The set: [`Self::EndTurn`],
    /// [`Self::StopSequence`], [`Self::ToolUse`].
    pub fn is_normal(self) -> bool {
        matches!(self, Self::EndTurn | Self::StopSequence | Self::ToolUse)
    }

    /// Complement of [`Self::is_normal`].
    pub fn is_abnormal(self) -> bool {
        !self.is_normal()
    }

    /// "Clean turn-end with no tool call" — matches both
    /// [`Self::EndTurn`] and [`Self::StopSequence`]. Used by the
    /// `vercel-ai-ai` text-generation loop to decide whether another
    /// turn is needed (a tool call would be a separate `ToolUse`).
    pub fn is_stop(self) -> bool {
        matches!(self, Self::EndTurn | Self::StopSequence)
    }

    /// Whether this is the content-filter / refusal bucket.
    pub fn is_content_filter(self) -> bool {
        matches!(self, Self::ContentFilter)
    }

    /// Whether this is the provider-error bucket.
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    /// Snake-case wire string. Matches the
    /// `#[serde(rename_all = "snake_case")]` representation so
    /// SDK/transcript JSON round-trips. Also the [`fmt::Display`]
    /// representation.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::StopSequence => "stop_sequence",
            Self::ToolUse => "tool_use",
            Self::MaxTokens => "max_tokens",
            Self::ContextWindowExceeded => "model_context_window_exceeded",
            Self::ContentFilter => "content_filter",
            Self::Error => "error",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for UnifiedFinishReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// The reason why a model response finished.
///
/// Contains both the typed [`UnifiedFinishReason`] (canonical for
/// behavioral matching) and the provider-original [`Self::raw`] wire
/// string (preserved for diagnostics / logs / transcript provenance).
/// `unified` is the field higher layers should match on; `raw` is
/// for humans reading logs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReason {
    /// Typed unified finish reason — set once at the
    /// provider-adapter seam.
    pub unified: UnifiedFinishReason,

    /// Provider-original raw value (e.g. Anthropic `refusal`,
    /// OpenAI `content_filter`, Google `RECITATION`). Useful for
    /// debugging / telemetry — not used for behavioral decisions
    /// (those go through [`Self::unified`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl FinishReason {
    /// Create a new finish reason with the given unified value.
    pub fn new(unified: UnifiedFinishReason) -> Self {
        Self { unified, raw: None }
    }

    /// Create a finish reason with both unified and raw values.
    pub fn with_raw(unified: UnifiedFinishReason, raw: impl Into<String>) -> Self {
        Self {
            unified,
            raw: Some(raw.into()),
        }
    }

    /// Create an `EndTurn` finish reason.
    pub fn end_turn() -> Self {
        Self::new(UnifiedFinishReason::EndTurn)
    }

    /// Create a `MaxTokens` finish reason.
    pub fn max_tokens() -> Self {
        Self::new(UnifiedFinishReason::MaxTokens)
    }

    /// Create a content filter finish reason.
    pub fn content_filter() -> Self {
        Self::new(UnifiedFinishReason::ContentFilter)
    }

    /// Create a `ToolUse` finish reason.
    pub fn tool_use() -> Self {
        Self::new(UnifiedFinishReason::ToolUse)
    }

    /// Create an error finish reason.
    pub fn error() -> Self {
        Self::new(UnifiedFinishReason::Error)
    }

    /// Create an other finish reason.
    pub fn other() -> Self {
        Self::new(UnifiedFinishReason::Other)
    }

    /// Set the raw finish reason.
    pub fn with_raw_value(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }

    /// Whether the unified reason is a normal completion.
    pub fn is_normal(&self) -> bool {
        self.unified.is_normal()
    }

    /// Whether the unified reason is abnormal.
    pub fn is_abnormal(&self) -> bool {
        self.unified.is_abnormal()
    }

    /// Pre-extension semantics: model said done normally without
    /// invoking a tool. See [`UnifiedFinishReason::is_stop`].
    pub fn is_stop(&self) -> bool {
        self.unified.is_stop()
    }

    /// Whether the unified reason is content-filter.
    pub fn is_content_filter(&self) -> bool {
        self.unified.is_content_filter()
    }

    /// Whether the unified reason is error.
    pub fn is_error(&self) -> bool {
        self.unified.is_error()
    }
}

impl From<UnifiedFinishReason> for FinishReason {
    fn from(unified: UnifiedFinishReason) -> Self {
        Self::new(unified)
    }
}

#[cfg(test)]
#[path = "finish_reason.test.rs"]
mod tests;
