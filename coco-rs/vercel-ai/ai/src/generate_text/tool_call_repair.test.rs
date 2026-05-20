use super::*;
use serde_json::json;

fn make_tool_call(id: &str, name: &str, args: JSONValue) -> ToolCall {
    ToolCall::new(id, name, args)
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
async fn test_repair_tool_call_dispatch_returns_repaired() {
    // Exercise the `repair_tool_call` dispatch with a caller-supplied
    // `CustomRepairFunction`. The SDK ships no default fixer
    // (parity with TS `@ai-sdk/ai`), so the test verifies the dispatch
    // wiring, not any built-in repair strategy.
    let tool_call = make_tool_call("id_1", "test", json!({}));
    let error = ToolCallRepairError::new(
        "Invalid JSON",
        ToolCallRepairOriginalError::InvalidToolInput(InvalidToolInputError::new(
            "test",
            r#"{a: 1}"#,
        )),
    );

    let repair_fn = CustomRepairFunction::new(|tc, _err| {
        Some(ToolCall::new(
            &tc.tool_call_id,
            &tc.tool_name,
            json!({"a": 1}),
        ))
    });
    let result = repair_tool_call(&tool_call, &error, &repair_fn).await;
    match result {
        RepairResult::Repaired(repaired) => {
            assert_eq!(repaired.tool_name, "test");
            assert_eq!(repaired.args, json!({"a": 1}));
        }
        _ => panic!("Expected Repaired result"),
    }
}

#[tokio::test]
async fn test_repair_tool_call_dispatch_returns_cannot_repair_when_callback_returns_none() {
    let tool_call = make_tool_call("id_1", "test", json!({}));
    let error = ToolCallRepairError::new(
        "Invalid JSON",
        ToolCallRepairOriginalError::InvalidToolInput(InvalidToolInputError::new(
            "test",
            r#"{a: 1}"#,
        )),
    );

    let repair_fn = CustomRepairFunction::new(|_tc, _err| None);
    let result = repair_tool_call(&tool_call, &error, &repair_fn).await;
    match result {
        RepairResult::CannotRepair { .. } => {}
        _ => panic!("Expected CannotRepair result when callback returns None"),
    }
}
