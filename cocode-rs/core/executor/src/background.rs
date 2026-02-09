use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Represents a background execution task.
///
/// Background executions run asynchronously and write their output to a file
/// that can be polled or awaited by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundExecution {
    /// Unique task identifier.
    pub task_id: String,

    /// Path to the file where output will be written.
    pub output_file: PathBuf,
}

#[cfg(test)]
#[path = "background.test.rs"]
mod tests;
