//! Token budget tracking for context management.
//!
//! Tracks token allocations across categories to ensure the context window
//! is used efficiently and does not overflow.

use serde::Deserialize;
use serde::Serialize;

/// Categories for token budget allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetCategory {
    /// System prompt tokens.
    SystemPrompt,
    /// Conversation history tokens.
    ConversationHistory,
    /// Tool definition tokens.
    ToolDefinitions,
    /// Memory file tokens (CLAUDE.md, etc.).
    MemoryFiles,
    /// Injected content tokens.
    Injections,
    /// Safety margin reserved tokens.
    Reserved,
}

impl BudgetCategory {
    /// Get the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetCategory::SystemPrompt => "system_prompt",
            BudgetCategory::ConversationHistory => "conversation_history",
            BudgetCategory::ToolDefinitions => "tool_definitions",
            BudgetCategory::MemoryFiles => "memory_files",
            BudgetCategory::Injections => "injections",
            BudgetCategory::Reserved => "reserved",
        }
    }
}

impl std::fmt::Display for BudgetCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single budget allocation for a category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAllocation {
    /// Which category this allocation is for.
    pub category: BudgetCategory,
    /// Allocated token count.
    pub allocated: i32,
    /// Currently used token count.
    pub used: i32,
}

impl BudgetAllocation {
    /// Remaining tokens in this allocation.
    pub fn remaining(&self) -> i32 {
        self.allocated - self.used
    }
}

/// Token budget tracker for the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Total context window tokens.
    pub total_tokens: i32,
    /// Tokens reserved for model output.
    pub output_reserved: i32,
    /// Per-category allocations.
    allocations: Vec<BudgetAllocation>,
}

impl ContextBudget {
    /// Create a new context budget.
    pub fn new(total_tokens: i32, output_reserved: i32) -> Self {
        Self {
            total_tokens,
            output_reserved,
            allocations: Vec::new(),
        }
    }

    /// Input token budget (total minus output reserved).
    pub fn input_budget(&self) -> i32 {
        self.total_tokens - self.output_reserved
    }

    /// Total tokens currently used across all categories.
    pub fn total_used(&self) -> i32 {
        self.allocations.iter().map(|a| a.used).sum()
    }

    /// Available tokens (input budget minus total used).
    pub fn available(&self) -> i32 {
        self.input_budget() - self.total_used()
    }

    /// Set allocation for a category.
    pub fn set_allocation(&mut self, category: BudgetCategory, allocated: i32) {
        if let Some(alloc) = self.allocations.iter_mut().find(|a| a.category == category) {
            alloc.allocated = allocated;
        } else {
            self.allocations.push(BudgetAllocation {
                category,
                allocated,
                used: 0,
            });
        }
    }

    /// Remaining tokens for a specific category.
    pub fn remaining_for(&self, category: BudgetCategory) -> i32 {
        self.allocations
            .iter()
            .find(|a| a.category == category)
            .map_or(0, BudgetAllocation::remaining)
    }

    /// Record token usage for a category.
    pub fn record_usage(&mut self, category: BudgetCategory, tokens: i32) {
        if let Some(alloc) = self.allocations.iter_mut().find(|a| a.category == category) {
            alloc.used += tokens;
        } else {
            self.allocations.push(BudgetAllocation {
                category,
                allocated: 0,
                used: tokens,
            });
        }
    }

    /// Context utilization ratio (0.0 to 1.0).
    pub fn utilization(&self) -> f32 {
        let budget = self.input_budget();
        if budget <= 0 {
            return 1.0;
        }
        self.total_used() as f32 / budget as f32
    }

    /// Get all allocations.
    pub fn allocations(&self) -> &[BudgetAllocation] {
        &self.allocations
    }
}

#[cfg(test)]
#[path = "budget.test.rs"]
mod tests;
