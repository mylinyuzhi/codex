use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::GeneratorContext;
use crate::generator::UnreadMessage;

fn default_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn make_regular_msg(from: &str, content: &str) -> UnreadMessage {
    UnreadMessage {
        id: "msg-1".to_string(),
        from: from.to_string(),
        content: content.to_string(),
        message_type: "message".to_string(),
        timestamp: 1710000000,
    }
}

fn make_shutdown_msg(from: &str) -> UnreadMessage {
    UnreadMessage {
        id: "msg-2".to_string(),
        from: from.to_string(),
        content: "Please shut down".to_string(),
        message_type: crate::generator::message_types::SHUTDOWN_REQUEST.to_string(),
        timestamp: 1710000000,
    }
}

#[tokio::test]
async fn returns_none_when_no_messages() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .build();

    let generator = TeamMailboxGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn formats_regular_messages() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .unread_messages(vec![make_regular_msg("alice", "hello team!")])
        .build();

    let generator = TeamMailboxGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_some());

    let content = result.unwrap().content().unwrap().to_string();
    assert!(content.contains("Unread Messages"));
    assert!(content.contains("alice"));
    assert!(content.contains("hello team!"));
}

#[tokio::test]
async fn formats_shutdown_request_prominently() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .unread_messages(vec![make_shutdown_msg("team-lead")])
        .build();

    let generator = TeamMailboxGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_some());

    let content = result.unwrap().content().unwrap().to_string();
    assert!(content.contains("SHUTDOWN REQUESTED"));
    assert!(content.contains("team-lead"));
    assert!(content.contains("shutdown_response"));
}

#[tokio::test]
async fn mixed_messages_ordered_correctly() {
    let config = default_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .unread_messages(vec![
            make_regular_msg("bob", "status update"),
            make_shutdown_msg("team-lead"),
        ])
        .build();

    let generator = TeamMailboxGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    let content = result.unwrap().content().unwrap().to_string();

    // Shutdown should appear before regular messages
    let shutdown_pos = content.find("SHUTDOWN").unwrap();
    let regular_pos = content.find("Unread Messages").unwrap();
    assert!(shutdown_pos < regular_pos);
}

#[test]
fn format_timestamp_known_value() {
    // 2024-03-09 16:00:00 UTC
    let ts = 1710000000_i64;
    let result = format_timestamp(ts);
    assert_eq!(result, "2024-03-09 16:00 UTC");
}

#[test]
fn format_timestamp_negative_is_valid() {
    // -1 is 1969-12-31 23:59:59 UTC — valid timestamp
    let result = format_timestamp(-1);
    assert!(result.contains("1969"));
}
