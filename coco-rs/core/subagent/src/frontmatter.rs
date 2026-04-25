//! Parse a markdown agent file into an `AgentDefinition`.
//!
//! TS: `loadAgentsDir.ts:541-755` — frontmatter shapes per-field validation.
//! Both kebab-case (`max_turns`) and TS camelCase (`maxTurns`) keys are
//! accepted; output normalizes to the `AgentDefinition` field set.

use std::path::Path;

use coco_frontmatter::FrontmatterValue;
use coco_types::{
    AgentColorName, AgentDefinition, AgentIsolation, AgentSource, AgentTypeId, MemoryScope,
};

use crate::validation::ValidationError;

/// Errors that prevent a markdown file from producing a usable definition.
#[derive(Debug, thiserror::Error)]
pub enum FrontmatterParseError {
    #[error("missing `name` frontmatter field")]
    MissingName,
    #[error("missing `description` frontmatter field")]
    MissingDescription,
    #[error("invalid frontmatter value for `{field}`: {message}")]
    InvalidValue {
        field: &'static str,
        message: String,
    },
}

/// Parse the markdown body of an agent file along with its frontmatter map.
///
/// `body` is the post-frontmatter content (the system prompt). `path` is
/// recorded as `filename` and `base_dir` for diagnostics. Unknown fields
/// are silently ignored; invalid values for known fields are surfaced as
/// `ValidationError` entries in the returned `warnings` vector — the
/// definition still loads with the field defaulted.
pub fn parse_agent_markdown(
    path: &Path,
    body: &str,
    frontmatter: &std::collections::HashMap<String, FrontmatterValue>,
    source: AgentSource,
) -> Result<(AgentDefinition, Vec<ValidationError>), FrontmatterParseError> {
    let name = read_str(frontmatter, "name").ok_or(FrontmatterParseError::MissingName)?;
    // TS `loadAgentsDir.ts:565`: `whenToUse = whenToUse.replace(/\\n/g, '\n')`.
    // YAML keeps the literal `\n` token; restore the real newline.
    let description = read_str_aliased(frontmatter, &["description", "whenToUse", "when_to_use"])
        .map(|s| s.replace("\\n", "\n"))
        .ok_or(FrontmatterParseError::MissingDescription)?;

    let body_trimmed = body.trim();
    let mut warnings = Vec::new();
    let mut def = AgentDefinition {
        agent_type: AgentTypeId::from_str_infallible(&name),
        name: name.clone(),
        when_to_use: Some(description.clone()),
        description: Some(description),
        source,
        filename: path.file_name().and_then(|s| s.to_str()).map(str::to_owned),
        base_dir: path.parent().and_then(|p| p.to_str()).map(str::to_owned),
        system_prompt: (!body_trimmed.is_empty()).then(|| body_trimmed.to_owned()),
        ..Default::default()
    };

    if let Some(model) = read_str(frontmatter, "model") {
        let normalized = if model.trim() == "inherit" {
            "inherit".to_owned()
        } else {
            model.trim().to_ascii_lowercase()
        };
        def.model = Some(normalized);
    }
    if let Some(effort) = read_str(frontmatter, "effort").or_else(|| {
        // TS `parseEffortValue` accepts `effort: 64000` numeric form too.
        read_int(frontmatter, "effort").map(|n| n.to_string())
    }) {
        match validate_effort(&effort) {
            Some(e) => def.effort = Some(e),
            None => warnings.push(ValidationError::InvalidFrontmatter {
                message: format!("effort: unrecognized value `{effort}`"),
            }),
        }
    }
    if let Some(initial) = read_str_aliased(frontmatter, &["initialPrompt", "initial_prompt"]) {
        def.initial_prompt = Some(initial);
    }
    if let Some(reminder) = read_str_aliased(
        frontmatter,
        &[
            "criticalSystemReminder",
            "criticalSystemReminder_EXPERIMENTAL",
            "critical_system_reminder",
        ],
    ) {
        def.critical_system_reminder = Some(reminder);
    }
    if let Some(identity) = read_str(frontmatter, "identity") {
        def.identity = Some(identity);
    }

    if let Some(value) = read_str_aliased(frontmatter, &["permissionMode", "permission_mode"]) {
        if VALID_PERMISSION_MODES.contains(&value.as_str()) {
            def.permission_mode = Some(value);
        } else {
            warnings.push(ValidationError::InvalidPermissionMode { value });
        }
    }

    if let Some(raw) = read_str(frontmatter, "color")
        && let Some(color) = parse_color_value(&raw, &mut warnings)
    {
        def.color = Some(color);
    }

    if let Some(raw) = read_str(frontmatter, "isolation") {
        match parse_isolation_value(&raw) {
            Ok(value) => def.isolation = value,
            Err(err) => warnings.push(err),
        }
    }

    if let Some(raw) = read_str(frontmatter, "memory") {
        match parse_memory_value(&raw) {
            Ok(value) => def.memory_scope = Some(value),
            Err(err) => warnings.push(err),
        }
    }

    if let Some(turns) = read_int_aliased(frontmatter, &["maxTurns", "max_turns"]) {
        if let Ok(n) = i32::try_from(turns)
            && n > 0
        {
            def.max_turns = Some(n);
        } else {
            warnings.push(ValidationError::InvalidMaxTurns {
                value: turns.to_string(),
            });
        }
    }

    if let Some(bg) = read_bool(frontmatter, "background") {
        def.background = bg;
    }
    if let Some(omit) = read_bool_aliased(frontmatter, &["omitClaudeMd", "omit_claude_md"]) {
        def.omit_claude_md = omit;
    }
    if let Some(exact) = read_bool_aliased(frontmatter, &["useExactTools", "use_exact_tools"]) {
        def.use_exact_tools = exact;
    }

    def.allowed_tools = read_csv_or_list_aliased(frontmatter, &["tools", "allowed_tools"])
        .map(collapse_wildcard_to_default)
        .unwrap_or_default();
    def.disallowed_tools =
        read_csv_or_list_aliased(frontmatter, &["disallowedTools", "disallowed_tools"])
            .unwrap_or_default();
    def.skills = read_csv_or_list(frontmatter, "skills").unwrap_or_default();
    def.mcp_servers =
        read_csv_or_list_aliased(frontmatter, &["mcpServers", "mcp_servers"]).unwrap_or_default();
    def.required_mcp_servers =
        read_csv_or_list_aliased(frontmatter, &["requiredMcpServers", "required_mcp_servers"])
            .unwrap_or_default();

    Ok((def, warnings))
}

/// Validate a color string against the TS `AgentColorName` set. Invalid
/// values produce a warning and the color is dropped.
pub fn parse_color_value(raw: &str, warnings: &mut Vec<ValidationError>) -> Option<AgentColorName> {
    match raw.parse::<AgentColorName>() {
        Ok(c) => Some(c),
        Err(_) => {
            warnings.push(ValidationError::InvalidColor { value: raw.into() });
            None
        }
    }
}

pub fn parse_isolation_value(raw: &str) -> Result<AgentIsolation, ValidationError> {
    raw.parse::<AgentIsolation>()
        .map_err(|_| ValidationError::InvalidIsolation { value: raw.into() })
}

pub fn parse_memory_value(raw: &str) -> Result<MemoryScope, ValidationError> {
    raw.parse::<MemoryScope>()
        .map_err(|_| ValidationError::InvalidMemoryScope { value: raw.into() })
}

fn read_str(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    key: &str,
) -> Option<String> {
    map.get(key).and_then(|v| v.as_str().map(str::to_owned))
}

fn read_int(map: &std::collections::HashMap<String, FrontmatterValue>, key: &str) -> Option<i64> {
    match map.get(key)? {
        FrontmatterValue::Int(n) => Some(*n),
        FrontmatterValue::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn read_bool(map: &std::collections::HashMap<String, FrontmatterValue>, key: &str) -> Option<bool> {
    map.get(key)
        .and_then(coco_frontmatter::FrontmatterValue::as_bool)
}

/// Try each key in order; return the first hit. Used for fields that accept
/// both camelCase (TS form) and snake_case (Rust form) keys.
fn read_str_aliased(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|k| read_str(map, k))
}

fn read_int_aliased(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    keys: &[&str],
) -> Option<i64> {
    keys.iter().find_map(|k| read_int(map, k))
}

fn read_bool_aliased(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    keys: &[&str],
) -> Option<bool> {
    keys.iter().find_map(|k| read_bool(map, k))
}

fn read_csv_or_list_aliased(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    keys: &[&str],
) -> Option<Vec<String>> {
    keys.iter().find_map(|k| read_csv_or_list(map, k))
}

/// TS `parseEffortValue` (`utils/effort.ts:71-87`) accepts the four named
/// levels plus any numeric token. Anything else is rejected.
const VALID_EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "max"];

fn validate_effort(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if VALID_EFFORT_LEVELS.contains(&trimmed) {
        return Some(trimmed.to_owned());
    }
    if trimmed.parse::<i64>().is_ok() {
        return Some(trimmed.to_owned());
    }
    None
}

/// TS `PermissionMode` (`types/permissions.ts`). Coco-rs accepts the same
/// nine variants; unrecognized values surface as a warning.
const VALID_PERMISSION_MODES: &[&str] = &[
    "default",
    "plan",
    "dontAsk",
    "acceptEdits",
    "bubble",
    "bypassPermissions",
    "auto",
    "ask",
    "deny",
];

/// TS `parseAgentToolsFromFrontmatter` (`utils/markdownConfigLoader.ts:122-124`)
/// collapses `['*']` to `undefined`, which downstream means "use the default
/// allow set = all tools". Coco-rs represents that with an empty allow-list,
/// so collapse to `vec![]` here.
fn collapse_wildcard_to_default(items: Vec<String>) -> Vec<String> {
    if items.len() == 1 && items[0].trim() == "*" {
        return Vec::new();
    }
    items
}

/// Read a frontmatter value that may be either:
/// - a YAML list (`tools:\n  - Read\n  - Edit`), or
/// - a single comma-separated string (`tools: Read, Edit`).
///
/// TS `markdownConfigLoader.ts` `parseToolListString` splits on commas; the
/// flat-string form is the most common idiom in user-authored agent files.
fn read_csv_or_list(
    map: &std::collections::HashMap<String, FrontmatterValue>,
    key: &str,
) -> Option<Vec<String>> {
    let raw = map.get(key)?.as_string_list()?;
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        // If a list element happens to contain commas, split it; otherwise
        // keep the trimmed value verbatim. Idempotent for clean YAML lists.
        if entry.contains(',') {
            out.extend(
                entry
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            );
        } else {
            let trimmed = entry.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_owned());
            }
        }
    }
    Some(out)
}

// Helper: AgentTypeId parse is infallible, but the FromStr `Err` is `Infallible`
// which is awkward at call sites. This wraps it.
trait AgentTypeIdExt {
    fn from_str_infallible(s: &str) -> Self;
}

impl AgentTypeIdExt for AgentTypeId {
    fn from_str_infallible(s: &str) -> Self {
        s.parse().unwrap_or_else(|_| AgentTypeId::Custom(s.into()))
    }
}
