//! `StructuredOutput` synthetic tool.
//!
//! Captures the model's final response as schema-validated JSON for SDK
//! consumers. Mirrors TS
//! [`tools/SyntheticOutputTool/SyntheticOutputTool.ts`](https://example/SyntheticOutputTool.ts):
//!
//! - Wire name is `"StructuredOutput"` (TS
//!   `SYNTHETIC_OUTPUT_TOOL_NAME = 'StructuredOutput'`).
//! - The tool's `input_schema()` **is** the user-supplied JSON schema —
//!   the model sees it via the standard tool-calling protocol and the
//!   provider's strict-tool-input validation does the first-line
//!   enforcement.
//! - `execute()` runs a second-line client-side check via the
//!   [`jsonschema`] crate; on success it forwards the input through
//!   [`ToolResult::with_structured_output`], which pushes a silent
//!   `StructuredOutput` attachment onto `new_messages` for the engine's
//!   side-channel pipeline.
//! - Tool is **never** registered by [`crate::register_all_tools`].
//!   Callers explicitly opt in via
//!   [`crate::register_structured_output_tool`] from non-interactive
//!   bootstrap paths (headless print mode, SDK NDJSON) after a
//!   `--json-schema` value has been parsed.
//!
//! TS-divergence note: TS uses Ajv for client-side validation;
//! coco-rs uses the [`jsonschema`] crate. Both validate against the same
//! Draft 7 / 2020-12 dialect range; observable behavior is equivalent
//! for the schemas the model is asked to produce.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use jsonschema::Validator;
use serde_json::Value;

/// User-facing prompt body — TS verbatim
/// (`SyntheticOutputTool.ts:50-52`).
const TOOL_PROMPT: &str = "Use this tool to return your final response in the requested structured format. You MUST call this tool exactly once at the end of your response to provide the structured output.";

/// User-facing description — TS verbatim (`SyntheticOutputTool.ts:48`).
const TOOL_DESCRIPTION: &str = "Return structured output in the requested format";

/// Compiled-validator-backed structured-output tool.
///
/// Stateless after construction: `schema` is the user-supplied JSON
/// Schema (rendered to the model via `input_schema()`), `validator` is
/// the pre-compiled validator. Clone-cheap via `Arc<Validator>`.
pub struct StructuredOutputTool {
    schema: Value,
    validator: Arc<Validator>,
}

impl std::fmt::Debug for StructuredOutputTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructuredOutputTool")
            .field("schema", &self.schema)
            .field("validator", &"<compiled>")
            .finish()
    }
}

impl StructuredOutputTool {
    /// Compile a `StructuredOutputTool` from a JSON Schema value.
    ///
    /// Returns an error string when the supplied schema fails its own
    /// meta-validation (invalid keywords, unsupported `$ref`, etc.).
    /// Mirrors TS `buildSyntheticOutputTool` which returns `{error}` for
    /// the same condition.
    pub fn new(schema: Value) -> Result<Self, String> {
        let validator =
            jsonschema::validator_for(&schema).map_err(|e| format!("invalid JSON schema: {e}"))?;
        Ok(Self {
            schema,
            validator: Arc::new(validator),
        })
    }

    /// The user-supplied JSON Schema — exposed for telemetry / `init`
    /// echo without cloning the validator.
    pub fn schema(&self) -> &Value {
        &self.schema
    }
}

#[async_trait]
impl Tool for StructuredOutputTool {
    /// Untyped input — the schema is user-supplied at runtime, not a
    /// fixed Rust type. Equivalent to TS's `z.object({}).passthrough()`.
    type Input = Value;
    /// String-typed output — `render_for_model` emits the canonical
    /// success message verbatim so the model sees the same text TS
    /// returns (`SyntheticOutputTool.ts:61`).
    type Output = String;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::StructuredOutput)
    }

    fn name(&self) -> &str {
        ToolName::StructuredOutput.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        TOOL_DESCRIPTION.into()
    }

    async fn prompt(&self, _options: &coco_tool_runtime::PromptOptions) -> String {
        TOOL_PROMPT.into()
    }

    /// Returns the user-supplied JSON Schema unchanged. The provider's
    /// strict-tool-input validation (Anthropic / OpenAI / Google) gives
    /// first-line enforcement before `execute` runs Ajv-equivalent
    /// client-side validation.
    fn input_schema(&self) -> ToolInputSchema {
        // The TS shape carries the full schema; coco-rs's
        // `ToolInputSchema` only exposes `properties` + `required`, so
        // when the user-provided schema is an object schema we forward
        // those two fields; otherwise we expose an empty
        // passthrough — same observable shape as `z.passthrough()`.
        let properties = self
            .schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let required = self
            .schema
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        ToolInputSchema {
            properties,
            required,
        }
    }

    /// Returns the user-supplied JSON Schema verbatim. The blanket default
    /// (`derive_input_schema_value::<Self::Input>()`) would derive from
    /// `Self::Input = Value`, producing a permissive schema that strict
    /// OpenAI-compatible providers (DeepSeek) reject with `type: null`.
    /// Mirrors the [`crate::tools::mcp_tools::McpTool::input_json_schema`]
    /// fix: stash the wire schema at construction, return it on demand.
    fn input_json_schema(&self) -> Option<Value> {
        Some(self.schema.clone())
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    /// Plan-mode safe — pure I/O of model-produced JSON.
    fn is_always_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("return the final response as structured JSON")
    }

    fn render_for_model(&self, out: &String) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<String>, ToolError> {
        if let Err(error) = self.validator.validate(&input) {
            let detail = format!("{}: {}", error.instance_path(), error);
            return Err(ToolError::ExecutionFailed {
                message: format!("Output does not match required schema: {detail}"),
                source: None,
            });
        }
        Ok(
            ToolResult::data("Structured output provided successfully".to_string())
                .with_structured_output(input),
        )
    }
}

#[cfg(test)]
#[path = "structured_output.test.rs"]
mod tests;
