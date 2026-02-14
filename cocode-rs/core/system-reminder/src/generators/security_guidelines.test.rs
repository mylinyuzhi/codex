use super::*;
use std::collections::HashMap;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_security_guidelines_full_default() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = SecurityGuidelinesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("CRITICAL SECURITY REMINDERS")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("NEVER execute commands")
    );
}

#[tokio::test]
async fn test_security_guidelines_sparse_via_flag() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::SecurityGuidelines, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();
    ctx.full_content_flags = flags;

    let generator = SecurityGuidelinesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Security guidelines active")
    );
    assert!(
        !reminder
            .content()
            .unwrap()
            .contains("CRITICAL SECURITY REMINDERS")
    );
}

#[tokio::test]
async fn test_security_guidelines_full_via_flag() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::SecurityGuidelines, true);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(6)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();
    ctx.full_content_flags = flags;

    let generator = SecurityGuidelinesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("CRITICAL SECURITY REMINDERS")
    );
}

#[tokio::test]
async fn test_security_guidelines_not_for_subagent() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(false) // subagent
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = SecurityGuidelinesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_security_guidelines_disabled() {
    let mut config = test_config();
    config.attachments.security_guidelines = false;

    let generator = SecurityGuidelinesGenerator;
    assert!(!generator.is_enabled(&config));
}

#[test]
fn test_generator_properties() {
    let generator = SecurityGuidelinesGenerator;
    assert_eq!(generator.name(), "SecurityGuidelinesGenerator");
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::SecurityGuidelines
    );

    let config = test_config();
    assert!(generator.is_enabled(&config));

    let throttle = generator.throttle_config();
    assert_eq!(throttle.full_content_every_n, Some(5));
}
