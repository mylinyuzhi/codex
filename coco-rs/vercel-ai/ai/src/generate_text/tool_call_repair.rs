//! Tool call repair functionality.
//!
//! Provides the trait + dispatch helpers for repairing malformed tool
//! calls when the model emits invalid JSON arguments or names a
//! non-existent tool. **No default repair implementation ships with
//! this module** — matching the upstream TypeScript Vercel AI SDK,
//! where `repairToolCall` is a user-supplied callback (typically
//! re-prompts the LLM to fix its own output) rather than a built-in
//! JSON fixer. Callers wire in their preferred strategy via
//! [`CustomRepairFunction`] or a custom impl of
//! [`ToolCallRepairFunction`].
//!
//! # Example: local JSON repair
//!
//! ```ignore
//! use std::sync::Arc;
//! use vercel_ai::{CustomRepairFunction, ToolCall};
//!
//! let repair = Arc::new(CustomRepairFunction::new(|tool_call, error| {
//!     // Implement repair logic here — e.g., call a JSON-repair crate
//!     // (`llm_json`, `jsonrepair`) on `error.original_error`'s raw
//!     // input, or re-prompt the LLM. Return `None` if not fixable.
//!     None
//! }));
//!
//! // Pass to `GenerateTextOptions::with_repair_tool_call(repair)`.
//! ```
//!
//! # Example: LLM re-prompt
//!
//! The typical TS implementation feeds the failing `tool_call` +
//! `inputSchema` + `error` back into the model and asks it to
//! produce corrected arguments. That strategy lives at the caller
//! layer — this module only provides the dispatch primitives.

use std::sync::Arc;

use serde_json::Value as JSONValue;

use crate::error::InvalidToolInputError;
use crate::error::NoSuchToolError;
use crate::error::ToolCallRepairError;
use crate::error::ToolCallRepairOriginalError;
use crate::types::ToolRegistry;

use super::generate_text_result::ToolCall;

/// A function that can repair a malformed tool call.
#[async_trait::async_trait]
pub trait ToolCallRepairFunction: Send + Sync {
    /// Attempt to repair a tool call.
    ///
    /// # Arguments
    ///
    /// * `tool_call` - The malformed tool call.
    /// * `error` - The error that occurred.
    ///
    /// # Returns
    ///
    /// The repaired tool call, or None if repair is not possible.
    async fn repair(&self, tool_call: &ToolCall, error: &ToolCallRepairError) -> Option<ToolCall>;
}

/// A repair function that uses a custom function to fix the tool call.
pub struct CustomRepairFunction<F>
where
    F: Fn(&ToolCall, &ToolCallRepairError) -> Option<ToolCall> + Send + Sync,
{
    repair_fn: F,
}

impl<F> CustomRepairFunction<F>
where
    F: Fn(&ToolCall, &ToolCallRepairError) -> Option<ToolCall> + Send + Sync,
{
    /// Create a new custom repair function.
    pub fn new(repair_fn: F) -> Self {
        Self { repair_fn }
    }
}

#[async_trait::async_trait]
impl<F> ToolCallRepairFunction for CustomRepairFunction<F>
where
    F: Fn(&ToolCall, &ToolCallRepairError) -> Option<ToolCall> + Send + Sync,
{
    async fn repair(&self, tool_call: &ToolCall, error: &ToolCallRepairError) -> Option<ToolCall> {
        (self.repair_fn)(tool_call, error)
    }
}

/// Repair result.
#[derive(Debug)]
pub enum RepairResult {
    /// Tool call was successfully repaired.
    Repaired(ToolCall),
    /// Tool call could not be repaired.
    CannotRepair {
        /// The original error.
        error: ToolCallRepairError,
    },
    /// Tool call should be skipped.
    Skip,
}

/// Repair a tool call.
///
/// # Arguments
///
/// * `tool_call` - The tool call to repair.
/// * `error` - The error that occurred.
/// * `repair_fn` - The repair function.
///
/// # Returns
///
/// The repair result.
pub async fn repair_tool_call(
    tool_call: &ToolCall,
    error: &ToolCallRepairError,
    repair_fn: &dyn ToolCallRepairFunction,
) -> RepairResult {
    match repair_fn.repair(tool_call, error).await {
        Some(repaired) => RepairResult::Repaired(repaired),
        None => RepairResult::CannotRepair {
            error: error.clone(),
        },
    }
}

/// Repair multiple tool calls.
///
/// # Arguments
///
/// * `tool_calls` - The tool calls to repair.
/// * `errors` - The errors for each tool call.
/// * `repair_fn` - The repair function.
///
/// # Returns
///
/// A vector of repair results.
pub async fn repair_tool_calls(
    tool_calls: &[ToolCall],
    errors: &[ToolCallRepairError],
    repair_fn: &dyn ToolCallRepairFunction,
) -> Vec<RepairResult> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for (tc, error) in tool_calls.iter().zip(errors.iter()) {
        let result = repair_tool_call(tc, error, repair_fn).await;
        results.push(result);
    }

    results
}

/// Validate a tool call against the tool registry.
///
/// # Arguments
///
/// * `tool_call` - The tool call to validate.
/// * `tools` - The tool registry.
///
/// # Returns
///
/// Ok if valid, Err with repair error if invalid.
pub fn validate_tool_call_for_repair(
    tool_call: &ToolCall,
    tools: &Arc<ToolRegistry>,
) -> Result<(), ToolCallRepairError> {
    // Check if tool exists
    if tools.get(&tool_call.tool_name).is_none() {
        let available_tools: Vec<String> =
            tools.definitions().iter().map(|d| d.name.clone()).collect();

        return Err(ToolCallRepairError::new(
            "Tool not found",
            ToolCallRepairOriginalError::NoSuchTool(
                NoSuchToolError::new(&tool_call.tool_name).with_available_tools(available_tools),
            ),
        ));
    }

    // Validate arguments against schema if available
    if let Some(tool) = tools.get(&tool_call.tool_name) {
        let def = tool.definition();
        {
            let schema = &def.input_schema;
            if let Err(e) = validate_against_schema(&tool_call.args, schema) {
                return Err(ToolCallRepairError::new(
                    format!("Schema validation failed: {e}"),
                    ToolCallRepairOriginalError::InvalidToolInput(
                        InvalidToolInputError::new(
                            &tool_call.tool_name,
                            tool_call.args.to_string(),
                        )
                        .with_message(e),
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// Validate JSON value against a JSON schema.
fn validate_against_schema(
    value: &JSONValue,
    schema: &vercel_ai_provider::JSONSchema,
) -> Result<(), String> {
    // JSONSchema is just a type alias for JSONValue
    // Check type
    if let Some(schema_type) = schema.get("type").and_then(|t| t.as_str()) {
        let valid = match schema_type {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };

        if !valid {
            return Err(format!(
                "Expected type '{}', got '{}'",
                schema_type,
                json_type_name(value)
            ));
        }
    }

    Ok(())
}

/// Get the JSON type name for a value.
fn json_type_name(value: &JSONValue) -> &'static str {
    match value {
        JSONValue::Null => "null",
        JSONValue::Bool(_) => "boolean",
        JSONValue::Number(_) => "number",
        JSONValue::String(_) => "string",
        JSONValue::Array(_) => "array",
        JSONValue::Object(_) => "object",
    }
}

#[cfg(test)]
#[path = "tool_call_repair.test.rs"]
mod tests;
