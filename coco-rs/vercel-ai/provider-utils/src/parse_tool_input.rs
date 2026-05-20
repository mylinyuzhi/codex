//! Adapter-side helper that bridges raw tool-call `arguments` strings
//! into `ToolCallPart`-friendly `(Value, invalid_flag)` pairs.
//!
//! Replaces the historical
//! `serde_json::from_str(args).unwrap_or(Value::Null)` pattern with:
//!
//! 1. Empty / whitespace-only input → `(Object({}), false)`. Some
//!    providers emit a tool call with no input deltas for
//!    parameterless tools; treat as empty arguments, not an error.
//! 2. Caller-supplied
//!    [`ToolInputParseFunction`](vercel_ai_provider::ToolInputParseFunction)
//!    when present (wired by `coco-inference::build_call_options`).
//!    Repair-assisted parses emit a `warn!` log so dashboards can
//!    monitor real-world repair frequency.
//! 3. Strict [`serde_json`] fallback when no callback wired.
//! 4. On failure (either path) → `(Value::Null, true)` with a
//!    `warn!` carrying the raw bytes. Caller adapters surface this
//!    via `ToolCallPart.invalid = true`; higher layers (agent loop,
//!    side queries) push a synthetic `tool_result` back to the LLM
//!    explaining the parse failure so the model can self-correct.
//!
//! TS parity: matches the `invalid: true` semantics on
//! `@ai-sdk/ai` `TypedToolCall`
//! (`packages/ai/src/generate-text/parse-tool-call.ts:97-117`).

use serde_json::Value;
use vercel_ai_provider::ToolInputParseHandle;

/// Parse outcome — caller writes the pair onto
/// [`vercel_ai_provider::ToolCallPart`].
#[derive(Debug, Clone)]
pub struct ParsedToolInput {
    pub value: Value,
    /// `true` when parse + repair both failed. Caller sets
    /// `ToolCallPart.invalid = invalid`.
    pub invalid: bool,
}

impl ParsedToolInput {
    fn ok(value: Value) -> Self {
        Self {
            value,
            invalid: false,
        }
    }

    fn failed() -> Self {
        Self {
            value: Value::Null,
            invalid: true,
        }
    }
}

/// Apply the caller-supplied parse function if present; otherwise
/// strict [`serde_json`]. Returns the [`ParsedToolInput`] the
/// adapter writes onto its emitted `ToolCallPart`.
///
/// `tool_name` is used solely for the `warn!` log emitted on
/// repair-assisted parses and on failure; pass the resolved tool
/// name from the provider's wire payload.
pub fn parse_tool_call_arguments(
    raw: &str,
    parser: Option<&ToolInputParseHandle>,
    tool_name: &str,
) -> ParsedToolInput {
    // Empty buffer convention: some providers omit `ToolInputDelta`
    // entirely when the model invokes a parameterless tool. The
    // adapter sees `arguments = ""` and we treat that as an empty
    // arguments object — NOT a parse failure.
    if raw.trim().is_empty() {
        return ParsedToolInput::ok(Value::Object(Default::default()));
    }
    if let Some(parser) = parser {
        match parser.parse(raw) {
            Ok(result) => {
                if result.was_repaired {
                    tracing::warn!(
                        target: "vercel_ai::tool_call",
                        tool_name,
                        args_bytes = raw.len(),
                        "tool-call arguments JSON required repair before parse"
                    );
                }
                ParsedToolInput::ok(result.value)
            }
            Err(err) => {
                tracing::warn!(
                    target: "vercel_ai::tool_call",
                    tool_name,
                    args_bytes = raw.len(),
                    error = %err,
                    raw_args = %raw,
                    "tool-call arguments parse failed even after repair; \
                     marking ToolCallPart.invalid = true"
                );
                ParsedToolInput::failed()
            }
        }
    } else {
        match serde_json::from_str::<Value>(raw) {
            Ok(v) => ParsedToolInput::ok(v),
            Err(err) => {
                tracing::warn!(
                    target: "vercel_ai::tool_call",
                    tool_name,
                    args_bytes = raw.len(),
                    error = %err,
                    raw_args = %raw,
                    "strict JSON parse failed and no repair callback is wired; \
                     marking ToolCallPart.invalid = true"
                );
                ParsedToolInput::failed()
            }
        }
    }
}

#[cfg(test)]
#[path = "parse_tool_input.test.rs"]
mod tests;
