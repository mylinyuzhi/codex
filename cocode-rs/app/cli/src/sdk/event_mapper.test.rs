use cocode_app_server_protocol::*;
use cocode_protocol::LoopEvent;
use cocode_protocol::McpStartupStatus;
use cocode_protocol::ToolResultContent;
use serde_json::json;

use super::event_mapper::EventMapper;

#[test]
fn test_text_delta_emits_item_started_then_delta() {
    let mut mapper = EventMapper::new("turn_1".into());
    let events = mapper.map(LoopEvent::TextDelta {
        turn_id: "turn_1".into(),
        delta: "Hello".into(),
    });

    // First delta should emit item/started + agentMessage/delta
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], ServerNotification::ItemStarted(_)));
    match &events[1] {
        ServerNotification::AgentMessageDelta(params) => {
            assert_eq!(params.delta, "Hello");
            assert_eq!(params.turn_id, "turn_1");
        }
        other => panic!("expected AgentMessageDelta, got {other:?}"),
    }

    // Second delta should only emit agentMessage/delta (no item/started)
    let events = mapper.map(LoopEvent::TextDelta {
        turn_id: "turn_1".into(),
        delta: " World".into(),
    });
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        ServerNotification::AgentMessageDelta(_)
    ));
}

#[test]
fn test_thinking_delta_emits_item_started_then_delta() {
    let mut mapper = EventMapper::new("turn_1".into());
    let events = mapper.map(LoopEvent::ThinkingDelta {
        turn_id: "turn_1".into(),
        delta: "Thinking...".into(),
    });

    // First delta should emit item/started + reasoning/delta
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], ServerNotification::ItemStarted(_)));
    match &events[1] {
        ServerNotification::ReasoningDelta(params) => {
            assert_eq!(params.delta, "Thinking...");
        }
        other => panic!("expected ReasoningDelta, got {other:?}"),
    }
}

#[test]
fn test_thinking_to_text_transition_completes_reasoning() {
    let mut mapper = EventMapper::new("turn_1".into());

    // Start thinking
    mapper.map(LoopEvent::ThinkingDelta {
        turn_id: "turn_1".into(),
        delta: "Let me think".into(),
    });

    // Transition to text — should complete reasoning item
    let events = mapper.map(LoopEvent::TextDelta {
        turn_id: "turn_1".into(),
        delta: "Answer".into(),
    });

    // Should have: reasoning/item_completed + text/item_started + text/delta
    assert_eq!(events.len(), 3);
    match &events[0] {
        ServerNotification::ItemCompleted(params) => match &params.item.details {
            ThreadItemDetails::Reasoning(r) => {
                assert_eq!(r.text, "Let me think");
            }
            other => panic!("expected Reasoning, got {other:?}"),
        },
        other => panic!("expected ItemCompleted, got {other:?}"),
    }
    assert!(matches!(&events[1], ServerNotification::ItemStarted(_)));
    assert!(matches!(
        &events[2],
        ServerNotification::AgentMessageDelta(_)
    ));
}

#[test]
fn test_flush_emits_accumulated_items() {
    let mut mapper = EventMapper::new("turn_1".into());

    // Accumulate text
    mapper.map(LoopEvent::TextDelta {
        turn_id: "turn_1".into(),
        delta: "Hello World".into(),
    });

    // Flush should emit completed AgentMessage
    let flushed = mapper.flush();
    assert_eq!(flushed.len(), 1);
    match &flushed[0] {
        ServerNotification::ItemCompleted(params) => match &params.item.details {
            ThreadItemDetails::AgentMessage(msg) => {
                assert_eq!(msg.text, "Hello World");
            }
            other => panic!("expected AgentMessage, got {other:?}"),
        },
        other => panic!("expected ItemCompleted, got {other:?}"),
    }

    // Second flush should be empty
    assert!(mapper.flush().is_empty());
}

#[test]
fn test_flush_emits_reasoning_and_text() {
    let mut mapper = EventMapper::new("turn_1".into());

    // Only reasoning (no text transition)
    mapper.map(LoopEvent::ThinkingDelta {
        turn_id: "turn_1".into(),
        delta: "deep thought".into(),
    });

    let flushed = mapper.flush();
    assert_eq!(flushed.len(), 1);
    match &flushed[0] {
        ServerNotification::ItemCompleted(params) => {
            assert!(matches!(
                &params.item.details,
                ThreadItemDetails::Reasoning(_)
            ));
        }
        other => panic!("expected ItemCompleted, got {other:?}"),
    }
}

#[test]
fn test_tool_use_lifecycle_bash() {
    let mut mapper = EventMapper::new("turn_1".into());

    // ToolUseQueued → ItemStarted
    let events = mapper.map(LoopEvent::ToolUseQueued {
        call_id: "call_1".into(),
        name: "Bash".into(),
        input: json!({"command": "ls -la"}),
    });
    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::ItemStarted(params) => match &params.item.details {
            ThreadItemDetails::CommandExecution(cmd) => {
                assert_eq!(cmd.command, "ls -la");
                assert_eq!(cmd.status, ItemStatus::InProgress);
            }
            other => panic!("expected CommandExecution, got {other:?}"),
        },
        other => panic!("expected ItemStarted, got {other:?}"),
    }

    // ToolUseStarted → ItemUpdated
    let events = mapper.map(LoopEvent::ToolUseStarted {
        call_id: "call_1".into(),
        name: "Bash".into(),
        batch_id: None,
    });
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], ServerNotification::ItemUpdated(_)));

    // ToolUseCompleted → ItemCompleted
    let events = mapper.map(LoopEvent::ToolUseCompleted {
        call_id: "call_1".into(),
        output: ToolResultContent::Text("file1.rs\nfile2.rs".into()),
        is_error: false,
    });
    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::ItemCompleted(params) => match &params.item.details {
            ThreadItemDetails::CommandExecution(cmd) => {
                assert_eq!(cmd.status, ItemStatus::Completed);
                assert_eq!(cmd.aggregated_output, "file1.rs\nfile2.rs");
            }
            other => panic!("expected CommandExecution, got {other:?}"),
        },
        other => panic!("expected ItemCompleted, got {other:?}"),
    }
}

#[test]
fn test_tool_use_edit_maps_to_file_change() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::ToolUseQueued {
        call_id: "call_2".into(),
        name: "Edit".into(),
        input: json!({"file_path": "/src/main.rs", "old_string": "a", "new_string": "b"}),
    });

    match &events[0] {
        ServerNotification::ItemStarted(params) => match &params.item.details {
            ThreadItemDetails::FileChange(fc) => {
                assert_eq!(fc.changes[0].path, "/src/main.rs");
                assert_eq!(fc.changes[0].kind, FileChangeKind::Update);
            }
            other => panic!("expected FileChange, got {other:?}"),
        },
        other => panic!("expected ItemStarted, got {other:?}"),
    }
}

#[test]
fn test_mcp_tool_maps_to_mcp_tool_call() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::ToolUseQueued {
        call_id: "call_3".into(),
        name: "mcp__myserver__search".into(),
        input: json!({"query": "test"}),
    });

    match &events[0] {
        ServerNotification::ItemStarted(params) => match &params.item.details {
            ThreadItemDetails::McpToolCall(mcp) => {
                assert_eq!(mcp.server, "myserver");
                assert_eq!(mcp.tool, "search");
            }
            other => panic!("expected McpToolCall, got {other:?}"),
        },
        other => panic!("expected ItemStarted, got {other:?}"),
    }
}

#[test]
fn test_mcp_tool_call_begin_end_lifecycle() {
    let mut mapper = EventMapper::new("turn_1".into());

    // McpToolCallBegin → ItemStarted
    let events = mapper.map(LoopEvent::McpToolCallBegin {
        server: "db-server".into(),
        tool: "query".into(),
        call_id: "mcp_1".into(),
    });
    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::ItemStarted(params) => match &params.item.details {
            ThreadItemDetails::McpToolCall(mcp) => {
                assert_eq!(mcp.server, "db-server");
                assert_eq!(mcp.tool, "query");
                assert_eq!(mcp.status, ItemStatus::InProgress);
            }
            other => panic!("expected McpToolCall, got {other:?}"),
        },
        other => panic!("expected ItemStarted, got {other:?}"),
    }

    // McpToolCallEnd → ItemCompleted
    let events = mapper.map(LoopEvent::McpToolCallEnd {
        server: "db-server".into(),
        tool: "query".into(),
        call_id: "mcp_1".into(),
        is_error: false,
    });
    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::ItemCompleted(params) => match &params.item.details {
            ThreadItemDetails::McpToolCall(mcp) => {
                assert_eq!(mcp.status, ItemStatus::Completed);
            }
            other => panic!("expected McpToolCall, got {other:?}"),
        },
        other => panic!("expected ItemCompleted, got {other:?}"),
    }
}

#[test]
fn test_mcp_startup_update_forwarded() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::McpStartupUpdate {
        server: "my-mcp".into(),
        status: McpStartupStatus::Connecting,
    });

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        ServerNotification::McpStartupStatus(_)
    ));
}

#[test]
fn test_generic_tool_maps_to_tool_call() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::ToolUseQueued {
        call_id: "call_4".into(),
        name: "Read".into(),
        input: json!({"file_path": "/tmp/test.txt"}),
    });

    match &events[0] {
        ServerNotification::ItemStarted(params) => match &params.item.details {
            ThreadItemDetails::ToolCall(tc) => {
                assert_eq!(tc.tool, "Read");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected ItemStarted, got {other:?}"),
    }
}

#[test]
fn test_subagent_events() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::SubagentSpawned {
        agent_id: "agent_1".into(),
        agent_type: "Explore".into(),
        description: "Search codebase".into(),
        color: Some("blue".into()),
    });

    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::SubagentSpawned(params) => {
            assert_eq!(params.agent_id, "agent_1");
            assert_eq!(params.agent_type, "Explore");
        }
        other => panic!("expected SubagentSpawned, got {other:?}"),
    }
}

#[test]
fn test_compaction_completed() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::CompactionCompleted {
        removed_messages: 5,
        summary_tokens: 100,
    });

    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::ContextCompacted(params) => {
            assert_eq!(params.removed_messages, 5);
            assert_eq!(params.summary_tokens, 100);
        }
        other => panic!("expected ContextCompacted, got {other:?}"),
    }
}

#[test]
fn test_ui_only_events_dropped() {
    let mut mapper = EventMapper::new("turn_1".into());

    let dropped_events = vec![
        LoopEvent::StreamRequestStart,
        LoopEvent::Interrupted,
        LoopEvent::MaxTurnsReached,
        LoopEvent::CompactionStarted,
        LoopEvent::PromptCacheMiss,
        LoopEvent::ModelFallbackCompleted,
    ];

    for event in dropped_events {
        let notifications = mapper.map(event);
        assert!(
            notifications.is_empty(),
            "UI-only event should produce no notifications"
        );
    }
}

#[test]
fn test_error_event() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::Error {
        error: cocode_protocol::LoopError {
            code: "INTERNAL".into(),
            message: "something went wrong".into(),
            recoverable: false,
        },
    });

    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::Error(params) => {
            assert!(params.message.contains("something went wrong"));
            assert_eq!(params.category.as_deref(), Some("internal"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_failed_tool_use() {
    let mut mapper = EventMapper::new("turn_1".into());

    mapper.map(LoopEvent::ToolUseQueued {
        call_id: "call_5".into(),
        name: "Bash".into(),
        input: json!({"command": "false"}),
    });

    let events = mapper.map(LoopEvent::ToolUseCompleted {
        call_id: "call_5".into(),
        output: ToolResultContent::Text("command failed".into()),
        is_error: true,
    });

    match &events[0] {
        ServerNotification::ItemCompleted(params) => match &params.item.details {
            ThreadItemDetails::CommandExecution(cmd) => {
                assert_eq!(cmd.status, ItemStatus::Failed);
            }
            other => panic!("expected CommandExecution, got {other:?}"),
        },
        other => panic!("expected ItemCompleted, got {other:?}"),
    }
}

#[test]
fn test_question_asked_forwarded() {
    let mut mapper = EventMapper::new("turn_1".into());

    let events = mapper.map(LoopEvent::QuestionAsked {
        request_id: "q_1".into(),
        questions: json!([{"question": "Which option?"}]),
    });

    assert_eq!(events.len(), 1);
    match &events[0] {
        ServerNotification::Error(params) => {
            assert_eq!(params.category.as_deref(), Some("user_question"));
            assert!(params.message.contains("q_1"));
        }
        other => panic!("expected Error with user_question category, got {other:?}"),
    }
}
