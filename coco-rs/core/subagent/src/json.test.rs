use super::*;
use coco_types::{AgentColorName, AgentSource};
use pretty_assertions::assert_eq;

/// Helper that synthesises a minimal valid JSON entry. Both
/// `description` and `prompt` are required by TS
/// `AgentJsonSchema`, so test cases that don't care about either
/// can build through this and override only the fields they
/// exercise.
fn entry_with(extra: serde_json::Value) -> serde_json::Value {
    let mut base = serde_json::json!({
        "description": "test agent",
        "prompt": "you are a test agent",
    });
    if let (Some(map), Some(extras)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in extras {
            map.insert(k.clone(), v.clone());
        }
    }
    base
}

#[test]
fn parse_single_agent_with_required_fields() {
    let entry = serde_json::json!({
        "description": "Hand-rolled JSON agent",
        "prompt": "You are a helpful agent.",
        "model": "haiku",
    });
    let (def, warnings) = parse_agent_json("custom-json", &entry, AgentSource::FlagSettings)
        .expect("required fields present");
    assert!(warnings.is_empty(), "no warnings expected: {warnings:?}");
    assert_eq!(def.name, "custom-json");
    assert_eq!(def.when_to_use.as_deref(), Some("Hand-rolled JSON agent"));
    assert_eq!(def.model.as_deref(), Some("haiku"));
    assert_eq!(
        def.system_prompt.as_deref(),
        Some("You are a helpful agent.")
    );
    assert_eq!(def.source, AgentSource::FlagSettings);
}

#[test]
fn parse_propagates_camelcase_and_snakecase_aliases() {
    let entry = entry_with(serde_json::json!({
        "maxTurns": 7,
        "permissionMode": "acceptEdits",
        "initialPrompt": "first turn prefix",
    }));
    let (def, _warn) = parse_agent_json("aliased", &entry, AgentSource::FlagSettings).unwrap();
    assert_eq!(def.max_turns, Some(7));
    assert_eq!(def.permission_mode.as_deref(), Some("acceptEdits"));
    assert_eq!(def.initial_prompt.as_deref(), Some("first turn prefix"));
}

#[test]
fn parse_arrays_and_csv_for_tools() {
    let entry = entry_with(serde_json::json!({
        "tools": ["Read", "Edit"],
        "disallowedTools": "Write, NotebookEdit",
    }));
    let (def, _warn) = parse_agent_json("arr", &entry, AgentSource::FlagSettings).unwrap();
    assert_eq!(
        def.allowed_tools,
        coco_types::ToolAllowList::Explicit(vec!["Read".into(), "Edit".into()])
    );
    assert_eq!(def.disallowed_tools, vec!["Write", "NotebookEdit"]);
}

#[test]
fn parse_wildcard_tools_collapses_to_default() {
    // TS `parseAgentToolsFromFrontmatter`: ['*'] → undefined (use default
    // allow set). JSON path inherits the same behaviour via the shared
    // markdown parser.
    let entry = entry_with(serde_json::json!({ "tools": ["*"] }));
    let (def, _warn) = parse_agent_json("wild", &entry, AgentSource::FlagSettings).unwrap();
    assert!(def.allowed_tools.is_wildcard());
}

#[test]
fn parse_inline_mcp_servers_uses_inline_form() {
    let entry = entry_with(serde_json::json!({
        "mcpServers": [
            {"slack": {"command": "./mcp-slack"}},
            "github",
        ],
    }));
    let (def, _warn) = parse_agent_json("mcp", &entry, AgentSource::FlagSettings).unwrap();
    assert_eq!(def.mcp_servers.len(), 2);
    assert!(matches!(
        def.mcp_servers[0],
        coco_types::AgentMcpServerSpec::Inline(_)
    ));
    assert!(matches!(
        def.mcp_servers[1],
        coco_types::AgentMcpServerSpec::Name(_)
    ));
}

#[test]
fn parse_invalid_color_warns_and_drops() {
    let entry = entry_with(serde_json::json!({ "color": "chartreuse" }));
    let (def, warnings) = parse_agent_json("bad", &entry, AgentSource::FlagSettings).unwrap();
    assert!(def.color.is_none());
    assert!(matches!(
        warnings.as_slice(),
        [crate::ValidationError::InvalidColor { .. }]
    ));
}

#[test]
fn parse_valid_color_round_trips() {
    let entry = entry_with(serde_json::json!({ "color": "purple" }));
    let (def, warnings) = parse_agent_json("good", &entry, AgentSource::FlagSettings).unwrap();
    assert_eq!(def.color, Some(AgentColorName::Purple));
    assert!(warnings.is_empty());
}

#[test]
fn parse_missing_description_errors() {
    // TS `AgentJsonSchema`: `description: z.string().min(1)`.
    let entry = serde_json::json!({
        "prompt": "no description here",
    });
    let err = parse_agent_json("nodesc", &entry, AgentSource::FlagSettings).unwrap_err();
    assert!(matches!(err, FrontmatterParseError::MissingDescription));
}

#[test]
fn parse_empty_description_errors() {
    // TS `z.string().min(1, 'Description cannot be empty')` — empty string
    // fails Zod parse with the same message; Rust mirrors via the same
    // `MissingDescription` variant.
    let entry = serde_json::json!({
        "description": "",
        "prompt": "you are a test agent",
    });
    let err = parse_agent_json("empty-desc", &entry, AgentSource::FlagSettings).unwrap_err();
    assert!(matches!(err, FrontmatterParseError::MissingDescription));
}

#[test]
fn parse_missing_prompt_errors() {
    // TS `AgentJsonSchema`: `prompt: z.string().min(1, 'Prompt cannot be empty')`.
    // Rust mirrors as `InvalidValue { field: "prompt", message: ... }`.
    let entry = serde_json::json!({
        "description": "no prompt here",
    });
    let err = parse_agent_json("noprompt", &entry, AgentSource::FlagSettings).unwrap_err();
    assert!(matches!(
        err,
        FrontmatterParseError::InvalidValue {
            field: "prompt",
            ..
        }
    ));
}

#[test]
fn parse_empty_prompt_errors() {
    let entry = serde_json::json!({
        "description": "ok",
        "prompt": "   ",
    });
    let err = parse_agent_json("emptyprompt", &entry, AgentSource::FlagSettings).unwrap_err();
    assert!(matches!(
        err,
        FrontmatterParseError::InvalidValue {
            field: "prompt",
            ..
        }
    ));
}

#[test]
fn parse_non_object_input_errors() {
    let entry = serde_json::json!("not an object");
    let err = parse_agent_json("scalar", &entry, AgentSource::FlagSettings).unwrap_err();
    assert!(matches!(
        err,
        FrontmatterParseError::InvalidValue {
            field: "definition",
            ..
        }
    ));
}

#[test]
fn parse_agents_json_skips_failed_entries() {
    let agents = serde_json::json!({
        "good": { "description": "ok", "prompt": "body" },
        "bad":  { "prompt": "no description" },
    });
    let parsed = parse_agents_json(&agents, AgentSource::FlagSettings);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "good");
}

#[test]
fn parse_agents_json_returns_empty_for_non_object() {
    let agents = serde_json::json!(["not", "a", "map"]);
    assert!(parse_agents_json(&agents, AgentSource::FlagSettings).is_empty());
}

#[test]
fn parse_ignores_inner_name_key() {
    // Hostile JSON might set an inner `name` field; the outer key
    // wins so a payload like `{"foo": {"name": "bar", ...}}` still
    // results in `agent_type = "foo"`.
    let entry = entry_with(serde_json::json!({ "name": "spoof" }));
    let (def, _warn) = parse_agent_json("real", &entry, AgentSource::FlagSettings).unwrap();
    assert_eq!(def.name, "real");
}
