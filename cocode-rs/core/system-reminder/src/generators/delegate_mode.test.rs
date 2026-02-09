use super::*;
use crate::generator::DelegatedAgentInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_when_not_delegate_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_delegate_mode(false)
        .build();

    let generator = DelegateModeGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_triggered_in_delegate_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_delegate_mode(true)
        .build();

    let generator = DelegateModeGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Delegate Mode Active"));
}

#[tokio::test]
async fn test_shows_agent_status() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_delegate_mode(true)
        .delegated_agents(vec![
            DelegatedAgentInfo {
                agent_id: "agent-1".to_string(),
                agent_type: "Explore".to_string(),
                status: "running".to_string(),
                description: "Searching for API endpoints".to_string(),
            },
            DelegatedAgentInfo {
                agent_id: "agent-2".to_string(),
                agent_type: "Plan".to_string(),
                status: "completed".to_string(),
                description: "Planning implementation".to_string(),
            },
        ])
        .build();

    let generator = DelegateModeGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Active Agents"));
    assert!(reminder.content().unwrap().contains("agent-1"));
    assert!(reminder.content().unwrap().contains("Explore"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Searching for API endpoints")
    );
}

#[tokio::test]
async fn test_exit_message() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_delegate_mode(true)
        .delegate_mode_exiting(true)
        .build();

    let generator = DelegateModeGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Exiting Delegate Mode")
    );
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Synthesize the results")
    );
}

#[test]
fn test_throttle_config() {
    let generator = DelegateModeGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
}
