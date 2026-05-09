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

// ── render_for_model — TS parity for answer envelopes ────────────────

mod render_tests {
    use super::AskUserQuestionTool;
    use coco_tool_runtime::Tool;
    use coco_tool_runtime::ToolResultContentPart;
    use serde_json::json;

    fn text_of(parts: &[ToolResultContentPart]) -> &str {
        match &parts[0] {
            ToolResultContentPart::Text { text, .. } => text.as_str(),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn formats_single_answer_with_trailing_continuation_clause() {
        // TS `AskUserQuestionTool.tsx:241-244`:
        // `User has answered your questions: "Q"="A". You can now
        // continue with the user's answers in mind.`
        let data = json!({
            "answers": {"What's your name?": "Alice"},
        });
        let parts = AskUserQuestionTool.render_for_model(&data);
        assert_eq!(
            text_of(&parts),
            "User has answered your questions: \"What's your name?\"=\"Alice\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn joins_multiple_answers_with_comma_space() {
        // Order is not guaranteed (HashMap-backed) so check both
        // entries are present and the boilerplate wraps them.
        let data = json!({
            "answers": {"Q1": "A1", "Q2": "A2"},
        });
        let parts = AskUserQuestionTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.starts_with("User has answered your questions: "));
        assert!(text.contains("\"Q1\"=\"A1\""));
        assert!(text.contains("\"Q2\"=\"A2\""));
        assert!(text.ends_with(". You can now continue with the user's answers in mind."));
    }

    #[test]
    fn appends_preview_and_notes_when_annotation_present() {
        // TS `AskUserQuestionTool.tsx:230-234`: `selected preview:\n...`
        // and `user notes: ...` join with single space.
        let data = json!({
            "answers": {"Pick a layout": "two-column"},
            "annotations": {
                "Pick a layout": {
                    "preview": "+----+----+",
                    "notes": "user prefers compact"
                }
            }
        });
        let parts = AskUserQuestionTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.contains("\"Pick a layout\"=\"two-column\""));
        assert!(text.contains("selected preview:\n+----+----+"));
        assert!(text.contains("user notes: user prefers compact"));
    }

    #[test]
    fn preview_only_skips_notes_clause() {
        let data = json!({
            "answers": {"Q": "A"},
            "annotations": {"Q": {"preview": "snip"}}
        });
        let parts = AskUserQuestionTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.contains("selected preview:\nsnip"));
        assert!(!text.contains("user notes:"));
    }

    #[test]
    fn missing_answers_falls_through_to_json_envelope() {
        // Pre-splicer envelope (questions only) — the TUI hasn't
        // collected answers yet. Defensive path emits the data as
        // JSON-or-string text so nothing leaks unrendered.
        let data = json!({
            "questions": [{"question": "Pick", "options": [], "header": "h"}]
        });
        let parts = AskUserQuestionTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(
            text.starts_with('{') || text.starts_with('"'),
            "expected JSON or string fallback, got: {text}"
        );
    }

    #[test]
    fn empty_answers_object_falls_through() {
        let data = json!({"answers": {}});
        let parts = AskUserQuestionTool.render_for_model(&data);
        let text = text_of(&parts);
        // The defensive `render_text_or_json` JSON-stringifies the
        // whole envelope when it can't extract a flat string.
        assert!(text.contains("\"answers\""), "got: {text}");
    }
}
