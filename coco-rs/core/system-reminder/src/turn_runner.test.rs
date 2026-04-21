use super::*;
use crate::types::AttachmentType;
use coco_config::SystemReminderConfig;
use coco_context::Phase4Variant;
use coco_context::PlanWorkflow;
use coco_messages::MessageHistory;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use coco_types::ToolName;
use pretty_assertions::assert_eq;

fn minimal_input<'a>(
    config: &'a SystemReminderConfig,
    app_state: &'a ToolAppState,
    history: &'a MessageHistory,
) -> TurnReminderInput<'a> {
    TurnReminderInput {
        config,
        turn_number: 0,
        agent_id: None,
        user_input: None,
        last_human_turn_uuid: None,
        plan_file_path: None,
        plan_exists: false,
        plan_workflow: PlanWorkflow::FivePhase,
        phase4_variant: Phase4Variant::Standard,
        explore_agent_count: 3,
        plan_agent_count: 1,
        is_plan_interview_phase: false,
        app_state,
        fallback_permission_mode: PermissionMode::Default,
        is_auto_classifier_active: false,
        tools: vec![ToolName::Read.as_str().to_string()],
        is_task_v2_enabled: false,
        history,
        todo_key: "session-1".to_string(),
        is_auto_compact_enabled: false,
        context_window: 200_000,
        effective_context_window: 180_000,
        used_tokens: 10_000,
        new_date: None,
        has_pending_plan_verification: false,
        total_cost_usd: 0.0,
        max_budget_usd: None,
        output_tokens_turn: 0,
        output_tokens_session: 0,
        output_token_budget: None,
        companion_name: None,
        companion_species: None,
        has_prior_companion_intro: false,
        deferred_tools_delta: None,
        agent_listing_delta: None,
        mcp_instructions_delta: None,
        hook_events: Vec::new(),
        diagnostics: Vec::new(),
        output_style: None,
        queued_commands: Vec::new(),
        task_statuses: Vec::new(),
        skill_listing: None,
        invoked_skills: Vec::new(),
        teammate_mailbox: None,
        team_context: None,
        agent_pending_messages: Vec::new(),
        at_mentioned_files: Vec::new(),
        mcp_resources: Vec::new(),
        agent_mentions: Vec::new(),
        ide_selection: None,
        ide_opened_file: None,
        nested_memories: Vec::new(),
        relevant_memories: Vec::new(),
        already_read_file_paths: Vec::new(),
        edited_image_file_paths: Vec::new(),
    }
}

#[tokio::test]
async fn no_generators_produces_empty() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState::default();
    let history = MessageHistory::default();
    let orchestrator = SystemReminderOrchestrator::new(config.clone());
    let out = run_turn_reminders(&orchestrator, minimal_input(&config, &app_state, &history)).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn default_registry_fires_plan_reminder_in_plan_mode() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState {
        permission_mode: Some(PermissionMode::Plan),
        ..Default::default()
    };
    let history = MessageHistory::default();

    let orchestrator = SystemReminderOrchestrator::new(config.clone()).with_default_generators();
    let out = run_turn_reminders(&orchestrator, minimal_input(&config, &app_state, &history)).await;

    let types: std::collections::HashSet<_> = out.iter().map(|r| r.attachment_type).collect();
    assert!(types.contains(&AttachmentType::PlanMode), "got: {types:?}");
}

#[tokio::test]
async fn one_shot_exit_flags_trigger_their_generators() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState {
        needs_plan_mode_exit_attachment: true,
        needs_auto_mode_exit_attachment: true,
        ..Default::default()
    };
    let history = MessageHistory::default();

    let orchestrator = SystemReminderOrchestrator::new(config.clone()).with_default_generators();
    let out = run_turn_reminders(&orchestrator, minimal_input(&config, &app_state, &history)).await;

    let types: std::collections::HashSet<_> = out.iter().map(|r| r.attachment_type).collect();
    assert!(types.contains(&AttachmentType::PlanModeExit));
    assert!(types.contains(&AttachmentType::AutoModeExit));
}

#[tokio::test]
async fn todo_reminder_gates_on_tool_presence_and_turn_counter() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState::default();
    // Build a history with NO TodoWrite invocations across many assistant
    // turns — counter will be large (equal to total assistant turns).
    let mut history = MessageHistory::default();
    for i in 0..15 {
        history.messages.push(coco_types::Message::Assistant(
            coco_types::AssistantMessage {
                message: coco_types::LlmMessage::Assistant {
                    content: vec![coco_types::AssistantContent::Text(
                        coco_types::TextContent {
                            text: format!("turn {i}"),
                            provider_metadata: None,
                        },
                    )],
                    provider_options: None,
                },
                uuid: uuid::Uuid::new_v4(),
                model: String::new(),
                stop_reason: None,
                usage: None,
                cost_usd: None,
                request_id: None,
                api_error: None,
            },
        ));
    }
    let orchestrator = SystemReminderOrchestrator::new(config.clone()).with_default_generators();

    // TodoWrite NOT in the tool set → reminder skipped.
    let input_no_todo = TurnReminderInput {
        tools: vec![ToolName::Read.as_str().to_string()],
        ..minimal_input(&config, &app_state, &history)
    };
    let out = run_turn_reminders(&orchestrator, input_no_todo).await;
    let has_todo = out
        .iter()
        .any(|r| r.attachment_type == AttachmentType::TodoReminder);
    assert!(!has_todo, "no TodoWrite tool → no reminder");

    // TodoWrite present → reminder fires because counter >= 10.
    let input_with_todo = TurnReminderInput {
        tools: vec![ToolName::TodoWrite.as_str().to_string()],
        ..minimal_input(&config, &app_state, &history)
    };
    let out = run_turn_reminders(&orchestrator, input_with_todo).await;
    let has_todo = out
        .iter()
        .any(|r| r.attachment_type == AttachmentType::TodoReminder);
    assert!(has_todo, "TodoWrite tool present + 15 stale turns → fires");
}

#[tokio::test]
async fn brief_tool_suppresses_todo_reminder() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState::default();
    let mut history = MessageHistory::default();
    for i in 0..15 {
        history.messages.push(coco_types::Message::Assistant(
            coco_types::AssistantMessage {
                message: coco_types::LlmMessage::Assistant {
                    content: vec![coco_types::AssistantContent::Text(
                        coco_types::TextContent {
                            text: format!("turn {i}"),
                            provider_metadata: None,
                        },
                    )],
                    provider_options: None,
                },
                uuid: uuid::Uuid::new_v4(),
                model: String::new(),
                stop_reason: None,
                usage: None,
                cost_usd: None,
                request_id: None,
                api_error: None,
            },
        ));
    }
    let orchestrator = SystemReminderOrchestrator::new(config.clone()).with_default_generators();
    let input = TurnReminderInput {
        tools: vec![
            ToolName::TodoWrite.as_str().to_string(),
            ToolName::Brief.as_str().to_string(),
        ],
        ..minimal_input(&config, &app_state, &history)
    };
    let out = run_turn_reminders(&orchestrator, input).await;
    let has_todo = out
        .iter()
        .any(|r| r.attachment_type == AttachmentType::TodoReminder);
    assert!(!has_todo, "Brief present → suppress TodoWrite nudge");
}

#[tokio::test]
async fn date_change_wires_new_date_into_reminder() {
    let config = SystemReminderConfig::default();
    let app_state = ToolAppState::default();
    let history = MessageHistory::default();
    let orchestrator = SystemReminderOrchestrator::new(config.clone()).with_default_generators();
    let input = TurnReminderInput {
        new_date: Some("2026-04-21".to_string()),
        ..minimal_input(&config, &app_state, &history)
    };
    let out = run_turn_reminders(&orchestrator, input).await;
    let date = out
        .iter()
        .find(|r| r.attachment_type == AttachmentType::DateChange)
        .expect("date_change fires");
    let text = date.content().expect("text output");
    assert!(
        text.contains("2026-04-21"),
        "body includes new date: {text}"
    );
}

#[tokio::test]
async fn throttle_gap_uses_sentinel_for_never_emitted() {
    // Indirect test: todo reminder fires when counter >= 10 AND throttle
    // has never emitted (sentinel). Validated by the tool-presence test
    // above; here we assert the pure gap helper via its observable effect.
    let config = SystemReminderConfig::default();
    let orchestrator = SystemReminderOrchestrator::new(config);
    let gap = super::throttle_gap(&orchestrator, AttachmentType::TodoReminder, 5);
    assert_eq!(gap, i32::MAX);
}
