//! Tests for parse_tool_call.rs

use super::*;

#[test]
fn test_parse_empty_input() {
    let result = parse_tool_call_input("").unwrap();
    assert!(result.is_object());
    assert!(result.as_object().unwrap().is_empty());
}

#[test]
fn test_parse_valid_json() {
    let result = parse_tool_call_input(r#"{"name": "test"}"#).unwrap();
    assert_eq!(result["name"], "test");
}

#[test]
fn test_parse_invalid_json() {
    let result = parse_tool_call_input("not json");
    assert!(result.is_err());
}
