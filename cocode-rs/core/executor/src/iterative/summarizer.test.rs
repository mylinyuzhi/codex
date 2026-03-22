use super::*;

#[test]
fn test_summarize_empty() {
    let summary = Summarizer::summarize_iterations(&[]);
    assert_eq!(summary, "No iterations executed.");
}

#[test]
fn test_summarize_single() {
    let records = vec![IterationRecord::new(0, "success".to_string(), 100)];
    let summary = Summarizer::summarize_iterations(&records);
    assert!(summary.contains("1 iteration(s)"));
    assert!(summary.contains("100ms total"));
    assert!(summary.contains("[0]"));
    assert!(summary.contains("[OK]"));
}

#[test]
fn test_summarize_with_git_info() {
    let records = vec![IterationRecord::with_git_info(
        0,
        "done".to_string(),
        200,
        Some("abc123".to_string()),
        vec!["file.rs".to_string()],
        "Did the thing".to_string(),
        true,
    )];
    let summary = Summarizer::summarize_iterations(&records);
    assert!(summary.contains("1 iteration(s)"));
    assert!(summary.contains("commit abc123"));
    assert!(summary.contains("Did the thing"));
}

#[test]
fn test_summarize_multiple() {
    let records = vec![
        IterationRecord::with_git_info(
            0,
            "done1".to_string(),
            200,
            Some("abc".to_string()),
            vec!["file1.rs".to_string()],
            "Step 1".to_string(),
            true,
        ),
        IterationRecord::with_git_info(
            1,
            "failed".to_string(),
            300,
            None,
            vec![],
            "Failed step".to_string(),
            false,
        ),
        IterationRecord::with_git_info(
            2,
            "done3".to_string(),
            150,
            Some("def".to_string()),
            vec!["file2.rs".to_string()],
            "Step 3".to_string(),
            true,
        ),
    ];
    let summary = Summarizer::summarize_iterations(&records);
    assert!(summary.contains("3 iteration(s)"));
    assert!(summary.contains("650ms total"));
    assert!(summary.contains("[0]"));
    assert!(summary.contains("[1]"));
    assert!(summary.contains("[2]"));
    assert!(summary.contains("[FAILED]"));
}

#[test]
fn test_generate_file_based_summary_empty() {
    let summary = generate_file_based_summary(0, &[], true);
    assert!(summary.contains("no file changes"));
    assert!(summary.contains("succeeded"));
}

#[test]
fn test_generate_file_based_summary_with_files() {
    let files = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
    let summary = generate_file_based_summary(1, &files, true);
    assert!(summary.contains("Iteration 1 succeeded"));
    assert!(summary.contains("2 file(s)"));
    assert!(summary.contains(".rs"));
}

#[test]
fn test_generate_file_based_summary_multiple_extensions() {
    let files = vec![
        "src/main.rs".to_string(),
        "README.md".to_string(),
        "test.py".to_string(),
    ];
    let summary = generate_file_based_summary(2, &files, false);
    assert!(summary.contains("Iteration 2 failed"));
    assert!(summary.contains("3 file(s)"));
}

#[test]
fn test_generate_fallback_commit_message_few_files() {
    let files = vec!["a.rs".to_string(), "b.rs".to_string()];
    let msg = generate_fallback_commit_message(0, &files);
    assert!(msg.contains("[iter-0]"));
    assert!(msg.contains("a.rs, b.rs"));
}

#[test]
fn test_generate_fallback_commit_message_many_files() {
    let files: Vec<String> = (0..10).map(|i| format!("file{i}.rs")).collect();
    let msg = generate_fallback_commit_message(3, &files);
    assert!(msg.contains("[iter-3]"));
    assert!(msg.contains("... (5 more)"));
}

#[test]
fn test_prompts_format_summary() {
    let prompt = prompts::format_summary_prompt("Fix the bug", &["src/main.rs".to_string()]);
    assert!(prompt.contains("Fix the bug"));
    assert!(prompt.contains("src/main.rs"));
}

#[test]
fn test_prompts_format_commit_msg() {
    let prompt = prompts::format_commit_msg_prompt(
        1,
        "Implement feature",
        &["src/lib.rs".to_string()],
        "Added new feature",
    );
    assert!(prompt.contains("Iteration: 1"));
    assert!(prompt.contains("Implement feature"));
    assert!(prompt.contains("src/lib.rs"));
    assert!(prompt.contains("Added new feature"));
}

#[test]
fn test_prompts_truncation() {
    let long_task = "a".repeat(600);
    let prompt = prompts::format_summary_prompt(&long_task, &[]);
    assert!(prompt.contains("..."));
    // The truncated task (503 chars) + template should be shorter than full task + template
    assert!(prompt.len() < long_task.len() + prompts::ITERATION_SUMMARY_USER.len());
}
