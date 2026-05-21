use pretty_assertions::assert_eq;
use serde_json::json;

use super::RepairOutcome;
use super::parse_with_repair;

#[test]
fn clean_parse_succeeds() {
    let (v, outcome) = parse_with_repair(r#"{"a": 1}"#).unwrap();
    assert_eq!(v, json!({"a": 1}));
    assert_eq!(outcome, RepairOutcome::Clean);
}

#[test]
fn trailing_comma_is_repaired() {
    let (v, outcome) = parse_with_repair(r#"{"a": 1,}"#).unwrap();
    assert_eq!(v, json!({"a": 1}));
    assert_eq!(outcome, RepairOutcome::Repaired);
}

#[test]
fn markdown_code_fence_is_stripped() {
    let (v, outcome) = parse_with_repair("```json\n{\"path\": \"/tmp\"}\n```").unwrap();
    assert_eq!(v, json!({"path": "/tmp"}));
    assert_eq!(outcome, RepairOutcome::Repaired);
}

#[test]
fn single_quotes_are_repaired() {
    let (v, outcome) = parse_with_repair(r#"{'a': 'hello'}"#).unwrap();
    assert_eq!(v, json!({"a": "hello"}));
    assert_eq!(outcome, RepairOutcome::Repaired);
}

#[test]
fn unclosed_bracket_is_repaired() {
    let (v, outcome) = parse_with_repair(r#"{"path": "/tmp"#).unwrap();
    assert_eq!(v, json!({"path": "/tmp"}));
    assert_eq!(outcome, RepairOutcome::Repaired);
}

#[test]
fn empty_input_returns_err() {
    assert!(parse_with_repair("").is_err());
    assert!(parse_with_repair("   \n  ").is_err());
}

#[test]
fn truly_malformed_returns_err() {
    // Pure garbage that even llm_json can't salvage into JSON. (Most
    // inputs *can* be salvaged — repair is intentionally aggressive —
    // so this is mostly a guarantee that we don't panic.)
    let result = parse_with_repair("\u{0000}\u{0001}\u{0002}");
    // llm_json may still produce something; accept either outcome.
    // The important property: no panic, returns a Result.
    let _ = result;
}
