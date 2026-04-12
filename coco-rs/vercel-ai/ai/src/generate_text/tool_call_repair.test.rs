use super::*;
use serde_json::json;

fn make_tool_call(id: &str, name: &str, args: JSONValue) -> ToolCall {
    ToolCall::new(id, name, args)
}

#[tokio::test]
async fn test_json_repair_function() {
    let repair_fn = JsonRepairFunction;
    let error = ToolCallRepairError::new(
        "Invalid JSON",
        ToolCallRepairOriginalError::InvalidToolInput(InvalidToolInputError::new(
            "test",
            r#"{valid: true}"#,
        )),
    );

    let tool_call = make_tool_call("id_1", "test", json!({}));
    let result = repair_fn.repair(&tool_call, &error).await;

    // Should attempt to fix the JSON
    assert!(result.is_some());
}

#[test]
fn test_fix_missing_brackets() {
    let fixed = fix_missing_brackets(r#"{"a": 1"#);
    assert_eq!(fixed, r#"{"a": 1}"#);

    let fixed = fix_missing_brackets(r#"[1, 2, 3"#);
    assert_eq!(fixed, r#"[1, 2, 3]"#);

    let fixed = fix_missing_brackets(r#"{"a": [1, 2"#);
    assert_eq!(fixed, r#"{"a": [1, 2]}"#);
}

#[test]
fn test_fix_trailing_commas() {
    let fixed = fix_trailing_commas(r#"{"a": 1,}"#);
    assert_eq!(fixed, r#"{"a": 1}"#);

    let fixed = fix_trailing_commas(r#"[1, 2, 3,]"#);
    assert_eq!(fixed, r#"[1, 2, 3]"#);
}

#[test]
fn test_json_type_name() {
    assert_eq!(json_type_name(&json!(null)), "null");
    assert_eq!(json_type_name(&json!(true)), "boolean");
    assert_eq!(json_type_name(&json!(42)), "number");
    assert_eq!(json_type_name(&json!("hello")), "string");
    assert_eq!(json_type_name(&json!([])), "array");
    assert_eq!(json_type_name(&json!({})), "object");
}

#[tokio::test]
async fn test_repair_tool_call() {
    let repair_fn = JsonRepairFunction;
    let tool_call = make_tool_call("id_1", "test", json!({}));
    let error = ToolCallRepairError::new(
        "Invalid JSON",
        ToolCallRepairOriginalError::InvalidToolInput(InvalidToolInputError::new(
            "test",
            r#"{a: 1}"#,
        )),
    );

    let result = repair_tool_call(&tool_call, &error, &repair_fn).await;

    match result {
        RepairResult::Repaired(repaired) => {
            assert_eq!(repaired.tool_name, "test");
        }
        _ => panic!("Expected repaired result"),
    }
}