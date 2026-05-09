use super::CronCreateTool;
use super::CronListTool;
use super::is_valid_cron_expression;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
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

// ── render_for_model — TS parity for cron tool envelopes ──────────────

#[test]
fn cron_create_render_recurring_durable() {
    use coco_tool_runtime::ToolResultContentPart;
    let data = json!({
        "id": "abc-123",
        "humanSchedule": "every Monday at 09:00",
        "recurring": true,
        "durable": true,
        "status": "created",
    });
    let parts = CronCreateTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    // TS `CronCreateTool.ts:151` recurring branch: id, schedule,
    // durable persistence string, TTL-in-days, CronDelete hint.
    assert!(
        text.starts_with("Scheduled recurring job abc-123"),
        "got: {text}"
    );
    assert!(text.contains("(every Monday at 09:00)"), "got: {text}");
    assert!(
        text.contains("Persisted to .claude/scheduled_tasks.json"),
        "got: {text}"
    );
    assert!(text.contains("Auto-expires after 7 days"), "got: {text}");
    assert!(text.contains("CronDelete"), "got: {text}");
}

#[test]
fn cron_create_render_one_shot_in_memory() {
    use coco_tool_runtime::ToolResultContentPart;
    let data = json!({
        "id": "x",
        "humanSchedule": "Feb 28 14:30",
        "recurring": false,
        "durable": false,
        "status": "created",
    });
    let parts = CronCreateTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    // TS `CronCreateTool.ts:152` one-shot branch.
    assert!(text.starts_with("Scheduled one-shot task x"), "got: {text}");
    assert!(text.contains("Session-only"), "got: {text}");
    assert!(
        text.contains("It will fire once then auto-delete."),
        "got: {text}"
    );
}

#[test]
fn cron_delete_render_uses_cancelled_verb() {
    use super::CronDeleteTool;
    use coco_tool_runtime::ToolResultContentPart;
    // TS `CronDeleteTool.ts:90`: `Cancelled job ${id}.`.
    let data = json!({"id": "job-42"});
    let parts = CronDeleteTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert_eq!(text, "Cancelled job job-42.");
}

#[test]
fn cron_list_render_empty_branch() {
    use coco_tool_runtime::ToolResultContentPart;
    let data = json!({"jobs": []});
    let parts = CronListTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert_eq!(text, "No scheduled tasks.");
}

#[test]
fn cron_list_render_summarizes_jobs() {
    use coco_tool_runtime::ToolResultContentPart;
    let data = json!({
        "jobs": [
            {"id": "job-1", "humanSchedule": "every 5 min", "prompt": "ping"},
            {"id": "job-2", "humanSchedule": "Monday 9am", "prompt": "weekly review"},
        ]
    });
    let parts = CronListTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with("2 scheduled tasks:"), "got: {text}");
    assert!(text.contains("job-1: every 5 min → ping"), "got: {text}");
    assert!(
        text.contains("job-2: Monday 9am → weekly review"),
        "got: {text}"
    );
}
