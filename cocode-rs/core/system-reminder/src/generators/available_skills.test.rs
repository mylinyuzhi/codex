use super::*;
use crate::generator::SkillInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_skills() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AvailableSkillsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_skills() {
    let config = test_config();
    let skills: Vec<SkillInfo> = vec![
        SkillInfo {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
            when_to_use: None,
        },
        SkillInfo {
            name: "review-pr".to_string(),
            description: "Review a pull request".to_string(),
            when_to_use: Some("Use when the user asks to review a PR".to_string()),
        },
    ];

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .available_skills(skills)
        .build();

    let generator = AvailableSkillsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("commit"));
    assert!(reminder.content().unwrap().contains("review-pr"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Generate a commit message")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("When to use: Use when the user asks to review a PR")
    );
}

#[test]
fn test_generator_properties() {
    let generator = AvailableSkillsGenerator;
    assert_eq!(generator.name(), "AvailableSkillsGenerator");
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 50);
}
