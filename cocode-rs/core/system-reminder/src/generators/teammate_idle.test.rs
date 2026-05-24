use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::TeamContextData;

fn default_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn make_team_ctx(is_leader: bool) -> TeamContextData {
    TeamContextData {
        agent_id: "worker-1".to_string(),
        agent_name: Some("Alice".to_string()),
        team_name: "alpha".to_string(),
        agent_type: "general-purpose".to_string(),
        is_leader,
        members: vec![],
    }
}

#[tokio::test]
async fn fires_for_non_leader_teammate() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .team_context(make_team_ctx(/*is_leader=*/ false))
        .build();

    let generator = TeammateIdleGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_some(), "Should fire for idle non-leader");
    let reminder = result.unwrap();
    let text = reminder.content().unwrap_or_default();
    assert!(text.contains("Teammate Idle"));
    assert!(text.contains("Alice"));
}

#[tokio::test]
async fn does_not_fire_for_leader() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .team_context(make_team_ctx(/*is_leader=*/ true))
        .build();

    let generator = TeammateIdleGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none(), "Should not fire for leader");
}

#[tokio::test]
async fn does_not_fire_without_team() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .build();

    let generator = TeammateIdleGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none(), "Should not fire without team context");
}
