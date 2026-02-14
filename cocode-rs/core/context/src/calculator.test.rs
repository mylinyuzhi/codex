use super::*;

#[test]
fn test_estimate_tokens_empty() {
    let calc = ContextCalculator::default();
    assert_eq!(calc.estimate_tokens(""), 0);
}

#[test]
fn test_estimate_tokens_basic() {
    let calc = ContextCalculator::default();
    // "hello" = 5 chars / 4.0 = 1.25 -> ceil = 2
    assert_eq!(calc.estimate_tokens("hello"), 2);

    // 100 chars / 4.0 = 25
    let text = "a".repeat(100);
    assert_eq!(calc.estimate_tokens(&text), 25);
}

#[test]
fn test_estimate_tokens_custom_ratio() {
    let calc = ContextCalculator::new(3.0);
    // "hello" = 5 chars / 3.0 = 1.67 -> ceil = 2
    assert_eq!(calc.estimate_tokens("hello"), 2);

    // 90 chars / 3.0 = 30
    let text = "a".repeat(90);
    assert_eq!(calc.estimate_tokens(&text), 30);
}

#[test]
fn test_compute_budget() {
    let env = EnvironmentInfo::builder()
        .cwd("/tmp/test")
        .context_window(100000)
        .max_output_tokens(10000)
        .build()
        .unwrap();

    let calc = ContextCalculator::default();
    let system_prompt = "a".repeat(4000); // ~1000 tokens
    let tool_defs = vec!["a".repeat(400)]; // ~100 tokens
    let memory = vec![MemoryFile {
        path: "CLAUDE.md".to_string(),
        content: "a".repeat(2000),
        priority: 0,
    }]; // ~500 tokens

    let budget = calc.compute_budget(&env, &system_prompt, &tool_defs, &memory);

    assert_eq!(budget.total_tokens, 100000);
    assert_eq!(budget.output_reserved, 10000);
    assert!(budget.total_used() > 0);
    assert!(budget.available() > 0);

    // Conversation history should get the rest
    assert!(budget.remaining_for(BudgetCategory::ConversationHistory) > 0);
}

#[test]
fn test_needs_compaction() {
    let calc = ContextCalculator::default();

    let mut budget = ContextBudget::new(100000, 10000);
    assert!(!calc.needs_compaction(&budget, 0.8));

    // Use 80% of input budget
    budget.record_usage(BudgetCategory::ConversationHistory, 72000);
    assert!(calc.needs_compaction(&budget, 0.8));
}
