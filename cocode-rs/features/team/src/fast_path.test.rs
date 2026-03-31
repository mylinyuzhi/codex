use crate::types::AgentMessage;
use crate::types::MessageType;

use super::*;

#[tokio::test]
async fn test_register_and_send() {
    let fp = FastPath::new();
    let mut rx = fp.register("team1", "agent-1").await;

    let msg = AgentMessage::new("leader", "agent-1", "hello", MessageType::Message);
    assert!(fp.try_send("team1", "agent-1", msg.clone()).await);

    let received = rx.recv().await.unwrap();
    assert_eq!(received.content, "hello");
}

#[tokio::test]
async fn test_send_to_unregistered_returns_false() {
    let fp = FastPath::new();
    let msg = AgentMessage::new("leader", "agent-1", "hello", MessageType::Message);
    assert!(!fp.try_send("team1", "agent-1", msg).await);
}

#[tokio::test]
async fn test_unregister() {
    let fp = FastPath::new();
    let _rx = fp.register("team1", "agent-1").await;
    assert!(fp.has_agent("team1", "agent-1").await);

    fp.unregister("team1", "agent-1").await;
    assert!(!fp.has_agent("team1", "agent-1").await);
}

#[tokio::test]
async fn test_broadcast() {
    let fp = FastPath::new();
    let mut rx1 = fp.register("team1", "agent-1").await;
    let mut rx2 = fp.register("team1", "agent-2").await;
    let _rx3 = fp.register("team1", "leader").await;

    let msg = AgentMessage::new("leader", "all", "broadcast", MessageType::Broadcast);
    let members = vec![
        "agent-1".to_string(),
        "agent-2".to_string(),
        "leader".to_string(),
    ];

    let delivered = fp.broadcast("team1", &msg, &members).await;
    assert_eq!(delivered, 2); // Excludes sender "leader"

    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert_eq!(r1.content, "broadcast");
    assert_eq!(r2.content, "broadcast");
    assert_eq!(r1.to, "agent-1");
    assert_eq!(r2.to, "agent-2");
    // Each recipient gets a unique ID.
    assert_ne!(r1.id, r2.id);
}

#[tokio::test]
async fn test_agent_count() {
    let fp = FastPath::new();
    assert_eq!(fp.agent_count().await, 0);

    let _rx1 = fp.register("team1", "a1").await;
    let _rx2 = fp.register("team1", "a2").await;
    assert_eq!(fp.agent_count().await, 2);

    fp.unregister("team1", "a1").await;
    assert_eq!(fp.agent_count().await, 1);
}
