use super::*;
use crate::generator::TodoItem;
use cocode_protocol::ToolName;
use std::collections::HashMap;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

// ---------------------------------------------------------------------------
// PlanModeEnterGenerator tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_mode_enter_not_in_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(false)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_plan_mode_enter_full() {
    let config = test_config();
    // Default (no flag) → full content
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .plan_file_path(PathBuf::from("/home/user/.cocode/plans/test-plan.md"))
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Phase 1: Initial Understanding")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Phase 5: Call ExitPlanMode")
    );
    assert!(reminder.content().unwrap().contains("Write tool"));
    // Plan file doesn't exist on disk → uses "No plan file exists" path (no "Edit tool")
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("No plan file exists yet")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(".cocode/plans/test-plan.md")
    );
}

#[tokio::test]
async fn test_plan_mode_enter_sparse_via_reentry() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(!reminder.content().unwrap().contains("Phase 1")); // Sparse doesn't have phases
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(ToolName::ExitPlanMode.as_str())
    );
}

#[tokio::test]
async fn test_plan_mode_enter_sparse_via_flag() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::PlanModeEnter, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .build();
    ctx.full_content_flags = flags;

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(!reminder.content().unwrap().contains("Phase 1")); // Sparse doesn't have phases
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(ToolName::ExitPlanMode.as_str())
    );
}

#[tokio::test]
async fn test_plan_mode_enter_full_via_flag() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::PlanModeEnter, true);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(6)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();
    ctx.full_content_flags = flags;

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Phase 1: Initial Understanding")
    ); // Full has phases
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Phase 5: Call ExitPlanMode")
    );
}

#[tokio::test]
async fn test_plan_tool_reminder() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_file_path(PathBuf::from("/home/user/.cocode/plans/plan.md"))
        .build();

    let generator = PlanToolReminderGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Write tool"));
    assert!(reminder.content().unwrap().contains("Edit tool"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(".cocode/plans/plan.md")
    );
}

#[tokio::test]
async fn test_plan_tool_reminder_no_plan_path() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        // No plan_file_path
        .build();

    let generator = PlanToolReminderGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Interview phase tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_mode_enter_interview_full() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .is_plan_interview_phase(true)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("Iterative Planning Workflow"));
    assert!(content.contains("The Loop"));
    assert!(content.contains("Asking Good Questions"));
    assert!(!content.contains("Phase 1")); // Interview mode uses iterative loop, not phases
}

#[tokio::test]
async fn test_plan_mode_enter_interview_sparse() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::PlanModeEnter, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .is_plan_interview_phase(true)
        .build();
    ctx.full_content_flags = flags;

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("iterative workflow"));
    assert!(!content.contains("Iterative Planning Workflow")); // Sparse, not full
}

#[tokio::test]
async fn test_plan_mode_reentry_takes_priority_over_interview() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .is_plan_interview_phase(true)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    // Reentry takes priority over interview
    assert!(content.contains("Re-entered"));
    assert!(!content.contains("Iterative Planning Workflow"));
}

#[test]
fn test_throttle_configs() {
    let enter_generator = PlanModeEnterGenerator;
    let throttle = enter_generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
    assert_eq!(throttle.full_content_every_n, Some(5));

    let tool_generator = PlanToolReminderGenerator;
    let throttle = tool_generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 3);
    assert_eq!(throttle.min_turns_after_trigger, 5);
}

// ---------------------------------------------------------------------------
// PlanModeExitGenerator tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_not_triggered_without_pending_flag() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(false)
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_triggered_with_pending() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(true)
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("Exited Plan Mode"));
    assert!(content.contains("You can now make edits, run tools, and take actions"));
    // No approved plan content appended (CC doesn't do this)
    assert!(!content.contains("Your Approved Plan"));
}

#[tokio::test]
async fn test_exit_appends_plan_file_ref_when_exists() {
    // Create a temp file so the path "exists"
    let tmp = std::env::temp_dir().join("plan_exit_test_plan.md");
    std::fs::write(&tmp, "plan content").expect("write tmp");

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(true)
        .plan_file_path(tmp.clone())
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("The plan file is located at"));
    assert!(content.contains(&tmp.display().to_string()));

    std::fs::remove_file(&tmp).ok();
}

#[tokio::test]
async fn test_exit_no_plan_ref_when_path_missing() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(true)
        .plan_file_path(PathBuf::from("/nonexistent/plan.md"))
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(!content.contains("The plan file is located at"));
}

#[test]
fn test_exit_throttle_config() {
    let generator = PlanModeExitGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}

// ---------------------------------------------------------------------------
// PlanVerificationGenerator tests
// ---------------------------------------------------------------------------

fn completed_todo(id: &str) -> TodoItem {
    TodoItem {
        id: id.to_string(),
        subject: format!("Task {id}"),
        status: TodoStatus::Completed,
        is_blocked: false,
    }
}

fn pending_todo(id: &str) -> TodoItem {
    TodoItem {
        id: id.to_string(),
        subject: format!("Task {id}"),
        status: TodoStatus::Pending,
        is_blocked: false,
    }
}

/// Helper: build a context that satisfies all trigger conditions.
fn firing_context(config: &SystemReminderConfig) -> GeneratorContext<'_> {
    GeneratorContext::builder()
        .config(config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1"), completed_todo("2")])
        .build()
}

#[tokio::test]
async fn test_returns_none_in_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(true)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_for_subagent() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(false)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_without_plan_file() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_without_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_with_pending_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![pending_todo("1"), completed_todo("2")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_fires_when_all_todos_completed() {
    let config = test_config();
    let ctx = firing_context(&config);

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.unwrap();
    assert_eq!(reminder.attachment_type, AttachmentType::PlanVerification);
}

#[tokio::test]
async fn test_content_matches_cc_template() {
    let config = test_config();
    let ctx = firing_context(&config);

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("should fire");

    let expected = "You have completed implementing the plan. \
                    Please verify that all changes are correct by reviewing the modified files \
                    and running relevant tests. Do NOT delegate verification to the \
                    Task tool or an agent.";
    assert_eq!(reminder.content(), Some(expected));
}

#[test]
fn test_verification_attachment_type() {
    let generator = PlanVerificationGenerator;
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::PlanVerification
    );
}

#[test]
fn test_verification_throttle_config_min_turns_between() {
    let generator = PlanVerificationGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
}

// ---------------------------------------------------------------------------
// Phase4Variant tests (Gap 5)
// ---------------------------------------------------------------------------

#[test]
fn test_phase4_variant_default_is_full() {
    assert_eq!(Phase4Variant::default(), Phase4Variant::Full);
}

#[test]
fn test_phase4_instructions_full_contains_context_section() {
    let text = phase4_instructions(Phase4Variant::Full);
    assert!(text.contains("**Context** section"));
    assert!(text.contains("verification section"));
}

#[test]
fn test_phase4_instructions_trimmed_has_one_line_context() {
    let text = phase4_instructions(Phase4Variant::Trimmed);
    assert!(text.contains("One-line **Context**"));
    assert!(text.contains("**Verification**"));
    assert!(!text.contains("verification section describing how to test"));
}

#[test]
fn test_phase4_instructions_compact_no_context() {
    let text = phase4_instructions(Phase4Variant::Compact);
    assert!(text.contains("Do NOT write a Context or Background section"));
    assert!(text.contains("under 40 lines"));
}

#[test]
fn test_phase4_instructions_minimal_hard_limit() {
    let text = phase4_instructions(Phase4Variant::Minimal);
    assert!(text.contains("**Hard limit: 40 lines.**"));
    assert!(text.contains("Do NOT restate the user's request"));
    assert!(text.contains("file:line"));
}

#[tokio::test]
async fn test_plan_mode_full_uses_trimmed_variant() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Trimmed)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("One-line **Context**"));
    assert!(!content.contains("Begin with a **Context** section"));
}

#[tokio::test]
async fn test_plan_mode_full_uses_compact_variant() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Compact)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("Do NOT write a Context or Background section"));
    assert!(content.contains("under 40 lines"));
}

#[tokio::test]
async fn test_plan_mode_full_uses_minimal_variant() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Minimal)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("**Hard limit: 40 lines.**"));
    assert!(content.contains("Do NOT restate the user's request"));
}

// ---------------------------------------------------------------------------
// Dynamic agent count tests (Gap 6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_mode_default_agent_counts() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .build();

    // Defaults should match protocol constants
    assert_eq!(
        ctx.explore_agent_count,
        cocode_protocol::DEFAULT_PLAN_EXPLORE_AGENT_COUNT
    );
    assert_eq!(
        ctx.plan_agent_count,
        cocode_protocol::DEFAULT_PLAN_AGENT_COUNT
    );

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    // Default: 3 explore agents, 1 plan agent
    assert!(content.contains("3 agents maximum"));
    assert!(content.contains("at least 1 Plan agent"));
}

#[tokio::test]
async fn test_plan_mode_custom_explore_count() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .explore_agent_count(5)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("5 agents maximum"));
}

#[tokio::test]
async fn test_plan_mode_custom_plan_count() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_agent_count(2)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("at least 2 Plan agents"));
    // Plural form
    assert!(content.contains("they help validate"));
}

#[tokio::test]
async fn test_plan_mode_singular_plan_agent_text() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_agent_count(1)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    assert!(content.contains("at least 1 Plan agent"));
    // Singular form
    assert!(content.contains("it helps validate"));
}

#[tokio::test]
async fn test_sparse_mode_ignores_phase4_variant() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::PlanModeEnter, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Minimal)
        .explore_agent_count(5)
        .build();
    ctx.full_content_flags = flags;

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();

    // Sparse mode doesn't include Phase 4 or agent count details
    assert!(!content.contains("Hard limit"));
    assert!(!content.contains("5 agents maximum"));
}
