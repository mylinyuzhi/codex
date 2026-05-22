//! Schemars 1.2 → `ToolInputSchema` / output-schema derive helpers.
//!
//! ## What this module replaces
//!
//! Pre-refactor each tool wrote `fn input_schema(&self) -> ToolInputSchema`
//! by hand, building a `HashMap<String, Value>` of properties plus a
//! `Vec<String>` of required field names. The args struct (used inside
//! `execute` to `serde_json::from_value`) lived in a separate file and
//! could drift from the schema silently — renaming a struct field did
//! NOT cause a compile error in the schema.
//!
//! Post-refactor each tool declares ONE `#[derive(Deserialize, JsonSchema)]
//! struct XxxInput;` and calls [`derive_input_schema::<XxxInput>`]. The
//! schema and the typed parse share a single artifact; field renames are
//! caught at `cargo check`.
//!
//! ## TS parity
//!
//! TS uses `z.object({...})` as both the runtime validator AND the
//! TypeScript type source (`z.infer<typeof inputSchema>`). The Rust
//! mirror is `T: JsonSchema + Deserialize` — `T` provides the schema
//! via the JsonSchema derive and the parsed type via the Deserialize
//! derive.
//!
//! ## Subschema inlining
//!
//! Schemars' default behaviour is to extract reused types into a
//! `$defs` table and reference them via `$ref: "#/$defs/Foo"`. TS zod
//! schemas instead expand inline; provider tool APIs (Anthropic,
//! OpenAI, Gemini) all accept `$ref` but their handling differs in
//! edge cases. For first-party parity with TS we set
//! [`SchemaSettings::inline_subschemas`] = `true` so the generated
//! schema is a single flat document.
//!
//! ## Contract direction
//!
//! Input schemas describe what we'll **deserialize FROM** the model
//! (`for_deserialize`). Output schemas describe what the tool will
//! **serialize INTO** the response (`for_serialize`). The distinction
//! only matters for types that customise `JsonSchema` based on
//! contract; for typical `#[derive]`-ed types the two contracts
//! produce identical output.

use coco_types::ToolInputSchema;
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Map as JsonMap;
use serde_json::Value;
use std::collections::HashMap;

/// Derive a [`ToolInputSchema`] (model-facing properties + required)
/// from a `T: JsonSchema` input struct.
///
/// `T` should be the tool's typed input — e.g. for a tool whose
/// `execute` takes `BashInput`, call `derive_input_schema::<BashInput>()`.
/// All subschemas are inlined (no `$ref` in the result).
///
/// # Panics
///
/// Panics if the top-level derived schema is not an object schema.
/// That can only happen if `T` is something like a bare `String` /
/// `i32` / tuple — none of which make sense as a tool's input. Tool
/// inputs MUST be structs (or struct-shaped enums) so the panic
/// signals a tool-author programming error, not a runtime failure.
#[must_use]
pub fn derive_input_schema<T: JsonSchema>() -> ToolInputSchema {
    let value = derive_input_schema_value::<T>();
    schema_value_to_tool_input_schema(value)
}

/// Like [`derive_input_schema`] but returns the entire JSON Schema
/// document instead of stripping it down to `ToolInputSchema`.
///
/// Useful for the validator path
/// ([`crate::schema::effective_tool_schema`]) which wants the full
/// envelope (`{"type":"object","properties":...,"required":...}`).
#[must_use]
pub fn derive_input_schema_value<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::default()
        .with(|s| {
            s.inline_subschemas = true;
        })
        .for_deserialize()
        .into_generator();
    let schema = generator.into_root_schema_for::<T>();
    // `schemars::Schema` always round-trips through `serde_json::Value`
    // by construction; on the off-chance a future schemars upgrade
    // breaks that, surface as `Value::Null` rather than panicking the
    // tool-listing path — the validator will then reject the empty
    // schema and the model gets a clean error.
    serde_json::to_value(&schema).unwrap_or(Value::Null)
}

/// Derive a JSON Schema document from `T: JsonSchema` for use as a
/// tool's output schema (`Tool::output_schema()`). Inlines subschemas.
///
/// Uses the `serialize` contract — output schemas describe what the
/// tool will emit to the model, not what gets deserialised from it.
/// For trivially-derived types this is identical to the deserialize
/// contract; types that vary their JsonSchema impl based on contract
/// (rare in our codebase) get the correct direction.
#[must_use]
pub fn derive_output_schema<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::default()
        .with(|s| {
            s.inline_subschemas = true;
        })
        .for_serialize()
        .into_generator();
    let schema = generator.into_root_schema_for::<T>();
    // `schemars::Schema` always round-trips through `serde_json::Value`
    // by construction; on the off-chance a future schemars upgrade
    // breaks that, surface as `Value::Null` rather than panicking the
    // tool-listing path — the validator will then reject the empty
    // schema and the model gets a clean error.
    serde_json::to_value(&schema).unwrap_or(Value::Null)
}

/// Strip a top-level JSON Schema object document down to
/// `ToolInputSchema { properties, required }`. Meta fields
/// (`$schema`, `title`, `description`, `$defs`, `type`,
/// `additionalProperties`, etc.) are dropped — they're noise from the
/// model's perspective once the model already knows it's looking at a
/// tool input object.
fn schema_value_to_tool_input_schema(schema_value: Value) -> ToolInputSchema {
    let Value::Object(mut map) = schema_value else {
        panic!(
            "derive_input_schema: top-level schema must be an object schema, got: {schema_value}"
        );
    };
    let properties = extract_properties(map.remove("properties"));
    let required = extract_required(map.remove("required"));
    ToolInputSchema {
        properties,
        required,
    }
}

fn extract_properties(props: Option<Value>) -> HashMap<String, Value> {
    match props {
        Some(Value::Object(map)) => map_to_hashmap(map),
        None => HashMap::new(),
        Some(other) => panic!("derive_input_schema: `properties` must be an object, got: {other}"),
    }
}

fn extract_required(req: Option<Value>) -> Vec<String> {
    match req {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => s,
                other => {
                    panic!("derive_input_schema: `required` entries must be strings, got: {other}")
                }
            })
            .collect(),
        None => Vec::new(),
        Some(other) => panic!("derive_input_schema: `required` must be an array, got: {other}"),
    }
}

fn map_to_hashmap(map: JsonMap<String, Value>) -> HashMap<String, Value> {
    map.into_iter().collect()
}

#[cfg(test)]
#[path = "derive.test.rs"]
mod tests;
