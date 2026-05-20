//! Best-effort repair of malformed tool-call input JSON.
//!
//! LLMs occasionally emit tool arguments with trailing commas, unquoted
//! keys, single quotes, markdown wrappers, or unclosed
//! brackets / strings. Without repair, these become parse failures at
//! the tool boundary and the call gets dropped — even though the
//! intent is unambiguous.
//!
//! [`parse_tool_input`] is the canonical entry: it normalises empty
//! buffers to `{}` (matches the engine's existing "no
//! `ToolInputDelta` emitted" case) and delegates the actual repair to
//! [`coco_utils_json_repair`]. Single source of truth across the
//! workspace — every consumer (engine streaming dispatch, future
//! callers in `recall.rs`) sees identical repair semantics.
//!
//! **Streaming policy**: do NOT call this on a still-streaming buffer;
//! the repairer interprets `{"a":1,` as "needs closing brace" and
//! produces `{"a":1}`, which loses any pending fields the model was
//! still emitting. Always call at `ToolInputEnd` / `ToolCall` (full
//! buffer received).

use serde_json::Value;

use coco_utils_json_repair::JsonRepairError;
use coco_utils_json_repair::RepairOutcome;
use coco_utils_json_repair::parse_with_repair;

/// Outcome of a parse attempt — distinguishes a clean parse from a
/// repair-aided one so callers can record telemetry on real-world
/// hit rates.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Empty input — treated as `{}` (matches the engine's existing
    /// behavior for an empty `input_json` buffer that never received
    /// a `ToolInputDelta`).
    Empty,
    /// Strict [`serde_json::from_str`] succeeded.
    Clean,
    /// Required one or more repair passes via
    /// [`coco_utils_json_repair`].
    Repaired,
}

/// Parse a tool-input JSON string, attempting common-case repairs on
/// failure. Empty / whitespace-only input maps to an empty object.
///
/// Returns `(value, outcome)` so callers can decide whether to log a
/// `repaired = true` event.
pub fn parse_tool_input(raw: &str) -> Result<(Value, ParseOutcome), ToolInputParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok((Value::Object(Default::default()), ParseOutcome::Empty));
    }
    match parse_with_repair(trimmed) {
        Ok((v, RepairOutcome::Clean)) => Ok((v, ParseOutcome::Clean)),
        Ok((v, RepairOutcome::Repaired)) => Ok((v, ParseOutcome::Repaired)),
        // EmptyInput cannot fire — we already short-circuited above.
        // Repair / Postparse: input is genuinely malformed beyond
        // `coco_utils_json_repair`'s best-effort fix set.
        Err(JsonRepairError::EmptyInput)
        | Err(JsonRepairError::Repair(_))
        | Err(JsonRepairError::Postparse(_)) => Err(ToolInputParseError {
            raw: raw.to_string(),
        }),
    }
}

/// Tool-input parsing failed even after repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputParseError {
    /// The raw input that could not be parsed (kept for diagnostics).
    pub raw: String,
}

impl std::fmt::Display for ToolInputParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool input is not valid JSON and could not be repaired")
    }
}

impl std::error::Error for ToolInputParseError {}

#[cfg(test)]
#[path = "tool_input_repair.test.rs"]
mod tests;
