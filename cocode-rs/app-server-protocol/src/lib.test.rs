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
        protocol_version: "1".into(),
        models: None,
        commands: None,
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
    let req = ClientRequest::SessionStart(Box::new(SessionStartRequestParams {
        prompt: "hello".into(),
        model: Some("sonnet".into()),
        max_turns: Some(10),
        cwd: None,
        system_prompt_suffix: None,
        system_prompt: None,
        permission_mode: None,
        env: None,
        agents: None,
        mcp_servers: None,
        output_format: None,
        sandbox: None,
        thinking: None,
        tools: None,
        permission_rules: None,
        max_budget_cents: None,
        hooks: None,
        disable_builtin_agents: None,
        prompt_suggestions: None,
        permission_prompt_tool: None,
    }));

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
        permission_suggestions: None,
        blocked_path: None,
        decision_reason: None,
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

#[test]
fn test_client_request_update_env_roundtrip() {
    let req = ClientRequest::UpdateEnv(UpdateEnvRequestParams {
        env: [("FOO".into(), "bar".into())].into_iter().collect(),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "control/updateEnv");
    assert_eq!(value["params"]["env"]["FOO"], "bar");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::UpdateEnv(params) => {
            assert_eq!(params.env.get("FOO").unwrap(), "bar");
        }
        other => panic!("expected UpdateEnv, got {other:?}"),
    }
}

#[test]
fn test_client_request_keep_alive_roundtrip() {
    let req = ClientRequest::KeepAlive(KeepAliveRequestParams {
        timestamp: Some(12345),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "control/keepAlive");
    assert_eq!(value["params"]["timestamp"], 12345);

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::KeepAlive(params) => {
            assert_eq!(params.timestamp, Some(12345));
        }
        other => panic!("expected KeepAlive, got {other:?}"),
    }
}

#[test]
fn test_server_notification_keep_alive_roundtrip() {
    let notif = ServerNotification::KeepAlive(KeepAliveParams { timestamp: 99999 });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["method"], "keepAlive");
    assert_eq!(value["params"]["timestamp"], 99999);

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::KeepAlive(params) => {
            assert_eq!(params.timestamp, 99999);
        }
        other => panic!("expected KeepAlive, got {other:?}"),
    }
}

#[test]
fn test_server_notification_session_ended_roundtrip() {
    let notif = ServerNotification::SessionEnded(SessionEndedParams {
        reason: SessionEndedReason::MaxTurns,
    });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["method"], "session/ended");
    assert_eq!(value["params"]["reason"], "max_turns");

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::SessionEnded(params) => {
            assert_eq!(params.reason, SessionEndedReason::MaxTurns);
        }
        other => panic!("expected SessionEnded, got {other:?}"),
    }
}

#[test]
fn test_session_ended_reason_all_variants() {
    let variants = [
        (SessionEndedReason::Completed, "completed"),
        (SessionEndedReason::MaxTurns, "max_turns"),
        (SessionEndedReason::MaxBudget, "max_budget"),
        (SessionEndedReason::Error, "error"),
        (SessionEndedReason::UserInterrupt, "user_interrupt"),
        (SessionEndedReason::StdinClosed, "stdin_closed"),
    ];
    for (variant, expected_str) in variants {
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(
            json, expected_str,
            "SessionEndedReason::{variant:?} should serialize to {expected_str:?}"
        );
        let parsed: SessionEndedReason = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn test_hook_input_output_types_roundtrip() {
    let pre = PreToolUseHookInput {
        tool_name: "Bash".into(),
        tool_input: json!({"command": "ls"}),
        tool_use_id: Some("tu_1".into()),
    };
    let json = serde_json::to_string(&pre).unwrap();
    let parsed: PreToolUseHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.tool_name, "Bash");
    assert_eq!(parsed.tool_use_id.as_deref(), Some("tu_1"));

    let post = PostToolUseHookInput {
        tool_name: "Read".into(),
        tool_input: json!({"path": "/tmp/test"}),
        tool_output: Some("file contents".into()),
        is_error: false,
        tool_use_id: None,
    };
    let json = serde_json::to_string(&post).unwrap();
    let parsed: PostToolUseHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.tool_name, "Read");
    assert_eq!(parsed.tool_output.as_deref(), Some("file contents"));
    assert!(!parsed.is_error);

    let output = HookCallbackOutput {
        behavior: HookBehavior::Deny,
        message: Some("Not allowed".into()),
        updated_input: None,
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: HookCallbackOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.behavior, HookBehavior::Deny);
    assert_eq!(parsed.message.as_deref(), Some("Not allowed"));
}

#[test]
fn test_agent_definition_config_with_new_fields() {
    let config = json!({
        "description": "test agent",
        "model": "sonnet",
        "background": true,
        "isolation": "worktree",
        "color": "cyan",
        "permission_mode": "bypass",
        "fork_context": true,
        "use_custom_prompt": true
    });

    let parsed: crate::AgentDefinitionConfig = serde_json::from_value(config).unwrap();
    assert_eq!(parsed.description.as_deref(), Some("test agent"));
    assert_eq!(parsed.model.as_deref(), Some("sonnet"));
    assert!(parsed.background);
    assert_eq!(parsed.isolation, Some(crate::AgentIsolationMode::Worktree));
    assert_eq!(parsed.color.as_deref(), Some("cyan"));
    assert_eq!(parsed.permission_mode.as_deref(), Some("bypass"));
    assert!(parsed.fork_context);
    assert!(parsed.use_custom_prompt);
}

#[test]
fn test_server_request_hook_callback_roundtrip() {
    let req = ServerRequest::HookCallback(HookCallbackParams {
        request_id: "req_hook_1".into(),
        callback_id: "cb_1".into(),
        event_type: "PreToolUse".into(),
        input: json!({"tool_name": "Bash", "command": "ls"}),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "hook/callback");
    assert_eq!(value["params"]["callback_id"], "cb_1");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ServerRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerRequest::HookCallback(params) => {
            assert_eq!(params.request_id, "req_hook_1");
            assert_eq!(params.callback_id, "cb_1");
            assert_eq!(params.event_type, "PreToolUse");
        }
        other => panic!("expected HookCallback, got {other:?}"),
    }
}

#[test]
fn test_client_request_hook_callback_response_roundtrip() {
    let req = ClientRequest::HookCallbackResponse(HookCallbackResponseParams {
        request_id: "req_hook_1".into(),
        output: json!({"continue_execution": true}),
        error: None,
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "hook/callbackResponse");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::HookCallbackResponse(params) => {
            assert_eq!(params.request_id, "req_hook_1");
            assert!(params.error.is_none());
        }
        other => panic!("expected HookCallbackResponse, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// New type tests (SDK alignment review)
// ---------------------------------------------------------------------------

#[test]
fn test_session_result_notification_roundtrip() {
    let notif = ServerNotification::SessionResult(SessionResultParams {
        session_id: "sess_1".into(),
        total_turns: 5,
        total_cost_cents: Some(42),
        duration_ms: 12000,
        duration_api_ms: Some(8000),
        usage: Usage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        },
        stop_reason: SessionEndedReason::Completed,
        structured_output: None,
    });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["method"], "session/result");
    assert_eq!(value["params"]["total_turns"], 5);
    assert_eq!(value["params"]["total_cost_cents"], 42);
    assert_eq!(value["params"]["duration_ms"], 12000);

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::SessionResult(params) => {
            assert_eq!(params.session_id, "sess_1");
            assert_eq!(params.total_turns, 5);
            assert_eq!(params.total_cost_cents, Some(42));
        }
        other => panic!("expected SessionResult, got {other:?}"),
    }
}

#[test]
fn test_prompt_suggestion_notification_roundtrip() {
    let notif = ServerNotification::PromptSuggestion(PromptSuggestionParams {
        suggestions: vec!["Fix the tests".into(), "Add documentation".into()],
    });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["method"], "prompt/suggestion");
    assert_eq!(value["params"]["suggestions"][0], "Fix the tests");

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::PromptSuggestion(params) => {
            assert_eq!(params.suggestions.len(), 2);
        }
        other => panic!("expected PromptSuggestion, got {other:?}"),
    }
}

#[test]
fn test_set_thinking_request_roundtrip() {
    let req = ClientRequest::SetThinking(SetThinkingRequestParams {
        thinking: ThinkingConfig {
            mode: ThinkingMode::Enabled,
            max_tokens: Some(4096),
        },
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "control/setThinking");
    assert_eq!(value["params"]["thinking"]["mode"], "enabled");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::SetThinking(params) => {
            assert_eq!(params.thinking.mode, ThinkingMode::Enabled);
            assert_eq!(params.thinking.max_tokens, Some(4096));
        }
        other => panic!("expected SetThinking, got {other:?}"),
    }
}

#[test]
fn test_rewind_files_request_roundtrip() {
    let req = ClientRequest::RewindFiles(RewindFilesRequestParams {
        turn_id: "turn_3".into(),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "control/rewindFiles");
    assert_eq!(value["params"]["turn_id"], "turn_3");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::RewindFiles(params) => {
            assert_eq!(params.turn_id, "turn_3");
        }
        other => panic!("expected RewindFiles, got {other:?}"),
    }
}

#[test]
fn test_ask_for_approval_with_new_fields() {
    let req = ServerRequest::AskForApproval(AskForApprovalParams {
        request_id: "req_3".into(),
        tool_name: "Bash".into(),
        input: json!({"command": "rm -rf /tmp/test"}),
        description: Some("Delete test files".into()),
        permission_suggestions: Some(vec![PermissionSuggestion {
            behavior: "allow".into(),
            reason: Some("Safe temporary directory".into()),
        }]),
        blocked_path: Some("/tmp/test".into()),
        decision_reason: Some("Destructive operation".into()),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["params"]["blocked_path"], "/tmp/test");
    assert_eq!(
        value["params"]["permission_suggestions"][0]["behavior"],
        "allow"
    );

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ServerRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerRequest::AskForApproval(params) => {
            assert_eq!(params.blocked_path.as_deref(), Some("/tmp/test"));
            assert!(params.permission_suggestions.is_some());
            assert_eq!(params.permission_suggestions.unwrap().len(), 1);
        }
        other => panic!("expected AskForApproval, got {other:?}"),
    }
}

#[test]
fn test_sandbox_config_with_new_fields() {
    let config = json!({
        "mode": "read_only",
        "network_access": true,
        "auto_allow_bash_if_sandboxed": true,
        "exclude_commands": ["git", "npm"]
    });
    let parsed: SandboxConfig = serde_json::from_value(config).unwrap();
    assert_eq!(parsed.mode, SandboxMode::ReadOnly);
    assert!(parsed.network_access);
    assert!(parsed.auto_allow_bash_if_sandboxed);
    assert_eq!(parsed.exclude_commands, vec!["git", "npm"]);
}

#[test]
fn test_sandbox_config_backward_compatible() {
    // Old format without new fields still deserializes
    let config = json!({"mode": "none"});
    let parsed: SandboxConfig = serde_json::from_value(config).unwrap();
    assert!(!parsed.auto_allow_bash_if_sandboxed);
    assert!(parsed.exclude_commands.is_empty());
}

#[test]
fn test_new_hook_input_types_roundtrip() {
    let stop = StopHookInput {
        stop_reason: "max_turns".into(),
    };
    let json = serde_json::to_string(&stop).unwrap();
    let parsed: StopHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.stop_reason, "max_turns");

    let start = SubagentStartHookInput {
        agent_type: "Explore".into(),
        prompt: "Find the bug".into(),
        agent_id: Some("agent_1".into()),
    };
    let json = serde_json::to_string(&start).unwrap();
    let parsed: SubagentStartHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.agent_type, "Explore");
    assert_eq!(parsed.agent_id.as_deref(), Some("agent_1"));

    let stop_agent = SubagentStopHookInput {
        agent_type: "Plan".into(),
        agent_id: "agent_2".into(),
        output: Some("done".into()),
    };
    let json = serde_json::to_string(&stop_agent).unwrap();
    let parsed: SubagentStopHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.agent_id, "agent_2");

    let submit = UserPromptSubmitHookInput {
        prompt: "Hello".into(),
    };
    let json = serde_json::to_string(&submit).unwrap();
    let parsed: UserPromptSubmitHookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.prompt, "Hello");
}

#[test]
fn test_client_request_session_list_roundtrip() {
    let req = ClientRequest::SessionList(SessionListRequestParams {
        limit: Some(10),
        cursor: Some("prev_id".into()),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "session/list");
    assert_eq!(value["params"]["limit"], 10);

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::SessionList(params) => {
            assert_eq!(params.limit, Some(10));
            assert_eq!(params.cursor.as_deref(), Some("prev_id"));
        }
        other => panic!("expected SessionList, got {other:?}"),
    }
}

#[test]
fn test_session_list_result_roundtrip() {
    let result = SessionListResult {
        sessions: vec![SessionSummary {
            id: "sess_1".into(),
            name: Some("My session".into()),
            working_dir: Some("/home/user".into()),
            model: Some("sonnet".into()),
            created_at: Some("2026-03-26T12:00:00Z".into()),
            updated_at: None,
            turn_count: 5,
        }],
        next_cursor: None,
    };

    let json = serde_json::to_string(&result).unwrap();
    let parsed: SessionListResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.sessions.len(), 1);
    assert_eq!(parsed.sessions[0].id, "sess_1");
    assert_eq!(parsed.sessions[0].turn_count, 5);
}

#[test]
fn test_config_read_write_roundtrip() {
    let req = ClientRequest::ConfigRead(ConfigReadRequestParams {
        key: Some("model.main".into()),
    });
    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "config/read");

    let req = ClientRequest::ConfigWrite(ConfigWriteRequestParams {
        key: "model.main".into(),
        value: json!("opus"),
        scope: ConfigWriteScope::User,
    });
    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "config/value/write");
    assert_eq!(value["params"]["scope"], "user");
}

#[test]
fn test_error_notification_with_error_info() {
    let notif = ServerNotification::Error(ErrorNotificationParams {
        message: "rate limit exceeded".into(),
        category: Some(ErrorCategory::Api),
        retryable: true,
        error_info: Some(ErrorInfo::RateLimitExceeded),
    });

    let value: serde_json::Value = serde_json::to_value(&notif).unwrap();
    assert_eq!(value["params"]["retryable"], true);
    assert_eq!(value["params"]["error_info"], "rate_limit_exceeded");

    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::Error(params) => {
            assert!(params.retryable);
            assert!(matches!(
                params.error_info,
                Some(ErrorInfo::RateLimitExceeded)
            ));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_error_info_server_overloaded_with_retry() {
    let info = ErrorInfo::ServerOverloaded {
        retry_after_ms: Some(5000),
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: ErrorInfo = serde_json::from_str(&json).unwrap();
    match parsed {
        ErrorInfo::ServerOverloaded { retry_after_ms } => {
            assert_eq!(retry_after_ms, Some(5000));
        }
        other => panic!("expected ServerOverloaded, got {other:?}"),
    }
}

#[test]
fn test_initialize_request_roundtrip() {
    let req = ClientRequest::Initialize(InitializeRequestParams {
        client_info: Some(ClientInfo {
            name: "test_client".into(),
            title: None,
            version: Some("0.1.0".into()),
        }),
        capabilities: Some(InitializeCapabilities {
            experimental_api: true,
            opt_out_notification_methods: None,
        }),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "initialize");
    assert_eq!(value["params"]["client_info"]["name"], "test_client");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClientRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientRequest::Initialize(params) => {
            assert_eq!(params.client_info.unwrap().name, "test_client");
            assert!(params.capabilities.unwrap().experimental_api);
        }
        other => panic!("expected Initialize, got {other:?}"),
    }
}

#[test]
fn test_server_cancel_request_roundtrip() {
    let req = ServerRequest::CancelRequest(ServerCancelRequestParams {
        request_id: "req_cancel_1".into(),
        reason: Some("timed out".into()),
    });

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(value["method"], "control/cancelRequest");

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ServerRequest = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerRequest::CancelRequest(params) => {
            assert_eq!(params.request_id, "req_cancel_1");
            assert_eq!(params.reason.as_deref(), Some("timed out"));
        }
        other => panic!("expected CancelRequest, got {other:?}"),
    }
}

#[test]
fn test_mcp_tool_call_item_with_result() {
    let item = ThreadItem {
        id: "mcp_1".into(),
        details: ThreadItemDetails::McpToolCall(McpToolCallItem {
            server: "weather".into(),
            tool: "get_forecast".into(),
            arguments: json!({"city": "Tokyo"}),
            result: Some(McpToolCallResult {
                content: vec![json!({"type": "text", "text": "Sunny"})],
                structured_content: None,
            }),
            error: None,
            status: ItemStatus::Completed,
        }),
    };

    let json = serde_json::to_string(&item).unwrap();
    let parsed: ThreadItem = serde_json::from_str(&json).unwrap();
    match parsed.details {
        ThreadItemDetails::McpToolCall(mcp) => {
            assert!(mcp.result.is_some());
            assert_eq!(mcp.result.unwrap().content.len(), 1);
        }
        other => panic!("expected McpToolCall, got {other:?}"),
    }
}

#[test]
fn test_mcp_server_config_all_variants() {
    let stdio = McpServerConfig::Stdio {
        command: "npx".into(),
        args: vec!["server".into()],
        env: None,
    };
    let value: serde_json::Value = serde_json::to_value(&stdio).unwrap();
    assert_eq!(value["type"], "stdio");
    assert_eq!(value["command"], "npx");

    let sdk = McpServerConfig::Sdk {
        tools: vec![SdkMcpToolDef {
            name: "search".into(),
            description: Some("Search stuff".into()),
            input_schema: Some(json!({"type": "object"})),
        }],
    };
    let value: serde_json::Value = serde_json::to_value(&sdk).unwrap();
    assert_eq!(value["type"], "sdk");
    assert_eq!(value["tools"][0]["name"], "search");

    let json = serde_json::to_string(&sdk).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();
    match parsed {
        McpServerConfig::Sdk { tools } => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].name, "search");
        }
        other => panic!("expected Sdk, got {other:?}"),
    }
}

// ── Category B notification round-trip tests ────────────────────────

#[test]
fn test_plan_mode_changed_roundtrip() {
    let notif = ServerNotification::PlanModeChanged(PlanModeChangedParams {
        entered: true,
        plan_file: Some("/tmp/plan.md".into()),
        approved: None,
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["method"], "plan/modeChanged");
    match parsed {
        ServerNotification::PlanModeChanged(p) => {
            assert!(p.entered);
            assert_eq!(p.plan_file.as_deref(), Some("/tmp/plan.md"));
            assert_eq!(p.approved, None);
        }
        other => panic!("expected PlanModeChanged, got {other:?}"),
    }
}

#[test]
fn test_queue_state_changed_roundtrip() {
    let notif = ServerNotification::QueueStateChanged(QueueStateChangedParams { queued: 5 });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::QueueStateChanged(p) => assert_eq!(p.queued, 5),
        other => panic!("expected QueueStateChanged, got {other:?}"),
    }
}

#[test]
fn test_rewind_completed_roundtrip() {
    let notif = ServerNotification::RewindCompleted(RewindCompletedParams {
        rewound_turn: 3,
        restored_files: 5,
        messages_removed: 12,
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::RewindCompleted(p) => {
            assert_eq!(p.rewound_turn, 3);
            assert_eq!(p.restored_files, 5);
            assert_eq!(p.messages_removed, 12);
        }
        other => panic!("expected RewindCompleted, got {other:?}"),
    }
}

#[test]
fn test_rewind_failed_roundtrip() {
    let notif = ServerNotification::RewindFailed(RewindFailedParams {
        error: "no snapshot".into(),
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::RewindFailed(p) => assert_eq!(p.error, "no snapshot"),
        other => panic!("expected RewindFailed, got {other:?}"),
    }
}

#[test]
fn test_cost_warning_roundtrip() {
    let notif = ServerNotification::CostWarning(CostWarningParams {
        current_cost_cents: 800,
        threshold_cents: 500,
        budget_cents: Some(2000),
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::CostWarning(p) => {
            assert_eq!(p.current_cost_cents, 800);
            assert_eq!(p.threshold_cents, 500);
            assert_eq!(p.budget_cents, Some(2000));
        }
        other => panic!("expected CostWarning, got {other:?}"),
    }
}

#[test]
fn test_sandbox_state_changed_roundtrip() {
    let notif = ServerNotification::SandboxStateChanged(SandboxStateChangedParams {
        active: true,
        enforcement: "read_only".into(),
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::SandboxStateChanged(p) => {
            assert!(p.active);
            assert_eq!(p.enforcement, "read_only");
        }
        other => panic!("expected SandboxStateChanged, got {other:?}"),
    }
}

#[test]
fn test_fast_mode_changed_roundtrip() {
    let notif = ServerNotification::FastModeChanged(FastModeChangedParams { active: true });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::FastModeChanged(p) => assert!(p.active),
        other => panic!("expected FastModeChanged, got {other:?}"),
    }
}

#[test]
fn test_agents_registered_roundtrip() {
    let notif = ServerNotification::AgentsRegistered(AgentsRegisteredParams {
        agents: vec![AgentInfo {
            name: "Review".into(),
            agent_type: "code-review".into(),
            description: Some("Reviews PRs".into()),
        }],
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::AgentsRegistered(p) => {
            assert_eq!(p.agents.len(), 1);
            assert_eq!(p.agents[0].name, "Review");
            assert_eq!(p.agents[0].agent_type, "code-review");
        }
        other => panic!("expected AgentsRegistered, got {other:?}"),
    }
}

#[test]
fn test_hook_executed_roundtrip() {
    let notif = ServerNotification::HookExecuted(HookExecutedParams {
        hook_type: "before_tool_call".into(),
        hook_name: "lint".into(),
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::HookExecuted(p) => {
            assert_eq!(p.hook_type, "before_tool_call");
            assert_eq!(p.hook_name, "lint");
        }
        other => panic!("expected HookExecuted, got {other:?}"),
    }
}

#[test]
fn test_summarize_completed_roundtrip() {
    let notif = ServerNotification::SummarizeCompleted(SummarizeCompletedParams {
        from_turn: 2,
        summary_tokens: 300,
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::SummarizeCompleted(p) => {
            assert_eq!(p.from_turn, 2);
            assert_eq!(p.summary_tokens, 300);
        }
        other => panic!("expected SummarizeCompleted, got {other:?}"),
    }
}

#[test]
fn test_summarize_failed_roundtrip() {
    let notif = ServerNotification::SummarizeFailed(SummarizeFailedParams {
        error: "no messages".into(),
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed: ServerNotification = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerNotification::SummarizeFailed(p) => assert_eq!(p.error, "no messages"),
        other => panic!("expected SummarizeFailed, got {other:?}"),
    }
}
