use super::*;
use std::path::PathBuf;

#[test]
fn test_default_compaction_config() {
    let config = CompactionConfig::default();
    assert!((config.threshold - 0.8).abs() < f64::EPSILON);
    assert!(config.micro_compact);
    assert_eq!(config.min_messages_to_keep, 4);
}

#[test]
fn test_should_compact_below_threshold() {
    assert!(!should_compact(7000, 10000, 0.8));
}

#[test]
fn test_should_compact_at_threshold() {
    assert!(should_compact(8000, 10000, 0.8));
}

#[test]
fn test_should_compact_above_threshold() {
    assert!(should_compact(9500, 10000, 0.8));
}

#[test]
fn test_should_compact_zero_max() {
    assert!(!should_compact(100, 0, 0.8));
}

#[test]
fn test_should_compact_negative_max() {
    assert!(!should_compact(100, -1, 0.8));
}

#[test]
fn test_micro_compact_candidates_empty() {
    let messages: Vec<serde_json::Value> = vec![];
    assert!(micro_compact_candidates(&messages).is_empty());
}

#[test]
fn test_micro_compact_candidates_no_tool_results() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    assert!(micro_compact_candidates(&messages).is_empty());
}

#[test]
fn test_micro_compact_candidates_small_tool_result() {
    let messages = vec![serde_json::json!({"role": "tool", "content": "ok"})];
    assert!(micro_compact_candidates(&messages).is_empty());
}

#[test]
fn test_micro_compact_candidates_large_tool_result() {
    let large_content = "x".repeat(3000);
    let messages = vec![
        serde_json::json!({"role": "user", "content": "do something"}),
        serde_json::json!({"role": "tool", "content": large_content}),
        serde_json::json!({"role": "assistant", "content": "done"}),
    ];
    let candidates = micro_compact_candidates(&messages);
    assert_eq!(candidates, vec![1]);
}

#[test]
fn test_micro_compact_candidates_tool_result_role() {
    let large_content = "y".repeat(2500);
    let messages = vec![serde_json::json!({"role": "tool_result", "content": large_content})];
    let candidates = micro_compact_candidates(&messages);
    assert_eq!(candidates, vec![0]);
}

#[test]
fn test_parse_session_memory_simple() {
    let content = "This is a summary of the conversation.";
    let summary = parse_session_memory(content).unwrap();
    assert_eq!(summary.summary, "This is a summary of the conversation.");
    assert!(summary.last_summarized_id.is_none());
}

#[test]
fn test_parse_session_memory_with_frontmatter() {
    let content = "---\nlast_summarized_id: turn-42\n---\nSummary content here.";
    let summary = parse_session_memory(content).unwrap();
    assert_eq!(summary.summary, "Summary content here.");
    assert_eq!(summary.last_summarized_id, Some("turn-42".to_string()));
}

#[test]
fn test_parse_session_memory_empty() {
    let content = "";
    assert!(parse_session_memory(content).is_none());
}

#[test]
fn test_build_context_restoration_within_budget() {
    let files = vec![
        FileRestoration {
            path: PathBuf::from("/test/file1.rs"),
            content: "fn main() {}".to_string(),
            priority: 10,
            tokens: 100,
            last_accessed: 2000,
        },
        FileRestoration {
            path: PathBuf::from("/test/file2.rs"),
            content: "struct Foo {}".to_string(),
            priority: 5,
            tokens: 50,
            last_accessed: 1000,
        },
    ];

    let restoration =
        build_context_restoration(files, Some("- TODO 1".to_string()), None, vec![], 500);

    assert!(restoration.todos.is_some());
    assert_eq!(restoration.files.len(), 2);
    // Higher priority file should be first
    assert_eq!(restoration.files[0].path, PathBuf::from("/test/file1.rs"));
}

#[test]
fn test_build_context_restoration_budget_exceeded() {
    let files = vec![FileRestoration {
        path: PathBuf::from("/test/large.rs"),
        content: "x".repeat(10000),
        priority: 10,
        tokens: 2500,
        last_accessed: 1000,
    }];

    // Budget too small for the file
    let restoration = build_context_restoration(files, None, None, vec![], 100);
    assert!(restoration.files.is_empty());
}

#[test]
fn test_format_restoration_message_empty() {
    let restoration = ContextRestoration::default();
    let msg = format_restoration_message(&restoration);
    assert!(msg.is_empty());
}

#[test]
fn test_format_restoration_message_with_content() {
    let mut restoration = ContextRestoration::default();
    restoration.todos = Some("- Fix bug".to_string());
    restoration.files.push(FileRestoration {
        path: PathBuf::from("/test.rs"),
        content: "fn main() {}".to_string(),
        priority: 1,
        tokens: 10,
        last_accessed: 1000,
    });

    let msg = format_restoration_message(&restoration);
    assert!(msg.contains("<restored_context>"));
    assert!(msg.contains("<todo_list>"));
    assert!(msg.contains("- Fix bug"));
    assert!(msg.contains("<file path=\"/test.rs\">"));
}

#[test]
fn test_session_memory_config_default() {
    let config = SessionMemoryConfig::default();
    assert!(!config.enabled);
    assert!(config.summary_path.is_none());
    assert_eq!(config.min_savings_tokens, 10_000);
}

#[test]
fn test_compaction_tier_variants() {
    let tiers = vec![
        CompactionTier::SessionMemory,
        CompactionTier::Full,
        CompactionTier::Micro,
    ];
    for tier in tiers {
        let json = serde_json::to_string(&tier).unwrap();
        let back: CompactionTier = serde_json::from_str(&json).unwrap();
        assert_eq!(tier, back);
    }
}

// ========================================================================
// Phase 2: Threshold Status Tests
// ========================================================================

#[test]
fn test_threshold_status_ok() {
    let config = CompactConfig::default();
    // Well below any threshold
    let status = ThresholdStatus::calculate(50000, 200000, &config);

    assert!(status.percent_left > 0.7);
    assert!(!status.is_above_warning_threshold);
    assert!(!status.is_above_error_threshold);
    assert!(!status.is_above_auto_compact_threshold);
    assert!(!status.is_at_blocking_limit);
    assert_eq!(status.status_description(), "ok");
    assert!(!status.needs_action());
}

#[test]
fn test_threshold_status_warning() {
    let config = CompactConfig::default();
    // Above warning but below auto-compact
    // target = 200000 - 13000 = 187000
    // warning = 187000 - 20000 = 167000
    let status = ThresholdStatus::calculate(170000, 200000, &config);

    assert!(status.is_above_warning_threshold);
    assert!(status.needs_action());
}

#[test]
fn test_threshold_status_auto_compact() {
    let config = CompactConfig::default();
    // With default config (80% auto_compact_pct):
    // auto_compact_target = 200000 * 0.80 = 160000
    // blocking_limit = 200000 - 13000 = 187000
    // So 170000 is between auto-compact target (160000) and blocking limit (187000)
    let status = ThresholdStatus::calculate(170000, 200000, &config);

    assert!(status.is_above_warning_threshold);
    assert!(status.is_above_error_threshold);
    assert!(status.is_above_auto_compact_threshold);
    assert!(!status.is_at_blocking_limit);
    assert_eq!(status.status_description(), "auto-compact");
}

#[test]
fn test_threshold_status_blocking() {
    let config = CompactConfig::default();
    // blocking_limit = 200000 - 13000 = 187000
    let status = ThresholdStatus::calculate(190000, 200000, &config);

    assert!(status.is_at_blocking_limit);
    assert_eq!(status.status_description(), "blocking");
}

#[test]
fn test_threshold_status_zero_available() {
    let config = CompactConfig::default();
    let status = ThresholdStatus::calculate(100, 0, &config);

    assert!(status.is_at_blocking_limit);
    assert_eq!(status.percent_left, 0.0);
}

// ========================================================================
// Phase 2: Compactable Tools Tests
// ========================================================================

#[test]
fn test_compactable_tools_set() {
    assert!(COMPACTABLE_TOOLS.contains("Read"));
    assert!(COMPACTABLE_TOOLS.contains("Bash"));
    assert!(COMPACTABLE_TOOLS.contains("Grep"));
    assert!(COMPACTABLE_TOOLS.contains("Glob"));
    assert!(COMPACTABLE_TOOLS.contains("WebSearch"));
    assert!(COMPACTABLE_TOOLS.contains("WebFetch"));
    assert!(COMPACTABLE_TOOLS.contains("Edit"));
    assert!(COMPACTABLE_TOOLS.contains("Write"));

    // Non-compactable tools
    assert!(!COMPACTABLE_TOOLS.contains("Task"));
    assert!(!COMPACTABLE_TOOLS.contains("AskUser"));
}

// ========================================================================
// Phase 2: Micro-Compact Execution Tests
// ========================================================================

#[test]
fn test_collect_tool_result_candidates() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-1",
            "content": "file content here"
        }),
        serde_json::json!({"role": "assistant", "content": "done"}),
        serde_json::json!({
            "role": "tool_result",
            "name": "Bash",
            "tool_use_id": "tool-2",
            "content": "command output"
        }),
    ];

    let candidates = collect_tool_result_candidates(&messages);
    assert_eq!(candidates.len(), 2);

    assert_eq!(candidates[0].index, 1);
    assert_eq!(candidates[0].tool_name, Some("Read".to_string()));
    assert!(candidates[0].is_compactable);

    assert_eq!(candidates[1].index, 3);
    assert_eq!(candidates[1].tool_name, Some("Bash".to_string()));
    assert!(candidates[1].is_compactable);
}

#[test]
fn test_execute_micro_compact_disabled() {
    let mut messages = vec![serde_json::json!({"role": "user", "content": "test"})];
    let mut config = CompactConfig::default();
    config.disable_micro_compact = true;

    let result = execute_micro_compact(&mut messages, 100000, 200000, &config, None);
    assert!(result.is_none());
}

#[test]
fn test_execute_micro_compact_no_candidates() {
    let mut messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    let config = CompactConfig::default();

    let result = execute_micro_compact(&mut messages, 100000, 200000, &config, None);
    assert!(result.is_none());
}

#[test]
fn test_execute_micro_compact_below_threshold() {
    let large_content = "x".repeat(5000);
    let mut messages = vec![
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-1",
            "content": large_content
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-2",
            "content": large_content
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-3",
            "content": large_content
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-4",
            "content": large_content
        }),
    ];
    let config = CompactConfig::default();

    // Context usage well below warning threshold
    let result = execute_micro_compact(&mut messages, 50000, 200000, &config, None);
    assert!(result.is_none());
}

#[test]
fn test_execute_micro_compact_success() {
    // Large content: 50000 chars = ~12500 tokens each
    // With 5 candidates and keeping 3, we compact 2
    // Potential savings: 2 * 12500 = 25000 tokens > 20000 min savings
    let large_content = "x".repeat(50000);
    let mut messages = vec![
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-1",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-2",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-3",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-4",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-5",
            "content": large_content
        }),
    ];
    let config = CompactConfig::default();

    // Context usage above warning threshold (167000 for 200K available)
    let result = execute_micro_compact(&mut messages, 180000, 200000, &config, None);

    assert!(result.is_some());
    let result = result.unwrap();
    // Should compact 2 results (5 - 3 recent to keep)
    assert_eq!(result.compacted_count, 2);
    assert!(result.tokens_saved > 0);

    // First two messages should have been compacted
    let content1 = messages[0]["content"].as_str().unwrap();
    assert!(content1.contains(CLEARED_CONTENT_MARKER));

    let content2 = messages[1]["content"].as_str().unwrap();
    assert!(content2.contains(CLEARED_CONTENT_MARKER));

    // Last three should be unchanged
    let content5 = messages[4]["content"].as_str().unwrap();
    assert!(!content5.contains(CLEARED_CONTENT_MARKER));
}

#[test]
fn test_execute_micro_compact_tracks_file_paths() {
    // Test that micro-compact tracks file paths from Read tool results
    let large_content = "x".repeat(50000);
    let mut messages = vec![
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-1",
            "file_path": "/src/main.rs",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-2",
            "input": {"file_path": "/src/lib.rs"},
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Bash",
            "tool_use_id": "tool-3",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-4",
            "file_path": "/src/test.rs",
            "content": large_content.clone()
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "tool_use_id": "tool-5",
            "file_path": "/src/config.rs",
            "content": large_content
        }),
    ];
    let config = CompactConfig::default();

    // Context usage above warning threshold
    let result = execute_micro_compact(&mut messages, 180000, 200000, &config, None);

    assert!(result.is_some());
    let result = result.unwrap();

    // Should compact 2 results (5 - 3 recent to keep)
    assert_eq!(result.compacted_count, 2);

    // Should track file paths from compacted Read tool results
    // First two are compacted: tool-1 (Read) and tool-2 (Read)
    // tool-3 (Bash) was before tool-4 and tool-5 which are kept
    assert_eq!(result.cleared_file_paths.len(), 2);
    assert!(
        result
            .cleared_file_paths
            .contains(&PathBuf::from("/src/main.rs"))
    );
    assert!(
        result
            .cleared_file_paths
            .contains(&PathBuf::from("/src/lib.rs"))
    );
}

// ========================================================================
// Phase 2: Compact Instructions Tests
// ========================================================================

#[test]
fn test_build_compact_instructions() {
    let instructions = build_compact_instructions(16000);

    // Check all 9 sections are present
    assert!(instructions.contains("1. Summary Purpose and Scope"));
    assert!(instructions.contains("2. Key Decisions and Outcomes"));
    assert!(instructions.contains("3. Code Changes Made"));
    assert!(instructions.contains("4. Files Modified"));
    assert!(instructions.contains("5. Errors Encountered and Resolutions"));
    assert!(instructions.contains("6. User Preferences Learned"));
    assert!(instructions.contains("7. Pending Tasks and Next Steps"));
    assert!(instructions.contains("8. Important Context to Preserve"));
    assert!(instructions.contains("9. Format"));

    // Check max tokens is included
    assert!(instructions.contains("16000"));
}

// ========================================================================
// Phase 2: Task Status Restoration Tests
// ========================================================================

#[test]
fn test_format_restoration_with_tasks() {
    let mut restoration = ContextRestoration::default();
    restoration.todos = Some("- Fix bug".to_string());

    let tasks = TaskStatusRestoration {
        tasks: vec![
            TaskInfo {
                id: "task-1".to_string(),
                subject: "Implement feature".to_string(),
                status: "in_progress".to_string(),
                owner: Some("agent-1".to_string()),
            },
            TaskInfo {
                id: "task-2".to_string(),
                subject: "Write tests".to_string(),
                status: "pending".to_string(),
                owner: None,
            },
        ],
    };

    let msg = format_restoration_with_tasks(&restoration, Some(&tasks));

    assert!(msg.contains("<restored_context>"));
    assert!(msg.contains("<todo_list>"));
    assert!(msg.contains("<task_status>"));
    assert!(msg.contains("[in_progress] task-1"));
    assert!(msg.contains("(agent-1)"));
    assert!(msg.contains("[pending] task-2"));
    assert!(msg.contains("(unassigned)"));
}

#[test]
fn test_format_restoration_with_empty_tasks() {
    let restoration = ContextRestoration::default();
    let tasks = TaskStatusRestoration { tasks: vec![] };

    let msg = format_restoration_with_tasks(&restoration, Some(&tasks));
    assert!(msg.is_empty());
}

#[test]
fn test_task_info_serde() {
    let task = TaskInfo {
        id: "task-1".to_string(),
        subject: "Test task".to_string(),
        status: "pending".to_string(),
        owner: Some("agent".to_string()),
    };

    let json = serde_json::to_string(&task).unwrap();
    let parsed: TaskInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, "task-1");
    assert_eq!(parsed.subject, "Test task");
    assert_eq!(parsed.status, "pending");
    assert_eq!(parsed.owner, Some("agent".to_string()));
}

#[test]
fn test_task_status_from_tool_calls() {
    let tool_calls = vec![
        (
            "Read".to_string(),
            serde_json::json!({"path": "/tmp/file.txt"}),
        ),
        (
            "TodoWrite".to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "1", "subject": "Fix bug", "status": "completed"},
                    {"id": "2", "subject": "Add tests", "status": "in_progress"},
                    {"id": "3", "subject": "Deploy", "status": "pending"}
                ]
            }),
        ),
    ];

    let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
    assert_eq!(task_status.tasks.len(), 3);
    assert_eq!(task_status.tasks[0].id, "1");
    assert_eq!(task_status.tasks[0].subject, "Fix bug");
    assert_eq!(task_status.tasks[0].status, "completed");
    assert_eq!(task_status.tasks[1].status, "in_progress");
    assert_eq!(task_status.tasks[2].status, "pending");
}

#[test]
fn test_task_status_from_tool_calls_empty() {
    let tool_calls: Vec<(String, serde_json::Value)> = vec![];
    let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
    assert!(task_status.tasks.is_empty());
}

#[test]
fn test_task_status_from_tool_calls_uses_latest() {
    let tool_calls = vec![
        (
            "TodoWrite".to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "old", "subject": "Old task", "status": "pending"}
                ]
            }),
        ),
        (
            "TodoWrite".to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "new", "subject": "New task", "status": "in_progress"}
                ]
            }),
        ),
    ];

    let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
    assert_eq!(task_status.tasks.len(), 1);
    // Should use the most recent (last) TodoWrite call
    assert_eq!(task_status.tasks[0].id, "new");
    assert_eq!(task_status.tasks[0].subject, "New task");
}

#[test]
fn test_task_status_from_tool_calls_with_legacy_content() {
    let tool_calls = vec![(
        "TodoWrite".to_string(),
        serde_json::json!({
            "todos": [
                {"id": "1", "content": "Legacy task description", "status": "pending"}
            ]
        }),
    )];

    let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
    assert_eq!(task_status.tasks.len(), 1);
    assert_eq!(task_status.tasks[0].subject, "Legacy task description");
}

// ========================================================================
// Phase 3: Session Memory Write Tests
// ========================================================================

#[tokio::test]
async fn test_write_session_memory() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join(format!(
        "cocode-test-session-memory-{}.md",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let summary = "## Summary\nThis is a test summary.";
    let turn_id = "turn-42";

    // Write session memory
    let result = write_session_memory(&test_path, summary, turn_id).await;
    assert!(result.is_ok());

    // Read and verify
    let content = std::fs::read_to_string(&test_path).unwrap();

    // Check frontmatter
    assert!(content.starts_with("---\n"));
    assert!(content.contains("last_summarized_id: turn-42"));
    assert!(content.contains("timestamp:"));
    assert!(content.contains("---\n## Summary\nThis is a test summary."));

    // Parse it back
    let parsed = parse_session_memory(&content).unwrap();
    assert_eq!(parsed.last_summarized_id, Some("turn-42".to_string()));
    assert!(parsed.summary.contains("## Summary"));

    // Cleanup
    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_write_session_memory_creates_parent_dirs() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join(format!(
        "cocode-test-deep/{}/summary.md",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let summary = "Test with nested dirs";
    let turn_id = "turn-1";

    // Write should create parent directories
    let result = write_session_memory(&test_path, summary, turn_id).await;
    assert!(result.is_ok());

    // Verify file exists
    assert!(test_path.exists());

    // Cleanup
    let _ = std::fs::remove_file(&test_path);
    let _ = std::fs::remove_dir(test_path.parent().unwrap());
}

#[test]
fn test_try_session_memory_compact_disabled() {
    let config = SessionMemoryConfig {
        enabled: false,
        ..Default::default()
    };

    let result = try_session_memory_compact(&config);
    assert!(result.is_none());
}

#[test]
fn test_try_session_memory_compact_no_path() {
    let config = SessionMemoryConfig {
        enabled: true,
        summary_path: None,
        ..Default::default()
    };

    let result = try_session_memory_compact(&config);
    assert!(result.is_none());
}

#[tokio::test]
async fn test_session_memory_roundtrip() {
    let temp_dir = std::env::temp_dir();
    let test_path = temp_dir.join(format!(
        "cocode-test-roundtrip-{}.md",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    // Write
    let original_summary = "## Code Changes\n- Added new feature\n- Fixed bug in auth";
    let turn_id = "turn-99";
    write_session_memory(&test_path, original_summary, turn_id)
        .await
        .unwrap();

    // Read via try_session_memory_compact
    let config = SessionMemoryConfig {
        enabled: true,
        summary_path: Some(test_path.clone()),
        ..Default::default()
    };

    let result = try_session_memory_compact(&config);
    assert!(result.is_some());

    let summary = result.unwrap();
    assert_eq!(summary.last_summarized_id, Some("turn-99".to_string()));
    assert!(summary.summary.contains("## Code Changes"));
    assert!(summary.summary.contains("Added new feature"));
    assert!(summary.token_estimate > 0);

    // Cleanup
    let _ = std::fs::remove_file(&test_path);
}

// ========================================================================
// Phase 3: New Compact Feature Tests
// ========================================================================

#[test]
fn test_format_summary_with_transcript() {
    let summary = "## Summary\nUser worked on fixing a bug.";
    let transcript_path = PathBuf::from("/tmp/session-123.jsonl");

    let formatted =
        format_summary_with_transcript(summary, Some(&transcript_path), true, 50000);

    assert!(formatted.contains("session is being continued"));
    assert!(formatted.contains("50000 tokens"));
    assert!(formatted.contains("/tmp/session-123.jsonl"));
    assert!(formatted.contains("Recent messages are preserved"));
    assert!(formatted.contains("## Summary"));
}

#[test]
fn test_format_summary_without_transcript() {
    let summary = "## Summary\nUser worked on a feature.";

    let formatted = format_summary_with_transcript(summary, None, false, 30000);

    assert!(formatted.contains("session is being continued"));
    assert!(formatted.contains("30000 tokens"));
    assert!(!formatted.contains("transcript at"));
    assert!(!formatted.contains("Recent messages are preserved"));
}

#[test]
fn test_create_invoked_skills_attachment() {
    let skills = vec![
        InvokedSkillRestoration {
            name: "commit".to_string(),
            last_invoked_turn: 5,
            args: Some("-m 'fix bug'".to_string()),
        },
        InvokedSkillRestoration {
            name: "review-pr".to_string(),
            last_invoked_turn: 3,
            args: None,
        },
    ];

    let attachment = create_invoked_skills_attachment(&skills);
    assert!(attachment.is_some());

    let content = attachment.unwrap();
    assert!(content.contains("<invoked_skills>"));
    assert!(content.contains("commit"));
    assert!(content.contains("-m 'fix bug'"));
    assert!(content.contains("review-pr"));
    assert!(content.contains("turn 5"));
    assert!(content.contains("turn 3"));
}

#[test]
fn test_create_invoked_skills_attachment_empty() {
    let skills: Vec<InvokedSkillRestoration> = vec![];
    let attachment = create_invoked_skills_attachment(&skills);
    assert!(attachment.is_none());
}

#[test]
fn test_create_compact_boundary_message() {
    let metadata = CompactBoundaryMetadata {
        trigger: CompactTrigger::Auto,
        pre_tokens: 180000,
        post_tokens: Some(50000),
        transcript_path: Some(PathBuf::from("/home/user/.claude/session.jsonl")),
        recent_messages_preserved: true,
    };

    let message = create_compact_boundary_message(&metadata);

    assert!(message.contains("Conversation compacted"));
    assert!(message.contains("Trigger: auto"));
    assert!(message.contains("Tokens before: 180000"));
    assert!(message.contains("Tokens after: 50000"));
    assert!(message.contains("session.jsonl"));
    assert!(message.contains("Recent messages preserved"));
}

#[test]
fn test_create_compact_boundary_message_manual() {
    let metadata = CompactBoundaryMetadata {
        trigger: CompactTrigger::Manual,
        pre_tokens: 100000,
        post_tokens: None,
        transcript_path: None,
        recent_messages_preserved: false,
    };

    let message = create_compact_boundary_message(&metadata);

    assert!(message.contains("Trigger: manual"));
    assert!(message.contains("Tokens before: 100000"));
    assert!(!message.contains("Tokens after"));
    assert!(!message.contains("transcript"));
}

#[test]
fn test_wrap_hook_additional_context() {
    let contexts = vec![
        HookAdditionalContext {
            content: "Context from hook 1".to_string(),
            hook_name: "env-loader".to_string(),
            suppress_output: false,
        },
        HookAdditionalContext {
            content: "Context from hook 2".to_string(),
            hook_name: "config-reader".to_string(),
            suppress_output: false,
        },
    ];

    let wrapped = wrap_hook_additional_context(&contexts);
    assert!(wrapped.is_some());

    let content = wrapped.unwrap();
    assert!(content.contains("<hook_additional_context>"));
    assert!(content.contains("env-loader"));
    assert!(content.contains("config-reader"));
    assert!(content.contains("Context from hook 1"));
    assert!(content.contains("Context from hook 2"));
}

#[test]
fn test_wrap_hook_additional_context_suppressed() {
    let contexts = vec![HookAdditionalContext {
        content: "Should not appear".to_string(),
        hook_name: "silent-hook".to_string(),
        suppress_output: true,
    }];

    let wrapped = wrap_hook_additional_context(&contexts);
    assert!(wrapped.is_none());
}

#[test]
fn test_wrap_hook_additional_context_empty() {
    let contexts: Vec<HookAdditionalContext> = vec![];
    let wrapped = wrap_hook_additional_context(&contexts);
    assert!(wrapped.is_none());
}

#[test]
fn test_build_token_breakdown() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "Hello, can you help me?"}),
        serde_json::json!({"role": "assistant", "content": "Sure, I'd be happy to help you."}),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "content": "File contents here..."
        }),
        serde_json::json!({"role": "user", "content": "Thanks!"}),
    ];

    let breakdown = build_token_breakdown(&messages);

    assert!(breakdown.total_tokens > 0);
    assert!(breakdown.human_message_tokens > 0);
    assert!(breakdown.assistant_message_tokens > 0);
    assert!(breakdown.local_command_output_tokens > 0);
    assert!(breakdown.human_message_pct > 0.0);
    assert!(breakdown.assistant_message_pct > 0.0);
    assert!(breakdown.tool_result_tokens.contains_key("Read"));
}

#[test]
fn test_build_token_breakdown_empty() {
    let messages: Vec<serde_json::Value> = vec![];
    let breakdown = build_token_breakdown(&messages);

    assert_eq!(breakdown.total_tokens, 0);
    assert_eq!(breakdown.human_message_tokens, 0);
    assert_eq!(breakdown.assistant_message_tokens, 0);
}

#[test]
fn test_compact_trigger_default() {
    let trigger = CompactTrigger::default();
    assert_eq!(trigger, CompactTrigger::Auto);
}

#[test]
fn test_compact_trigger_display() {
    assert_eq!(CompactTrigger::Auto.to_string(), "auto");
    assert_eq!(CompactTrigger::Manual.to_string(), "manual");
}

#[test]
fn test_persisted_tool_result_xml() {
    let persisted = PersistedToolResult {
        path: PathBuf::from("/tmp/tool-results/call-123.txt"),
        original_size: 50000,
        original_tokens: 12500,
        tool_use_id: "call-123".to_string(),
    };

    let xml = persisted.to_xml_reference();
    assert!(xml.contains("persisted-output"));
    assert!(xml.contains("/tmp/tool-results/call-123.txt"));
    assert!(xml.contains("50000"));
    assert!(xml.contains("12500"));
}

#[test]
fn test_invoked_skill_restoration_serde() {
    let skill = InvokedSkillRestoration {
        name: "test-skill".to_string(),
        last_invoked_turn: 10,
        args: Some("--verbose".to_string()),
    };

    let json = serde_json::to_string(&skill).unwrap();
    let parsed: InvokedSkillRestoration = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "test-skill");
    assert_eq!(parsed.last_invoked_turn, 10);
    assert_eq!(parsed.args, Some("--verbose".to_string()));
}

#[test]
fn test_invoked_skill_restoration_from_tool_calls() {
    let tool_calls = vec![
        (
            "Skill".to_string(),
            serde_json::json!({"skill": "commit", "args": "-m 'fix bug'"}),
            5,
        ),
        (
            "Read".to_string(), // Non-skill tool, should be ignored
            serde_json::json!({"path": "/test.rs"}),
            6,
        ),
        (
            "Skill".to_string(),
            serde_json::json!({"skill": "review-pr", "args": "123"}),
            7,
        ),
        (
            "Skill".to_string(), // Same skill again, more recent
            serde_json::json!({"skill": "commit", "args": "-m 'final fix'"}),
            10,
        ),
    ];

    let skills = InvokedSkillRestoration::from_tool_calls(&tool_calls);

    assert_eq!(skills.len(), 2);
    // Most recent first (turn 10, then turn 7)
    assert_eq!(skills[0].name, "commit");
    assert_eq!(skills[0].last_invoked_turn, 10);
    assert_eq!(skills[0].args, Some("-m 'final fix'".to_string()));

    assert_eq!(skills[1].name, "review-pr");
    assert_eq!(skills[1].last_invoked_turn, 7);
    assert_eq!(skills[1].args, Some("123".to_string()));
}

#[test]
fn test_invoked_skill_restoration_from_tool_calls_empty() {
    let tool_calls: Vec<(String, serde_json::Value, i32)> = vec![];
    let skills = InvokedSkillRestoration::from_tool_calls(&tool_calls);
    assert!(skills.is_empty());
}

#[test]
fn test_invoked_skill_restoration_from_tool_calls_no_skills() {
    let tool_calls = vec![
        (
            "Read".to_string(),
            serde_json::json!({"path": "/test.rs"}),
            1,
        ),
        ("Bash".to_string(), serde_json::json!({"command": "ls"}), 2),
    ];

    let skills = InvokedSkillRestoration::from_tool_calls(&tool_calls);
    assert!(skills.is_empty());
}

#[test]
fn test_micro_compact_result_trigger() {
    let mut result = MicroCompactResult::default();
    assert_eq!(result.trigger, CompactTrigger::Auto);

    result.trigger = CompactTrigger::Manual;
    assert_eq!(result.trigger, CompactTrigger::Manual);
}

// ========================================================================
// Keep Window Calculation Tests
// ========================================================================

#[test]
fn test_calculate_keep_start_index_empty() {
    let messages: Vec<serde_json::Value> = vec![];
    let config = KeepWindowConfig::default();

    let result = calculate_keep_start_index(&messages, &config);

    assert_eq!(result.keep_start_index, 0);
    assert_eq!(result.messages_to_keep, 0);
    assert_eq!(result.keep_tokens, 0);
    assert_eq!(result.text_messages_kept, 0);
}

#[test]
fn test_calculate_keep_start_index_few_messages() {
    // Create a few small messages
    let messages = vec![
        serde_json::json!({"role": "user", "content": "Hello"}),
        serde_json::json!({"role": "assistant", "content": "Hi there!"}),
        serde_json::json!({"role": "user", "content": "How are you?"}),
    ];

    let config = KeepWindowConfig {
        min_tokens: 100,
        min_text_messages: 2,
        max_tokens: 10000,
    };

    let result = calculate_keep_start_index(&messages, &config);

    // With small messages, should keep all to meet min requirements
    assert_eq!(result.keep_start_index, 0);
    assert_eq!(result.messages_to_keep, 3);
    assert_eq!(result.text_messages_kept, 3);
}

#[test]
fn test_calculate_keep_start_index_many_messages() {
    // Create many messages where we should only keep some
    let mut messages = Vec::new();
    for i in 0..20 {
        // Each message is ~1000 chars = ~250 tokens
        let content = "x".repeat(1000);
        if i % 2 == 0 {
            messages.push(serde_json::json!({"role": "user", "content": content}));
        } else {
            messages.push(serde_json::json!({"role": "assistant", "content": content}));
        }
    }

    let config = KeepWindowConfig {
        min_tokens: 500, // 2 messages worth
        min_text_messages: 3,
        max_tokens: 2000, // 8 messages worth
    };

    let result = calculate_keep_start_index(&messages, &config);

    // Should keep some recent messages but not all
    assert!(result.messages_to_keep > 0);
    assert!(result.messages_to_keep < 20);
    assert!(result.keep_tokens <= config.max_tokens);
    assert!(result.text_messages_kept >= config.min_text_messages);
}

#[test]
fn test_calculate_keep_start_index_with_tool_pairs() {
    // Tool result followed by user message - tool_use should be included
    let messages = vec![
        serde_json::json!({"role": "user", "content": "Read the file"}),
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "tool_use", "id": "tool-1", "name": "Read"}],
            "tool_use_id": "tool-1"
        }),
        serde_json::json!({
            "role": "tool",
            "tool_use_id": "tool-1",
            "content": "File contents here..."
        }),
        serde_json::json!({"role": "assistant", "content": "Here is the file content."}),
        serde_json::json!({"role": "user", "content": "Thanks!"}),
    ];

    let config = KeepWindowConfig {
        min_tokens: 10,
        min_text_messages: 2,
        max_tokens: 5000,
    };

    let result = calculate_keep_start_index(&messages, &config);

    // Should include the tool pair together
    assert!(result.messages_to_keep >= 2);
}

#[test]
fn test_calculate_keep_start_index_max_tokens_limit() {
    // Create messages that would exceed max if all kept
    let mut messages = Vec::new();
    for i in 0..10 {
        // Each message is ~4000 chars = ~1000 tokens
        let content = "y".repeat(4000);
        if i % 2 == 0 {
            messages.push(serde_json::json!({"role": "user", "content": content}));
        } else {
            messages.push(serde_json::json!({"role": "assistant", "content": content}));
        }
    }

    let config = KeepWindowConfig {
        min_tokens: 500,
        min_text_messages: 2,
        max_tokens: 3000, // Only allow ~3 messages
    };

    let result = calculate_keep_start_index(&messages, &config);

    // Should be limited by max_tokens
    assert!(result.keep_tokens <= config.max_tokens);
}

#[test]
fn test_keep_window_config_validate() {
    let valid_config = KeepWindowConfig::default();
    assert!(valid_config.validate().is_ok());

    let invalid_config = KeepWindowConfig {
        min_tokens: -1,
        ..Default::default()
    };
    assert!(invalid_config.validate().is_err());

    let invalid_config2 = KeepWindowConfig {
        min_tokens: 50000,
        max_tokens: 10000, // max < min
        ..Default::default()
    };
    assert!(invalid_config2.validate().is_err());
}

#[test]
fn test_file_restoration_config_should_exclude() {
    let config = FileRestorationConfig::default();

    // Should exclude transcript files
    assert!(config.should_exclude("session-123.jsonl"));
    assert!(config.should_exclude("/path/to/transcript.jsonl"));

    // Should exclude CLAUDE.md
    assert!(config.should_exclude("CLAUDE.md"));
    assert!(config.should_exclude("/project/CLAUDE.md"));

    // Should exclude plan files
    assert!(config.should_exclude("plan.md"));
    assert!(config.should_exclude("/path/plan-v2.md"));

    // Should NOT exclude regular files
    assert!(!config.should_exclude("main.rs"));
    assert!(!config.should_exclude("/src/lib.rs"));
    assert!(!config.should_exclude("README.md"));
}

#[test]
fn test_file_restoration_config_validate() {
    let valid_config = FileRestorationConfig::default();
    assert!(valid_config.validate().is_ok());

    let invalid_config = FileRestorationConfig {
        max_files: -1,
        ..Default::default()
    };
    assert!(invalid_config.validate().is_err());
}
