use super::*;
use serde_json::json;

#[test]
fn test_step_result_new() {
    let step = StepResult::new(
        0,
        "Hello".to_string(),
        Usage::default(),
        FinishReason::stop(),
    );

    assert_eq!(step.step, 0);
    assert_eq!(step.text, "Hello");
    assert!(!step.has_tool_calls());
    assert!(step.is_final());
}

#[test]
fn test_step_result_with_tools() {
    let tool_call = ToolCall::new("id_1", "test_tool", json!({}));
    let tool_result = ToolResult::new("id_1", "test_tool", json!({}));

    let step = StepResult::new(
        0,
        "".to_string(),
        Usage::default(),
        FinishReason::tool_calls(),
    )
    .with_tool_calls(vec![tool_call])
    .with_tool_results(vec![tool_result]);

    assert!(step.has_tool_calls());
    assert!(step.has_tool_results());
    assert!(!step.is_final());
}

#[test]
fn test_step_result_error() {
    let step = StepResult::error(0, "Something went wrong");

    assert!(step.is_error);
    assert_eq!(step.error_message, Some("Something went wrong".to_string()));
}

#[test]
fn test_step_result_from_content() {
    let content = vec![
        AssistantContentPart::text("Hello"),
        AssistantContentPart::text(" world"),
    ];

    let step = StepResult::from_content(0, content, Usage::default(), FinishReason::stop());

    assert_eq!(step.text, "Hello world");
}

#[test]
fn test_step_result_message_types() {
    let tool_call = ToolCall::new("id_1", "test", json!({}));
    let tool_result = ToolResult::new("id_1", "test", json!({}));

    let step = StepResult::new(
        0,
        "Result:".to_string(),
        Usage::default(),
        FinishReason::tool_calls(),
    )
    .with_tool_calls(vec![tool_call])
    .with_tool_results(vec![tool_result]);

    let types = step.message_types();
    assert!(types.contains(&"text"));
    assert!(types.contains(&"tool_calls"));
    assert!(types.contains(&"tool_results"));
}
