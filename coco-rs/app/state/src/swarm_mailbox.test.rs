use super::*;

#[test]
fn test_inbox_path() {
    let path = inbox_path("researcher", "my-team");
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("teams"));
    assert!(path_str.contains("my-team"));
    assert!(path_str.contains("inboxes"));
    assert!(path_str.ends_with("researcher.json"));
}

#[test]
fn test_read_write_mailbox() {
    let dir = tempfile::tempdir().unwrap();
    let team_dir = dir.path().join("teams").join("test-team").join("inboxes");
    std::fs::create_dir_all(&team_dir).unwrap();

    let msg = TeammateMessage {
        from: "leader".to_string(),
        text: "Hello teammate".to_string(),
        timestamp: "2026-04-06T10:00:00Z".to_string(),
        read: false,
        color: Some("blue".to_string()),
        summary: Some("greeting".to_string()),
    };

    // Write directly to the test dir
    let path = team_dir.join("worker.json");
    let messages = vec![msg.clone()];
    std::fs::write(&path, serde_json::to_string_pretty(&messages).unwrap()).unwrap();

    // Read back
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<TeammateMessage> = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].from, "leader");
    assert!(!parsed[0].read);
    assert_eq!(parsed[0].color.as_deref(), Some("blue"));
    assert_eq!(parsed[0].summary.as_deref(), Some("greeting"));
}

#[test]
fn test_format_teammate_messages() {
    let messages = vec![TeammateMessage {
        from: "researcher".to_string(),
        text: "Found the bug".to_string(),
        timestamp: "2026-04-06T10:00:00Z".to_string(),
        read: false,
        color: Some("green".to_string()),
        summary: Some("bug found".to_string()),
    }];
    let formatted = format_teammate_messages(&messages);
    assert!(formatted.contains("teammate_message"));
    assert!(formatted.contains("teammate_id=\"researcher\""));
    assert!(formatted.contains("color=\"green\""));
    assert!(formatted.contains("summary=\"bug found\""));
    assert!(formatted.contains("Found the bug"));
}

#[test]
fn test_is_structured_protocol_message() {
    assert!(is_structured_protocol_message(
        r#"{"type": "idle_notification", "from": "worker"}"#
    ));
    assert!(is_structured_protocol_message(
        r#"{"type": "shutdown_request", "request_id": "1", "from": "leader"}"#
    ));
    assert!(!is_structured_protocol_message("Hello world"));
    assert!(!is_structured_protocol_message(r#"{"type": "unknown"}"#));
    assert!(!is_structured_protocol_message(""));
}

#[test]
fn test_parse_protocol_message_idle() {
    let text =
        r#"{"type": "idle_notification", "from": "worker-1", "timestamp": "2026-04-06T10:00:00Z"}"#;
    let msg = parse_protocol_message(text).unwrap();
    match msg {
        ProtocolMessage::IdleNotification { from, .. } => {
            assert_eq!(from, "worker-1");
        }
        _ => panic!("Expected IdleNotification"),
    }
}

#[test]
fn test_parse_protocol_message_shutdown() {
    let text = r#"{"type": "shutdown_request", "request_id": "shutdown-1", "from": "leader", "timestamp": "2026-04-06T10:00:00Z"}"#;
    let msg = parse_protocol_message(text).unwrap();
    match msg {
        ProtocolMessage::ShutdownRequest {
            request_id, from, ..
        } => {
            assert_eq!(request_id, "shutdown-1");
            assert_eq!(from, "leader");
        }
        _ => panic!("Expected ShutdownRequest"),
    }
}

#[test]
fn test_parse_protocol_message_mode_set() {
    let text = r#"{"type": "mode_set_request", "mode": "plan", "from": "leader"}"#;
    let msg = parse_protocol_message(text).unwrap();
    match msg {
        ProtocolMessage::ModeSetRequest { mode, from } => {
            assert_eq!(mode, "plan");
            assert_eq!(from, "leader");
        }
        _ => panic!("Expected ModeSetRequest"),
    }
}

#[test]
fn test_create_idle_notification() {
    let text = create_idle_notification("worker-1", Some("available"), Some("done"));
    assert!(is_structured_protocol_message(&text));
    let msg = parse_protocol_message(&text).unwrap();
    match msg {
        ProtocolMessage::IdleNotification {
            from,
            idle_reason,
            summary,
            ..
        } => {
            assert_eq!(from, "worker-1");
            assert_eq!(idle_reason.as_deref(), Some("available"));
            assert_eq!(summary.as_deref(), Some("done"));
        }
        _ => panic!("Expected IdleNotification"),
    }
}

#[test]
fn test_create_mode_set_request() {
    let text = create_mode_set_request("plan", "leader");
    let msg = parse_protocol_message(&text).unwrap();
    match msg {
        ProtocolMessage::ModeSetRequest { mode, from } => {
            assert_eq!(mode, "plan");
            assert_eq!(from, "leader");
        }
        _ => panic!("Expected ModeSetRequest"),
    }
}

#[test]
fn test_teammate_message_serde_roundtrip() {
    let msg = TeammateMessage {
        from: "agent-1".to_string(),
        text: "test message".to_string(),
        timestamp: "2026-04-06T10:00:00Z".to_string(),
        read: true,
        color: None,
        summary: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: TeammateMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.from, "agent-1");
    assert!(parsed.read);
}
