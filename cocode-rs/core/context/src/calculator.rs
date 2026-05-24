//! Sync token estimation and budget computation.
//!
//! Provides approximate token counting based on character-to-token ratios,
//! and computes token budgets for the context window.

use crate::budget::BudgetCategory;
use crate::budget::ContextBudget;
use crate::conversation_context::MemoryFile;
use crate::environment::EnvironmentInfo;

/// Default characters-per-token ratio for estimation.
const DEFAULT_CHARS_PER_TOKEN: f32 = 4.0;

/// Default reserved safety margin (percentage of input budget).
const DEFAULT_RESERVED_PCT: f32 = 0.05;

/// Sync token estimator and budget calculator.
#[derive(Debug, Clone)]
pub struct ContextCalculator {
    /// Characters-per-token ratio for estimation.
    chars_per_token: f32,
}

impl Default for ContextCalculator {
    fn default() -> Self {
        Self {
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
        }
    }
}

impl ContextCalculator {
    /// Create a new calculator with custom chars-per-token ratio.
    pub fn new(chars_per_token: f32) -> Self {
        Self { chars_per_token }
    }

    /// Estimate token count for a text string.
    pub fn estimate_tokens(&self, text: &str) -> i32 {
        if text.is_empty() {
            return 0;
        }
        (text.len() as f32 / self.chars_per_token).ceil() as i32
    }

    /// Compute a context budget based on environment and content.
    pub fn compute_budget(
        &self,
        env: &EnvironmentInfo,
        system_prompt: &str,
        tool_definitions: &[String],
        memory_files: &[MemoryFile],
    ) -> ContextBudget {
        let mut budget = ContextBudget::new(env.context_window, env.max_output_tokens);

        // Reserve safety margin
        let reserved = (budget.input_budget() as f32 * DEFAULT_RESERVED_PCT) as i32;
        budget.set_allocation(BudgetCategory::Reserved, reserved);
        budget.record_usage(BudgetCategory::Reserved, reserved);

        // System prompt
        let system_tokens = self.estimate_tokens(system_prompt);
        budget.set_allocation(BudgetCategory::SystemPrompt, system_tokens);
        budget.record_usage(BudgetCategory::SystemPrompt, system_tokens);

        // Tool definitions
        let tool_tokens: i32 = tool_definitions
            .iter()
            .map(|t| self.estimate_tokens(t))
            .sum();
        budget.set_allocation(BudgetCategory::ToolDefinitions, tool_tokens);
        budget.record_usage(BudgetCategory::ToolDefinitions, tool_tokens);

        // Memory files
        let memory_tokens: i32 = memory_files
            .iter()
            .map(|m| self.estimate_tokens(&m.content))
            .sum();
        budget.set_allocation(BudgetCategory::MemoryFiles, memory_tokens);
        budget.record_usage(BudgetCategory::MemoryFiles, memory_tokens);

        // Remaining goes to conversation history
        let conversation_budget = budget.available();
        budget.set_allocation(BudgetCategory::ConversationHistory, conversation_budget);

        budget
    }

    /// Check if context needs compaction based on utilization threshold.
    pub fn needs_compaction(&self, budget: &ContextBudget, threshold: f32) -> bool {
        budget.utilization() >= threshold
    }
}

#[cfg(test)]
#[path = "calculator.test.rs"]
mod tests;
