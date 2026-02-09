use serde::Deserialize;
use serde::Serialize;

/// Defines when iterative execution should stop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IterationCondition {
    /// Stop after a fixed number of iterations.
    Count {
        /// Maximum iteration count.
        max: i32,
    },

    /// Stop after a duration limit is reached.
    Duration {
        /// Maximum allowed seconds.
        max_secs: i64,
    },

    /// Stop when a check condition is satisfied.
    Until {
        /// Description of the condition to check.
        check: String,
    },
}

#[cfg(test)]
#[path = "condition.test.rs"]
mod tests;
