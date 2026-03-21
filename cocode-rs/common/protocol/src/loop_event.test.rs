use super::*;

#[test]
fn test_token_usage() {
    let usage = TokenUsage::new(100, 50);
    assert_eq!(usage.input_tokens, 100i64);
    assert_eq!(usage.output_tokens, 50i64);
    assert_eq!(usage.total(), 150i64);
}

#[test]
fn test_abort_reason() {
    assert_eq!(
        AbortReason::StreamingFallback.as_str(),
        "streaming_fallback"
    );
    assert_eq!(AbortReason::SiblingError.as_str(), "sibling_error");
    assert_eq!(AbortReason::UserInterrupted.as_str(), "user_interrupted");
}

#[test]
fn test_hook_event_type() {
    assert_eq!(HookEventType::PreToolUse.as_str(), "pre_tool_use");
    assert_eq!(HookEventType::PostToolUse.as_str(), "post_tool_use");
    assert_eq!(
        HookEventType::PostToolUseFailure.as_str(),
        "post_tool_use_failure"
    );
    assert_eq!(HookEventType::SessionStart.as_str(), "session_start");
    assert_eq!(HookEventType::PreCompact.as_str(), "pre_compact");
    assert_eq!(HookEventType::PostCompact.as_str(), "post_compact");
    assert_eq!(HookEventType::Stop.as_str(), "stop");
    assert_eq!(HookEventType::SubagentStart.as_str(), "subagent_start");
    assert_eq!(HookEventType::SubagentStop.as_str(), "subagent_stop");
}

#[test]
fn test_hook_event_type_from_str_snake_case() {
    assert_eq!(
        "pre_tool_use".parse::<HookEventType>().unwrap(),
        HookEventType::PreToolUse
    );
    assert_eq!(
        "post_tool_use".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUse
    );
    assert_eq!(
        "post_tool_use_failure".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUseFailure
    );
    assert_eq!(
        "user_prompt_submit".parse::<HookEventType>().unwrap(),
        HookEventType::UserPromptSubmit
    );
    assert_eq!(
        "session_start".parse::<HookEventType>().unwrap(),
        HookEventType::SessionStart
    );
    assert_eq!(
        "session_end".parse::<HookEventType>().unwrap(),
        HookEventType::SessionEnd
    );
    assert_eq!(
        "stop".parse::<HookEventType>().unwrap(),
        HookEventType::Stop
    );
    assert_eq!(
        "subagent_start".parse::<HookEventType>().unwrap(),
        HookEventType::SubagentStart
    );
    assert_eq!(
        "subagent_stop".parse::<HookEventType>().unwrap(),
        HookEventType::SubagentStop
    );
    assert_eq!(
        "pre_compact".parse::<HookEventType>().unwrap(),
        HookEventType::PreCompact
    );
    assert_eq!(
        "post_compact".parse::<HookEventType>().unwrap(),
        HookEventType::PostCompact
    );
    assert_eq!(
        "notification".parse::<HookEventType>().unwrap(),
        HookEventType::Notification
    );
    assert_eq!(
        "permission_request".parse::<HookEventType>().unwrap(),
        HookEventType::PermissionRequest
    );
    assert_eq!(
        "teammate_idle".parse::<HookEventType>().unwrap(),
        HookEventType::TeammateIdle
    );
    assert_eq!(
        "task_completed".parse::<HookEventType>().unwrap(),
        HookEventType::TaskCompleted
    );
}

#[test]
fn test_hook_event_type_from_str_pascal_case() {
    assert_eq!(
        "PreToolUse".parse::<HookEventType>().unwrap(),
        HookEventType::PreToolUse
    );
    assert_eq!(
        "PostToolUse".parse::<HookEventType>().unwrap(),
        HookEventType::PostToolUse
    );
    assert_eq!(
        "SessionStart".parse::<HookEventType>().unwrap(),
        HookEventType::SessionStart
    );
    assert_eq!(
        "Stop".parse::<HookEventType>().unwrap(),
        HookEventType::Stop
    );
    assert_eq!(
        "TeammateIdle".parse::<HookEventType>().unwrap(),
        HookEventType::TeammateIdle
    );
    assert_eq!(
        "TaskCompleted".parse::<HookEventType>().unwrap(),
        HookEventType::TaskCompleted
    );
}

#[test]
fn test_hook_event_type_from_str_unknown() {
    assert!("unknown_event".parse::<HookEventType>().is_err());
    let err = "bogus".parse::<HookEventType>().unwrap_err();
    assert!(err.contains("unknown hook event type"));
}

#[test]
fn test_compaction_skipped_by_hook_event() {
    let event = LoopEvent::CompactionSkippedByHook {
        hook_name: "save-work-first".to_string(),
        reason: "Unsaved changes detected".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("compaction_skipped_by_hook"));
    assert!(json.contains("save-work-first"));
    assert!(json.contains("Unsaved changes detected"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::CompactionSkippedByHook { hook_name, reason } => {
            assert_eq!(hook_name, "save-work-first");
            assert_eq!(reason, "Unsaved changes detected");
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_compaction_retry_event() {
    let event = LoopEvent::CompactionRetry {
        attempt: 1,
        max_attempts: 3,
        delay_ms: 1000,
        reason: "API timeout".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("compaction_retry"));
    assert!(json.contains("API timeout"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::CompactionRetry {
            attempt,
            max_attempts,
            delay_ms,
            reason,
        } => {
            assert_eq!(attempt, 1);
            assert_eq!(max_attempts, 3);
            assert_eq!(delay_ms, 1000);
            assert_eq!(reason, "API timeout");
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_compaction_failed_event() {
    let event = LoopEvent::CompactionFailed {
        attempts: 3,
        error: "All retries exhausted".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("compaction_failed"));
    assert!(json.contains("All retries exhausted"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::CompactionFailed { attempts, error } => {
            assert_eq!(attempts, 3);
            assert_eq!(error, "All retries exhausted");
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_loop_event_serde() {
    let event = LoopEvent::TurnStarted {
        turn_id: "turn-1".to_string(),
        turn_number: 1,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("turn_started"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    if let LoopEvent::TurnStarted {
        turn_id,
        turn_number,
    } = parsed
    {
        assert_eq!(turn_id, "turn-1");
        assert_eq!(turn_number, 1);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_retry_info() {
    let info = RetryInfo {
        attempt: 1,
        max_attempts: 3,
        delay_ms: 1000,
        retriable: true,
    };

    let json = serde_json::to_string(&info).unwrap();
    let parsed: RetryInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, info);
}

#[test]
fn test_tool_result_content() {
    let text = ToolResultContent::Text("Hello".to_string());
    let json = serde_json::to_string(&text).unwrap();
    assert_eq!(json, "\"Hello\"");

    let structured = ToolResultContent::Structured(serde_json::json!({"key": "value"}));
    let json = serde_json::to_string(&structured).unwrap();
    assert!(json.contains("key"));
}

#[test]
fn test_mcp_startup_status() {
    let status = McpStartupStatus::Ready;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"ready\"");

    let parsed: McpStartupStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, McpStartupStatus::Ready);
}

#[test]
fn test_context_usage_warning_event() {
    let event = LoopEvent::ContextUsageWarning {
        estimated_tokens: 150000,
        warning_threshold: 140000,
        percent_left: 0.25,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("context_usage_warning"));
    assert!(json.contains("150000"));
    assert!(json.contains("0.25"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::ContextUsageWarning {
            estimated_tokens,
            warning_threshold,
            percent_left,
        } => {
            assert_eq!(estimated_tokens, 150000);
            assert_eq!(warning_threshold, 140000);
            assert!((percent_left - 0.25).abs() < f64::EPSILON);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_micro_compaction_started_event() {
    let event = LoopEvent::MicroCompactionStarted {
        candidates: 5,
        potential_savings: 25000,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("micro_compaction_started"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::MicroCompactionStarted {
            candidates,
            potential_savings,
        } => {
            assert_eq!(candidates, 5);
            assert_eq!(potential_savings, 25000);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_micro_compaction_applied_event() {
    let event = LoopEvent::MicroCompactionApplied {
        removed_results: 3,
        tokens_saved: 15000,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("micro_compaction_applied"));
    assert!(json.contains("tokens_saved"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::MicroCompactionApplied {
            removed_results,
            tokens_saved,
        } => {
            assert_eq!(removed_results, 3);
            assert_eq!(tokens_saved, 15000);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_compaction_events_serde() {
    // Test CompactionStarted
    let event = LoopEvent::CompactionStarted;
    let json = serde_json::to_string(&event).unwrap();
    let _: LoopEvent = serde_json::from_str(&json).unwrap();

    // Test CompactionCompleted
    let event = LoopEvent::CompactionCompleted {
        removed_messages: 10,
        summary_tokens: 2000,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::CompactionCompleted {
            removed_messages,
            summary_tokens,
        } => {
            assert_eq!(removed_messages, 10);
            assert_eq!(summary_tokens, 2000);
        }
        _ => panic!("Wrong event type"),
    }

    // Test SessionMemoryCompactApplied
    let event = LoopEvent::SessionMemoryCompactApplied {
        saved_tokens: 50000,
        summary_tokens: 3000,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SessionMemoryCompactApplied {
            saved_tokens,
            summary_tokens,
        } => {
            assert_eq!(saved_tokens, 50000);
            assert_eq!(summary_tokens, 3000);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_session_memory_extraction_events() {
    // Test SessionMemoryExtractionStarted
    let event = LoopEvent::SessionMemoryExtractionStarted {
        current_tokens: 50000,
        tool_calls_since: 15,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("session_memory_extraction_started"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SessionMemoryExtractionStarted {
            current_tokens,
            tool_calls_since,
        } => {
            assert_eq!(current_tokens, 50000);
            assert_eq!(tool_calls_since, 15);
        }
        _ => panic!("Wrong event type"),
    }

    // Test SessionMemoryExtractionCompleted
    let event = LoopEvent::SessionMemoryExtractionCompleted {
        summary_tokens: 3000,
        last_summarized_id: "msg-abc123".to_string(),
        messages_summarized: 25,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("session_memory_extraction_completed"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SessionMemoryExtractionCompleted {
            summary_tokens,
            last_summarized_id,
            messages_summarized,
        } => {
            assert_eq!(summary_tokens, 3000);
            assert_eq!(last_summarized_id, "msg-abc123");
            assert_eq!(messages_summarized, 25);
        }
        _ => panic!("Wrong event type"),
    }

    // Test SessionMemoryExtractionFailed
    let event = LoopEvent::SessionMemoryExtractionFailed {
        error: "API timeout".to_string(),
        attempts: 2,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("session_memory_extraction_failed"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SessionMemoryExtractionFailed { error, attempts } => {
            assert_eq!(error, "API timeout");
            assert_eq!(attempts, 2);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_speculative_execution_events() {
    // Test SpeculativeStarted
    let event = LoopEvent::SpeculativeStarted {
        speculation_id: "spec-1".to_string(),
        tool_calls: vec!["call-1".to_string(), "call-2".to_string()],
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("speculative_started"));
    assert!(json.contains("spec-1"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SpeculativeStarted {
            speculation_id,
            tool_calls,
        } => {
            assert_eq!(speculation_id, "spec-1");
            assert_eq!(tool_calls.len(), 2);
        }
        _ => panic!("Wrong event type"),
    }

    // Test SpeculativeCommitted
    let event = LoopEvent::SpeculativeCommitted {
        speculation_id: "spec-1".to_string(),
        committed_count: 2,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("speculative_committed"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SpeculativeCommitted {
            speculation_id,
            committed_count,
        } => {
            assert_eq!(speculation_id, "spec-1");
            assert_eq!(committed_count, 2);
        }
        _ => panic!("Wrong event type"),
    }

    // Test SpeculativeRolledBack
    let event = LoopEvent::SpeculativeRolledBack {
        speculation_id: "spec-1".to_string(),
        reason: "Model reconsideration".to_string(),
        rolled_back_calls: vec!["call-1".to_string()],
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("speculative_rolled_back"));
    assert!(json.contains("Model reconsideration"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SpeculativeRolledBack {
            speculation_id,
            reason,
            rolled_back_calls,
        } => {
            assert_eq!(speculation_id, "spec-1");
            assert_eq!(reason, "Model reconsideration");
            assert_eq!(rolled_back_calls.len(), 1);
        }
        _ => panic!("Wrong event type"),
    }
}

// ============================================================================
// Rewind/Summarize Event Serde Tests
// ============================================================================

#[test]
fn test_rewind_mode_serde() {
    // Test all RewindMode variants
    let modes = vec![
        RewindMode::CodeAndConversation,
        RewindMode::ConversationOnly,
        RewindMode::CodeOnly,
    ];

    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: RewindMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }
}

#[test]
fn test_rewind_completed_event() {
    let event = LoopEvent::RewindCompleted {
        rewound_turn: 5,
        restored_files: 3,
        messages_removed: 12,
        mode: RewindMode::CodeAndConversation,
        restored_prompt: Some("Original user prompt".to_string()),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("rewind_completed"));
    // RewindMode serializes as PascalCase (no rename_all attribute)
    assert!(json.contains("CodeAndConversation"));
    assert!(json.contains("rewound_turn"));
    assert!(json.contains("restored_files"));
    assert!(json.contains("messages_removed"));
    assert!(json.contains("restored_prompt"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::RewindCompleted {
            rewound_turn,
            restored_files,
            messages_removed,
            mode,
            restored_prompt,
        } => {
            assert_eq!(rewound_turn, 5);
            assert_eq!(restored_files, 3);
            assert_eq!(messages_removed, 12);
            assert_eq!(mode, RewindMode::CodeAndConversation);
            assert_eq!(restored_prompt, Some("Original user prompt".to_string()));
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_rewind_failed_event() {
    let event = LoopEvent::RewindFailed {
        error: "No ghost commit available".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("rewind_failed"));
    assert!(json.contains("No ghost commit available"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::RewindFailed { error } => {
            assert_eq!(error, "No ghost commit available");
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_rewind_checkpoints_ready_event() {
    let event = LoopEvent::RewindCheckpointsReady {
        checkpoints: vec![
            RewindCheckpointItem {
                turn_number: 1,
                file_count: 0,
                user_message_preview: "Hello".to_string(),
                has_ghost_commit: false,
                modified_files: vec![],
                diff_stats: None,
            },
            RewindCheckpointItem {
                turn_number: 2,
                file_count: 2,
                user_message_preview: "Fix the bug".to_string(),
                has_ghost_commit: true,
                modified_files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
                diff_stats: Some(RewindDiffStats {
                    files_changed: 2,
                    insertions: 10,
                    deletions: 5,
                }),
            },
        ],
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("rewind_checkpoints_ready"));
    assert!(json.contains("turn_number"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::RewindCheckpointsReady { checkpoints } => {
            assert_eq!(checkpoints.len(), 2);
            assert_eq!(checkpoints[0].turn_number, 1);
            assert_eq!(checkpoints[1].file_count, 2);
            assert!(checkpoints[1].has_ghost_commit);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_summarize_completed_event() {
    let event = LoopEvent::SummarizeCompleted {
        from_turn: 3,
        summary_tokens: 1500,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("summarize_completed"));
    assert!(json.contains("from_turn"));
    assert!(json.contains("summary_tokens"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SummarizeCompleted {
            from_turn,
            summary_tokens,
        } => {
            assert_eq!(from_turn, 3);
            assert_eq!(summary_tokens, 1500);
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_summarize_failed_event() {
    let event = LoopEvent::SummarizeFailed {
        error: "Context window too small".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("summarize_failed"));
    assert!(json.contains("Context window too small"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::SummarizeFailed { error } => {
            assert_eq!(error, "Context window too small");
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_rewind_mode_variants() {
    // Test ConversationOnly mode
    let event = LoopEvent::RewindCompleted {
        rewound_turn: 2,
        restored_files: 0,
        messages_removed: 5,
        mode: RewindMode::ConversationOnly,
        restored_prompt: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    // RewindMode serializes as PascalCase (no rename_all attribute)
    assert!(json.contains("ConversationOnly"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    if let LoopEvent::RewindCompleted { mode, .. } = parsed {
        assert_eq!(mode, RewindMode::ConversationOnly);
    } else {
        panic!("Wrong event type");
    }

    // Test CodeOnly mode
    let event = LoopEvent::RewindCompleted {
        rewound_turn: 3,
        restored_files: 5,
        messages_removed: 0,
        mode: RewindMode::CodeOnly,
        restored_prompt: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("CodeOnly"));
    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    if let LoopEvent::RewindCompleted { mode, .. } = parsed {
        assert_eq!(mode, RewindMode::CodeOnly);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_compaction_circuit_breaker_open_event() {
    let event = LoopEvent::CompactionCircuitBreakerOpen {
        consecutive_failures: 3,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("compaction_circuit_breaker_open"));
    assert!(json.contains("3"));

    let parsed: LoopEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        LoopEvent::CompactionCircuitBreakerOpen {
            consecutive_failures,
        } => {
            assert_eq!(consecutive_failures, 3);
        }
        _ => panic!("Wrong event type"),
    }
}
