//! Tool-call input parsing callback.
//!
//! When an LLM emits a tool call, the wire layer carries the
//! arguments as a (stringified) JSON object. Adapters historically
//! parsed it inline with `serde_json::from_str(...).unwrap_or(Value::Null)`,
//! which silently swallows malformed JSON. This module defines an
//! optional caller-supplied parser that adapters consult before the
//! built-in path:
//!
//! - **Caller wires in a repair-capable parser** (e.g. backed by
//!   `llm_json` / `jsonrepair`) via
//!   [`LanguageModelV4CallOptions::tool_input_parse_fn`].
//!   On `Ok(ToolInputParseResult)` the adapter uses the value; a
//!   `was_repaired: true` flag tells the adapter to emit a `warn!`
//!   log so dashboards can monitor repair frequency.
//! - **Caller does not wire one** → the adapter falls back to its
//!   built-in strict `serde_json::from_str` path and marks any failure
//!   `invalid: true` on the resulting
//!   [`crate::ToolCallPart`].
//!
//! In both failure paths the adapter must propagate failure via
//! `ToolCallPart.invalid = true`, never silently fall back to
//! `Value::Null`. Caller layers (agent loop, side queries) read the
//! flag and decide how to surface it — typically by pushing a
//! synthetic tool_result back to the LLM with an explanatory error
//! message so the model can self-correct on the next turn.
//!
//! # Relationship to [`ToolCallRepairFunction`](crate)
//!
//! Two parser/repair seams cooperate in the SDK:
//!
//! - **`ToolInputParseFunction` (this module, sync, adapter-layer)**:
//!   fires INSIDE `do_generate` / `do_stream` at the raw-arguments
//!   boundary, **before** the adapter constructs `ToolCallPart`.
//!   Purpose: local string-level repair (markdown fence strip,
//!   trailing comma, missing bracket, …). Built-in implementations
//!   use libraries like `llm_json`.
//! - **`ToolCallRepairFunction` (in `vercel-ai` SDK, async,
//!   post-parse)**: fires AFTER the adapter returned a parsed (or
//!   invalid) `ToolCall`, when SDK-side schema validation rejects
//!   it. Typical implementation re-prompts the LLM to fix its own
//!   output.
//!
//! Both are optional callbacks the caller wires in. They are
//! complementary — one repairs JSON syntax, the other repairs
//! semantic mismatches with the tool schema. The same `Arc<dyn ...>`
//! ownership / `CustomXxxFunction` adapter pattern is used in both
//! to keep the SDK ergonomics consistent.

use std::sync::Arc;

use serde_json::Value;

/// A function that parses (and optionally repairs) the raw stringified
/// JSON arguments of a tool call before the adapter materialises them
/// onto a [`crate::ToolCallPart`].
///
/// Sync intentionally: JSON repair is CPU-bound; an async signature
/// would add wake/poll overhead on a hot path that runs once per
/// tool call. See module docs for the relationship to the async
/// `ToolCallRepairFunction` post-parse seam.
///
/// `Debug` is a supertrait so that
/// [`LanguageModelV4CallOptions`](crate::LanguageModelV4CallOptions) —
/// which contains `Option<Arc<dyn ToolInputParseFunction>>` and
/// derives `Debug` — keeps its derive. Implementations either
/// derive `Debug` directly (struct-shaped parsers) or provide a
/// `finish_non_exhaustive`-style manual impl (closure-shaped
/// parsers — see [`CustomToolInputParseFunction`]).
pub trait ToolInputParseFunction: std::fmt::Debug + Send + Sync {
    /// Parse `raw` (the stringified tool-call arguments JSON) into a
    /// [`Value`]. Implementations that successfully repaired the
    /// input on the way to `Ok` should set
    /// [`ToolInputParseResult::was_repaired`] so the adapter can
    /// emit telemetry.
    fn parse(&self, raw: &str) -> Result<ToolInputParseResult, ToolInputParseError>;
}

/// Successful parse outcome plus a repair-fired flag.
///
/// Adapters that see `was_repaired = true` emit a `warn!` log
/// carrying the tool name and the original raw bytes so operators
/// can monitor repair-rate per (provider, model, tool) tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputParseResult {
    pub value: Value,
    /// `true` when the parser had to repair the input (strict
    /// [`serde_json::from_str`] would have failed). Drives adapter
    /// telemetry. `false` for clean parses.
    pub was_repaired: bool,
}

impl ToolInputParseResult {
    /// Constructor for a clean parse.
    pub fn clean(value: Value) -> Self {
        Self {
            value,
            was_repaired: false,
        }
    }

    /// Constructor for a repair-assisted parse.
    pub fn repaired(value: Value) -> Self {
        Self {
            value,
            was_repaired: true,
        }
    }
}

/// Reasons a tool-input parse failed even after repair.
///
/// Adapters that receive `Err(_)` from
/// [`ToolInputParseFunction::parse`] surface the failure as
/// `ToolCallPart.invalid = true` with `input = Value::Null`, plus a
/// `warn!` log. Caller layers (agent loop, side queries) read the
/// flag and emit a synthetic tool_result back to the LLM so the
/// model can correct on the next turn.
#[derive(Debug, thiserror::Error)]
pub enum ToolInputParseError {
    /// Strict JSON parse failed and no repair was attempted. Body
    /// is the underlying [`serde_json`] error rendered to string.
    #[error("strict JSON parse failed: {0}")]
    Parse(String),
    /// Repair was attempted but did not produce parseable output.
    /// Body is the repair library's error or a synthetic message
    /// from the wrapping caller.
    #[error("repair failed: {0}")]
    Repair(String),
}

/// Shared-ownership handle adapters store on
/// [`LanguageModelV4CallOptions`](crate::LanguageModelV4CallOptions).
pub type ToolInputParseHandle = Arc<dyn ToolInputParseFunction>;

/// Adapter that lifts an arbitrary `Fn(&str) -> Result<...>` closure
/// into a [`ToolInputParseFunction`]. Mirrors the
/// `CustomRepairFunction` shape used by the SDK-level
/// `ToolCallRepairFunction` so the two seams expose the same
/// caller-side ergonomics.
pub struct CustomToolInputParseFunction<F>
where
    F: Fn(&str) -> Result<ToolInputParseResult, ToolInputParseError> + Send + Sync,
{
    parse_fn: F,
}

impl<F> CustomToolInputParseFunction<F>
where
    F: Fn(&str) -> Result<ToolInputParseResult, ToolInputParseError> + Send + Sync,
{
    /// Create a custom parser from a closure.
    pub fn new(parse_fn: F) -> Self {
        Self { parse_fn }
    }
}

impl<F> ToolInputParseFunction for CustomToolInputParseFunction<F>
where
    F: Fn(&str) -> Result<ToolInputParseResult, ToolInputParseError> + Send + Sync,
{
    fn parse(&self, raw: &str) -> Result<ToolInputParseResult, ToolInputParseError> {
        (self.parse_fn)(raw)
    }
}

impl<F> std::fmt::Debug for CustomToolInputParseFunction<F>
where
    F: Fn(&str) -> Result<ToolInputParseResult, ToolInputParseError> + Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Closures don't implement `Debug`; show the wrapper type
        // alone so call-site `{:?}` panics don't leak through.
        f.debug_struct("CustomToolInputParseFunction")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "tool_input_parse.test.rs"]
mod tests;
