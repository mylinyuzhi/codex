use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Represents a subagent running in the background.
///
/// Background agents write their output to a file so the parent can retrieve
/// results asynchronously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundAgent {
    /// Unique identifier for the background agent instance.
    pub agent_id: String,

    /// Path to the file where the agent writes its output.
    pub output_file: PathBuf,
}

#[cfg(test)]
#[path = "background.test.rs"]
mod tests;
