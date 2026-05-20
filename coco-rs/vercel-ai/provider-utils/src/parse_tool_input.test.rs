use super::*;
use serde_json::json;
use std::sync::Arc;
use vercel_ai_provider::{CustomToolInputParseFunction, ToolInputParseError, ToolInputParseResult};

#[test]
fn empty_input_yields_empty_object_not_failure() {
    let out = parse_tool_call_arguments("", None, "Read");
    assert_eq!(out.value, json!({}));
    assert!(!out.invalid);

    let out = parse_tool_call_arguments("  \n\t  ", None, "Read");
    assert_eq!(out.value, json!({}));
    assert!(!out.invalid);
}

#[test]
fn strict_parse_path_succeeds_without_callback() {
    let out = parse_tool_call_arguments(r#"{"path": "/tmp"}"#, None, "Read");
    assert_eq!(out.value, json!({"path": "/tmp"}));
    assert!(!out.invalid);
}

#[test]
fn strict_parse_path_marks_invalid_on_failure() {
    let out = parse_tool_call_arguments(r#"{path: /tmp"#, None, "Read");
    assert_eq!(out.value, json!(null));
    assert!(out.invalid);
}

#[test]
fn callback_path_uses_provided_parser() {
    let parser: ToolInputParseHandle = Arc::new(CustomToolInputParseFunction::new(|raw: &str| {
        // Stub repair: replace `{a: 1}` with `{"a": 1}`.
        if raw == "{a: 1}" {
            Ok(ToolInputParseResult::repaired(json!({"a": 1})))
        } else {
            serde_json::from_str::<serde_json::Value>(raw)
                .map(ToolInputParseResult::clean)
                .map_err(|e| ToolInputParseError::Parse(e.to_string()))
        }
    }));
    let out = parse_tool_call_arguments(r#"{a: 1}"#, Some(&parser), "Tool");
    assert_eq!(out.value, json!({"a": 1}));
    assert!(!out.invalid);
}

#[test]
fn callback_failure_marks_invalid() {
    let parser: ToolInputParseHandle = Arc::new(CustomToolInputParseFunction::new(|_raw: &str| {
        Err(ToolInputParseError::Repair("nope".into()))
    }));
    let out = parse_tool_call_arguments(r#"garbage"#, Some(&parser), "Tool");
    assert_eq!(out.value, json!(null));
    assert!(out.invalid);
}
