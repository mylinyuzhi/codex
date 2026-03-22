use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn test_ctx(config: &SystemReminderConfig) -> GeneratorContext<'_> {
    GeneratorContext::builder()
        .config(config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .cwd(PathBuf::from("/tmp/test"))
        .build()
}

#[test]
fn test_orchestrator_creation() {
    let config = test_config();
    let orchestrator = SystemReminderOrchestrator::new(config);

    assert!(orchestrator.generator_count() > 0);
    assert_eq!(orchestrator.timeout_duration().as_millis(), 1000);
}

#[test]
fn test_orchestrator_disabled() {
    let config = SystemReminderConfig {
        enabled: false,
        ..Default::default()
    };
    let orchestrator = SystemReminderOrchestrator::new(config.clone());
    let ctx = test_ctx(&config);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let reminders = rt.block_on(orchestrator.generate_all(ctx));

    assert!(reminders.is_empty());
}

#[test]
fn test_tier_filtering_subagent() {
    let config = test_config();
    let orchestrator = SystemReminderOrchestrator::new(config.clone());

    // Create context as a subagent
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(false) // subagent
        .has_user_input(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    // MainAgentOnly generators should not run for subagents
    for g in &orchestrator.generators {
        if g.tier() == ReminderTier::MainAgentOnly {
            assert!(!orchestrator.should_run_generator(g.as_ref(), &ctx));
        }
    }
}

#[test]
fn test_tier_filtering_no_user_input() {
    let config = test_config();
    let orchestrator = SystemReminderOrchestrator::new(config.clone());

    // Create context without user input
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(false) // no user input
        .cwd(PathBuf::from("/tmp"))
        .build();

    // UserPrompt generators should not run without user input
    for g in &orchestrator.generators {
        if g.tier() == ReminderTier::UserPrompt {
            assert!(!orchestrator.should_run_generator(g.as_ref(), &ctx));
        }
    }
}

#[tokio::test]
async fn test_generate_all_basic() {
    let config = test_config();
    let orchestrator = SystemReminderOrchestrator::new(config.clone());
    let ctx = test_ctx(&config);

    // Should run without panicking
    let reminders = orchestrator.generate_all(ctx).await;

    // Most generators will return None without proper setup,
    // but the orchestrator should handle that gracefully
    assert!(reminders.len() <= orchestrator.generator_count());
}

#[test]
fn test_throttle_reset() {
    let config = test_config();
    let orchestrator = SystemReminderOrchestrator::new(config);

    // Mark some generation
    orchestrator
        .throttle_manager()
        .mark_generated(crate::types::AttachmentType::ChangedFiles, 1);

    // State should exist
    assert!(
        orchestrator
            .throttle_manager()
            .get_state(crate::types::AttachmentType::ChangedFiles)
            .is_some()
    );

    // Reset
    orchestrator.reset_throttle();

    // State should be gone
    assert!(
        orchestrator
            .throttle_manager()
            .get_state(crate::types::AttachmentType::ChangedFiles)
            .is_none()
    );
}
