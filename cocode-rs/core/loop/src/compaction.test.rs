use super::*;
use cocode_protocol::ToolName;
use std::path::PathBuf;

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
    let restoration = ContextRestoration {
        todos: Some("- Fix bug".to_string()),
        files: vec![FileRestoration {
            path: PathBuf::from("/test.rs"),
            content: "fn main() {}".to_string(),
            priority: 1,
            tokens: 10,
            last_accessed: 1000,
        }],
        ..Default::default()
    };

    let msg = format_restoration_message(&restoration);
    assert!(msg.contains("<restored_context>"));
    assert!(msg.contains("<todo_list>"));
    assert!(msg.contains("- Fix bug"));
    assert!(msg.contains("<file path=\"/test.rs\">"));
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
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Read.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Bash.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Grep.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Glob.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::WebSearch.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::WebFetch.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Edit.as_str()));
    assert!(COMPACTABLE_TOOLS.contains(ToolName::Write.as_str()));

    // Non-compactable tools
    assert!(!COMPACTABLE_TOOLS.contains(ToolName::Task.as_str()));
    assert!(!COMPACTABLE_TOOLS.contains("AskUser"));
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
    let restoration = ContextRestoration {
        todos: Some("- Fix bug".to_string()),
        ..Default::default()
    };

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
            ToolName::Read.as_str().to_string(),
            serde_json::json!({"path": "/tmp/file.txt"}),
        ),
        (
            ToolName::TodoWrite.as_str().to_string(),
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
            ToolName::TodoWrite.as_str().to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "old", "subject": "Old task", "status": "pending"}
                ]
            }),
        ),
        (
            ToolName::TodoWrite.as_str().to_string(),
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
        ToolName::TodoWrite.as_str().to_string(),
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
    let config = CompactConfig {
        enable_sm_compact: false,
        ..Default::default()
    };

    let result = try_session_memory_compact(&config);
    assert!(result.is_none());
}

#[test]
fn test_try_session_memory_compact_no_path() {
    let config = CompactConfig {
        enable_sm_compact: true,
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

    // Read via try_session_memory_compact (uses CompactConfig directly)
    let config = CompactConfig {
        enable_sm_compact: true,
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
// Truncate Sections Tests
// ========================================================================

#[test]
fn test_truncate_sections_within_budget() {
    let summary = "### Section 1\nShort content.\n### Section 2\nMore content.";
    let result = truncate_sections(summary, 2000, 12000);
    // Both per-section and total limits are satisfied — output unchanged
    assert_eq!(result, summary);
}

#[test]
fn test_truncate_sections_enforces_per_section_even_under_total() {
    // Total is under 12000 tokens, but one section exceeds the 2000 per-section limit.
    // Per-section enforcement must still apply (no fast-path bypass).
    let long_section = "x".repeat(8000); // ~2667 tokens, exceeds 2000
    let summary = format!("### Section 1\n{long_section}");
    let total_tokens = cocode_protocol::estimate_text_tokens(&summary);
    assert!(total_tokens <= 12000, "precondition: total under budget");

    let result = truncate_sections(&summary, 2000, 12000);
    assert!(
        result.contains("[truncated]"),
        "oversized section should be truncated even when total is within budget"
    );
    assert!(result.len() < summary.len());
}

#[test]
fn test_truncate_sections_oversized_section() {
    // Create sections that together exceed total limit and individually exceed per-section limit
    let long_content = "x".repeat(20000); // ~6667 tokens, way over 2000 per-section
    let summary = format!("### Section 1\n{long_content}\n### Section 2\n{long_content}");
    let result = truncate_sections(&summary, 2000, 12000);
    // Section 1 should be truncated to fit per-section limit
    assert!(result.contains("[truncated]"));
    // Result should be substantially shorter than the input
    assert!(result.len() < summary.len());
}

#[test]
fn test_truncate_sections_total_limit() {
    // Create multiple sections that together exceed total limit
    let section_content = "y".repeat(4000); // ~1333 tokens each
    let summary = format!(
        "### S1\n{section_content}\n### S2\n{section_content}\n### S3\n{section_content}\n### S4\n{section_content}\n### S5\n{section_content}\n### S6\n{section_content}\n### S7\n{section_content}\n### S8\n{section_content}\n### S9\n{section_content}\n### S10\n{section_content}",
    );
    let result = truncate_sections(&summary, 2000, 12000);
    // Should not include all 10 sections (10 * ~1333 = ~13330 > 12000)
    let section_count = result.matches("### S").count();
    assert!(
        section_count < 10,
        "Expected fewer than 10 sections, got {section_count}"
    );
    assert!(
        section_count >= 8,
        "Expected at least 8 sections, got {section_count}"
    );
}

#[test]
fn test_truncate_sections_empty() {
    let result = truncate_sections("", 2000, 12000);
    assert_eq!(result, "");
}

#[test]
fn test_truncate_sections_no_headers() {
    let content = "Just plain text without any headers.";
    let result = truncate_sections(content, 2000, 12000);
    assert_eq!(result, content);
}

// ========================================================================
// Phase 3: New Compact Feature Tests
// ========================================================================

#[test]
fn test_format_summary_with_transcript() {
    let summary = "## Summary\nUser worked on fixing a bug.";
    let transcript_path = PathBuf::from("/tmp/session-123.jsonl");

    let formatted = format_summary_with_transcript(summary, Some(&transcript_path), true, 50000);

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
    assert!(
        breakdown
            .tool_result_tokens
            .contains_key(ToolName::Read.as_str())
    );
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
fn test_build_token_breakdown_duplicate_reads() {
    let messages = vec![
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "file_path": "/src/main.rs",
            "content": "fn main() {}"
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "file_path": "/src/main.rs",
            "content": "fn main() { println!(\"hello\"); }"
        }),
        serde_json::json!({
            "role": "tool",
            "name": "Read",
            "file_path": "/src/lib.rs",
            "content": "pub mod tests;"
        }),
    ];

    let breakdown = build_token_breakdown(&messages);
    assert_eq!(breakdown.duplicate_read_file_count, 1);
    assert!(breakdown.duplicate_read_tokens > 0);
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
            ToolName::Skill.as_str().to_string(),
            serde_json::json!({"skill": "commit", "args": "-m 'fix bug'"}),
            5,
        ),
        (
            ToolName::Read.as_str().to_string(), // Non-skill tool, should be ignored
            serde_json::json!({"path": "/test.rs"}),
            6,
        ),
        (
            ToolName::Skill.as_str().to_string(),
            serde_json::json!({"skill": "review-pr", "args": "123"}),
            7,
        ),
        (
            ToolName::Skill.as_str().to_string(), // Same skill again, more recent
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
            ToolName::Read.as_str().to_string(),
            serde_json::json!({"path": "/test.rs"}),
            1,
        ),
        (
            ToolName::Bash.as_str().to_string(),
            serde_json::json!({"command": "ls"}),
            2,
        ),
    ];

    let skills = InvokedSkillRestoration::from_tool_calls(&tool_calls);
    assert!(skills.is_empty());
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

// ============================================================================
// Session Memory Boundary Finding Tests
// ============================================================================

#[test]
fn test_find_session_memory_boundary_empty() {
    let config = cocode_protocol::KeepWindowConfig::default();
    let result = find_session_memory_boundary(&[], &config, None);
    assert_eq!(result.keep_start_index, 0);
    assert_eq!(result.messages_to_keep, 0);
}

#[test]
fn test_find_session_memory_boundary_no_anchor_falls_back() {
    // Without an anchor, should fall back to calculate_keep_start_index
    let messages: Vec<serde_json::Value> = (0..10)
        .map(|i| {
            serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": "x".repeat(2000), // ~500 tokens each
            })
        })
        .collect();

    let config = cocode_protocol::KeepWindowConfig {
        min_tokens: 1000,
        min_text_messages: 2,
        max_tokens: 5000,
    };

    let result = find_session_memory_boundary(&messages, &config, None);
    assert!(result.messages_to_keep > 0);
    assert!(result.keep_tokens >= config.min_tokens);
}

#[test]
fn test_find_session_memory_boundary_with_anchor() {
    let messages: Vec<serde_json::Value> = (0..10)
        .map(|i| {
            serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": "x".repeat(2000),
                "turn_id": format!("turn-{i}"),
            })
        })
        .collect();

    let config = cocode_protocol::KeepWindowConfig {
        min_tokens: 500,
        min_text_messages: 2,
        max_tokens: 40000,
    };

    // Anchor at message 4 — should keep messages from index 5 onward (anchor was summarized)
    let result = find_session_memory_boundary(&messages, &config, Some("turn-4"));
    assert!(result.keep_start_index <= 5);
    assert!(result.messages_to_keep >= 5);
}

#[test]
fn test_find_session_memory_boundary_anchor_not_found() {
    let messages: Vec<serde_json::Value> = (0..5)
        .map(|i| {
            serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": "x".repeat(2000),
                "turn_id": format!("turn-{i}"),
            })
        })
        .collect();

    let config = cocode_protocol::KeepWindowConfig {
        min_tokens: 500,
        min_text_messages: 1,
        max_tokens: 40000,
    };

    // Non-existent anchor falls back to generic calculation
    let result = find_session_memory_boundary(&messages, &config, Some("turn-999"));
    assert!(result.messages_to_keep > 0);
}

#[test]
fn test_adjust_boundaries_for_tools_no_tools() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    // No tool messages — should not adjust
    let result = adjust_boundaries_for_tools(&messages, 1);
    assert_eq!(result, 1);
}

#[test]
fn test_adjust_boundaries_for_tools_pairs_tool_use_result() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "read file"}),
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "tool_use", "id": "tool-1", "name": "Read"}]
        }),
        serde_json::json!({"role": "tool", "content": "file content", "tool_use_id": "tool-1"}),
        serde_json::json!({"role": "assistant", "content": "I see the file"}),
    ];

    // Raw start at 2 (tool_result) — should pull back to 1 (assistant with tool_use)
    let result = adjust_boundaries_for_tools(&messages, 2);
    assert!(result <= 1);
}

// ============================================================================
// Token Estimation Tests
// ============================================================================

#[test]
fn test_estimate_message_tokens_string_content() {
    // 300 chars → ceil(300/3) = 100 tokens
    let msg = serde_json::json!({"role": "user", "content": "x".repeat(300)});
    assert_eq!(estimate_message_tokens(&msg), 100);
}

#[test]
fn test_estimate_message_tokens_array_content() {
    // Two text blocks: 90 + 60 = 150 chars → ceil(150/3) = 50 tokens
    let msg = serde_json::json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "x".repeat(90)},
            {"type": "text", "text": "y".repeat(60)}
        ]
    });
    assert_eq!(estimate_message_tokens(&msg), 50);
}

#[test]
fn test_estimate_message_tokens_rounding() {
    // 1 char → ceil(1/3) = 1 token (not 0)
    let msg = serde_json::json!({"role": "user", "content": "x"});
    assert_eq!(estimate_message_tokens(&msg), 1);

    // 2 chars → ceil(2/3) = 1 token
    let msg = serde_json::json!({"role": "user", "content": "xy"});
    assert_eq!(estimate_message_tokens(&msg), 1);

    // 3 chars → ceil(3/3) = 1 token
    let msg = serde_json::json!({"role": "user", "content": "xyz"});
    assert_eq!(estimate_message_tokens(&msg), 1);

    // 4 chars → ceil(4/3) = 2 tokens
    let msg = serde_json::json!({"role": "user", "content": "xyzw"});
    assert_eq!(estimate_message_tokens(&msg), 2);
}

#[test]
fn test_estimate_message_tokens_empty() {
    let msg = serde_json::json!({"role": "user"});
    assert_eq!(estimate_message_tokens(&msg), 0);

    let msg = serde_json::json!({"role": "user", "content": ""});
    assert_eq!(estimate_message_tokens(&msg), 0);
}

#[test]
fn test_estimate_text_tokens_canonical() {
    // 300 chars → ceil(300/3) = 100 tokens
    let text = "x".repeat(300);
    assert_eq!(cocode_protocol::estimate_text_tokens(&text), 100);
    // Empty → 0
    assert_eq!(cocode_protocol::estimate_text_tokens(""), 0);
    // 1 char → ceil(1/3) = 1
    assert_eq!(cocode_protocol::estimate_text_tokens("a"), 1);
}

// ============================================================================
// Compact Boundary Message Tests
// ============================================================================

#[test]
fn test_is_compact_boundary_message() {
    let boundary = serde_json::json!({
        "role": "user",
        "content": "Conversation compacted.\nTrigger: auto\nTokens before: 100000"
    });
    assert!(is_compact_boundary_message(&boundary));
}

#[test]
fn test_is_compact_boundary_message_non_boundary() {
    let regular = serde_json::json!({"role": "user", "content": "Hello world"});
    assert!(!is_compact_boundary_message(&regular));

    let assistant = serde_json::json!({
        "role": "assistant",
        "content": "Conversation compacted."
    });
    assert!(!is_compact_boundary_message(&assistant));
}

// ============================================================================
// Session Memory Boundary Phase 2 Tests
// ============================================================================

#[test]
fn test_find_session_memory_boundary_backward_walk_from_anchor() {
    // Anchor exists but only 1 message after it — insufficient for min_text_messages=3.
    // Phase 2 should walk backward from the anchor.
    let messages: Vec<serde_json::Value> = (0..10)
        .map(|i| {
            serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": "x".repeat(300), // ~100 tokens each
                "turn_id": format!("turn-{i}"),
            })
        })
        .collect();

    // Place anchor near the end so Phase 1 fails (not enough text after anchor)
    let config = cocode_protocol::KeepWindowConfig {
        min_tokens: 200,
        min_text_messages: 3,
        max_tokens: 40000,
    };

    let result = find_session_memory_boundary(&messages, &config, Some("turn-9"));
    // Should walk backward from index 9 to include enough messages
    assert!(result.keep_start_index < 9);
    assert!(result.text_messages_kept >= 3);
}

#[test]
fn test_find_session_memory_boundary_respects_compact_boundary() {
    // A boundary message exists at index 2. Phase 2 should not cross it.
    let messages = vec![
        serde_json::json!({"role": "user", "content": "old message 1", "turn_id": "turn-0"}),
        serde_json::json!({"role": "assistant", "content": "old response", "turn_id": "turn-1"}),
        serde_json::json!({
            "role": "user",
            "content": "Conversation compacted.\nTrigger: auto",
            "turn_id": "boundary"
        }),
        serde_json::json!({"role": "user", "content": "x".repeat(30), "turn_id": "turn-3"}),
        serde_json::json!({"role": "assistant", "content": "y".repeat(30), "turn_id": "turn-4"}),
        serde_json::json!({"role": "user", "content": "z".repeat(30), "turn_id": "turn-5"}),
        serde_json::json!({"role": "assistant", "content": "w".repeat(30), "turn_id": "turn-6"}),
    ];

    let config = cocode_protocol::KeepWindowConfig {
        min_tokens: 50,
        min_text_messages: 3,
        max_tokens: 40000,
    };

    // Anchor at turn-6 (last message) — insufficient alone, walk back
    let result = find_session_memory_boundary(&messages, &config, Some("turn-6"));
    // Should NOT go before index 3 (boundary at 2 means stop at 3)
    assert!(
        result.keep_start_index >= 3,
        "keep_start_index {} should not cross compact boundary at index 2",
        result.keep_start_index
    );
}

// ============================================================================
// From<KeepWindowResult> Test
// ============================================================================

#[test]
fn test_session_memory_boundary_from_keep_window() {
    let kwr = KeepWindowResult {
        keep_start_index: 5,
        messages_to_keep: 10,
        keep_tokens: 3000,
        text_messages_kept: 4,
    };
    let smbr: SessionMemoryBoundaryResult = kwr.into();
    assert_eq!(smbr.keep_start_index, 5);
    assert_eq!(smbr.messages_to_keep, 10);
    assert_eq!(smbr.keep_tokens, 3000);
    assert_eq!(smbr.text_messages_kept, 4);
}

// ============================================================================
// Environment Variable Override Tests
// ============================================================================

#[test]
fn test_compact_config_with_env_overrides_default() {
    // Without env vars set, should be unchanged
    let config = CompactConfig::default().with_env_overrides();
    assert!(!config.disable_compact);
    assert!(!config.disable_auto_compact);
}

// ============================================================================
// Threshold Recalculation After Compaction Tests (Plan 1.1)
// ============================================================================

#[test]
fn test_threshold_status_recalculation_after_token_reduction() {
    let config = CompactConfig::default();
    let context_window = 200_000;

    // Before compaction: at blocking limit
    let pre_compact_tokens = 190_000;
    let pre_status = ThresholdStatus::calculate(pre_compact_tokens, context_window, &config);
    assert!(pre_status.is_at_blocking_limit);
    assert!(pre_status.is_above_auto_compact_threshold);

    // After compaction: tokens reduced significantly
    let post_compact_tokens = 80_000;
    let post_status = ThresholdStatus::calculate(post_compact_tokens, context_window, &config);
    assert!(!post_status.is_at_blocking_limit);
    assert!(!post_status.is_above_auto_compact_threshold);
    assert!(!post_status.is_above_warning_threshold);

    // Verify using the stale pre-compaction status would be wrong
    assert_ne!(
        pre_status.is_at_blocking_limit,
        post_status.is_at_blocking_limit
    );
}

// ============================================================================
// Context Restoration Data Extraction Tests (Plan 1.2)
// ============================================================================

#[test]
fn test_task_and_skill_restoration_from_tool_calls() {
    // Simulate tool calls that include both TodoWrite and Skill invocations
    let tool_calls_with_turns = vec![
        (
            ToolName::TodoWrite.as_str().to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "1", "subject": "Fix bug", "status": "in_progress"},
                    {"id": "2", "subject": "Write tests", "status": "pending"}
                ]
            }),
            3,
        ),
        (
            ToolName::Skill.as_str().to_string(),
            serde_json::json!({
                "skill": "commit",
                "args": "-m 'fix: bug'"
            }),
            5,
        ),
        (
            ToolName::Read.as_str().to_string(),
            serde_json::json!({"file_path": "/test/file.rs"}),
            6,
        ),
    ];

    // Derive tool_calls (without turns) for task status
    let tool_calls: Vec<(String, serde_json::Value)> = tool_calls_with_turns
        .iter()
        .map(|(name, input, _)| (name.clone(), input.clone()))
        .collect();

    let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
    let invoked_skills = InvokedSkillRestoration::from_tool_calls(&tool_calls_with_turns);

    // Verify task status was extracted
    assert_eq!(task_status.tasks.len(), 2);
    assert_eq!(task_status.tasks[0].id, "1");
    assert_eq!(task_status.tasks[0].status, "in_progress");

    // Verify invoked skills were extracted
    assert_eq!(invoked_skills.len(), 1);
    assert_eq!(invoked_skills[0].name, "commit");
    assert_eq!(invoked_skills[0].last_invoked_turn, 5);
    assert_eq!(invoked_skills[0].args, Some("-m 'fix: bug'".to_string()));
}

#[test]
fn test_context_restoration_includes_todos_and_skills() {
    let files = vec![FileRestoration {
        path: PathBuf::from("/test/file.rs"),
        content: "fn main() {}".to_string(),
        priority: 1,
        tokens: 100,
        last_accessed: 1000,
    }];

    let todos = Some("- [in_progress] 1: Fix bug\n- [pending] 2: Write tests".to_string());
    let skills = vec!["commit".to_string()];

    let restoration = build_context_restoration(files, todos.clone(), None, skills.clone(), 50000);

    // Verify todos and skills are included
    assert_eq!(restoration.todos, todos);
    assert_eq!(restoration.skills, skills);
    assert_eq!(restoration.files.len(), 1);
}

// ============================================================================
// is_empty_template Tests
// ============================================================================

#[test]
fn test_is_empty_template_all_na() {
    let template =
        "### Session Title\nN/A\n### 1. Current State\nN/A\n### 2. Task Specification\nN/A";
    assert!(is_empty_template(template));
}

#[test]
fn test_is_empty_template_with_content() {
    let template = "### Session Title\nImplementing compaction\n### 1. Current State\nWorking on P3\n### 2. Task Specification\nFix the compaction system\nMultiple improvements needed";
    assert!(!is_empty_template(template));
}

#[test]
fn test_is_empty_template_empty_sections() {
    let template = "### Session Title\n\n### 1. Current State\n\n### 2. Task Specification\n";
    assert!(is_empty_template(template));
}
