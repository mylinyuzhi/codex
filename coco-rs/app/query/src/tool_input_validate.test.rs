use super::*;
use coco_tool_runtime::SchemaIssue;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn normalize_value_string_recovers_object() {
    let mut input = json!("{\"path\": \"/tmp\"}");
    normalize_value_string(&mut input);
    assert_eq!(input, json!({"path": "/tmp"}));
}

#[test]
fn normalize_value_string_recovers_markdown_fence() {
    let mut input = json!("```json\n{\"path\": \"/tmp\"}\n```");
    normalize_value_string(&mut input);
    assert_eq!(input, json!({"path": "/tmp"}));
}

#[test]
fn normalize_value_string_keeps_non_object_recovery() {
    // String that parses to a number — schema validator should catch
    // the type mismatch; we keep the original String so the issue is
    // visible to the model.
    let mut input = json!("42");
    normalize_value_string(&mut input);
    assert_eq!(input, json!("42"));
}

#[test]
fn normalize_value_string_passes_through_object() {
    let mut input = json!({"path": "/tmp"});
    normalize_value_string(&mut input);
    assert_eq!(input, json!({"path": "/tmp"}));
}

#[test]
fn normalize_value_string_passes_through_other_types() {
    let mut input = json!(42);
    normalize_value_string(&mut input);
    assert_eq!(input, json!(42));

    let mut input = json!([1, 2, 3]);
    normalize_value_string(&mut input);
    assert_eq!(input, json!([1, 2, 3]));
}

#[test]
fn format_schema_error_single_missing_required() {
    let issues = vec![SchemaIssue::MissingRequired {
        path: String::new(),
        field: "command".to_string(),
    }];
    let out = format_schema_error("Bash", &issues);
    assert_eq!(
        out,
        "Bash failed due to the following issue:\nThe required parameter `command` is missing"
    );
}

#[test]
fn format_schema_error_multiple_issues_pluralizes() {
    let issues = vec![
        SchemaIssue::MissingRequired {
            path: String::new(),
            field: "command".to_string(),
        },
        SchemaIssue::TypeMismatch {
            path: "/timeout".to_string(),
            expected: "number".to_string(),
            received: "string".to_string(),
        },
    ];
    let out = format_schema_error("Bash", &issues);
    assert_eq!(
        out,
        "Bash failed due to the following issues:\n\
         The required parameter `command` is missing\n\
         The parameter `timeout` type is expected as `number` but provided as `string`"
    );
}

#[test]
fn format_schema_error_unexpected_field() {
    let issues = vec![SchemaIssue::UnexpectedField {
        path: String::new(),
        field: "extra_field".to_string(),
    }];
    let out = format_schema_error("Read", &issues);
    assert_eq!(
        out,
        "Read failed due to the following issue:\nAn unexpected parameter `extra_field` was provided"
    );
}

#[test]
fn format_schema_error_nested_path() {
    let issues = vec![SchemaIssue::TypeMismatch {
        path: "/edits/0/old_string".to_string(),
        expected: "string".to_string(),
        received: "number".to_string(),
    }];
    let out = format_schema_error("MultiEdit", &issues);
    assert_eq!(
        out,
        "MultiEdit failed due to the following issue:\n\
         The parameter `edits[0].old_string` type is expected as `string` but provided as `number`"
    );
}

#[test]
fn format_schema_error_empty_falls_back() {
    let out = format_schema_error("Tool", &[]);
    assert_eq!(out, "Tool failed schema validation");
}

#[test]
fn display_path_translates_json_pointer() {
    assert_eq!(display_path(""), "");
    assert_eq!(display_path("/foo"), "foo");
    assert_eq!(display_path("/foo/bar"), "foo.bar");
    assert_eq!(display_path("/edits/0/old_string"), "edits[0].old_string");
}
