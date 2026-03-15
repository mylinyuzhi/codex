use super::*;

fn create_test_request(id: &str, tool: &str) -> ApprovalRequest {
    ApprovalRequest {
        request_id: id.to_string(),
        tool_name: tool.to_string(),
        description: format!("Test request for {tool}"),
        risks: vec![],
        allow_remember: false,
        proposed_prefix_pattern: None,
    }
}

#[tokio::test]
async fn test_request_and_respond() {
    let queue = WorkerPermissionQueue::new();

    let request = create_test_request("req-1", "Bash");

    // Spawn a task to respond
    let queue_clone = queue.requests.clone();
    let notify_rx = queue.notify_tx.subscribe();
    tokio::spawn(async move {
        // Wait a bit for request to be queued
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check request is in queue
        let requests = queue_clone.lock().await;
        assert!(requests.contains_key("req-1"));
        drop(requests);
        drop(notify_rx);
    });

    // Queue clone for response
    let queue2 = Arc::new(queue);
    let queue_for_response = queue2.clone();

    // Spawn response task
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        queue_for_response
            .respond("req-1", ApprovalDecision::Approved)
            .await;
    });

    // Request permission (should wait for response)
    let result = queue2.request_permission(request, "worker-1").await;
    assert_eq!(result, ApprovalDecision::Approved);
}

#[tokio::test]
async fn test_request_timeout() {
    let queue = WorkerPermissionQueue::new().with_default_timeout(Duration::from_millis(100));

    let request = create_test_request("req-1", "Bash");

    // Request without responding - should timeout
    let result = queue.request_permission(request, "worker-1").await;
    assert_eq!(result, ApprovalDecision::Denied);
}

#[tokio::test]
async fn test_pending_requests() {
    let queue = WorkerPermissionQueue::new();

    // Queue is empty initially
    assert_eq!(queue.pending_count().await, 0);

    // Add a request manually (simulating the internal state)
    {
        let mut requests = queue.requests.lock().await;
        requests.insert(
            "req-1".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-1", "Bash"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-1".to_string(),
                response_tx: None,
            },
        );
    }

    assert_eq!(queue.pending_count().await, 1);

    let pending = queue.pending_requests().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tool_name, "Bash");
}

#[tokio::test]
async fn test_cancel_worker_requests() {
    let queue = WorkerPermissionQueue::new();

    // Add requests for different workers
    {
        let mut requests = queue.requests.lock().await;
        requests.insert(
            "req-1".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-1", "Bash"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-1".to_string(),
                response_tx: None,
            },
        );
        requests.insert(
            "req-2".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-2", "Edit"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-2".to_string(),
                response_tx: None,
            },
        );
    }

    // Cancel worker-1's requests
    let cancelled = queue.cancel_worker_requests("worker-1").await;
    assert_eq!(cancelled, 1);

    // Only worker-2's request remains
    assert_eq!(queue.pending_count().await, 1);
}

#[tokio::test]
async fn test_cancel_all() {
    let queue = WorkerPermissionQueue::new();

    {
        let mut requests = queue.requests.lock().await;
        requests.insert(
            "req-1".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-1", "Bash"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-1".to_string(),
                response_tx: None,
            },
        );
        requests.insert(
            "req-2".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-2", "Edit"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-2".to_string(),
                response_tx: None,
            },
        );
    }

    let cancelled = queue.cancel_all().await;
    assert_eq!(cancelled, 2);
    assert_eq!(queue.pending_count().await, 0);
}

#[tokio::test]
async fn test_stats() {
    let queue = WorkerPermissionQueue::new();

    let stats = queue.stats().await;
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.total, 0);
    assert!(!stats.has_pending());

    {
        let mut requests = queue.requests.lock().await;
        requests.insert(
            "req-1".to_string(),
            QueuedPermissionRequest {
                request: create_test_request("req-1", "Bash"),
                queued_at: Instant::now(),
                timeout: Duration::from_secs(300),
                worker_id: "worker-1".to_string(),
                response_tx: None,
            },
        );
    }

    let stats = queue.stats().await;
    assert_eq!(stats.pending, 1);
    assert!(stats.has_pending());
}

#[test]
fn test_permission_request_status() {
    assert!(PermissionRequestStatus::Pending.is_pending());
    assert!(!PermissionRequestStatus::Approved.is_pending());

    assert!(PermissionRequestStatus::Approved.is_approved());
    assert!(!PermissionRequestStatus::Denied.is_approved());

    assert!(!PermissionRequestStatus::Pending.is_resolved());
    assert!(PermissionRequestStatus::Approved.is_resolved());
    assert!(PermissionRequestStatus::Denied.is_resolved());
    assert!(PermissionRequestStatus::TimedOut.is_resolved());
}
