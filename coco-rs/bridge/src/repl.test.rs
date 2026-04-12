use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_repl_in_message_user_serialization() {
    let msg = ReplInMessage::UserMessage {
        text: "Hello Claude".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"user_message\""));
    assert!(json.contains("\"text\":\"Hello Claude\""));

    let decoded: ReplInMessage = serde_json::from_str(&json).unwrap();
    match decoded {
        ReplInMessage::UserMessage { text } => assert_eq!(text, "Hello Claude"),
        _ => panic!("expected UserMessage"),
    }
}

#[test]
fn test_repl_in_message_control_request_serialization() {
    let msg = ReplInMessage::ControlRequest {
        request_id: "req-1".to_string(),
        request: ControlRequest::Interrupt,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"control_request\""));
    assert!(json.contains("\"subtype\":\"interrupt\""));

    let decoded: ReplInMessage = serde_json::from_str(&json).unwrap();
    match decoded {
        ReplInMessage::ControlRequest {
            request_id,
            request,
        } => {
            assert_eq!(request_id, "req-1");
            assert!(matches!(request, ControlRequest::Interrupt));
        }
        _ => panic!("expected ControlRequest"),
    }
}

#[test]
fn test_repl_in_message_permission_response_serialization() {
    let msg = ReplInMessage::PermissionResponse {
        request_id: "perm-1".to_string(),
        decision: PermissionDecision::Allow,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"permission_response\""));
    assert!(json.contains("\"behavior\":\"allow\""));

    let decoded: ReplInMessage = serde_json::from_str(&json).unwrap();
    match decoded {
        ReplInMessage::PermissionResponse {
            request_id,
            decision,
        } => {
            assert_eq!(request_id, "perm-1");
            assert!(matches!(decision, PermissionDecision::Allow));
        }
        _ => panic!("expected PermissionResponse"),
    }
}

#[test]
fn test_repl_out_message_stream_event_serialization() {
    let msg = ReplOutMessage::StreamEvent {
        content: "chunk of text".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"stream_event\""));

    let decoded: ReplOutMessage = serde_json::from_str(&json).unwrap();
    match decoded {
        ReplOutMessage::StreamEvent { content } => {
            assert_eq!(content, "chunk of text");
        }
        _ => panic!("expected StreamEvent"),
    }
}

#[test]
fn test_repl_out_message_result_serialization() {
    let msg = ReplOutMessage::Result {
        text: "done".to_string(),
        session_id: Some("sess-1".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"result\""));
    assert!(json.contains("\"session_id\":\"sess-1\""));

    // Result without session_id should omit the field
    let msg2 = ReplOutMessage::Result {
        text: "done".to_string(),
        session_id: None,
    };
    let json2 = serde_json::to_string(&msg2).unwrap();
    assert!(!json2.contains("session_id"));
}

#[test]
fn test_repl_out_message_control_request_serialization() {
    let msg = ReplOutMessage::ControlRequest {
        request_id: "cr-1".to_string(),
        request: SdkControlOutbound::CanUseTool {
            tool_name: "Bash".to_string(),
            tool_use_id: "tu-1".to_string(),
            input: serde_json::json!({"command": "ls"}),
            title: None,
            description: Some("List files".to_string()),
        },
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"control_request\""));
    assert!(json.contains("\"subtype\":\"can_use_tool\""));
    assert!(json.contains("\"tool_name\":\"Bash\""));
}

#[test]
fn test_bridge_state_serialization() {
    let states = [
        (BridgeState::Idle, "\"idle\""),
        (BridgeState::Connected, "\"connected\""),
        (BridgeState::Reconnecting, "\"reconnecting\""),
        (BridgeState::Failed, "\"failed\""),
    ];

    for (state, expected) in states {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, expected);
    }
}

#[test]
fn test_encode_decode_repl_ndjson_roundtrip() {
    let msg = ReplOutMessage::Pong;
    let encoded = encode_repl_ndjson(&msg).unwrap();
    assert!(encoded.ends_with('\n'));

    let decoded: ReplOutMessage = serde_json::from_str(encoded.trim()).unwrap();
    assert!(matches!(decoded, ReplOutMessage::Pong));
}

#[test]
fn test_decode_repl_ndjson_with_whitespace() {
    let line = "  {\"type\":\"ping\"}  \n";
    let msg = decode_repl_ndjson(line).unwrap();
    assert!(matches!(msg, ReplInMessage::Ping));
}

#[tokio::test]
async fn test_repl_bridge_creation() {
    let bridge = ReplBridge::new("test-session".to_string());
    assert_eq!(bridge.session_id(), "test-session");
    assert_eq!(bridge.state(), BridgeState::Idle);
    assert_eq!(bridge.buffer_len().await, 0);
}

#[tokio::test]
async fn test_repl_bridge_state_transitions() {
    let bridge = ReplBridge::new("sess-1".to_string());
    let mut rx = bridge.watch_state();

    assert_eq!(bridge.state(), BridgeState::Idle);

    bridge.set_state(BridgeState::Connected);
    rx.changed().await.unwrap();
    assert_eq!(*rx.borrow(), BridgeState::Connected);

    bridge.set_state(BridgeState::Reconnecting);
    rx.changed().await.unwrap();
    assert_eq!(*rx.borrow(), BridgeState::Reconnecting);

    // Setting same state should be a no-op
    bridge.set_state(BridgeState::Reconnecting);
    assert_eq!(bridge.state(), BridgeState::Reconnecting);
}

#[tokio::test]
async fn test_repl_bridge_buffer_when_disconnected() {
    let bridge = ReplBridge::new("sess-1".to_string());

    // Bridge is Idle, messages should be buffered
    bridge
        .send(ReplOutMessage::AssistantMessage {
            text: "hello".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(bridge.buffer_len().await, 1);

    bridge
        .send(ReplOutMessage::AssistantMessage {
            text: "world".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(bridge.buffer_len().await, 2);
}

#[tokio::test]
async fn test_repl_bridge_send_when_connected() {
    let mut bridge = ReplBridge::new("sess-1".to_string());
    let mut rx = bridge.take_outgoing().unwrap();

    bridge.set_state(BridgeState::Connected);

    bridge
        .send(ReplOutMessage::AssistantMessage {
            text: "hello".to_string(),
        })
        .await
        .unwrap();

    let msg = rx.recv().await.unwrap();
    match msg {
        ReplOutMessage::AssistantMessage { text } => assert_eq!(text, "hello"),
        _ => panic!("expected AssistantMessage"),
    }

    // Buffer should remain empty
    assert_eq!(bridge.buffer_len().await, 0);
}

#[tokio::test]
async fn test_repl_bridge_drain_buffer() {
    let mut bridge = ReplBridge::new("sess-1".to_string());
    let mut rx = bridge.take_outgoing().unwrap();

    // Buffer messages while disconnected
    for i in 0..3 {
        bridge
            .send(ReplOutMessage::AssistantMessage {
                text: format!("msg-{i}"),
            })
            .await
            .unwrap();
    }
    assert_eq!(bridge.buffer_len().await, 3);

    // Connect and drain
    bridge.set_state(BridgeState::Connected);
    bridge.drain_buffer().await.unwrap();

    assert_eq!(bridge.buffer_len().await, 0);

    // Verify all messages arrived in order
    for i in 0..3 {
        let msg = rx.recv().await.unwrap();
        match msg {
            ReplOutMessage::AssistantMessage { text } => {
                assert_eq!(text, format!("msg-{i}"));
            }
            _ => panic!("expected AssistantMessage"),
        }
    }
}

#[tokio::test]
async fn test_repl_bridge_send_result() {
    let mut bridge = ReplBridge::new("sess-1".to_string());
    let mut rx = bridge.take_outgoing().unwrap();

    bridge.set_state(BridgeState::Connected);
    bridge
        .send_result("final answer".to_string())
        .await
        .unwrap();

    let msg = rx.recv().await.unwrap();
    match msg {
        ReplOutMessage::Result { text, session_id } => {
            assert_eq!(text, "final answer");
            assert_eq!(session_id.as_deref(), Some("sess-1"));
        }
        _ => panic!("expected Result"),
    }
}
