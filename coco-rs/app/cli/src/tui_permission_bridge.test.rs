use super::*;
use coco_types::CoreEvent;

fn dummy_request(id: &str) -> ToolPermissionRequest {
    ToolPermissionRequest {
        id: id.into(),
        tool_use_id: format!("use-{id}"),
        agent_id: "leader".into(),
        tool_name: "Bash".into(),
        description: "ls".into(),
        input: serde_json::json!({"command": "ls"}),
    }
}

#[tokio::test]
async fn approve_flow_sends_approved_decision() {
    let pending = new_pending_map();
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(8);
    let bridge = TuiPermissionBridge::new(tx, pending.clone());

    let request_handle =
        tokio::spawn(async move { bridge.request_permission(dummy_request("r1")).await });

    // Bridge should emit ApprovalRequired before awaiting.
    let event = rx.recv().await.expect("bridge emits an event");
    match event {
        CoreEvent::Tui(TuiOnlyEvent::ApprovalRequired { request_id, .. }) => {
            assert_eq!(request_id, "r1");
        }
        other => panic!("expected Tui(ApprovalRequired); got {other:?}"),
    }

    // Simulate user approval.
    let resolved = resolve_pending(&pending, "r1", true, None).await;
    assert!(resolved);

    let resolution = request_handle.await.unwrap().unwrap();
    assert_eq!(resolution.decision, ToolPermissionDecision::Approved);
}

#[tokio::test]
async fn reject_flow_propagates_feedback() {
    let pending = new_pending_map();
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(8);
    let bridge = TuiPermissionBridge::new(tx, pending.clone());

    let handle = tokio::spawn(async move { bridge.request_permission(dummy_request("r2")).await });
    let _ = rx.recv().await;

    let resolved = resolve_pending(&pending, "r2", false, Some("not safe".into())).await;
    assert!(resolved);

    let resolution = handle.await.unwrap().unwrap();
    assert_eq!(resolution.decision, ToolPermissionDecision::Rejected);
    assert_eq!(resolution.feedback.as_deref(), Some("not safe"));
}

#[tokio::test]
async fn unknown_request_id_returns_false() {
    let pending = new_pending_map();
    let resolved = resolve_pending(&pending, "ghost", true, None).await;
    assert!(!resolved);
}

#[tokio::test]
async fn channel_close_returns_error() {
    let pending = new_pending_map();
    let (tx, _rx) = mpsc::channel::<CoreEvent>(8);
    drop(_rx); // close the channel before the bridge sends

    let bridge = TuiPermissionBridge::new(tx, pending.clone());
    let result = bridge.request_permission(dummy_request("r3")).await;
    assert!(
        result.is_err(),
        "channel closed → request_permission errors"
    );
    // Pending map should not retain the entry.
    assert!(pending.read().await.is_empty());
}
