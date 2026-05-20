use super::*;
use serde_json::json;

#[test]
fn empty_input_returns_empty_object() {
    let (v, outcome) = parse_tool_input("").unwrap();
    assert_eq!(v, json!({}));
    assert_eq!(outcome, ParseOutcome::Empty);

    let (v, outcome) = parse_tool_input("   \n  ").unwrap();
    assert_eq!(v, json!({}));
    assert_eq!(outcome, ParseOutcome::Empty);
}

#[test]
fn clean_input_takes_fast_path() {
    let (v, outcome) = parse_tool_input(r#"{"path": "/tmp"}"#).unwrap();
    assert_eq!(v, json!({"path": "/tmp"}));
    assert_eq!(outcome, ParseOutcome::Clean);
}

#[test]
fn unquoted_keys_repaired() {
    let (v, outcome) = parse_tool_input(r#"{path: "/tmp/foo"}"#).unwrap();
    assert_eq!(v, json!({"path": "/tmp/foo"}));
    assert_eq!(outcome, ParseOutcome::Repaired);
}

#[test]
fn trailing_commas_repaired() {
    let (v, outcome) = parse_tool_input(r#"{"a": 1, "b": 2,}"#).unwrap();
    assert_eq!(v, json!({"a": 1, "b": 2}));
    assert_eq!(outcome, ParseOutcome::Repaired);

    let (v, _) = parse_tool_input(r#"["x", "y",]"#).unwrap();
    assert_eq!(v, json!(["x", "y"]));
}

#[test]
fn missing_closing_brackets_repaired() {
    let (v, outcome) = parse_tool_input(r#"{"path": "/tmp"#).unwrap();
    assert_eq!(v, json!({"path": "/tmp"}));
    assert_eq!(outcome, ParseOutcome::Repaired);
}

#[test]
fn single_quotes_repaired() {
    // Expanded coverage from the previous hand-rolled fixer: `llm_json`
    // converts single-quoted strings to JSON-compliant double quotes.
    let (v, outcome) = parse_tool_input(r#"{'path': '/tmp/foo'}"#).unwrap();
    assert_eq!(v, json!({"path": "/tmp/foo"}));
    assert_eq!(outcome, ParseOutcome::Repaired);
}

#[test]
fn markdown_fence_stripped() {
    // Models occasionally wrap tool input in fenced code blocks despite
    // the schema spec — `llm_json` strips the fence.
    let raw = "```json\n{\"file_path\": \"/tmp\"}\n```";
    let (v, outcome) = parse_tool_input(raw).unwrap();
    assert_eq!(v, json!({"file_path": "/tmp"}));
    assert_eq!(outcome, ParseOutcome::Repaired);
}

#[test]
fn repair_does_not_corrupt_quoted_braces() {
    // Brace inside a string must NOT be counted as an opening brace.
    let (v, _) = parse_tool_input(r#"{"text": "hello {world}"}"#).unwrap();
    assert_eq!(v, json!({"text": "hello {world}"}));
}

#[test]
fn repair_handles_escaped_quotes_in_strings() {
    let (v, _) = parse_tool_input(r#"{"q": "he said \"hi\""}"#).unwrap();
    assert_eq!(v, json!({"q": r#"he said "hi""#}));
}

#[test]
fn repair_unclosed_string_then_closes_object() {
    // String never closed AND brace never closed.
    let (v, outcome) = parse_tool_input(r#"{"a": "open"#).unwrap();
    // `llm_json` closes the string and brace at the truncation point.
    assert_eq!(v, json!({"a": "open"}));
    assert_eq!(outcome, ParseOutcome::Repaired);
}
