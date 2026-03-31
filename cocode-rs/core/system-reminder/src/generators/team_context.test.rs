use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::GeneratorContext;
use crate::generator::TeamContextData;
use crate::generator::TeamMemberInfo;
use cocode_protocol::ToolName;

fn default_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn make_team_context() -> TeamContextData {
    TeamContextData {
        agent_id: "a12345".to_string(),
        agent_name: Some("researcher-1".to_string()),
        team_name: "my-team".to_string(),
        agent_type: "Explore".to_string(),
        is_leader: false,
        members: vec![
            TeamMemberInfo {
                agent_id: "lead".to_string(),
                name: Some("team-lead".to_string()),
                agent_type: Some("general-purpose".to_string()),
                status: "active".to_string(),
            },
            TeamMemberInfo {
                agent_id: "a12345".to_string(),
                name: Some("researcher-1".to_string()),
                agent_type: Some("Explore".to_string()),
                status: "active".to_string(),
            },
        ],
    }
}

#[tokio::test]
async fn generates_context_when_team_present() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .team_context(make_team_context())
        .build();

    let generator = TeamContextGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_some());

    let reminder = result.unwrap();
    let content = reminder.content().unwrap();
    assert!(content.contains("researcher-1"));
    assert!(content.contains("my-team"));
    assert!(content.contains("team-lead"));
    assert!(content.contains(ToolName::SendMessage.as_str()));
}

#[tokio::test]
async fn returns_none_when_no_team() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .build();

    let generator = TeamContextGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}
