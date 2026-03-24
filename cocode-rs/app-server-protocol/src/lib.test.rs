use pretty_assertions::assert_eq;
use serde_json::json;

use crate::*;

#[test]
fn test_server_notification_turn_started_roundtrip() {
    let notif = ServerNotification::TurnStarted(TurnStartedParams {
        turn_id: "turn_1".into(),
        turn_number: 1,
    });

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();

    match parsed {
        ServerNotification::TurnStarted(params) => {
            assert_eq!(params.turn_id, "turn_1");
            assert_eq!(params.turn_number, 1);
        }
        other => panic!("expected TurnStarted, got {other:?}"),
    }
}

#[test]
fn test_server_notification_turn_completed_roundtrip() {
    let notif = ServerNotification::TurnCompleted(TurnCompletedParams {
        turn_id: "turn_1".into(),
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(20),
            cache_creation_tokens: None,
            reasoning_tokens: None,
        },
    });

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();

    match parsed {
        ServerNotification::TurnCompleted(params) => {
            assert_eq!(params.turn_id, "turn_1");
            assert_eq!(params.usage.input_tokens, 100);
            assert_eq!(params.usage.output_tokens, 50);
            assert_eq!(params.usage.cache_read_tokens, Some(20));
            assert_eq!(params.usage.total(), 150);
        }
        other => panic!("expected TurnCompleted, got {other:?}"),
    }
}

#[test]
fn test_server_notification_serde_tag() {
    let notif = ServerNotification::SessionStarted(SessionStartedParams {
        session_id: "sess_abc".into(),
    });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["method"], "session/started");
    assert_eq!(value["params"]["session_id"], "sess_abc");
}

#[test]
fn test_thread_item_command_execution_roundtrip() {
    let item = ThreadItem {
        id: "item_1".into(),
        details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
            command: "git status".into(),
            aggregated_output: "On branch main".into(),
            exit_code: Some(0),
            status: ItemStatus::Completed,
        }),
    };

    let json = serde_json::to_string(&item).unwrap();
    let parsed: ThreadItem = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, "item_1");
    match parsed.details {
        ThreadItemDetails::CommandExecution(cmd) => {
            assert_eq!(cmd.command, "git status");
            assert_eq!(cmd.exit_code, Some(0));
            assert_eq!(cmd.status, ItemStatus::Completed);
        }
        other => panic!("expected CommandExecution, got {other:?}"),
    }
}

#[test]
fn test_thread_item_serde_tag() {
    let item = ThreadItem {
        id: "item_2".into(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
            text: "Hello!".into(),
        }),
    };

    let value: serde_json::Value = serde_json::to_value(&item).unwrap();
    assert_eq!(value["type"], "agent_message");
    assert_eq!(value["id"], "item_2");
    assert_eq!(value["text"], "Hello!");
}

#[test]
fn test_thread_item_file_change() {
    let item = ThreadItem {
        id: "item_3".into(),
        details: ThreadItemDetails::FileChange(FileChangeItem {
            changes: vec![
                FileChange {
                    path: "src/main.rs".into(),
                    kind: FileChangeKind::Update,
                },
                FileChange {
                    path: "src/new.rs".into(),
                    kind: FileChangeKind::Add,
                },
            ],
            status: ItemStatus::Completed,
        }),
    };

    let json = serde_json::to_string(&item).unwrap();
    let parsed: ThreadItem = serde_json::from_str(&json).unwrap();
    match parsed.details {
        ThreadItemDetails::FileChange(fc) => {
            assert_eq!(fc.changes.len(), 2);
            assert_eq!(fc.changes[0].kind, FileChangeKind::Update);
            assert_eq!(fc.changes[1].kind, FileChangeKind::Add);
        }
        other => panic!("expected FileChange, got {other:?}"),
    }
}

#[test]
fn test_client_request_session_start() {
    let req = ClientRequest::SessionStart(SessionStartRequestParams {
        prompt: "hello".into(),
        model: Some("sonnet".into()),
        max_turns: Some(10),
        cwd: None,
        system_prompt_suffix: None,
        permission_mode: None,
        env: None,
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "session/start");
    assert_eq!(value["params"]["prompt"], "hello");
    assert_eq!(value["params"]["model"], "sonnet");
}

#[test]
fn test_client_request_approval_resolve() {
    let req = ClientRequest::ApprovalResolve(ApprovalResolveRequestParams {
        request_id: "req_1".into(),
        decision: ApprovalDecision::Approve,
    });

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::ApprovalResolve(params) => {
            assert_eq!(params.request_id, "req_1");
            assert!(matches!(params.decision, ApprovalDecision::Approve));
        }
        other => panic!("expected ApprovalResolve, got {other:?}"),
    }
}

#[test]
fn test_server_request_ask_for_approval() {
    let req = ServerRequest::AskForApproval(AskForApprovalParams {
        request_id: "req_2".into(),
        tool_name: "Bash".into(),
        input: json!({"command": "rm -rf /"}),
        description: Some("Delete everything".into()),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "approval/askForApproval");
    assert_eq!(value["params"]["tool_name"], "Bash");
}

#[test]
fn test_item_notification_roundtrip() {
    let notif = ServerNotification::ItemStarted(ItemEventParams {
        item: ThreadItem {
            id: "item_10".into(),
            details: ThreadItemDetails::McpToolCall(McpToolCallItem {
                server: "my-server".into(),
                tool: "search".into(),
                arguments: json!({"query": "test"}),
                result: None,
                error: None,
                status: ItemStatus::InProgress,
            }),
        },
    });

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::ItemStarted(params) => {
            assert_eq!(params.item.id, "item_10");
            match params.item.details {
                ThreadItemDetails::McpToolCall(mcp) => {
                    assert_eq!(mcp.server, "my-server");
                    assert_eq!(mcp.tool, "search");
                    assert_eq!(mcp.status, ItemStatus::InProgress);
                }
                other => panic!("expected McpToolCall, got {other:?}"),
            }
        }
        other => panic!("expected ItemStarted, got {other:?}"),
    }
}

#[test]
fn test_usage_total() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        ..Default::default()
    };
    assert_eq!(usage.total(), 150);
}

#[test]
fn test_subagent_item_roundtrip() {
    let item = ThreadItem {
        id: "item_sub".into(),
        details: ThreadItemDetails::Subagent(SubagentItem {
            agent_id: "agent_1".into(),
            agent_type: "Explore".into(),
            description: "Search codebase".into(),
            is_background: false,
            result: None,
            status: ItemStatus::InProgress,
        }),
    };

    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"type\":\"subagent\""));
    let parsed: ThreadItem = serde_json::from_str(&json).unwrap();
    match parsed.details {
        ThreadItemDetails::Subagent(sub) => {
            assert_eq!(sub.agent_type, "Explore");
            assert!(!sub.is_background);
        }
        other => panic!("expected Subagent, got {other:?}"),
    }
}
