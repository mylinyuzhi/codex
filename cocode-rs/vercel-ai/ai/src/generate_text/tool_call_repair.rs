//! Tool call repair functionality.
//!
//! This module provides functionality for repairing malformed tool calls
//! when the model generates invalid JSON arguments or non-existent tool names.

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

/// A simple repair function that attempts to fix JSON parsing errors.
pub struct JsonRepairFunction;

#[async_trait::async_trait]
impl ToolCallRepairFunction for JsonRepairFunction {
    async fn repair(&self, tool_call: &ToolCall, error: &ToolCallRepairError) -> Option<ToolCall> {
        // Check if this is an invalid tool input error
        match &error.original_error {
            ToolCallRepairOriginalError::InvalidToolInput(input_error) => {
                // Try to fix common JSON issues in the tool input
                if let Some(fixed) = try_fix_json(&input_error.tool_input) {
                    return Some(ToolCall::new(
                        &tool_call.tool_call_id,
                        &tool_call.tool_name,
                        fixed,
                    ));
                }
            }
            ToolCallRepairOriginalError::NoSuchTool(_) => {
                // Can't repair a tool that doesn't exist
            }
        }
        None
    }
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

/// Try to fix common JSON issues.
fn try_fix_json(raw: &str) -> Option<JSONValue> {
    let trimmed = raw.trim();

    // Try parsing as-is first
    if let Ok(v) = serde_json::from_str::<JSONValue>(trimmed) {
        return Some(v);
    }

    // Try adding missing quotes around keys
    let fixed = fix_unquoted_keys(trimmed);
    if let Ok(v) = serde_json::from_str::<JSONValue>(&fixed) {
        return Some(v);
    }

    // Try fixing trailing commas
    let fixed = fix_trailing_commas(trimmed);
    if let Ok(v) = serde_json::from_str::<JSONValue>(&fixed) {
        return Some(v);
    }

    // Try fixing missing closing brackets
    let fixed = fix_missing_brackets(trimmed);
    if let Ok(v) = serde_json::from_str::<JSONValue>(&fixed) {
        return Some(v);
    }

    None
}

/// Fix unquoted keys in JSON.
fn fix_unquoted_keys(json: &str) -> String {
    // Simple regex-like replacement for unquoted keys
    // This is a basic implementation; a full implementation would use a proper parser
    let mut result = String::new();
    let mut chars = json.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' || c == ',' {
            result.push(c);
            // Skip whitespace
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }
            // Check if key is unquoted
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

/// Fix trailing commas in JSON.
fn fix_trailing_commas(json: &str) -> String {
    // Remove trailing commas before ] and }
    let mut result = String::new();
    let mut chars = json.chars().peekable();

    while let Some(c) = chars.next() {
        if c == ',' {
            // Check if next non-whitespace is ] or }
            let mut temp = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    if let Some(c) = chars.next() {
                        temp.push(c);
                    }
                } else {
                    break;
                }
            }
            if let Some(&next) = chars.peek()
                && (next == ']' || next == '}')
            {
                // Skip the comma
                result.push_str(&temp);
                continue;
            }
            result.push(c);
            result.push_str(&temp);
        } else {
            result.push(c);
        }
    }

    result
}

/// Fix missing closing brackets.
fn fix_missing_brackets(json: &str) -> String {
    let mut result = json.to_string();
    let mut open_braces = 0i32;
    let mut open_brackets = 0i32;
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

    // Close any open strings
    if in_string {
        result.push('"');
    }

    // Add missing closing brackets
    for _ in 0..open_brackets {
        result.push(']');
    }
    for _ in 0..open_braces {
        result.push('}');
    }

    result
}
