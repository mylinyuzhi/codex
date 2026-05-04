//! Best-effort repair of malformed tool-call input JSON.
//!
//! LLMs occasionally emit tool arguments with trailing commas, unquoted
//! keys, or unclosed brackets / strings. Without repair, these become
//! parse failures at the tool boundary and the call gets dropped — even
//! though the intent is unambiguous.
//!
//! [`parse_tool_input`] is the canonical entry: tries strict parse
//! first (fast path), falls back to [`try_fix_json`] which runs four
//! string-level repair passes mirroring the strategies in
//! `vercel-ai/ai/src/generate_text/tool_call_repair.rs`. Each pass is
//! a pure character-stream transform — no JSON parser dependency, no
//! regex, no schema knowledge.
//!
//! The repair fixers are in this crate (not in `vercel-ai`) on
//! purpose: tool-input parsing is a tool-runtime concern, not an
//! AI-loop concern. `vercel-ai`'s `repair_tool_call` machinery exists
//! for the case where a multi-step loop wants to ask the LLM to
//! self-correct; this module is the smaller, always-on first-line
//! defense that runs before any execution.

use serde_json::Value;

/// Outcome of a parse attempt — distinguishes a clean parse from a
/// repair-aided one so callers can record telemetry on real-world
/// hit rates.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Empty input — treated as `{}` (matches the engine's existing
    /// behavior for an empty `input_json` buffer).
    Empty,
    /// Parsed cleanly via `serde_json::from_str`.
    Clean,
    /// Parsed only after repair — `repaired_with` names the fixer that
    /// won (for telemetry / debugging).
    Repaired { repaired_with: RepairKind },
}

/// Which repair pass succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairKind {
    UnquotedKeys,
    TrailingCommas,
    MissingBrackets,
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
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok((v, ParseOutcome::Clean));
    }
    if let Some((v, kind)) = try_fix_json(trimmed) {
        return Ok((
            v,
            ParseOutcome::Repaired {
                repaired_with: kind,
            },
        ));
    }
    Err(ToolInputParseError {
        raw: raw.to_string(),
    })
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

/// Try to fix common JSON issues. Returns the parsed value plus the
/// repair pass that succeeded.
fn try_fix_json(raw: &str) -> Option<(Value, RepairKind)> {
    let fixed = fix_unquoted_keys(raw);
    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
        return Some((v, RepairKind::UnquotedKeys));
    }

    let fixed = fix_trailing_commas(raw);
    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
        return Some((v, RepairKind::TrailingCommas));
    }

    let fixed = fix_missing_brackets(raw);
    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
        return Some((v, RepairKind::MissingBrackets));
    }

    None
}

/// Add `"` around unquoted object keys after `{` / `,`. Single-pass
/// character iterator; no JSON parser dependency.
fn fix_unquoted_keys(json: &str) -> String {
    let mut result = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' || c == ',' {
            result.push(c);
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some(&next) = chars.peek()
                && next != '"'
                && next.is_alphabetic()
            {
                result.push('"');
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '_' {
                        if let Some(c) = chars.next() {
                            result.push(c);
                        }
                    } else {
                        break;
                    }
                }
                result.push('"');
                continue;
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Drop a `,` that is followed (modulo whitespace) by `]` or `}`.
fn fix_trailing_commas(json: &str) -> String {
    let mut result = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();

    while let Some(c) = chars.next() {
        if c == ',' {
            let mut whitespace = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    if let Some(c) = chars.next() {
                        whitespace.push(c);
                    }
                } else {
                    break;
                }
            }
            if let Some(&next) = chars.peek()
                && (next == ']' || next == '}')
            {
                // Drop the comma; keep the whitespace so error column
                // numbers in the parser don't shift.
                result.push_str(&whitespace);
                continue;
            }
            result.push(c);
            result.push_str(&whitespace);
        } else {
            result.push(c);
        }
    }

    result
}

/// Close any unbalanced strings / `[` / `{`. Useful when the model
/// truncates mid-arguments.
fn fix_missing_brackets(json: &str) -> String {
    let mut result = json.to_string();
    let mut open_braces: i32 = 0;
    let mut open_brackets: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for c in json.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match c {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => open_braces += 1,
            '}' if !in_string => open_braces -= 1,
            '[' if !in_string => open_brackets += 1,
            ']' if !in_string => open_brackets -= 1,
            _ => {}
        }
    }

    if in_string {
        result.push('"');
    }
    for _ in 0..open_brackets {
        result.push(']');
    }
    for _ in 0..open_braces {
        result.push('}');
    }
    result
}

#[cfg(test)]
#[path = "tool_input_repair.test.rs"]
mod tests;
