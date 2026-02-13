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
    assert_eq!(HookEventType::Stop.as_str(), "stop");
    assert_eq!(HookEventType::SubagentStart.as_str(), "subagent_start");
    assert_eq!(HookEventType::SubagentStop.as_str(), "subagent_stop");
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
