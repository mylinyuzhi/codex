use pretty_assertions::assert_eq;

use super::*;

// ---------------------------------------------------------------------------
// TransportEvent serialization
// ---------------------------------------------------------------------------

#[test]
fn test_transport_event_stream_serialization() {
    let event = TransportEvent::StreamEvent {
        content: "hello".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"stream_event\""));
    assert!(json.contains("\"content\":\"hello\""));

    let decoded: TransportEvent = serde_json::from_str(&json).unwrap();
    match decoded {
        TransportEvent::StreamEvent { content } => assert_eq!(content, "hello"),
        _ => panic!("expected StreamEvent"),
    }
}

#[test]
fn test_transport_event_tool_use_serialization() {
    let event = TransportEvent::ToolUseStart {
        tool_use_id: "tu-1".to_string(),
        tool_name: "Bash".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"tool_use_start\""));
    assert!(json.contains("\"tool_name\":\"Bash\""));

    let end_event = TransportEvent::ToolUseEnd {
        tool_use_id: "tu-1".to_string(),
        tool_name: "Bash".to_string(),
        is_error: true,
    };
    let json2 = serde_json::to_string(&end_event).unwrap();
    assert!(json2.contains("\"is_error\":true"));
}

#[test]
fn test_transport_event_result_serialization() {
    let event = TransportEvent::Result {
        text: "done".to_string(),
        turns: 5,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"result\""));
    assert!(json.contains("\"turns\":5"));
}

#[test]
fn test_transport_event_usage_serialization() {
    let event = TransportEvent::Usage {
        input_tokens: 1000,
        output_tokens: 500,
        cost_usd: 0.0123,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"input_tokens\":1000"));
    assert!(json.contains("\"cost_usd\":0.0123"));
}

#[test]
fn test_transport_input_user_message_serialization() {
    let input = TransportInput::UserMessage {
        text: "hello".to_string(),
    };
    let json = serde_json::to_string(&input).unwrap();
    assert!(json.contains("\"type\":\"user_message\""));

    let decoded: TransportInput = serde_json::from_str(&json).unwrap();
    match decoded {
        TransportInput::UserMessage { text } => assert_eq!(text, "hello"),
        _ => panic!("expected UserMessage"),
    }
}

#[test]
fn test_transport_input_permission_serialization() {
    let input = TransportInput::PermissionResponse {
        request_id: "perm-1".to_string(),
        approved: true,
    };
    let json = serde_json::to_string(&input).unwrap();
    assert!(json.contains("\"type\":\"permission_response\""));
    assert!(json.contains("\"approved\":true"));
}

// ---------------------------------------------------------------------------
// SSE Frame Parsing
// ---------------------------------------------------------------------------

#[test]
fn test_parse_sse_frames_single_data() {
    let buffer = "data: hello world\n\n";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data.as_deref(), Some("hello world"));
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_sse_frames_with_event_and_id() {
    let buffer = "event: message\nid: 42\ndata: {\"text\":\"hi\"}\n\n";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].event.as_deref(), Some("message"));
    assert_eq!(frames[0].id.as_deref(), Some("42"));
    assert_eq!(frames[0].data.as_deref(), Some("{\"text\":\"hi\"}"));
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_sse_frames_multiple() {
    let buffer = "data: first\n\ndata: second\n\n";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].data.as_deref(), Some("first"));
    assert_eq!(frames[1].data.as_deref(), Some("second"));
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_sse_frames_incomplete() {
    let buffer = "data: first\n\ndata: incomp";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data.as_deref(), Some("first"));
    assert_eq!(remaining, "data: incomp");
}

#[test]
fn test_parse_sse_frames_comment_only() {
    let buffer = ":keepalive\n\n";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert!(frames.is_empty());
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_sse_frames_multiline_data() {
    let buffer = "data: line1\ndata: line2\n\n";
    let (frames, remaining) = parse_sse_frames(buffer);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data.as_deref(), Some("line1\nline2"));
    assert!(remaining.is_empty());
}

// ---------------------------------------------------------------------------
// Transport State
// ---------------------------------------------------------------------------

#[test]
fn test_transport_state_labels() {
    assert_eq!(TransportState::Idle.label(), "idle");
    assert_eq!(TransportState::Connected.label(), "connected");
    assert_eq!(TransportState::Reconnecting.label(), "reconnecting");
    assert_eq!(TransportState::Closing.label(), "closing");
    assert_eq!(TransportState::Closed.label(), "closed");
}

// ---------------------------------------------------------------------------
// WebSocket permanent close codes
// ---------------------------------------------------------------------------

#[test]
fn test_websocket_permanent_close_codes() {
    assert!(WebSocketTransport::is_permanent_close(1002));
    assert!(WebSocketTransport::is_permanent_close(4001));
    assert!(WebSocketTransport::is_permanent_close(4003));
    assert!(!WebSocketTransport::is_permanent_close(1000));
    assert!(!WebSocketTransport::is_permanent_close(1006));
}

// ---------------------------------------------------------------------------
// SSE Transport
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sse_transport_creation() {
    let config = SseConfig {
        url: "https://example.com/sse".to_string(),
        ..Default::default()
    };
    let (transport, _rx) = SseTransport::new(config);
    assert_eq!(transport.url(), "https://example.com/sse");
    assert_eq!(transport.state(), TransportState::Idle);
    assert!(transport.last_event_id().await.is_none());
}

#[tokio::test]
async fn test_sse_transport_buffer_events() {
    let config = SseConfig::default();
    let (transport, _rx) = SseTransport::new(config);

    transport
        .send_event(TransportEvent::KeepAlive)
        .await
        .unwrap();

    assert_eq!(transport.outbound.lock().await.len(), 1);
}

// ---------------------------------------------------------------------------
// WebSocket Transport
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_websocket_transport_creation() {
    let config = WebSocketConfig {
        url: "wss://example.com/ws".to_string(),
        ..Default::default()
    };
    let (transport, _rx) = WebSocketTransport::new(config);
    assert_eq!(transport.url(), "wss://example.com/ws");
    assert_eq!(transport.state(), TransportState::Idle);
}

#[tokio::test]
async fn test_websocket_transport_buffer_overflow() {
    let config = WebSocketConfig::default();
    let (transport, _rx) = WebSocketTransport::new(config);

    // Fill beyond max buffer
    for i in 0..1001 {
        transport
            .send_event(TransportEvent::StreamEvent {
                content: format!("msg-{i}"),
            })
            .await
            .unwrap();
    }

    // Buffer should be capped at max_buffer_size
    assert_eq!(transport.outbound.lock().await.len(), 1000);
}

#[tokio::test]
async fn test_websocket_transport_close() {
    let config = WebSocketConfig::default();
    let (transport, _rx) = WebSocketTransport::new(config);

    assert_eq!(transport.state(), TransportState::Idle);

    transport.close().await.unwrap();
    assert_eq!(transport.state(), TransportState::Closed);
}

// ---------------------------------------------------------------------------
// Stdio Transport
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stdio_transport_input_channel() {
    let mut transport = StdioTransport::new();
    let tx = transport.input_sender();
    let mut rx = transport.take_input_receiver().unwrap();

    tx.send(TransportInput::Interrupt).await.unwrap();
    let input = rx.recv().await.unwrap();
    assert!(matches!(input, TransportInput::Interrupt));
}
