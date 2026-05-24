use super::*;

#[test]
fn test_budget_new() {
    let budget = ContextBudget::new(200000, 16384);
    assert_eq!(budget.total_tokens, 200000);
    assert_eq!(budget.output_reserved, 16384);
    assert_eq!(budget.input_budget(), 200000 - 16384);
    assert_eq!(budget.total_used(), 0);
    assert_eq!(budget.available(), 200000 - 16384);
}

#[test]
fn test_budget_allocation_and_usage() {
    let mut budget = ContextBudget::new(200000, 16384);

    budget.set_allocation(BudgetCategory::SystemPrompt, 10000);
    budget.set_allocation(BudgetCategory::ToolDefinitions, 5000);

    assert_eq!(budget.remaining_for(BudgetCategory::SystemPrompt), 10000);

    budget.record_usage(BudgetCategory::SystemPrompt, 3000);
    assert_eq!(budget.remaining_for(BudgetCategory::SystemPrompt), 7000);
    assert_eq!(budget.total_used(), 3000);

    budget.record_usage(BudgetCategory::ToolDefinitions, 2000);
    assert_eq!(budget.total_used(), 5000);
    assert_eq!(budget.available(), 200000 - 16384 - 5000);
}

#[test]
fn test_budget_utilization() {
    let mut budget = ContextBudget::new(100000, 10000);
    assert!((budget.utilization() - 0.0).abs() < f32::EPSILON);

    budget.record_usage(BudgetCategory::SystemPrompt, 45000);
    assert!((budget.utilization() - 0.5).abs() < f32::EPSILON);

    budget.record_usage(BudgetCategory::ConversationHistory, 45000);
    assert!((budget.utilization() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn test_budget_record_usage_auto_creates() {
    let mut budget = ContextBudget::new(100000, 10000);
    budget.record_usage(BudgetCategory::Injections, 500);
    assert_eq!(budget.total_used(), 500);
    // allocated is 0 but used is 500
    assert_eq!(budget.remaining_for(BudgetCategory::Injections), -500);
}

#[test]
fn test_budget_category_display() {
    assert_eq!(BudgetCategory::SystemPrompt.to_string(), "system_prompt");
    assert_eq!(
        BudgetCategory::ConversationHistory.to_string(),
        "conversation_history"
    );
    assert_eq!(
        BudgetCategory::ToolDefinitions.to_string(),
        "tool_definitions"
    );
}
