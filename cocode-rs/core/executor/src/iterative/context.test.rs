use super::*;

#[test]
fn test_iteration_context_new() {
    let ctx = IterationContext::new(2, 5);
    assert_eq!(ctx.iteration, 2);
    assert_eq!(ctx.total_iterations, 5);
    assert!(ctx.base_commit_id.is_none());
    assert!(ctx.iterations.is_empty());
}

#[test]
fn test_iteration_context_with_context_passing() {
    let ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "Implement feature".to_string(),
        Some("## Plan\n1. Do X".to_string()),
        5,
    );

    assert_eq!(ctx.base_commit_id, Some("abc123".to_string()));
    assert_eq!(ctx.initial_prompt, "Implement feature");
    assert!(ctx.plan_content.is_some());
    assert_eq!(ctx.total_iterations, 5);
    assert!(ctx.iterations.is_empty());
    assert!(ctx.context_passing_enabled());
}

#[test]
fn test_iteration_context_add_iteration() {
    let mut ctx =
        IterationContext::with_context_passing("abc".to_string(), "task".to_string(), None, 3);

    ctx.add_iteration(IterationRecord::with_git_info(
        0,
        "Done step 1".to_string(),
        1000,
        Some("def456".to_string()),
        vec!["file1.rs".to_string()],
        "Did something".to_string(),
        true,
    ));

    assert_eq!(ctx.current_iteration(), 1);
    assert_eq!(ctx.successful_iterations(), 1);
    assert_eq!(ctx.failed_iterations(), 0);

    ctx.add_iteration(IterationRecord::with_git_info(
        1,
        "Failed".to_string(),
        500,
        None,
        vec![],
        "Failed".to_string(),
        false,
    ));

    assert_eq!(ctx.current_iteration(), 2);
    assert_eq!(ctx.successful_iterations(), 1);
    assert_eq!(ctx.failed_iterations(), 1);
}

#[test]
fn test_iteration_record_basic() {
    let record = IterationRecord::new(0, "compiled successfully".to_string(), 1500);
    assert_eq!(record.iteration, 0);
    assert_eq!(record.result, "compiled successfully");
    assert_eq!(record.duration_ms, 1500);
    assert!(record.success);
}

#[test]
fn test_iteration_record_commit_status() {
    let with_commit = IterationRecord::with_git_info(
        0,
        "done".to_string(),
        100,
        Some("abcdef1234567890".to_string()),
        vec![],
        "summary".to_string(),
        true,
    );
    assert_eq!(with_commit.commit_status(), "commit abcdef1");

    let no_commit = IterationRecord::new(0, "done".to_string(), 100);
    assert_eq!(no_commit.commit_status(), "no changes");

    let short_commit = IterationRecord::with_git_info(
        0,
        "done".to_string(),
        100,
        Some("abc".to_string()),
        vec![],
        "summary".to_string(),
        true,
    );
    assert_eq!(short_commit.commit_status(), "commit abc");
}

#[test]
fn test_iteration_record_files_display() {
    let empty = IterationRecord::new(0, "s".to_string(), 100);
    assert_eq!(empty.files_display(), "none");

    let few = IterationRecord::with_git_info(
        0,
        "s".to_string(),
        100,
        None,
        vec!["a.rs".to_string(), "b.rs".to_string()],
        "s".to_string(),
        true,
    );
    assert_eq!(few.files_display(), "a.rs, b.rs");

    let many = IterationRecord::with_git_info(
        0,
        "s".to_string(),
        100,
        None,
        vec![
            "1.rs".to_string(),
            "2.rs".to_string(),
            "3.rs".to_string(),
            "4.rs".to_string(),
            "5.rs".to_string(),
            "6.rs".to_string(),
            "7.rs".to_string(),
        ],
        "s".to_string(),
        true,
    );
    assert_eq!(
        many.files_display(),
        "1.rs, 2.rs, 3.rs, 4.rs, 5.rs, ... (2 more)"
    );
}

#[test]
fn test_iteration_record_serde() {
    let record = IterationRecord::with_git_info(
        3,
        "test passed".to_string(),
        250,
        Some("abc123".to_string()),
        vec!["file.rs".to_string()],
        "Did the thing".to_string(),
        true,
    );
    let json = serde_json::to_string(&record).expect("serialize");
    let back: IterationRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.iteration, record.iteration);
    assert_eq!(back.result, record.result);
    assert_eq!(back.duration_ms, record.duration_ms);
    assert_eq!(back.commit_id, record.commit_id);
    assert_eq!(back.changed_files, record.changed_files);
    assert_eq!(back.summary, record.summary);
    assert_eq!(back.success, record.success);
}

#[test]
fn test_iteration_context_serde() {
    let mut ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "task".to_string(),
        Some("plan".to_string()),
        10,
    );
    ctx.add_iteration(IterationRecord::new(0, "done".to_string(), 100));

    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: IterationContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.iteration, 0);
    assert_eq!(back.total_iterations, 10);
    assert_eq!(back.base_commit_id, Some("abc123".to_string()));
    assert_eq!(back.initial_prompt, "task");
    assert_eq!(back.iterations.len(), 1);
}
