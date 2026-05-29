//! schema validation: tool input normalization + schema validation.
//!
//! Sits between the provider-adapter-driven wire parsing (which parses raw
//! `arguments` strings into [`coco_llm_types::ToolCallPart::input`]
//! values, falling back to `{}` on parse failure) and the per-tool
//! execution gate in [`tool_call_preparer`].
//!
//! Mirrors TS Claude Code:
//! - [`normalize_value_string`] → `utils/messages.ts:2676-2697`
//!   (recursive `Value::String` parse for providers that occasionally
//!   nest stringified JSON inside the tool-call `input` field).
//! - [`validate_tool_call`] → `services/tools/toolExecution.ts:614-680`
//!   (Zod schema check + NoSuchTool fallback) but **without** the
//!   schema-not-sent hint — the deferred-tool registry isn't ported
//!   yet.
//! - [`format_schema_error`] → `utils/toolErrors.ts:66-130`
//!   (`formatZodValidationError`).
//!
//! Failure paths populate
//! [`coco_llm_types::ToolInputInvalidReason`] so error wrap
//! ([`tool_call_preparer::prepare_one_pending_tool_call`]) can pick
//! the right `<tool_use_error>` wrap prefix without string-matching.

use std::sync::Arc;

use coco_llm_types::ToolCallPart;
use coco_llm_types::ToolInputInvalidReason;
use coco_tool_runtime::SchemaIssue;
use coco_tool_runtime::traits::DynTool;
use serde_json::Value;

use crate::tool_input_parse::parse_tool_arguments_or_empty;

/// Recover stringified JSON nested inside what the schema expects to
/// be an object/array. Mirrors TS Claude Code's `safeParseJSON`
/// branch in `normalizeContentFromAPI`.
///
/// - `Value::String(s)` → try [`parse_tool_arguments_or_empty`];
///   when it produces an `Object`/`Array`, overwrite `input` with the
///   parsed value. Otherwise keep the original `String` and let the
///   schema validator surface a type-mismatch error.
/// - Other variants pass through unchanged.
pub fn normalize_value_string(input: &mut Value) {
    if let Value::String(s) = input {
        let s_owned = std::mem::take(s);
        let recovered = parse_tool_arguments_or_empty(&s_owned, "(value-string)");
        match recovered {
            Value::Object(_) | Value::Array(_) => *input = recovered,
            // Couldn't recover into a structured value. Restore the
            // original string so the schema validator can flag the
            // type mismatch.
            _ => *input = Value::String(s_owned),
        }
    }
}

/// schema validation entry point: classifies + sets `invalid` / `invalid_reason`
/// on `tc` when the call cannot be executed as emitted.
///
/// Skipped when the call is already invalid from wire parsing — the
/// earlier provider-side classification stands.
pub fn validate_tool_call(tc: &mut ToolCallPart, tool: Option<&Arc<dyn DynTool>>) {
    if tc.invalid {
        return;
    }

    // 1. NoSuchTool — short-circuit before touching the schema.
    let Some(tool) = tool else {
        tc.invalid = true;
        tc.invalid_reason = Some(ToolInputInvalidReason::NoSuchTool {
            tool_name: tc.tool_name.clone(),
        });
        return;
    };

    // 2. Value::String recovery (mirrors TS recursive-parse).
    normalize_value_string(&mut tc.input);

    // 3. Schema validation — synchronous and lock-free; the validator is
    //    owned by the schema (v4.2). A schema-compile failure is impossible
    //    here: a tool is only registered if its schema compiled at
    //    construction, so the only outcomes are clean or classified issues.
    if let Err(issues) = tool.runtime_validation_schema().validate(&tc.input) {
        let message = format_schema_error(&tc.tool_name, &issues);
        tc.invalid = true;
        tc.invalid_reason = Some(ToolInputInvalidReason::SchemaViolation { message });
    }
}

/// Format a slice of [`SchemaIssue`]s into the TS-parity error body.
///
/// Mirrors `formatZodValidationError` (`utils/toolErrors.ts:66-130`):
/// the body is `"{tool} failed due to the following {issue|issues}:\n{lines}"`,
/// each line maps onto one of three patterns:
///
/// - `MissingRequired` → `"The required parameter \`{path}\` is missing"`
/// - `UnexpectedField` → `"An unexpected parameter \`{key}\` was provided"`
/// - `TypeMismatch` → `"The parameter \`{path}\` type is expected as \`{expected}\` but provided as \`{received}\`"`
/// - `Other` → falls through to the raw `jsonschema` message,
///   prefixed with the path when present.
///
/// Plural / singular selection follows the TS code: ≥2 lines → `"issues"`,
/// otherwise `"issue"`.
pub fn format_schema_error(tool_name: &str, issues: &[SchemaIssue]) -> String {
    if issues.is_empty() {
        return format!("{tool_name} failed schema validation");
    }

    let mut lines = Vec::with_capacity(issues.len());
    for issue in issues {
        match issue {
            SchemaIssue::MissingRequired { path, field } => {
                let full_path = join_path(path, field);
                lines.push(format!("The required parameter `{full_path}` is missing"));
            }
            SchemaIssue::UnexpectedField { field, .. } => {
                lines.push(format!("An unexpected parameter `{field}` was provided"));
            }
            SchemaIssue::TypeMismatch {
                path,
                expected,
                received,
            } => {
                let p = display_path(path);
                lines.push(format!(
                    "The parameter `{p}` type is expected as `{expected}` but provided as `{received}`"
                ));
            }
            SchemaIssue::Other { path, message } => {
                if path.is_empty() {
                    lines.push(message.clone());
                } else {
                    lines.push(format!("`{}`: {message}", display_path(path)));
                }
            }
        }
    }

    let word = if lines.len() > 1 { "issues" } else { "issue" };
    format!(
        "{tool_name} failed due to the following {word}:\n{}",
        lines.join("\n")
    )
}

/// Stitch a parent path + field name into a single user-readable
/// path. `jsonschema` returns paths as JSON Pointers (`/foo/0/bar`);
/// we convert to dotted+bracket form (`foo[0].bar`) to match the TS
/// `formatValidationPath` output.
fn join_path(parent: &str, field: &str) -> String {
    let parent = display_path(parent);
    if parent.is_empty() {
        field.to_string()
    } else {
        format!("{parent}.{field}")
    }
}

/// Convert a `/foo/0/bar`-style JSON Pointer into `foo[0].bar`.
fn display_path(pointer: &str) -> String {
    if pointer.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for segment in pointer.split('/').skip(1) {
        if segment.parse::<usize>().is_ok() {
            out.push('[');
            out.push_str(segment);
            out.push(']');
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(segment);
        }
    }
    out
}

#[cfg(test)]
#[path = "tool_input_validate.test.rs"]
mod tests;
