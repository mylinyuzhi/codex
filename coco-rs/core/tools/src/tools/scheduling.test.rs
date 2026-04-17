use super::CronCreateTool;
use super::CronListTool;
use super::is_valid_cron_expression;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use serde_json::json;

// ── R7-T22: cron expression validation tests ──
//
// TS `CronCreateTool.ts:82-103` validates the cron expression in
// `validateInput` so the model gets a clear error before the schedule
// store rejects. coco-rs implements a lightweight 5-field validator
// inline; these tests cover the grammar.

#[test]
fn test_cron_validator_accepts_basic_expressions() {
    assert!(is_valid_cron_expression("* * * * *"));
    assert!(is_valid_cron_expression("0 9 * * 1-5"));
    assert!(is_valid_cron_expression("*/5 * * * *"));
    assert!(is_valid_cron_expression("30 14 28 2 *"));
    assert!(is_valid_cron_expression("0 0,6,12,18 * * *"));
    assert!(is_valid_cron_expression("0-30 * * * *"));
}

#[test]
fn test_cron_validator_rejects_wrong_field_count() {
    assert!(!is_valid_cron_expression("* * * *")); // 4 fields
    assert!(!is_valid_cron_expression("* * * * * *")); // 6 fields
    assert!(!is_valid_cron_expression("")); // 0 fields
    assert!(!is_valid_cron_expression("hello world"));
}

#[test]
fn test_cron_validator_rejects_invalid_atoms() {
    assert!(!is_valid_cron_expression("* * * * abc"));
    assert!(!is_valid_cron_expression("*/abc * * * *"));
    assert!(!is_valid_cron_expression("5-2 * * * *")); // descending range
    assert!(!is_valid_cron_expression("* * * * /5")); // step with no base
}

#[test]
fn test_cron_create_validate_input_rejects_invalid_cron() {
    let ctx = ToolUseContext::test_default();
    let result =
        CronCreateTool.validate_input(&json!({"cron": "not a cron", "prompt": "do thing"}), &ctx);
    match result {
        ValidationResult::Invalid { message, .. } => {
            assert!(
                message.contains("Invalid cron expression"),
                "expected invalid-cron error, got: {message}"
            );
        }
        _ => panic!("expected Invalid result for malformed cron"),
    }
}

#[test]
fn test_cron_create_validate_input_requires_cron_and_prompt() {
    let ctx = ToolUseContext::test_default();
    // Empty cron.
    let result = CronCreateTool.validate_input(&json!({"prompt": "do thing"}), &ctx);
    assert!(matches!(result, ValidationResult::Invalid { .. }));
    // Empty prompt.
    let result = CronCreateTool.validate_input(&json!({"cron": "* * * * *"}), &ctx);
    assert!(matches!(result, ValidationResult::Invalid { .. }));
}

#[test]
fn test_cron_create_validate_input_accepts_valid() {
    let ctx = ToolUseContext::test_default();
    let result =
        CronCreateTool.validate_input(&json!({"cron": "*/15 * * * *", "prompt": "ping"}), &ctx);
    assert!(matches!(result, ValidationResult::Valid));
}

// ── R7-T21: CronList output shape tests ──

#[tokio::test]
async fn test_cron_list_returns_jobs_wrapper_when_empty() {
    let ctx = ToolUseContext::test_default();
    // The default test context's NoOpScheduleStore returns an empty list.
    let result = CronListTool.execute(json!({}), &ctx).await.unwrap();
    // TS shape: `{ jobs: [] }`. Not a bare array, not a string.
    assert!(
        result.data["jobs"].is_array(),
        "CronList output must be wrapped as {{ jobs: [...] }}, got: {:?}",
        result.data
    );
    assert_eq!(result.data["jobs"].as_array().unwrap().len(), 0);
}
