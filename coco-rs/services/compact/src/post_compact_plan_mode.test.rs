use super::*;
use coco_context::Phase4Variant;
use coco_context::PlanWorkflow;

fn fresh_attachment() -> PlanModeAttachment {
    PlanModeAttachment {
        reminder_type: ReminderType::Sparse,
        workflow: PlanWorkflow::FivePhase,
        phase4_variant: Phase4Variant::Standard,
        explore_agent_count: 3,
        plan_agent_count: 1,
        explore_plan_agents_available: true,
        is_sub_agent: false,
        plan_file_path: "/tmp/plan.md".to_string(),
        plan_exists: false,
        write_tool: coco_types::ToolName::Write,
        edit_tool: coco_types::ToolName::Edit,
    }
}

#[test]
fn returns_none_when_not_in_plan_mode() {
    let result = create_plan_mode_attachment_if_needed(false, fresh_attachment());
    assert!(
        result.is_none(),
        "non-plan-mode sessions emit no attachment"
    );
}

#[test]
fn returns_full_reminder_when_in_plan_mode() {
    let result = create_plan_mode_attachment_if_needed(true, fresh_attachment())
        .expect("plan-mode session must emit attachment");
    let LlmMessage::User { content, .. } = result.as_api_message().unwrap() else {
        panic!("attachment message should be a User LlmMessage");
    };
    let text = match &content[0] {
        coco_llm_types::UserContentPart::Text(t) => &t.text,
        _ => panic!("expected text part"),
    };
    assert!(
        text.starts_with("<system-reminder>\n"),
        "post-compact plan_mode must be SR-wrapped"
    );
    assert!(
        text.ends_with("</system-reminder>"),
        "post-compact plan_mode must be SR-wrapped"
    );
}

#[test]
fn forces_reminder_type_full_regardless_of_input() {
    let mut attachment = fresh_attachment();
    attachment.reminder_type = ReminderType::Sparse;
    let result =
        create_plan_mode_attachment_if_needed(true, attachment).expect("must emit attachment");
    // We can't directly inspect the rendered ReminderType, but the rendered
    // text differs between Full and Sparse — Full is markedly longer.
    let LlmMessage::User { content, .. } = result.as_api_message().unwrap() else {
        panic!("expected User LlmMessage");
    };
    let text = match &content[0] {
        coco_llm_types::UserContentPart::Text(t) => &t.text,
        _ => panic!("expected text part"),
    };
    // Full reminder mentions "Phase" of the workflow; Sparse doesn't.
    assert!(
        text.contains("Phase") || text.len() > 200,
        "post-compact plan_mode must render the FULL reminder text \
         (caller-provided Sparse must be coerced to Full)"
    );
}
