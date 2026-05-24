//! In-process JSON agent definitions.
//!
//! TS source: `loadAgentsDir.ts:445-536` — `parseAgentFromJson` and
//! `parseAgentsFromJson`. JSON agents arrive via two channels:
//!
//! - CLI flag `--agents '<json>'` (TS `main.tsx:2039`) — coco-rs
//!   equivalent maps to `--agents` plumbed by the CLI.
//! - SDK `initialize.agents` (TS `cli/print.ts:4382`) — coco-rs SDK
//!   bootstrap calls into [`parse_agents_json`] before installing the
//!   resulting definitions on the [`crate::AgentDefinitionStore`] via
//!   `insert_definition`.
//!
//! Field set matches the markdown frontmatter parser exactly — JSON
//! agents and markdown agents land in the same `AgentDefinition`
//! shape. The JSON top-level key becomes the `agentType` and the
//! `prompt` field becomes the system prompt body, mirroring TS
//! `parseAgentFromJson:476-489`.
//!
//! This module deliberately re-uses [`crate::frontmatter::parse_agent_markdown`]
//! by translating each JSON entry into the same frontmatter map shape
//! the markdown parser consumes. That keeps every per-field validation
//! rule (camelCase aliasing, color whitelist, isolation enum, effort
//! parsing, mcpServers shape, hooks pass-through) byte-faithful with
//! the markdown path — there is no second copy of those rules.

use std::collections::HashMap;
use std::path::PathBuf;

use coco_frontmatter::FrontmatterValue;
use coco_types::{AgentDefinition, AgentSource};
use serde_json::Value;

use crate::frontmatter::{FrontmatterParseError, parse_agent_markdown};
use crate::validation::ValidationError;

/// Parse a single JSON agent entry. `name` is the top-level map key
/// (becomes `agentType`); `definition` is the JSON object describing
/// the agent.
///
/// Returns the parsed [`AgentDefinition`] paired with any per-field
/// warnings (e.g. an invalid color was dropped). `Err` is reserved for
/// hard failures (missing `description`, malformed JSON shape) — those
/// callers should surface as `failed` diagnostics.
///
/// Source defaults to [`AgentSource::FlagSettings`] to match TS
/// `parseAgentFromJson(name, definition, source = 'flagSettings')`.
pub fn parse_agent_json(
    name: &str,
    definition: &Value,
    source: AgentSource,
) -> Result<(AgentDefinition, Vec<ValidationError>), FrontmatterParseError> {
    let Some(map) = definition.as_object() else {
        return Err(FrontmatterParseError::InvalidValue {
            field: "definition",
            message: format!(
                "expected JSON object at agent `{name}`, got {}",
                describe_json_kind(definition)
            ),
        });
    };

    // Extract `prompt` as the body — TS `parsed.prompt` becomes
    // `getSystemPrompt`. TS `AgentJsonSchema` declares
    // `prompt: z.string().min(1, 'Prompt cannot be empty')`, so a
    // missing-or-empty `prompt` rejects the whole entry. Built-ins
    // ship dynamic prompts, but they don't go through this JSON path —
    // built-ins live in `crate::builtins` and are constructed in-process.
    let body = match map.get("prompt").and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => s.to_owned(),
        _ => {
            return Err(FrontmatterParseError::InvalidValue {
                field: "prompt",
                message: "Prompt cannot be empty".to_owned(),
            });
        }
    };

    // Translate every other JSON key to a `FrontmatterValue`. The
    // markdown parser uses the same key names + camelCase aliases, so
    // no per-field re-mapping is needed beyond JSON → frontmatter
    // value type conversion.
    let mut frontmatter: HashMap<String, FrontmatterValue> = HashMap::with_capacity(map.len() + 1);
    // Inject `name` (the JSON key) so the frontmatter parser populates
    // `agent_type`. JSON agents store this externally rather than in
    // the agent body.
    frontmatter.insert("name".to_owned(), FrontmatterValue::String(name.to_owned()));
    for (key, value) in map {
        // `prompt` already extracted above as the body.
        if key == "prompt" {
            continue;
        }
        // `name` would shadow our injected agent_type if a hostile
        // JSON entry sets it; preserve the JSON-key as authoritative.
        if key == "name" {
            continue;
        }
        frontmatter.insert(key.clone(), json_to_frontmatter(value));
    }

    // Synthesise a stable diagnostic path so per-file warnings carry
    // a `<json:agentType>` provenance tag instead of an empty
    // `PathBuf`. Not used for IO.
    let synthetic_path = PathBuf::from(format!("<json:{name}>"));
    parse_agent_markdown(&synthetic_path, &body, &frontmatter, source)
}

/// Parse a top-level JSON map of `{ name: definition }` agent entries.
/// Per-entry parse failures are skipped (not propagated) — TS
/// `parseAgentsFromJson:530-535` swallows individual errors and
/// continues, returning whatever entries parsed cleanly.
///
/// Use [`parse_agent_json`] directly when you need per-entry error
/// handling (e.g. surfacing failed entries to the user).
pub fn parse_agents_json(agents: &Value, source: AgentSource) -> Vec<AgentDefinition> {
    let Some(map) = agents.as_object() else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, def)| match parse_agent_json(name, def, source) {
            Ok((d, _warnings)) => Some(d),
            Err(err) => {
                tracing::debug!(
                    target: "coco_subagent",
                    %name,
                    error = %err,
                    "skipping JSON agent: parse failed"
                );
                None
            }
        })
        .collect()
}

fn describe_json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Translate `serde_json::Value` to `FrontmatterValue`. Mappings
/// preserve TS shape so the markdown parser's per-field validators
/// see the same payload they would for a YAML-frontmatter agent.
fn json_to_frontmatter(v: &Value) -> FrontmatterValue {
    match v {
        Value::Null => FrontmatterValue::Null,
        Value::Bool(b) => FrontmatterValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FrontmatterValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FrontmatterValue::Float(f)
            } else {
                FrontmatterValue::String(n.to_string())
            }
        }
        Value::String(s) => FrontmatterValue::String(s.clone()),
        Value::Array(items) => {
            FrontmatterValue::Sequence(items.iter().map(json_to_frontmatter).collect::<Vec<_>>())
        }
        Value::Object(map) => FrontmatterValue::Mapping(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_frontmatter(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
#[path = "json.test.rs"]
mod tests;
