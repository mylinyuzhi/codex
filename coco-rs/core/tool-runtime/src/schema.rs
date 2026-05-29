//! Self-validating tool input schema: the [`ToolInputSchema`] newtype owns
//! both the JSON Schema document (model-facing serialization via
//! [`ToolInputSchema::as_value`]) AND the compiled [`jsonschema::Validator`]
//! (runtime validation via [`ToolInputSchema::validate`]), built ONCE at
//! construction.
//!
//! TS parity: `services/tools/toolExecution.ts:614` validates model input
//! against the tool's full Zod schema (`tool.inputSchema.safeParse`) — both on
//! pre-execution validation AND on re-validation after a PreToolUse hook
//! rewrites the input (`tool_call_preparer.rs`). [`ToolInputSchema::validate`]
//! is the Rust equivalent: sync, lock-free, classification-identical.
//!
//! ## Construction
//!
//! - [`ToolInputSchema::from_input_type`] — Bucket-A tools: derive from a
//!   `T: JsonSchema` input struct, close it (`additionalProperties:false`),
//!   then compile. A failure is a tool-author bug ⇒ panic with the type name
//!   (the registry force-init gate catches it before ship).
//! - [`ToolInputSchema::from_value`] — hand-built / MCP-wire / `--json-schema`
//!   tools: normalize the root (fold in `type:"object"` when absent; reject an
//!   explicit non-object root), then compile (= meta-validation) and KEEP the
//!   validator.
//!
//! ## No cache
//!
//! The validator lives inside the schema — compiled once, shared behind `Arc`
//! on clone. There is no separate `ToolId`-keyed cache, no lazy compile, and no
//! MCP-reconnect staleness: a reconnect builds a new tool ⇒ new schema ⇒ new
//! validator, and the registry overwrite drops the old one.
//!
//! ## Build invariant
//!
//! `jsonschema` stays `default-features = false` (resolve-http OFF) so a remote
//! `$ref` is rejected as a graceful `Err` at construction — never fetched
//! (SSRF / blocking-fetch guard for untrusted MCP schemas).
//!
//! ## Error shape
//!
//! Construction failures produce [`SchemaError`] (`StatusCode::InvalidArguments`);
//! validation failures produce `Vec<`[`SchemaIssue`]`>` carrying compact,
//! model-facing messages derived from `jsonschema::ValidationError`.

use std::sync::Arc;

use serde_json::Value;

use crate::derive::derive_input_schema_value;
use coco_error::ErrorExt;
use coco_error::StackError;
use coco_error::StatusCode;
use schemars::JsonSchema;

/// Self-validating tool input schema (v4.2 owner — supersedes the deleted
/// `coco_types::ToolInputSchema`). Owns the JSON Schema document (for
/// model-facing serialization via [`Self::as_value`]) AND the compiled
/// validator (for runtime validation via [`Self::validate`]), compiled ONCE at
/// construction. Cheap to clone — the validator is shared behind `Arc`.
///
/// This collapses the old separate `ToolSchemaValidator` cache: there is no
/// lazy compile, no `ToolId`-keyed lookup, and no MCP-reconnect staleness (a
/// reconnect builds a new tool → new schema → new validator; the registry
/// overwrite drops the old one).
#[derive(Clone)]
pub struct ToolInputSchema {
    value: Value,
    validator: Arc<jsonschema::Validator>,
}

impl std::fmt::Debug for ToolInputSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The compiled validator tree is large and noisy; elide it.
        f.debug_struct("ToolInputSchema")
            .field("value", &self.value)
            .field("validator", &"<compiled>")
            .finish()
    }
}

impl ToolInputSchema {
    /// Bucket A entry — schemars-derived from `T`, closed with
    /// `additionalProperties: false`, then compiled. A failure here is a
    /// tool-author bug, so it panics with the offending type name (the
    /// coco-tools force-init test turns that into a CI failure, not a
    /// production-only panic).
    #[must_use]
    pub fn from_input_type<T: JsonSchema>() -> Self {
        let mut raw = derive_input_schema_value::<T>();
        if let Some(obj) = raw.as_object_mut() {
            obj.insert("additionalProperties".to_string(), Value::Bool(false));
        }
        Self::from_value(raw).unwrap_or_else(|e| {
            panic!(
                "schemars-derived schema for {} failed validation: {e}",
                std::any::type_name::<T>(),
            )
        })
    }

    /// Bucket B/C/D/E entry — programmer-written Value, derive+mutate Value,
    /// external MCP wire schema, or user `--json-schema`. Normalizes the root
    /// (the ONLY mutation: fold-in `type:"object"` when neither `type` nor a
    /// composition keyword is present — see [`normalize_root_object`]), then
    /// compiles the validator (= meta-validation) and keeps it. External
    /// schemas are otherwise preserved verbatim; a remote `$ref` surfaces here
    /// as [`SchemaError::InvalidSchema`] (jsonschema returns `Err`, never
    /// fetches, with `resolve-http` off).
    pub fn from_value(mut raw: Value) -> Result<Self, SchemaError> {
        normalize_root_object(&mut raw)?;
        let validator =
            jsonschema::validator_for(&raw).map_err(|e| SchemaError::InvalidSchema {
                message: e.to_string(),
            })?;
        Ok(Self {
            value: raw,
            validator: Arc::new(validator),
        })
    }

    /// The JSON Schema document (root `type:"object"`). Serialized into the
    /// model-facing tool definition; also the base for
    /// [`schema_omit_properties`].
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.value
    }

    /// Validate `input` against the compiled validator. Synchronous and
    /// lock-free — the validator lives in `self`. Returns the same classified
    /// [`SchemaIssue`] list the legacy `validate_collect` produced, so
    /// `app/query::format_schema_error` is unaffected.
    pub fn validate(&self, input: &Value) -> Result<(), Vec<SchemaIssue>> {
        let issues: Vec<SchemaIssue> = self
            .validator
            .iter_errors(input)
            .map(SchemaIssue::from_jsonschema)
            .collect();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

/// Fold-in `type:"object"` when the root omits `type` AND carries no
/// composition keyword (`$ref`/`allOf`/`anyOf`/`oneOf`/`not`); reject an
/// explicit non-object root. This is the ONLY mutation [`ToolInputSchema::from_value`]
/// makes to an external schema. Rejecting array-form `["object","null"]`
/// prevents a `null` input from passing validation for dynamic `Value` tools.
pub(crate) fn normalize_root_object(value: &mut Value) -> Result<(), SchemaError> {
    let Some(obj) = value.as_object_mut() else {
        return Err(SchemaError::RootTypeNotObject);
    };
    match obj.get("type") {
        Some(Value::String(s)) if s == "object" => Ok(()),
        Some(Value::String(s)) if s == "null" => Err(SchemaError::RootTypeNull),
        Some(_) => Err(SchemaError::RootTypeNotObject),
        None => {
            const COMPOSITION: [&str; 5] = ["$ref", "allOf", "anyOf", "oneOf", "not"];
            if COMPOSITION.iter().any(|k| obj.contains_key(*k)) {
                // Composition root — leave the author's contract untouched.
                Ok(())
            } else {
                obj.insert("type".to_string(), Value::String("object".to_string()));
                Ok(())
            }
        }
    }
}

/// Return a clone of `schema` with every name in `fields` removed from
/// `properties` and `required`; drops `required` if it empties. Plural so a
/// multi-field omit (AgentTool hides `mcp_servers` + `run_in_background`) costs
/// a single clone. The model-facing view is never validated, so no recompile.
#[must_use]
pub fn schema_omit_properties(schema: &Value, fields: &[&str]) -> Value {
    let mut out = schema.clone();
    if let Some(obj) = out.as_object_mut() {
        if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            for f in fields {
                props.remove(*f);
            }
        }
        if let Some(req) = obj.get_mut("required").and_then(Value::as_array_mut) {
            req.retain(|v| !matches!(v.as_str(), Some(s) if fields.contains(&s)));
            if req.is_empty() {
                obj.remove("required");
            }
        }
    }
    out
}

/// Construction-time schema error. Tier-3 (`thiserror` + manual [`StackError`]
/// + [`ErrorExt`]), mirroring `coco_context::ContextError`. Always classifies
/// as [`StatusCode::InvalidArguments`] (non-retryable).
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error(
        "schema root must declare type:\"object\" as a single string \
         (composition/array forms like [\"object\",\"null\"] are rejected)"
    )]
    RootTypeNotObject,

    #[error("schema root is the singleton null type")]
    RootTypeNull,

    #[error("schema failed JSON Schema meta-validation: {message}")]
    InvalidSchema { message: String },
}

impl StackError for SchemaError {
    fn debug_fmt(&self, layer: usize, buf: &mut Vec<String>) {
        buf.push(format!("{layer}: {self}"));
    }

    fn next(&self) -> Option<&dyn StackError> {
        None
    }
}

impl ErrorExt for SchemaError {
    fn status_code(&self) -> StatusCode {
        StatusCode::InvalidArguments
    }

    fn as_any(&self) -> &(dyn std::any::Any + 'static) {
        self
    }
}

/// Test-double helper: a trivial closed `{"type":"object"}` schema for tool
/// stubs whose `runtime_validation_schema` content is irrelevant (they exercise
/// execution / registry / streaming, not schema validation). Available under
/// `cfg(test)` and the `testing` feature so doubles in dependent crates share it.
#[cfg(any(test, feature = "testing"))]
pub fn test_runtime_schema() -> &'static ToolInputSchema {
    static SCHEMA: std::sync::OnceLock<ToolInputSchema> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(|| {
        ToolInputSchema::from_value(serde_json::json!({ "type": "object" }))
            .expect("trivial object schema")
    })
}

/// Implement [`Tool::runtime_validation_schema`](crate::Tool) for a derive-only
/// (Bucket A) tool via a per-impl `OnceLock` cache of
/// [`ToolInputSchema::from_input_type`]. Keeps the tool a unit struct (no
/// field, no `new()`); the schema compiles on first access (the coco-tools
/// force-init test turns a bad schema into a CI panic).
#[macro_export]
macro_rules! impl_runtime_schema {
    ($input:ty) => {
        fn runtime_validation_schema(&self) -> &$crate::ToolInputSchema {
            static SCHEMA: ::std::sync::OnceLock<$crate::ToolInputSchema> =
                ::std::sync::OnceLock::new();
            SCHEMA.get_or_init($crate::ToolInputSchema::from_input_type::<$input>)
        }
    };
}

/// Structured form of a JSON Schema validation issue, captured at the
/// `core/tool-runtime` boundary so higher layers
/// (`app/query::tool_input_validate::format_schema_error`) can produce
/// TS-parity error text without depending on `jsonschema` directly.
///
/// Each variant maps onto a `formatZodValidationError`
/// (`utils/toolErrors.ts:66-130`) output line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaIssue {
    /// Required field is missing from the input. `path` is the
    /// JSON Pointer of the parent object; `field` is the missing key.
    MissingRequired { path: String, field: String },
    /// Input contains a field not in the schema.
    UnexpectedField { path: String, field: String },
    /// Field type does not match the schema.
    TypeMismatch {
        path: String,
        expected: String,
        received: String,
    },
    /// Catch-all for other validation failures (enum, pattern,
    /// min/max length, etc). `message` is the raw `jsonschema`
    /// rendering.
    Other { path: String, message: String },
}

impl SchemaIssue {
    /// Classify a `jsonschema::ValidationError` into a `SchemaIssue`.
    fn from_jsonschema(err: jsonschema::ValidationError<'_>) -> Self {
        use jsonschema::error::ValidationErrorKind;

        let path = err.instance_path().to_string();
        let message = err.to_string();
        let instance_type = json_type_name(err.instance());
        match err.kind() {
            ValidationErrorKind::Required { property } => SchemaIssue::MissingRequired {
                path,
                field: property
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| property.to_string()),
            },
            ValidationErrorKind::AdditionalProperties { unexpected } => {
                // jsonschema lumps unexpected keys into one error;
                // split into one issue per key so the formatter can
                // render them line-by-line.
                if let Some(first) = unexpected.first() {
                    SchemaIssue::UnexpectedField {
                        path,
                        field: first.clone(),
                    }
                } else {
                    SchemaIssue::Other { path, message }
                }
            }
            ValidationErrorKind::Type { kind } => {
                let expected = format_type_kind(kind);
                SchemaIssue::TypeMismatch {
                    path,
                    expected,
                    received: instance_type.into(),
                }
            }
            _ => SchemaIssue::Other { path, message },
        }
    }
}

fn format_type_kind(kind: &jsonschema::error::TypeKind) -> String {
    use jsonschema::error::TypeKind;
    match kind {
        TypeKind::Single(t) => format!("{t:?}").to_lowercase(),
        TypeKind::Multiple(_) => "multiple".to_string(),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
