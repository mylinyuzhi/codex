//! Background task management for long-running shell commands.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

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
    /// Cancellation token to signal the background task to stop.
    ///
    /// When cancelled, the spawned task can check this and kill the child process.
    pub cancel_token: CancellationToken,
}

/// Preserved state of a completed/stopped background task.
#[derive(Debug, Clone)]
struct CompletedTask {
    /// The command that was executed.
    command: String,
    /// Final accumulated output.
    output: String,
}

/// Snapshot of a background task's state for external consumers.
#[derive(Debug, Clone)]
pub struct BackgroundTaskSnapshot {
    /// Task ID.
    pub id: String,
    /// Command being executed.
    pub command: String,
    /// Whether the task is still running.
    pub is_running: bool,
}

/// Registry for tracking background shell processes.
#[derive(Debug, Clone)]
pub struct BackgroundTaskRegistry {
    tasks: Arc<Mutex<HashMap<String, BackgroundProcess>>>,
    /// Completed/stopped tasks with preserved output.
    completed_tasks: Arc<Mutex<HashMap<String, CompletedTask>>>,
}

impl BackgroundTaskRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registers a background process with the given task ID.
    pub async fn register(&self, task_id: String, process: BackgroundProcess) {
        let mut tasks = self.tasks.lock().await;
        tasks.insert(task_id, process);
    }

    /// Returns the accumulated output for the given task, if it exists.
    ///
    /// Checks active tasks first, then falls back to completed tasks.
    pub async fn get_output(&self, task_id: &str) -> Option<String> {
        // Check active tasks first
        {
            let tasks = self.tasks.lock().await;
            if let Some(process) = tasks.get(task_id) {
                let output = process.output.lock().await;
                return Some(output.clone());
            }
        }
        // Fallback to completed tasks
        let completed = self.completed_tasks.lock().await;
        completed.get(task_id).map(|c| c.output.clone())
    }

    /// Returns the command for the given task, if it exists.
    ///
    /// Checks active tasks first, then falls back to completed tasks.
    pub async fn get_command(&self, task_id: &str) -> Option<String> {
        {
            let tasks = self.tasks.lock().await;
            if let Some(process) = tasks.get(task_id) {
                return Some(process.command.clone());
            }
        }
        let completed = self.completed_tasks.lock().await;
        completed.get(task_id).map(|c| c.command.clone())
    }

    /// Signals the task to stop and removes it from the registry.
    ///
    /// Captures the final output before removal so it can still be retrieved
    /// via [`get_output`](Self::get_output) after the task is stopped.
    ///
    /// Returns true if the task was found and removed, false otherwise.
    pub async fn stop(&self, task_id: &str) -> bool {
        let process = {
            let mut tasks = self.tasks.lock().await;
            tasks.remove(task_id)
        };

        if let Some(process) = process {
            // Capture final output before cancellation
            let final_output = process.output.lock().await.clone();
            self.completed_tasks.lock().await.insert(
                task_id.to_string(),
                CompletedTask {
                    command: process.command.clone(),
                    output: final_output,
                },
            );

            // Cancel the token — the spawned task uses select! on this
            // and will drop the Child, triggering kill_on_drop.
            process.cancel_token.cancel();
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

    /// Returns the completion `Notify` handle for the given task, if registered.
    pub async fn get_completed_notify(&self, task_id: &str) -> Option<Arc<Notify>> {
        let tasks = self.tasks.lock().await;
        tasks.get(task_id).map(|p| Arc::clone(&p.completed))
    }

    /// Returns a snapshot of all tasks (both active and completed).
    pub async fn list_tasks(&self) -> Vec<BackgroundTaskSnapshot> {
        let mut snapshots = Vec::new();

        {
            let tasks = self.tasks.lock().await;
            for (id, process) in tasks.iter() {
                snapshots.push(BackgroundTaskSnapshot {
                    id: id.clone(),
                    command: process.command.clone(),
                    is_running: true,
                });
            }
        }

        {
            let completed = self.completed_tasks.lock().await;
            for (id, task) in completed.iter() {
                snapshots.push(BackgroundTaskSnapshot {
                    id: id.clone(),
                    command: task.command.clone(),
                    is_running: false,
                });
            }
        }

        snapshots
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
