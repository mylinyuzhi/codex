use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::TokenUsage;

#[test]
fn agent_stream_event_serializes_with_snake_case_tag() {
    let event = AgentStreamEvent::TextDelta {
        turn_id: "turn-1".into(),
        delta: "hello".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json,
        json!({
            "type": "text_delta",
            "turn_id": "turn-1",
            "delta": "hello"
        })
    );
}

#[test]
fn agent_stream_event_tool_use_queued_carries_full_input() {
    let event = AgentStreamEvent::ToolUseQueued {
        call_id: "call-1".into(),
        name: "Bash".into(),
        input: json!({ "command": "ls -la" }),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_use_queued");
    assert_eq!(json["input"]["command"], "ls -la");
}

#[test]
fn thread_item_command_execution_roundtrips() {
    let item = ThreadItem {
        item_id: "item-1".into(),
        turn_id: "turn-1".into(),
        details: ThreadItemDetails::CommandExecution {
            command: "ls".into(),
            output: "file1\nfile2".into(),
            exit_code: Some(0),
            status: ItemStatus::Completed,
        },
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: ThreadItem = serde_json::from_str(&json).unwrap();
    match back.details {
        ThreadItemDetails::CommandExecution {
            command,
            output,
            exit_code,
            status,
        } => {
            assert_eq!(command, "ls");
            assert_eq!(output, "file1\nfile2");
            assert_eq!(exit_code, Some(0));
            assert_eq!(status, ItemStatus::Completed);
        }
        _ => panic!("expected CommandExecution"),
    }
}

#[test]
fn thread_item_file_change_roundtrips() {
    let item = ThreadItem {
        item_id: "item-2".into(),
        turn_id: "turn-1".into(),
        details: ThreadItemDetails::FileChange {
            changes: vec![FileChangeInfo {
                path: "src/main.rs".into(),
                kind: "modify".into(),
            }],
            status: ItemStatus::InProgress,
        },
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: ThreadItem = serde_json::from_str(&json).unwrap();
    match back.details {
        ThreadItemDetails::FileChange { changes, status } => {
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].path, "src/main.rs");
            assert_eq!(status, ItemStatus::InProgress);
        }
        _ => panic!("expected FileChange"),
    }
}

#[test]
fn item_status_serializes_snake_case() {
    assert_eq!(
        serde_json::to_value(ItemStatus::InProgress).unwrap(),
        json!("in_progress")
    );
    assert_eq!(
        serde_json::to_value(ItemStatus::Completed).unwrap(),
        json!("completed")
    );
    assert_eq!(
        serde_json::to_value(ItemStatus::Failed).unwrap(),
        json!("failed")
    );
    assert_eq!(
        serde_json::to_value(ItemStatus::Declined).unwrap(),
        json!("declined")
    );
}

#[test]
fn server_notification_turn_started_wire_method() {
    let notif = ServerNotification::TurnStarted(TurnStartedParams {
        turn_id: Some("t1".into()),
        turn_number: 1,
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "turn/started");
    assert_eq!(json["params"]["turn_number"], 1);
    assert_eq!(json["params"]["turn_id"], "t1");
}

#[test]
fn server_notification_session_state_changed_wire_method() {
    let notif = ServerNotification::SessionStateChanged {
        state: SessionState::Running,
    };
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "session/stateChanged");
    assert_eq!(json["params"]["state"], "running");
}

#[test]
fn server_notification_hook_started_wire_method() {
    let notif = ServerNotification::HookStarted(HookStartedParams {
        hook_id: "h1".into(),
        hook_name: "pre-tool".into(),
        hook_event: "PreToolUse".into(),
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "hook/started");
    assert_eq!(json["params"]["hook_id"], "h1");
}

#[test]
fn server_notification_item_started_embeds_thread_item() {
    let item = ThreadItem {
        item_id: "item-1".into(),
        turn_id: "turn-1".into(),
        details: ThreadItemDetails::AgentMessage { text: "hi".into() },
    };
    let notif = ServerNotification::ItemStarted { item };
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "item/started");
    assert_eq!(json["params"]["item"]["item_id"], "item-1");
    assert_eq!(json["params"]["item"]["details"]["type"], "agent_message");
    assert_eq!(json["params"]["item"]["details"]["text"], "hi");
}

#[test]
fn server_notification_stream_request_end_carries_usage() {
    let notif = ServerNotification::StreamRequestEnd {
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
    };
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "stream/requestEnd");
    assert_eq!(json["params"]["usage"]["input_tokens"], 100);
}

#[test]
fn rate_limit_status_serializes_snake_case() {
    assert_eq!(
        serde_json::to_value(RateLimitStatus::AllowedWarning).unwrap(),
        json!("allowed_warning")
    );
}

#[test]
fn hook_outcome_status_serializes_snake_case() {
    assert_eq!(
        serde_json::to_value(HookOutcomeStatus::Cancelled).unwrap(),
        json!("cancelled")
    );
}

#[test]
fn core_event_debug_formatting_works() {
    let ev = CoreEvent::Protocol(ServerNotification::TurnStarted(TurnStartedParams {
        turn_id: None,
        turn_number: 1,
    }));
    let s = format!("{ev:?}");
    assert!(s.contains("Protocol"));
    assert!(s.contains("TurnStarted"));
}

// ---------- New P2 gap variants ----------

#[test]
fn local_command_output_wire_method() {
    let notif = ServerNotification::LocalCommandOutput(LocalCommandOutputParams {
        content: json!({"stdout": "hello\n"}),
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "localCommand/output");
    assert_eq!(json["params"]["content"]["stdout"], "hello\n");
}

#[test]
fn files_persisted_wire_method() {
    let notif = ServerNotification::FilesPersisted(FilesPersistedParams {
        files: vec![PersistedFileInfo {
            filename: "a.txt".into(),
            file_id: "f-1".into(),
        }],
        failed: vec![],
        processed_at: "2026-04-12T00:00:00Z".into(),
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "files/persisted");
    assert_eq!(json["params"]["files"][0]["file_id"], "f-1");
}

#[test]
fn elicitation_complete_wire_method() {
    let notif = ServerNotification::ElicitationComplete(ElicitationCompleteParams {
        mcp_server_name: "github".into(),
        elicitation_id: "e-1".into(),
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "elicitation/complete");
    assert_eq!(json["params"]["mcp_server_name"], "github");
}

#[test]
fn tool_use_summary_wire_method() {
    let notif = ServerNotification::ToolUseSummary(ToolUseSummaryParams {
        summary: "read 3 files".into(),
        preceding_tool_use_ids: vec!["t1".into(), "t2".into(), "t3".into()],
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "tool/useSummary");
    assert_eq!(json["params"]["summary"], "read 3 files");
    assert_eq!(
        json["params"]["preceding_tool_use_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn tool_progress_wire_method() {
    let notif = ServerNotification::ToolProgress(ToolProgressParams {
        tool_use_id: "tu-1".into(),
        tool_name: "Bash".into(),
        parent_tool_use_id: Some("parent".into()),
        elapsed_time_seconds: 42.5,
        task_id: None,
    });
    let json = serde_json::to_value(&notif).unwrap();
    assert_eq!(json["method"], "tool/progress");
    assert_eq!(json["params"]["tool_name"], "Bash");
    assert_eq!(json["params"]["elapsed_time_seconds"], 42.5);
}

// ---------- TS alignment tests ----------

#[test]
fn hook_response_params_has_stdout_stderr() {
    // Matches TS SDKHookResponseMessage (coreSchemas.ts:1631-1646)
    let p = HookResponseParams {
        hook_id: "h1".into(),
        hook_name: "pre".into(),
        hook_event: "PreToolUse".into(),
        output: "ok".into(),
        stdout: "out".into(),
        stderr: "err".into(),
        exit_code: Some(0),
        outcome: HookOutcomeStatus::Success,
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["stdout"], "out");
    assert_eq!(j["stderr"], "err");
    assert_eq!(j["exit_code"], 0);
    assert_eq!(j["outcome"], "success");
}

#[test]
fn task_started_params_description_required_task_type_optional() {
    // Matches TS SDKTaskStartedMessage
    let p = TaskStartedParams {
        task_id: "t1".into(),
        tool_use_id: Some("u1".into()),
        description: "do something".into(),
        task_type: None, // optional
        workflow_name: None,
        prompt: None,
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["description"], "do something");
    assert!(j.get("task_type").is_none() || j["task_type"].is_null());
}

#[test]
fn task_progress_params_description_and_usage_required() {
    let p = TaskProgressParams {
        task_id: "t1".into(),
        tool_use_id: None,
        description: "working".into(),
        usage: TaskUsage {
            total_tokens: 1000,
            tool_uses: 5,
            duration_ms: 12_000,
        },
        last_tool_name: Some("Bash".into()),
        summary: None,
        workflow_progress: vec![],
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["description"], "working");
    assert_eq!(j["usage"]["total_tokens"], 1000);
    assert_eq!(j["usage"]["tool_uses"], 5);
}

#[test]
fn task_completed_uses_ts_task_notification_shape() {
    let p = TaskCompletedParams {
        task_id: "t1".into(),
        tool_use_id: Some("u1".into()),
        status: TaskCompletionStatus::Completed,
        output_file: "/tmp/out.txt".into(),
        summary: "done".into(),
        usage: None,
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["status"], "completed");
    assert_eq!(j["output_file"], "/tmp/out.txt");
    assert_eq!(j["summary"], "done");
}

#[test]
fn session_result_has_model_usage_and_permission_denials() {
    let mut usage = std::collections::HashMap::new();
    usage.insert(
        "claude-opus".into(),
        SessionModelUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            web_search_requests: 0,
            cost_usd: 0.01,
            context_window: 200000,
            max_output_tokens: 16384,
        },
    );
    let p = SessionResultParams {
        session_id: "s1".into(),
        total_turns: 5,
        duration_ms: 10_000,
        duration_api_ms: 8_000,
        is_error: false,
        stop_reason: "end_turn".into(),
        total_cost_usd: 0.01,
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
        model_usage: usage,
        permission_denials: vec![PermissionDenialInfo {
            tool_name: "Bash".into(),
            tool_use_id: "u1".into(),
            tool_input: json!({"command": "rm -rf /"}),
        }],
        result: Some("done".into()),
        errors: vec![],
        structured_output: None,
        fast_mode_state: Some(FastModeState::On),
        num_api_calls: Some(3),
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["total_cost_usd"], 0.01);
    assert_eq!(j["model_usage"]["claude-opus"]["input_tokens"], 100);
    assert_eq!(j["permission_denials"][0]["tool_name"], "Bash");
    assert_eq!(j["fast_mode_state"], "on");
}

#[test]
fn session_started_has_all_init_fields() {
    let p = SessionStartedParams {
        session_id: "s1".into(),
        protocol_version: "1.0".into(),
        cwd: "/tmp".into(),
        model: "claude-opus".into(),
        permission_mode: "default".into(),
        tools: vec!["Bash".into(), "Read".into()],
        slash_commands: vec!["/help".into()],
        agents: vec!["researcher".into()],
        skills: vec![],
        mcp_servers: vec![McpServerInit {
            name: "github".into(),
            status: crate::server_request::McpConnectionStatus::Connected,
        }],
        plugins: vec![],
        api_key_source: Some("env".into()),
        betas: vec![],
        version: "0.0.1".into(),
        output_style: None,
        fast_mode_state: None,
    };
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["cwd"], "/tmp");
    assert_eq!(j["tools"].as_array().unwrap().len(), 2);
    assert_eq!(j["mcp_servers"][0]["name"], "github");
}
