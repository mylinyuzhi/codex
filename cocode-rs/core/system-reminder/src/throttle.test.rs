use super::*;

#[test]
fn test_default_throttle_config() {
    let config = ThrottleConfig::default();
    assert_eq!(config.min_turns_between, 0);
    assert_eq!(config.min_turns_after_trigger, 0);
    assert!(config.max_per_session.is_none());
}

#[test]
fn test_throttle_manager_first_time() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig::default();

    // First time should always be allowed
    assert!(manager.should_generate(AttachmentType::ChangedFiles, &config, 1));
}

#[test]
fn test_throttle_manager_min_turns_between() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        min_turns_between: 3,
        ..Default::default()
    };

    // Mark as generated at turn 1
    manager.mark_generated(AttachmentType::ChangedFiles, 1);

    // Should be blocked at turns 2, 3
    assert!(!manager.should_generate(AttachmentType::ChangedFiles, &config, 2));
    assert!(!manager.should_generate(AttachmentType::ChangedFiles, &config, 3));

    // Should be allowed at turn 4
    assert!(manager.should_generate(AttachmentType::ChangedFiles, &config, 4));
}

#[test]
fn test_throttle_manager_max_per_session() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        max_per_session: Some(2),
        ..Default::default()
    };

    // First two should be allowed
    assert!(manager.should_generate(AttachmentType::TodoReminders, &config, 1));
    manager.mark_generated(AttachmentType::TodoReminders, 1);

    assert!(manager.should_generate(AttachmentType::TodoReminders, &config, 2));
    manager.mark_generated(AttachmentType::TodoReminders, 2);

    // Third should be blocked
    assert!(!manager.should_generate(AttachmentType::TodoReminders, &config, 3));
}

#[test]
fn test_throttle_manager_trigger_turn() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        min_turns_after_trigger: 5,
        ..Default::default()
    };

    // Set trigger at turn 1
    manager.set_trigger_turn(AttachmentType::PlanToolReminder, 1);

    // Should be blocked until turn 6
    assert!(!manager.should_generate(AttachmentType::PlanToolReminder, &config, 2));
    assert!(!manager.should_generate(AttachmentType::PlanToolReminder, &config, 5));
    assert!(manager.should_generate(AttachmentType::PlanToolReminder, &config, 6));
}

#[test]
fn test_throttle_manager_reset() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        min_turns_between: 10,
        ..Default::default()
    };

    manager.mark_generated(AttachmentType::ChangedFiles, 1);
    assert!(!manager.should_generate(AttachmentType::ChangedFiles, &config, 2));

    manager.reset();
    assert!(manager.should_generate(AttachmentType::ChangedFiles, &config, 2));
}

#[test]
fn test_predefined_configs() {
    let plan_mode = ThrottleConfig::plan_mode();
    assert_eq!(plan_mode.min_turns_between, 5);
    assert_eq!(plan_mode.full_content_every_n, Some(5));

    let plan_tool = ThrottleConfig::plan_tool_reminder();
    assert_eq!(plan_tool.min_turns_between, 3);
    assert_eq!(plan_tool.min_turns_after_trigger, 5);

    let todo = ThrottleConfig::todo_reminder();
    assert_eq!(todo.min_turns_between, 5);

    let output_style = ThrottleConfig::output_style();
    assert_eq!(output_style.min_turns_between, 15);
    assert!(output_style.max_per_session.is_none());

    let security = ThrottleConfig::security_guidelines();
    assert_eq!(security.full_content_every_n, Some(5));
    assert_eq!(security.min_turns_between, 0);
}

#[test]
fn test_should_use_full_content_always_full() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig::default(); // full_content_every_n: None

    // None means always full
    assert!(manager.should_use_full_content(AttachmentType::ChangedFiles, &config));
    manager.mark_generated(AttachmentType::ChangedFiles, 1);
    assert!(manager.should_use_full_content(AttachmentType::ChangedFiles, &config));
}

#[test]
fn test_should_use_full_content_every_n() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        full_content_every_n: Some(5),
        ..Default::default()
    };

    // First generation (count=0) → full
    assert!(manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    // Generations 1-4 → sparse
    manager.mark_generated(AttachmentType::SecurityGuidelines, 1);
    assert!(!manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    manager.mark_generated(AttachmentType::SecurityGuidelines, 2);
    assert!(!manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    manager.mark_generated(AttachmentType::SecurityGuidelines, 3);
    assert!(!manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    manager.mark_generated(AttachmentType::SecurityGuidelines, 4);
    assert!(!manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    // Generation 5 (count=5, 5 % 5 == 0) → full
    manager.mark_generated(AttachmentType::SecurityGuidelines, 5);
    assert!(manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));
}

#[test]
fn test_should_use_full_content_reset() {
    let manager = ThrottleManager::new();
    let config = ThrottleConfig {
        full_content_every_n: Some(3),
        ..Default::default()
    };

    manager.mark_generated(AttachmentType::SecurityGuidelines, 1);
    assert!(!manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));

    manager.reset();
    // After reset, count=0 → full
    assert!(manager.should_use_full_content(AttachmentType::SecurityGuidelines, &config));
}
