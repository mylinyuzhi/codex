use crate::types::AgentMessage;
use crate::types::MessageType;

use super::*;

fn make_msg(from: &str, to: &str, content: &str) -> AgentMessage {
    AgentMessage::new(from, to, content, MessageType::Message).with_team("test-team")
}

#[tokio::test]
async fn send_and_read_unread() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    let msg = make_msg("alice", "bob", "hello");
    mailbox.send("test-team", &msg).await.unwrap();

    let unread = mailbox.read_unread("test-team", "bob").await.unwrap();
    assert_eq!(unread.len(), 1);
    assert_eq!(unread[0].content, "hello");
    assert_eq!(unread[0].from, "alice");
}

#[tokio::test]
async fn mark_read() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    let msg = make_msg("alice", "bob", "hi");
    let msg_id = msg.id.clone();
    mailbox.send("test-team", &msg).await.unwrap();

    mailbox
        .mark_read("test-team", "bob", &[msg_id])
        .await
        .unwrap();

    let unread = mailbox.read_unread("test-team", "bob").await.unwrap();
    assert!(unread.is_empty());

    // But the message still exists in the full list
    let all = mailbox.read_all("test-team", "bob").await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].read);
}

#[tokio::test]
async fn broadcast() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    let msg = AgentMessage::new("lead", "all", "attention", MessageType::Broadcast)
        .with_team("test-team");

    let members = vec!["lead".into(), "alice".into(), "bob".into()];
    mailbox
        .broadcast("test-team", &msg, &members)
        .await
        .unwrap();

    // Lead should not receive their own broadcast
    let lead_msgs = mailbox.read_unread("test-team", "lead").await.unwrap();
    assert!(lead_msgs.is_empty());

    // Others should receive it
    let alice_msgs = mailbox.read_unread("test-team", "alice").await.unwrap();
    assert_eq!(alice_msgs.len(), 1);
    assert_eq!(alice_msgs[0].content, "attention");

    let bob_msgs = mailbox.read_unread("test-team", "bob").await.unwrap();
    assert_eq!(bob_msgs.len(), 1);
}

#[tokio::test]
async fn pending_count() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    assert_eq!(mailbox.pending_count("test-team", "bob").await.unwrap(), 0);

    mailbox
        .send("test-team", &make_msg("alice", "bob", "1"))
        .await
        .unwrap();
    mailbox
        .send("test-team", &make_msg("alice", "bob", "2"))
        .await
        .unwrap();

    assert_eq!(mailbox.pending_count("test-team", "bob").await.unwrap(), 2);
}

#[tokio::test]
async fn clear_mailbox() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    mailbox
        .send("test-team", &make_msg("alice", "bob", "hi"))
        .await
        .unwrap();
    mailbox.clear("test-team", "bob").await.unwrap();

    let all = mailbox.read_all("test-team", "bob").await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn read_empty_mailbox() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    let msgs = mailbox.read_unread("test-team", "nobody").await.unwrap();
    assert!(msgs.is_empty());
}

#[tokio::test]
async fn multiple_messages_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::new(dir.path().to_path_buf());

    for i in 0..5 {
        let msg = make_msg("alice", "bob", &format!("msg-{i}"));
        mailbox.send("test-team", &msg).await.unwrap();
    }

    let all = mailbox.read_all("test-team", "bob").await.unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].content, "msg-0");
    assert_eq!(all[4].content, "msg-4");
}

#[tokio::test]
async fn concurrent_sends_no_message_loss() {
    let dir = tempfile::tempdir().unwrap();
    let mailbox = std::sync::Arc::new(Mailbox::new(dir.path().to_path_buf()));

    let mut handles = Vec::new();
    let count = 20;
    for i in 0..count {
        let mb = mailbox.clone();
        handles.push(tokio::spawn(async move {
            let msg = AgentMessage::new(
                format!("sender-{i}"),
                "target",
                format!("msg-{i}"),
                MessageType::Message,
            )
            .with_team("race-team");
            mb.send("race-team", &msg).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let all = mailbox.read_all("race-team", "target").await.unwrap();
    assert_eq!(
        all.len(),
        count,
        "Expected {count} messages but got {}. Race condition detected!",
        all.len()
    );
}
