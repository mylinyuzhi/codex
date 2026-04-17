//! Tool schema generation: convert tool definitions to API-compatible JSON
//! schemas, merge schemas from multiple sources, and filter by model support.
//!
//! TS: src/utils/api.ts (26K LOC) — tool schema conversion, CacheScope,
//! tool filtering per model capabilities.
//!
//! The inference layer needs API-ready tool definitions. This module bridges
//! the gap between `ToolInputSchema` (coco-types) and
//! `LanguageModelV4FunctionTool` (vercel-ai-provider), handling merging from
//! built-in, MCP, and plugin sources, model-specific filtering, and token
//! estimation for context budgeting.

use std::collections::HashSet;

use serde_json::Value;
use vercel_ai_provider::language_model::v4::function_tool::LanguageModelV4FunctionTool;

use coco_types::ToolInputSchema;

/// A tool definition from any source (built-in, MCP, plugin).
#[derive(Debug, Clone)]
pub struct ToolSchemaSource {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description for the model.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: ToolInputSchema,
    /// Origin of this tool definition.
    pub origin: ToolSchemaOrigin,
}

/// Where a tool definition came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSchemaOrigin {
    Builtin,
    Mcp { server: String },
    Plugin { plugin_name: String },
}

/// Result of schema generation: the API-ready definitions plus metadata.
#[derive(Debug, Clone)]
pub struct GeneratedSchemas {
    /// Tool definitions ready for the API request.
    pub definitions: Vec<LanguageModelV4FunctionTool>,
    /// Estimated token count consumed by these definitions.
    pub estimated_tokens: i64,
}

/// Convert tool sources into API-compatible `LanguageModelV4FunctionTool` schemas.
///
/// Each `ToolSchemaSource` is mapped to the vercel-ai function tool format.
/// Properties from `ToolInputSchema` are wrapped in a JSON Schema object
/// with `type: "object"`.
pub fn generate_tool_schemas(sources: &[ToolSchemaSource]) -> GeneratedSchemas {
    let definitions: Vec<LanguageModelV4FunctionTool> = sources
        .iter()
        .map(|source| {
            let properties_value = serde_json::to_value(&source.input_schema.properties)
                .unwrap_or(Value::Object(serde_json::Map::new()));

            let mut schema_map = serde_json::Map::new();
            schema_map.insert("type".to_string(), Value::String("object".to_string()));
            if let Value::Object(props) = properties_value {
                schema_map.insert("properties".to_string(), Value::Object(props));
            }

            LanguageModelV4FunctionTool {
                name: source.name.clone(),
                description: Some(source.description.clone()),
                input_schema: Value::Object(schema_map),
                input_examples: None,
                strict: None,
                provider_options: None,
            }
        })
        .collect();

    let estimated_tokens = estimate_schema_tokens_from_definitions(&definitions);

    GeneratedSchemas {
        definitions,
        estimated_tokens,
    }
}

/// Merge tool schemas from multiple sources: built-in, MCP, and plugin.
///
/// Resolution order when names collide:
/// 1. Built-in tools always win (they are the canonical definitions)
/// 2. Plugin tools take precedence over MCP tools
/// 3. Within the same origin type, first-seen wins
///
/// MCP tools are prefixed with `mcp__<server>__` to avoid collisions.
pub fn merge_tool_schemas(
    builtin: &[ToolSchemaSource],
    mcp: &[ToolSchemaSource],
    plugin: &[ToolSchemaSource],
) -> Vec<ToolSchemaSource> {
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut merged = Vec::with_capacity(builtin.len() + mcp.len() + plugin.len());

    // Phase 1: Built-in tools (highest priority)
    for source in builtin {
        if seen_names.insert(source.name.clone()) {
            merged.push(source.clone());
        }
    }

    // Phase 2: Plugin tools
    for source in plugin {
        if seen_names.insert(source.name.clone()) {
            merged.push(source.clone());
        }
    }

    // Phase 3: MCP tools (lowest priority, typically already prefixed)
    for source in mcp {
        if seen_names.insert(source.name.clone()) {
            merged.push(source.clone());
        }
    }

    merged
}

/// Filter tool schemas based on model capabilities.
///
/// Some models do not support certain tool types (e.g., older models may not
/// support computer-use tools, some may have tool count limits). This function
/// removes tools that the given model cannot handle.
///
/// `supported_tools`: if `Some`, only tools whose names are in the set are kept.
///                    If `None`, all tools are passed through.
/// `max_tools`: optional cap on total tool count (oldest/lowest-priority dropped).
pub fn filter_schemas_by_model(
    schemas: &[ToolSchemaSource],
    supported_tools: Option<&HashSet<String>>,
    max_tools: Option<usize>,
) -> Vec<ToolSchemaSource> {
    let mut filtered: Vec<ToolSchemaSource> = schemas
        .iter()
        .filter(|s| supported_tools.is_none_or(|supported| supported.contains(&s.name)))
        .cloned()
        .collect();

    if let Some(max) = max_tools {
        filtered.truncate(max);
    }

    filtered
}

/// Estimate the number of tokens consumed by tool definitions in the prompt.
///
/// Each tool definition contributes its name, description, and JSON schema
/// properties to the token count. This rough estimate uses ~4 chars per token,
/// which is conservative for most tokenizers.
pub fn estimate_schema_tokens(sources: &[ToolSchemaSource]) -> i64 {
    sources
        .iter()
        .map(|s| {
            let name_chars = s.name.len();
            let desc_chars = s.description.len();
            let schema_chars = serde_json::to_string(&s.input_schema.properties)
                .unwrap_or_default()
                .len();
            // Each tool also has structural overhead (~50 tokens for JSON
            // wrappers, type annotations, etc.)
            let overhead = 50;
            ((name_chars + desc_chars + schema_chars) as i64 / 4) + overhead
        })
        .sum()
}

/// Estimate tokens from already-generated definitions.
fn estimate_schema_tokens_from_definitions(definitions: &[LanguageModelV4FunctionTool]) -> i64 {
    definitions
        .iter()
        .map(|d| {
            let name_chars = d.name.len();
            let desc_chars = d.description.as_ref().map_or(0, String::len);
            let schema_chars = d.input_schema.to_string().len();
            let overhead = 50;
            ((name_chars + desc_chars + schema_chars) as i64 / 4) + overhead
        })
        .sum()
}

#[cfg(test)]
#[path = "tool_schemas.test.rs"]
mod tests;
