//! Plan mode configuration.
//!
//! Defines settings for plan mode agent behavior.

use serde::Deserialize;
use serde::Serialize;

/// Default number of agents for plan execution.
pub const DEFAULT_PLAN_AGENT_COUNT: i32 = 1;

/// Default number of agents for exploration during planning.
pub const DEFAULT_PLAN_EXPLORE_AGENT_COUNT: i32 = 3;

/// Minimum allowed agent count.
pub const MIN_AGENT_COUNT: i32 = 1;

/// Maximum allowed agent count.
pub const MAX_AGENT_COUNT: i32 = 5;

/// Plan mode configuration.
///
/// Controls the behavior of plan mode, including agent counts for execution
/// and exploration phases.
///
/// # Environment Variables
///
/// - `COCODE_PLAN_AGENT_COUNT`: Number of agents for plan execution (1-5)
/// - `COCODE_PLAN_EXPLORE_AGENT_COUNT`: Number of agents for exploration (1-5)
///
/// # Example
///
/// ```json
/// {
///   "plan": {
///     "agent_count": 2,
///     "explore_agent_count": 4
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlanModeConfig {
    /// Number of agents for plan execution.
    #[serde(default = "default_plan_agent_count")]
    pub agent_count: i32,

    /// Number of agents for exploration during planning.
    #[serde(default = "default_plan_explore_agent_count")]
    pub explore_agent_count: i32,
}

impl Default for PlanModeConfig {
    fn default() -> Self {
        Self {
            agent_count: DEFAULT_PLAN_AGENT_COUNT,
            explore_agent_count: DEFAULT_PLAN_EXPLORE_AGENT_COUNT,
        }
    }
}

impl PlanModeConfig {
    /// Validate configuration values.
    ///
    /// Returns an error message if any values are out of range (1-5).
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_AGENT_COUNT..=MAX_AGENT_COUNT).contains(&self.agent_count) {
            return Err(format!(
                "agent_count must be {MIN_AGENT_COUNT}-{MAX_AGENT_COUNT}, got {}",
                self.agent_count
            ));
        }

        if !(MIN_AGENT_COUNT..=MAX_AGENT_COUNT).contains(&self.explore_agent_count) {
            return Err(format!(
                "explore_agent_count must be {MIN_AGENT_COUNT}-{MAX_AGENT_COUNT}, got {}",
                self.explore_agent_count
            ));
        }

        Ok(())
    }

    /// Clamp agent_count to valid range.
    pub fn clamp_agent_count(&mut self) {
        self.agent_count = self.agent_count.clamp(MIN_AGENT_COUNT, MAX_AGENT_COUNT);
    }

    /// Clamp explore_agent_count to valid range.
    pub fn clamp_explore_agent_count(&mut self) {
        self.explore_agent_count = self
            .explore_agent_count
            .clamp(MIN_AGENT_COUNT, MAX_AGENT_COUNT);
    }

    /// Clamp all values to valid ranges.
    pub fn clamp_all(&mut self) {
        self.clamp_agent_count();
        self.clamp_explore_agent_count();
    }
}

fn default_plan_agent_count() -> i32 {
    DEFAULT_PLAN_AGENT_COUNT
}

fn default_plan_explore_agent_count() -> i32 {
    DEFAULT_PLAN_EXPLORE_AGENT_COUNT
}

#[cfg(test)]
#[path = "plan_config.test.rs"]
mod tests;
