//! Shared input validation helpers for built-in tools.
//!
//! Eliminates repetitive `input["field"].as_str().ok_or_else(|| InvalidInputSnafu { ... }.build())`
//! patterns across tool implementations.

use crate::error::ToolError;
use crate::error::tool_error::InvalidInputSnafu;
use snafu::IntoError;

/// Extracts a required string field from tool input JSON.
///
/// Returns an `InvalidInput` error if the field is missing or not a string.
pub(crate) fn require_str<'a>(
    input: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, ToolError> {
    input[field].as_str().ok_or_else(|| {
        InvalidInputSnafu {
            message: format!("{field} must be a string"),
        }
        .into_error(snafu::NoneError)
    })
}

/// Extracts an optional boolean field with a default value.
pub(crate) fn bool_or(input: &serde_json::Value, field: &str, default: bool) -> bool {
    input[field].as_bool().unwrap_or(default)
}

/// Extracts an optional array of strings, returning empty vec if missing.
pub(crate) fn string_array(input: &serde_json::Value, field: &str) -> Vec<String> {
    input[field]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
