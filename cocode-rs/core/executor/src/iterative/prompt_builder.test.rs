use super::*;
use crate::iterative::context::IterationRecord;

#[test]
fn test_first_iteration_with_assessment() {
    let builder = IterativePromptBuilder::new("Implement user auth");
    let result = builder.build(0);

    assert!(result.contains("<task_assessment>"));
    assert!(result.contains("Implement user auth"));
    assert!(result.contains("EnterPlanMode"));
}

#[test]
fn test_first_iteration_without_assessment() {
    let builder =
        IterativePromptBuilder::new("Implement user auth").without_complexity_assessment();
    let result = builder.build(0);

    assert!(!result.contains("<task_assessment>"));
    assert_eq!(result, "Implement user auth");
}

#[test]
fn test_subsequent_iterations_enhanced() {
    let builder =
        IterativePromptBuilder::new("Implement user auth").without_complexity_assessment();
    let result = builder.build(1);

    assert!(result.contains("Implement user auth"));
    assert!(result.contains("git log"));
    assert!(result.contains("iterative improvements"));
}

#[test]
fn test_custom_instruction() {
    let builder = IterativePromptBuilder::new("Fix tests")
        .without_complexity_assessment()
        .with_custom_instruction("Focus on edge cases and error handling");
    let result = builder.build(1);

    assert!(result.contains("Fix tests"));
    assert!(result.contains("Focus on edge cases"));
    assert!(!result.contains("git log")); // No default instruction
}

#[test]
fn test_build_with_context_first_iteration() {
    let builder = IterativePromptBuilder::new("Do the task");
    let ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "Implement X".to_string(),
        None,
        5,
    );
    let result = builder.build_with_context(0, &ctx);

    // First iteration includes complexity assessment prompt
    assert!(result.contains("<task_assessment>"));
    assert!(result.contains("Task Complexity Assessment"));
    assert!(result.contains("EnterPlanMode"));
    assert!(result.contains("Do the task"));
}

#[test]
fn test_build_with_context_with_history() {
    let builder = IterativePromptBuilder::new("Continue work");
    let mut ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "Implement X".to_string(),
        Some("## Plan\n1. Step one".to_string()),
        5,
    );
    ctx.add_iteration(IterationRecord::with_git_info(
        0,
        "Done step one".to_string(),
        1000,
        Some("def456789".to_string()),
        vec!["file.rs".to_string()],
        "Did step one".to_string(),
        true,
    ));

    let result = builder.build_with_context(1, &ctx);

    assert!(result.contains("<task_context>"));
    assert!(result.contains("## Original Task"));
    assert!(result.contains("Implement X"));
    assert!(result.contains("## Plan"));
    assert!(result.contains("Step one"));
    assert!(result.contains("Iteration: 2 of 5"));
    assert!(result.contains("Base commit: abc123"));
    assert!(result.contains("### Iteration 0"));
    assert!(result.contains("commit def4567"));
    assert!(result.contains("file.rs"));
    assert!(result.contains("Did step one"));
    assert!(result.contains("DO NOT run git commit"));
    assert!(result.contains("Continue work"));
    // Complexity assessment should also be present
    assert!(result.contains("<task_assessment>"));
    assert!(result.contains("EnterPlanMode"));
}

#[test]
fn test_build_with_context_duration_mode() {
    let builder = IterativePromptBuilder::new("Keep going");
    let ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "Long task".to_string(),
        None,
        -1, // Duration mode
    );

    let result = builder.build_with_context(1, &ctx);
    assert!(result.contains("Iteration: 2 of ongoing"));
}

#[test]
fn test_build_with_context_no_plan() {
    let builder = IterativePromptBuilder::new("Work");
    let ctx = IterationContext::with_context_passing(
        "abc123".to_string(),
        "Task without plan".to_string(),
        None,
        3,
    );

    let result = builder.build_with_context(1, &ctx);
    assert!(!result.contains("## Plan"));
    assert!(result.contains("## Original Task"));
}

#[test]
fn test_enhance_prompt_static() {
    let result = enhance_prompt("Test prompt", 0);
    assert_eq!(result, "Test prompt");

    let result = enhance_prompt("Test prompt", 1);
    assert!(result.contains("Test prompt"));
    assert!(result.contains("git log"));
}

#[test]
fn test_enhance_prompt_with_custom_static() {
    let result = enhance_prompt_with_custom("Test", 1, Some("Custom instruction"));
    assert!(result.contains("Test"));
    assert!(result.contains("Custom instruction"));
    assert!(!result.contains("git log"));

    let result = enhance_prompt_with_custom("Test", 1, None);
    assert!(result.contains("git log"));
}
