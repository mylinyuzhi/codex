use super::*;
use std::path::PathBuf;

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct TestConfig {
    name: String,
    value: i32,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct NestedConfig {
    models: Models,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct Models {
    main: String,
}

#[test]
fn test_valid_json_deserializes_ok() {
    let json = r#"{"name": "test", "value": 42}"#;
    let path = PathBuf::from("test.json");
    let result: Result<TestConfig, _> = deserialize_json_with_diagnostics(json, &path);
    assert!(result.is_ok());
}

#[test]
fn test_unknown_field_shows_line_and_column() {
    // Use deny_unknown_fields to trigger "unknown field" errors.
    #[derive(serde::Deserialize, Debug)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Strict {
        name: String,
        value: i32,
    }

    let json = r#"{
  "name": "test",
  "valuee": 42
}"#;
    let path = PathBuf::from("config.json");
    let result: Result<Strict, _> = deserialize_json_with_diagnostics(json, &path);
    let diag = result.unwrap_err();

    assert_eq!(diag.path, PathBuf::from("config.json"));
    assert!(diag.message.contains("valuee"), "message: {}", diag.message);
    assert!(diag.range.start.line >= 1);
}

#[test]
fn test_invalid_type_error() {
    let json = r#"{"name": "test", "value": "not_a_number"}"#;
    let path = PathBuf::from("config.json");
    let result: Result<TestConfig, _> = deserialize_json_with_diagnostics(json, &path);
    let diag = result.unwrap_err();

    assert!(
        diag.message.contains("invalid type"),
        "message: {}",
        diag.message
    );
    assert_eq!(diag.range.start.line, 1);
}

#[test]
fn test_nested_path_error() {
    let json = r#"{
  "models": {
    "main": 123
  }
}"#;
    let path = PathBuf::from("config.json");
    let result: Result<NestedConfig, _> = deserialize_json_with_diagnostics(json, &path);
    let diag = result.unwrap_err();

    assert!(
        diag.serde_path.contains("models") || diag.serde_path.contains("main"),
        "serde_path: {}",
        diag.serde_path
    );
}

#[test]
fn test_empty_json_error() {
    let json = "";
    let path = PathBuf::from("empty.json");
    let result: Result<TestConfig, _> = deserialize_json_with_diagnostics(json, &path);
    assert!(result.is_err());
}

#[test]
fn test_format_diagnostic_output() {
    let json = r#"{"name": "test", "value": "not_a_number"}"#;
    let path = PathBuf::from("config.json");
    let result: Result<TestConfig, _> = deserialize_json_with_diagnostics(json, &path);
    let diag = result.unwrap_err();

    let formatted = format_diagnostic(&diag, json);

    // Should contain path:line:column header
    assert!(
        formatted.contains("config.json:"),
        "missing path header: {formatted}"
    );
    // Should contain gutter and source line
    assert!(formatted.contains('|'), "missing gutter: {formatted}");
    // Should contain caret marker
    assert!(formatted.contains('^'), "missing caret: {formatted}");
}

#[test]
fn test_format_diagnostic_single_line() {
    let diag = ConfigDiagnostic {
        path: PathBuf::from("test.json"),
        range: TextRange {
            start: TextPosition { line: 1, column: 5 },
            end: TextPosition { line: 1, column: 5 },
        },
        message: "test error".to_string(),
        serde_path: ".".to_string(),
    };

    let contents = r#"{"name": "test"}"#;
    let formatted = format_diagnostic(&diag, contents);

    assert!(formatted.contains("test.json:1:5: test error"));
    assert!(formatted.contains('^'));
}

#[test]
fn test_compute_highlight_len_string() {
    // Highlight a quoted string key
    assert_eq!(compute_highlight_len(r#"  "hello": 5"#, 2), 7); // "hello"
}

#[test]
fn test_compute_highlight_len_number() {
    assert_eq!(compute_highlight_len("  42", 2), 2);
}

#[test]
fn test_compute_highlight_len_ident() {
    assert_eq!(compute_highlight_len("  true", 2), 4);
}

#[test]
fn test_compute_highlight_len_at_end() {
    assert_eq!(compute_highlight_len("abc", 10), 1);
}
