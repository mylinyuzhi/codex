use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::mailbox::Mailbox;
use crate::types::AgentMessage;
use crate::types::MessageType;

use super::*;

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[tokio::test]
async fn test_poller_receives_mailbox_messages() {
    let dir = temp_dir();
    let mailbox = Arc::new(Mailbox::new(dir.path().to_path_buf()));
    let cancel = CancellationToken::new();

    let config = PollConfig {
        poll_interval_ms: 50,
        leader_agent_id: Some("leader".to_string()),
    };

    // Send a message before starting poller.
    let msg = AgentMessage::new("leader", "worker-1", "do stuff", MessageType::Message);
    mailbox.send("team1", &msg).await.unwrap();

    let poller = TeamPoller::new("team1", "worker-1", config, mailbox, cancel.clone());
    let mut rx = poller.run();

    // Should receive the lead message.
    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    match result {
        PollResult::LeadMessage(m) => assert_eq!(m.content, "do stuff"),
        other => panic!("Expected LeadMessage, got {other:?}"),
    }

    cancel.cancel();
}

#[tokio::test]
async fn test_poller_shutdown_priority() {
    let dir = temp_dir();
    let mailbox = Arc::new(Mailbox::new(dir.path().to_path_buf()));
    let cancel = CancellationToken::new();

    // Send both a regular message and a shutdown.
    let regular = AgentMessage::new("peer", "worker-1", "hey", MessageType::Message);
    let shutdown = AgentMessage::new("leader", "worker-1", "stop", MessageType::ShutdownRequest);
    mailbox.send("team1", &regular).await.unwrap();
    mailbox.send("team1", &shutdown).await.unwrap();

    let config = PollConfig {
        poll_interval_ms: 50,
        leader_agent_id: Some("leader".to_string()),
    };

    let poller = TeamPoller::new("team1", "worker-1", config, mailbox, cancel.clone());
    let mut rx = poller.run();

    // Shutdown should come first (priority 2 > priority 3).
    let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    match first {
        PollResult::Shutdown(m) => assert_eq!(m.content, "stop"),
        other => panic!("Expected Shutdown first, got {other:?}"),
    }

    cancel.cancel();
}

#[tokio::test]
async fn test_poller_task_available() {
    let dir = temp_dir();
    let mailbox = Arc::new(Mailbox::new(dir.path().to_path_buf()));
    let ledger = Arc::new(crate::task_ledger::TaskLedger::new(
        PathBuf::from("/tmp/test-poll-ledger"),
        /*persist=*/ false,
    ));
    let cancel = CancellationToken::new();

    // Create a task in the ledger.
    ledger
        .create_task("team1", "Do work", "details", vec![])
        .await
        .unwrap();

    let config = PollConfig {
        poll_interval_ms: 50,
        leader_agent_id: None,
    };

    let poller = TeamPoller::new("team1", "worker-1", config, mailbox, cancel.clone())
        .with_task_ledger(ledger);
    let mut rx = poller.run();

    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    match result {
        PollResult::TaskAvailable(task) => assert_eq!(task.subject, "Do work"),
        other => panic!("Expected TaskAvailable, got {other:?}"),
    }

    cancel.cancel();
}

#[tokio::test]
async fn test_poller_cancellation() {
    let dir = temp_dir();
    let mailbox = Arc::new(Mailbox::new(dir.path().to_path_buf()));
    let cancel = CancellationToken::new();

    let config = PollConfig {
        poll_interval_ms: 50,
        leader_agent_id: None,
    };

    let poller = TeamPoller::new("team1", "worker-1", config, mailbox, cancel.clone());
    let mut rx = poller.run();

    // Cancel immediately.
    cancel.cancel();

    // Channel should close soon.
    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(result.is_ok(), "Should resolve (None) after cancellation");
}
