//! Async hook tracking.
//!
//! Tracks background hook tasks and their completion status. When hooks
//! return `{ "async": true }`, they are registered here and their results
//! are collected for delivery via system reminders.
//!
//! ## Usage
//!
//! 1. When a hook returns `HookResult::Async`, register it with [`AsyncHookTracker::register`]
//! 2. When the background task completes, call [`AsyncHookTracker::complete`]
//! 3. Periodically call [`AsyncHookTracker::take_completed`] to get finished hooks

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

use crate::lock_utils::lock_read;
use crate::lock_utils::lock_write;
use crate::result::HookResult;

/// Default timeout for async hooks in seconds.
pub const DEFAULT_ASYNC_TIMEOUT_SECS: u64 = 15;

/// Tracks pending and completed async hooks.
#[derive(Default)]
pub struct AsyncHookTracker {
    /// Pending async hooks indexed by task_id.
    pending: RwLock<HashMap<String, PendingAsyncHook>>,
    /// Completed async hooks ready for delivery.
    completed: RwLock<Vec<CompletedAsyncHook>>,
}

/// A pending async hook task.
#[derive(Debug, Clone)]
pub struct PendingAsyncHook {
    /// Unique task identifier.
    pub task_id: String,
    /// Name of the hook.
    pub hook_name: String,
    /// When the async task started.
    pub started_at: Instant,
    /// Timeout in seconds for this async hook.
    pub timeout_secs: u64,
}

/// A completed async hook with its result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedAsyncHook {
    /// Unique task identifier.
    pub task_id: String,
    /// Name of the hook.
    pub hook_name: String,
    /// Execution duration in milliseconds.
    pub duration_ms: i64,
    /// The result of the hook.
    pub result: HookResult,
    /// Additional context from the hook.
    pub additional_context: Option<String>,
    /// Whether the hook blocked execution (only possible for pre-hooks).
    pub was_blocking: bool,
    /// Reason for blocking (if was_blocking is true).
    pub blocking_reason: Option<String>,
}

impl AsyncHookTracker {
    /// Creates a new empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new async hook task with the default timeout.
    pub fn register(&self, task_id: String, hook_name: String) {
        self.register_with_timeout(task_id, hook_name, DEFAULT_ASYNC_TIMEOUT_SECS);
    }

    /// Registers a new async hook task with a custom timeout.
    pub fn register_with_timeout(&self, task_id: String, hook_name: String, timeout_secs: u64) {
        if let Some(mut pending) = lock_write(&self.pending, "pending") {
            pending.insert(
                task_id.clone(),
                PendingAsyncHook {
                    task_id,
                    hook_name,
                    started_at: Instant::now(),
                    timeout_secs,
                },
            );
        }
    }

    /// Marks an async hook as completed with its result.
    pub fn complete(&self, task_id: &str, result: HookResult) {
        // Get and remove the pending hook
        let pending_hook = if let Some(mut pending) = lock_write(&self.pending, "pending") {
            pending.remove(task_id)
        } else {
            return;
        };

        let Some(pending) = pending_hook else {
            tracing::warn!(task_id, "Completed unknown async hook task");
            return;
        };

        let duration_ms = pending.started_at.elapsed().as_millis() as i64;

        // Extract blocking info and additional context from result
        let (was_blocking, blocking_reason, additional_context) = match &result {
            HookResult::Reject { reason } => (true, Some(reason.clone()), None),
            HookResult::ContinueWithContext {
                additional_context: ctx,
                ..
            } => (false, None, ctx.clone()),
            _ => (false, None, None),
        };

        let completed = CompletedAsyncHook {
            task_id: pending.task_id,
            hook_name: pending.hook_name,
            duration_ms,
            result,
            additional_context,
            was_blocking,
            blocking_reason,
        };

        if let Some(mut completed_list) = lock_write(&self.completed, "completed") {
            completed_list.push(completed);
        }
    }

    /// Takes all completed hooks, clearing the completed list.
    ///
    /// Returns the completed hooks for processing (e.g., generating system reminders).
    pub fn take_completed(&self) -> Vec<CompletedAsyncHook> {
        if let Some(mut completed) = lock_write(&self.completed, "completed") {
            std::mem::take(&mut *completed)
        } else {
            Vec::new()
        }
    }

    /// Returns the number of pending async hooks.
    pub fn pending_count(&self) -> i32 {
        lock_read(&self.pending, "pending")
            .map(|p| p.len() as i32)
            .unwrap_or(0)
    }

    /// Returns the number of completed but not yet processed hooks.
    pub fn completed_count(&self) -> i32 {
        lock_read(&self.completed, "completed")
            .map(|c| c.len() as i32)
            .unwrap_or(0)
    }

    /// Checks if there are any pending or completed hooks.
    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0 && self.completed_count() == 0
    }

    /// Cancels a pending async hook.
    ///
    /// This removes the hook from pending without adding it to completed.
    /// Useful when a hook times out or is cancelled.
    pub fn cancel(&self, task_id: &str) -> bool {
        if let Some(mut pending) = lock_write(&self.pending, "pending") {
            pending.remove(task_id).is_some()
        } else {
            false
        }
    }

    /// Returns task IDs of expired async hooks (exceeded their timeout).
    ///
    /// Call this periodically (e.g., from the system-reminder generator tick)
    /// to detect timed-out async hooks.
    pub fn check_expired(&self) -> Vec<String> {
        if let Some(pending) = lock_read(&self.pending, "pending") {
            pending
                .values()
                .filter(|h| h.started_at.elapsed().as_secs() >= h.timeout_secs)
                .map(|h| h.task_id.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Cancels all pending async hooks and returns the count cancelled.
    ///
    /// Used for session-end cleanup to ensure no dangling background hooks.
    pub fn cancel_all(&self) -> usize {
        if let Some(mut pending) = lock_write(&self.pending, "pending") {
            let count = pending.len();
            pending.clear();
            count
        } else {
            0
        }
    }
}

impl std::fmt::Debug for AsyncHookTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncHookTracker")
            .field("pending_count", &self.pending_count())
            .field("completed_count", &self.completed_count())
            .finish()
    }
}

#[cfg(test)]
#[path = "async_tracker.test.rs"]
mod tests;
