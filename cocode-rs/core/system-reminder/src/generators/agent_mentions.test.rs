use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_mentions() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Hello, how are you?")
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AgentMentionsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_agent_mention() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Use @agent-search to find the files")
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AgentMentionsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("invoke the agent"));
    assert!(content.contains("search"));
}

#[tokio::test]
async fn test_multiple_agent_mentions() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Use @agent-plan then @agent-edit")
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AgentMentionsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("invoke the agent \"plan\""));
    assert!(content.contains("invoke the agent \"edit\""));
}

#[test]
fn test_generator_properties() {
    let generator = AgentMentionsGenerator;
    assert_eq!(generator.name(), "AgentMentionsGenerator");
    assert_eq!(generator.tier(), ReminderTier::UserPrompt);
    assert_eq!(generator.attachment_type(), AttachmentType::AgentMentions);
}
