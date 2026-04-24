//! Tool input-schema derivation + cached JSON Schema validator.
//!
//! TS: `services/tools/toolExecution.ts:614` validates model input
//! against the tool's full Zod schema via `tool.inputSchema.safeParse`.
//! The plan's Phase 4a calls for the Rust equivalent — a cached
//! [`jsonschema::Validator`] per `ToolId`, exercised both on
//! pre-execution validation AND on re-validation after a PreToolUse
//! hook rewrites the input (plan I3's Rust-side tightening).
//!
//! ## Current schema source
//!
//! Tools emit schemas via two trait methods:
//!
//! - [`Tool::input_json_schema`] — optional explicit override
//!   returning a full JSON Schema document. Preferred when present.
//! - [`Tool::input_schema`] — returns
//!   [`ToolInputSchema`](coco_types::ToolInputSchema), currently a
//!   `properties` sub-map. This module wraps it in a synthetic
//!   `{"type": "object", "properties": <map>}` envelope so the
//!   validator sees a complete JSON Schema document.
//!
//! The envelope wrap is the plan's Phase-4 "migration prerequisite"
//! deferred form — it accepts the properties-only legacy shape
//! without breaking existing tools. Future work (plan acceptance
//! gate 1) migrates every built-in to emit a full schema via
//! `input_json_schema`, eliminating the wrap. This module's
//! `effective_tool_schema` has ONE code path either way: always
//! emits a full document.
//!
//! ## Caching
//!
//! [`ToolSchemaValidator`] memoizes `jsonschema::Validator` keyed by
//! `ToolId`. Lookups + validations are lock-free-ish (one `RwLock`
//! read, potentially one write on cache miss). The cache is
//! per-session because tool sets change across sessions; reset
//! with [`ToolSchemaValidator::clear`] if the tool registry
//! rebuilds mid-session.
//!
//! ## Error shape
//!
//! Validation failures produce [`SchemaValidationError::Rejected`]
//! carrying a compact human-readable message derived from
//! `jsonschema::ValidationError`. Plan Phase 4 says textual parity
//! with Zod is OUT of scope for first slice; we surface a
//! useful-but-not-TS-identical message.

use std::collections::HashMap;
use std::sync::Arc;

use coco_types::ToolId;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::traits::Tool;

/// Build the full JSON Schema document the model saw for a tool.
///
/// Preference: `tool.input_json_schema()` if present (explicit
/// override); else wrap `tool.input_schema()`'s `properties` map in
/// the standard `{type: object, properties: _}` envelope.
///
/// TS parity: mirrors `toolUseContext.options.tools[name]` — both
/// validator input and model-visible schema come from the same
/// function so drift is impossible.
pub fn effective_tool_schema(tool: &dyn Tool) -> Value {
    if let Some(json) = tool.input_json_schema() {
        return json;
    }
    let schema = tool.input_schema();
    // Wrap the properties sub-map. This branch becomes unreachable
    // once all tools migrate to full-schema `input_json_schema`.
    let props = serde_json::to_value(&schema.properties).unwrap_or(Value::Null);
    serde_json::json!({
        "type": "object",
        "properties": props,
    })
}

/// Cached JSON Schema validator registry keyed by [`ToolId`].
///
/// Thread-safe and cheap to clone (shared state behind an
/// `Arc<RwLock<_>>`). Miss-path compiles the validator once and
/// caches it for subsequent calls.
#[derive(Clone, Default)]
pub struct ToolSchemaValidator {
    cache: Arc<RwLock<HashMap<ToolId, Arc<jsonschema::JSONSchema>>>>,
    // jsonschema 0.18 exposes `JSONSchema` as the compiled handle.
    // The 0.20 series renames it to `Validator`; when we upgrade,
    // this type alias + `compile` callsite change.
}

/// Error returned by [`ToolSchemaValidator::validate`].
#[derive(Debug, thiserror::Error)]
pub enum SchemaValidationError {
    /// Validation rejected the input. `message` is a compact
    /// explanation suitable for a tool_result error body.
    #[error("input schema validation failed: {message}")]
    Rejected { message: String },
    /// The tool's schema itself didn't compile. Almost always a
    /// tool-author bug (malformed `Tool::input_json_schema`). The
    /// caller should surface this as an internal error rather than
    /// a model-visible validation failure.
    #[error("tool schema failed to compile: {message}")]
    SchemaCompileFailed { message: String },
}

impl ToolSchemaValidator {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Drop the cache. Call on tool-registry reload.
    pub async fn clear(&self) {
        self.cache.write().await.clear();
    }

    /// Validate `input` against the tool's effective schema.
    ///
    /// On cache miss, compiles a `jsonschema::Validator` from
    /// [`effective_tool_schema`] and stores it keyed by
    /// `tool.id()`. Subsequent calls hit the cache.
    pub async fn validate(
        &self,
        tool: &dyn Tool,
        input: &Value,
    ) -> Result<(), SchemaValidationError> {
        let tool_id = tool.id();
        // Fast path: read-lock check.
        {
            let cache = self.cache.read().await;
            if let Some(validator) = cache.get(&tool_id) {
                return Self::validate_with(validator.as_ref(), input);
            }
        }
        // Slow path: compile + insert under write lock.
        let schema = effective_tool_schema(tool);
        let validator = jsonschema::JSONSchema::compile(&schema).map_err(|e| {
            SchemaValidationError::SchemaCompileFailed {
                message: e.to_string(),
            }
        })?;
        let validator = Arc::new(validator);
        // Another writer may have beaten us; idempotent insert.
        let mut cache = self.cache.write().await;
        let validator = cache.entry(tool_id).or_insert_with(|| validator).clone();
        Self::validate_with(validator.as_ref(), input)
    }

    /// Internal: run a single validation, aggregating errors.
    fn validate_with(
        validator: &jsonschema::JSONSchema,
        input: &Value,
    ) -> Result<(), SchemaValidationError> {
        match validator.validate(input) {
            Ok(()) => Ok(()),
            Err(errors) => {
                // Surface up to 3 errors for signal without flooding.
                let joined = errors
                    .take(3)
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                Err(SchemaValidationError::Rejected { message: joined })
            }
        }
    }
}

#[cfg(test)]
#[path = "schema.test.rs"]
mod tests;
