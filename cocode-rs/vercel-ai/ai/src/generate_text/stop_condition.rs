//! Stop conditions for multi-step generation.
//!
//! This module provides utilities for defining stop conditions
//! that determine when multi-step generation should end.

use futures::FutureExt;
use futures::future::BoxFuture;

use super::step_result::StepResult;

/// A function that determines if generation should stop.
pub type StopConditionFn = Box<dyn Fn(Vec<StepResult>) -> BoxFuture<'static, bool> + Send + Sync>;

/// Stop condition for multi-step generation.
pub struct StopCondition {
    /// The condition function.
    condition: StopConditionFn,
    /// Description of the condition.
    description: String,
}

impl StopCondition {
    /// Create a new stop condition.
    pub fn new<F, Fut>(description: impl Into<String>, condition: F) -> Self
    where
        F: Fn(Vec<StepResult>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        Self {
            condition: Box::new(move |steps| condition(steps).boxed()),
            description: description.into(),
        }
    }

    /// Check if this condition is met.
    pub async fn is_met(&self, steps: Vec<StepResult>) -> bool {
        (self.condition)(steps).await
    }

    /// Get the description.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Create a stop condition that triggers after a specific number of steps.
pub fn step_count_is(step_count: usize) -> StopCondition {
    StopCondition::new(format!("Stop after {step_count} steps"), move |steps| {
        let target = step_count;
        async move { steps.len() >= target }
    })
}

/// Create a stop condition that triggers when a specific tool is called.
pub fn has_tool_call(tool_name: impl Into<String>) -> StopCondition {
    let tool_name = tool_name.into();
    StopCondition::new(
        format!("Stop when tool '{tool_name}' is called"),
        move |steps| {
            let target_tool = tool_name.clone();
            async move {
                steps.last().is_some_and(|step| {
                    step.tool_calls.iter().any(|tc| tc.tool_name == target_tool)
                })
            }
        },
    )
}

/// Create a stop condition that triggers when the response contains specific text.
pub fn response_contains(text: impl Into<String>) -> StopCondition {
    let text = text.into();
    StopCondition::new(
        format!("Stop when response contains '{text}'"),
        move |steps| {
            let target_text = text.clone();
            async move {
                steps
                    .last()
                    .is_some_and(|step| step.text.contains(&target_text))
            }
        },
    )
}

/// Check if any stop condition is met.
pub async fn is_stop_condition_met(
    stop_conditions: &[StopCondition],
    steps: Vec<StepResult>,
) -> bool {
    for condition in stop_conditions {
        if condition.is_met(steps.clone()).await {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "stop_condition.test.rs"]
mod tests;
