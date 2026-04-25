use super::AskUserQuestionTool;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::PromptOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn name_matches_tool_name_enum() {
    let t = AskUserQuestionTool;
    assert_eq!(t.name(), coco_types::ToolName::AskUserQuestion.as_str());
}

#[test]
fn description_is_ts_aligned() {
    let t = AskUserQuestionTool;
    let d = t.description(&json!({}), &DescriptionOptions::default());
    // Must mention the TS DESCRIPTION's distinctive phrase.
    assert!(
        d.contains("multiple choice questions"),
        "description should match TS DESCRIPTION: {d}"
    );
}

#[tokio::test]
async fn prompt_includes_plan_mode_and_preview_guidance() {
    let t = AskUserQuestionTool;
    let p = t.prompt(&PromptOptions::default()).await;
    // Plan-mode guidance must call out ExitPlanMode by name — sourced
    // from `ToolName::ExitPlanMode.as_str()` so the test will fail
    // loudly if the enum variant is renamed without updating callers.
    let exit = coco_types::ToolName::ExitPlanMode.as_str();
    assert!(p.contains(exit), "prompt missing {exit} reference: {p}");
    // The IMPORTANT block must stay intact — TS calls this out
    // explicitly because the model otherwise asks meta-questions about
    // an invisible plan.
    assert!(
        p.contains("Do not reference \"the plan\""),
        "prompt missing IMPORTANT block on plan-reference avoidance"
    );
    // Preview-feature section must be present with the TS markdown wording.
    assert!(
        p.contains("Preview feature:"),
        "prompt missing preview feature section"
    );
    assert!(
        p.contains("multiSelect"),
        "prompt missing multiSelect guidance"
    );
    // "Other" option note from TS usage notes.
    assert!(p.contains("\"Other\""), "prompt missing Other option note");
    // Recommendation rule from TS usage notes.
    assert!(
        p.contains("(Recommended)"),
        "prompt missing recommendation guidance"
    );
}

#[test]
fn input_schema_has_questions_array() {
    let t = AskUserQuestionTool;
    let schema = t.input_schema();
    let questions = schema
        .properties
        .get("questions")
        .expect("questions property missing");
    assert_eq!(questions["type"], "array");
    assert_eq!(questions["minItems"], 1);
    assert_eq!(questions["maxItems"], 4);
    let items = &questions["items"];
    let required = items["required"]
        .as_array()
        .expect("items.required missing");
    assert!(required.iter().any(|v| v == "question"));
    assert!(required.iter().any(|v| v == "header"));
    assert!(required.iter().any(|v| v == "options"));
}

#[test]
fn tool_requires_user_interaction_and_is_concurrency_safe() {
    let t = AskUserQuestionTool;
    assert!(t.requires_user_interaction());
    assert!(t.is_concurrency_safe(&json!({})));
}

#[tokio::test]
async fn execute_echoes_questions_payload() {
    let t = AskUserQuestionTool;
    let ctx = ToolUseContext::test_default();
    let input = json!({
        "questions": [{
            "question": "Pick one",
            "header": "q1",
            "options": [
                {"label": "A", "description": "first"},
                {"label": "B", "description": "second"}
            ]
        }]
    });
    let out = t.execute(input.clone(), &ctx).await.expect("execute ok");
    assert_eq!(out.data["questions"], input["questions"]);
}
