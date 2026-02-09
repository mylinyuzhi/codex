//! Background task management for long-running shell commands.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::Notify;

/// A background process tracked by the registry.
#[derive(Debug, Clone)]
pub struct BackgroundProcess {
    /// Unique identifier for this background task.
    pub id: String,
    /// The command being executed.
    pub command: String,
    /// Accumulated output (stdout + stderr interleaved).
    pub output: Arc<Mutex<String>>,
    /// Notification sent when the process completes.
    pub completed: Arc<Notify>,
}

/// Registry for tracking background shell processes.
#[derive(Debug, Clone)]
pub struct BackgroundTaskRegistry {
    tasks: Arc<Mutex<HashMap<String, BackgroundProcess>>>,
}

impl BackgroundTaskRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registers a background process with the given task ID.
    pub async fn register(&self, task_id: String, process: BackgroundProcess) {
        let mut tasks = self.tasks.lock().await;
        tasks.insert(task_id, process);
    }

    /// Returns the accumulated output for the given task, if it exists.
    pub async fn get_output(&self, task_id: &str) -> Option<String> {
        let tasks = self.tasks.lock().await;
        let process = tasks.get(task_id)?;
        let output = process.output.lock().await;
        Some(output.clone())
    }

    /// Signals the task to stop and removes it from the registry.
    ///
    /// Returns true if the task was found and removed, false otherwise.
    pub async fn stop(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.lock().await;
        if let Some(process) = tasks.remove(task_id) {
            // Notify any waiters that the process is complete
            process.completed.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Returns true if the task is still registered (potentially running).
    pub async fn is_running(&self, task_id: &str) -> bool {
        let tasks = self.tasks.lock().await;
        tasks.contains_key(task_id)
    }
}

impl Default for BackgroundTaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "background.test.rs"]
mod tests;
