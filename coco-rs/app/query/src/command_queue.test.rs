use super::*;

#[tokio::test]
async fn test_enqueue_dequeue_priority_order() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("later".into(), QueuePriority::Later))
        .await;
    queue
        .enqueue(QueuedCommand::new("now".into(), QueuePriority::Now))
        .await;
    queue
        .enqueue(QueuedCommand::new("next".into(), QueuePriority::Next))
        .await;

    let first = queue.dequeue(None).await.unwrap();
    assert_eq!(first.prompt, "now");
    let second = queue.dequeue(None).await.unwrap();
    assert_eq!(second.prompt, "next");
    let third = queue.dequeue(None).await.unwrap();
    assert_eq!(third.prompt, "later");
}

#[tokio::test]
async fn test_slash_commands_excluded_from_dequeue() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("/commit".into(), QueuePriority::Now))
        .await;
    queue
        .enqueue(QueuedCommand::new("fix bug".into(), QueuePriority::Next))
        .await;

    let cmd = queue.dequeue(None).await.unwrap();
    assert_eq!(cmd.prompt, "fix bug");
}

#[tokio::test]
async fn test_agent_id_filtering() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("main".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("agent1".into(), QueuePriority::Next).with_agent("a1".into()))
        .await;

    // Only get agent1's commands.
    let cmd = queue.dequeue(Some("a1")).await.unwrap();
    assert_eq!(cmd.prompt, "agent1");

    // Main thread gets its own.
    let cmd = queue.dequeue(None).await.unwrap();
    assert_eq!(cmd.prompt, "main");
}

#[tokio::test]
async fn test_get_commands_by_max_priority() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("urgent".into(), QueuePriority::Now))
        .await;
    queue
        .enqueue(QueuedCommand::new("normal".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("bg".into(), QueuePriority::Later))
        .await;

    let up_to_next = queue
        .get_commands_by_max_priority(QueuePriority::Next, None)
        .await;
    assert_eq!(up_to_next.len(), 2);
}

#[tokio::test]
async fn test_remove_commands() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("keep".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("remove".into(), QueuePriority::Next))
        .await;

    queue.remove(&["remove".into()]).await;
    assert_eq!(queue.len().await, 1);
}

#[tokio::test]
async fn test_query_guard_lifecycle() {
    let guard = QueryGuard::new();
    assert_eq!(guard.status().await, QueryGuardStatus::Idle);

    // Reserve.
    assert!(guard.reserve().await);
    assert_eq!(guard.status().await, QueryGuardStatus::Dispatching);

    // Can't reserve again.
    assert!(!guard.reserve().await);

    // Start.
    let generation = guard.try_start().await.unwrap();
    assert_eq!(guard.status().await, QueryGuardStatus::Running);

    // Can't start again.
    assert!(guard.try_start().await.is_none());

    // End with correct generation.
    assert!(guard.end(generation).await);
    assert_eq!(guard.status().await, QueryGuardStatus::Idle);
}

#[tokio::test]
async fn test_query_guard_force_end() {
    let guard = QueryGuard::new();
    let _generation = guard.try_start().await.unwrap();
    guard.force_end().await;
    assert_eq!(guard.status().await, QueryGuardStatus::Idle);
}

#[tokio::test]
async fn test_query_guard_cancel_reservation() {
    let guard = QueryGuard::new();
    guard.reserve().await;
    guard.cancel_reservation().await;
    assert_eq!(guard.status().await, QueryGuardStatus::Idle);
}

#[tokio::test]
async fn test_inbox_drain() {
    let inbox = Inbox::new();
    inbox
        .push(InboxMessage {
            from_agent: "agent-1".into(),
            content: "hello".into(),
            consumed: false,
            timestamp: 100,
        })
        .await;
    inbox
        .push(InboxMessage {
            from_agent: "agent-2".into(),
            content: "world".into(),
            consumed: false,
            timestamp: 200,
        })
        .await;

    let drained = inbox.drain_unconsumed().await;
    assert_eq!(drained.len(), 2);

    // Second drain returns empty.
    let drained2 = inbox.drain_unconsumed().await;
    assert!(drained2.is_empty());
}

#[tokio::test]
async fn test_inbox_unconsumed_count() {
    let inbox = Inbox::new();
    assert_eq!(inbox.unconsumed_count().await, 0);

    inbox
        .push(InboxMessage {
            from_agent: "a".into(),
            content: "msg".into(),
            consumed: false,
            timestamp: 0,
        })
        .await;
    assert_eq!(inbox.unconsumed_count().await, 1);

    inbox.drain_unconsumed().await;
    assert_eq!(inbox.unconsumed_count().await, 0);
}

#[tokio::test]
async fn test_peek_does_not_remove() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("hello".into(), QueuePriority::Next))
        .await;

    let peeked = queue.peek(None).await;
    assert_eq!(peeked.unwrap().prompt, "hello");
    // Still in queue.
    assert_eq!(queue.len().await, 1);
}

#[tokio::test]
async fn test_dequeue_all() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("a".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("b".into(), QueuePriority::Later))
        .await;
    queue
        .enqueue(QueuedCommand::new("/cmd".into(), QueuePriority::Now))
        .await;

    let all = queue.dequeue_all(None).await;
    // Slash command excluded.
    assert_eq!(all.len(), 2);
    // Only slash command remains.
    assert_eq!(queue.len().await, 1);
}

#[tokio::test]
async fn test_dequeue_matching() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("keep".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("remove-1".into(), QueuePriority::Next))
        .await;
    queue
        .enqueue(QueuedCommand::new("remove-2".into(), QueuePriority::Later))
        .await;

    let removed = queue
        .dequeue_matching(|c| c.prompt.starts_with("remove"))
        .await;
    assert_eq!(removed.len(), 2);
    assert_eq!(queue.len().await, 1);
}

#[tokio::test]
async fn test_clear() {
    let queue = CommandQueue::new();
    queue
        .enqueue(QueuedCommand::new("a".into(), QueuePriority::Now))
        .await;
    queue
        .enqueue(QueuedCommand::new("b".into(), QueuePriority::Next))
        .await;

    queue.clear().await;
    assert!(queue.is_empty().await);
}
