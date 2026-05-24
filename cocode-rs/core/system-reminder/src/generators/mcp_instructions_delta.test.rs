use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_changes_returns_none() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = McpInstructionsDeltaGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_instruction_changes_generates_content() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .mcp_instructions_changes(vec![(
            "github".to_string(),
            "Use the search tool for code queries".to_string(),
        )])
        .build();

    let generator = McpInstructionsDeltaGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("github"));
}

#[tokio::test]
async fn test_disabled_returns_none() {
    let mut config = test_config();
    config.attachments.mcp_instructions_delta = false;

    let generator = McpInstructionsDeltaGenerator;
    assert!(!generator.is_enabled(&config));
}
