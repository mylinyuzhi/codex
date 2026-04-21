use super::*;
use crate::generator::AgentPendingMessage;
use crate::generator::GeneratorContext;
use crate::generator::TeamContextSnapshot;
use crate::generator::TeammateMailboxInfo;
use coco_config::SystemReminderConfig;

#[tokio::test]
async fn teammate_mailbox_skips_when_none_or_empty() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c).teammate_mailbox(None).build();
    assert!(
        TeammateMailboxGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
    let ctx = GeneratorContext::builder(&c)
        .teammate_mailbox(Some(TeammateMailboxInfo::default()))
        .build();
    assert!(
        TeammateMailboxGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn teammate_mailbox_passes_formatted_verbatim() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .teammate_mailbox(Some(TeammateMailboxInfo {
            formatted: "You have 2 messages:\n- @alice: hi".into(),
        }))
        .build();
    let text = TeammateMailboxGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert_eq!(text, "You have 2 messages:\n- @alice: hi");
}

#[tokio::test]
async fn team_context_skips_when_missing_required_fields() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .team_context(Some(TeamContextSnapshot {
            agent_id: String::new(), // missing
            agent_name: "name".into(),
            team_name: "team".into(),
            team_config_path: "/".into(),
            task_list_path: "/".into(),
        }))
        .build();
    assert!(TeamContextGenerator.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn team_context_emits_full_snapshot() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .team_context(Some(TeamContextSnapshot {
            agent_id: "worker-1".into(),
            agent_name: "Worker One".into(),
            team_name: "alpha".into(),
            team_config_path: "/cfg".into(),
            task_list_path: "/tasks".into(),
        }))
        .build();
    let text = TeamContextGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.starts_with("# Team Coordination"));
    assert!(text.contains("team \"alpha\""));
    assert!(text.contains("worker-1"));
    assert!(text.contains("Worker One"));
    assert!(text.contains("/cfg"));
    assert!(text.contains("/tasks"));
}

#[tokio::test]
async fn agent_pending_messages_skips_when_empty() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .agent_pending_messages(vec![])
        .build();
    assert!(
        AgentPendingMessagesGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn agent_pending_messages_lists_entries() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .agent_pending_messages(vec![
            AgentPendingMessage {
                from: "alice".into(),
                text: "ping".into(),
            },
            AgentPendingMessage {
                from: "bob".into(),
                text: "please review".into(),
            },
        ])
        .build();
    let text = AgentPendingMessagesGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.contains("You have pending messages from teammates:"));
    assert!(text.contains("- from alice: ping"));
    assert!(text.contains("- from bob: please review"));
}
