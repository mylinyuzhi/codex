use super::*;
use crate::generator::InvokedSkillInfo;
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

    let generator = InvokedSkillsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_skill() {
    let config = test_config();
    let skills: Vec<InvokedSkillInfo> = vec![InvokedSkillInfo {
        name: "commit".to_string(),
        prompt_content: "Generate a commit message for the staged changes.".to_string(),
    }];

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .invoked_skills(skills)
        .build();

    let generator = InvokedSkillsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("<command-name>commit</command-name>")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Generate a commit message")
    );
}

#[tokio::test]
async fn test_with_multiple_skills() {
    let config = test_config();
    let skills: Vec<InvokedSkillInfo> = vec![
        InvokedSkillInfo {
            name: "commit".to_string(),
            prompt_content: "Generate a commit message.".to_string(),
        },
        InvokedSkillInfo {
            name: "review-pr".to_string(),
            prompt_content: "Review the pull request.".to_string(),
        },
    ];

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .invoked_skills(skills)
        .build();

    let generator = InvokedSkillsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("commit"));
    assert!(reminder.content().unwrap().contains("review-pr"));
}

#[test]
fn test_generator_properties() {
    let generator = InvokedSkillsGenerator;
    assert_eq!(generator.name(), "InvokedSkillsGenerator");
    assert_eq!(generator.tier(), ReminderTier::UserPrompt);
    assert_eq!(generator.attachment_type(), AttachmentType::InvokedSkills);
}
