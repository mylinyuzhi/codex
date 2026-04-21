//! Async hook registry — tracks hooks that run in the background.
//!
//! TS: utils/hooks/AsyncHookRegistry.ts — manages pending async hooks,
//! polls for completion, delivers responses when ready.
//!
//! When a hook outputs `{"async": true}` as its first line, the hook
//! continues executing in the background. The registry tracks the pending
//! hook and polls for its completion. When the hook finishes, its response
//! is delivered to the caller.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::Mutex;

/// Default timeout for async hooks (15 seconds).
///
/// TS: DEFAULT_ASYNC_HOOK_TIMEOUT = 15000
const DEFAULT_ASYNC_TIMEOUT: Duration = Duration::from_secs(15);

/// A pending async hook awaiting completion.
#[derive(Debug)]
pub struct PendingAsyncHook {
    /// Unique identifier for the hook.
    pub hook_id: String,
    /// Human-readable name (command string or URL).
    pub hook_name: String,
    /// The hook event type name.
    pub hook_event: String,
    /// When the hook started executing.
    pub started_at: Instant,
    /// Maximum time to wait for completion.
    pub timeout: Duration,
    /// Accumulated stdout from the hook process.
    pub stdout: String,
    /// Accumulated stderr from the hook process.
    pub stderr: String,
    /// Exit code when the process completes.
    pub exit_code: Option<i32>,
    /// Whether the response has been delivered to the caller.
    pub delivered: bool,
}

/// Response from a completed async hook.
#[derive(Debug, Clone)]
pub struct AsyncHookResponse {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

/// Registry for managing pending async hooks.
///
/// Thread-safe via `Arc<Mutex<_>>` — can be shared across tasks.
#[derive(Clone, Default, Debug)]
pub struct AsyncHookRegistry {
    pending: Arc<Mutex<HashMap<String, PendingAsyncHook>>>,
}

impl AsyncHookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new pending async hook.
    pub async fn register(
        &self,
        hook_id: String,
        hook_name: String,
        hook_event: String,
        timeout: Option<Duration>,
    ) {
        let hook = PendingAsyncHook {
            hook_id: hook_id.clone(),
            hook_name,
            hook_event,
            started_at: Instant::now(),
            timeout: timeout.unwrap_or(DEFAULT_ASYNC_TIMEOUT),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            delivered: false,
        };
        self.pending.lock().await.insert(hook_id, hook);
    }

    /// Update the output of a pending async hook.
    pub async fn update_output(&self, hook_id: &str, stdout: &str, stderr: &str) {
        if let Some(hook) = self.pending.lock().await.get_mut(hook_id) {
            hook.stdout = stdout.to_string();
            hook.stderr = stderr.to_string();
        }
    }

    /// Mark a pending async hook as completed.
    pub async fn complete(&self, hook_id: &str, exit_code: i32) {
        if let Some(hook) = self.pending.lock().await.get_mut(hook_id) {
            hook.exit_code = Some(exit_code);
        }
    }

    /// Get responses from completed (but undelivered) async hooks.
    ///
    /// Also checks for timed-out hooks and marks them as completed.
    pub async fn collect_responses(&self) -> Vec<AsyncHookResponse> {
        let mut pending = self.pending.lock().await;
        let now = Instant::now();
        let mut responses = Vec::new();

        for hook in pending.values_mut() {
            if hook.delivered {
                continue;
            }

            let timed_out = now.duration_since(hook.started_at) > hook.timeout;
            let completed = hook.exit_code.is_some() || timed_out;

            if completed {
                hook.delivered = true;
                responses.push(AsyncHookResponse {
                    hook_id: hook.hook_id.clone(),
                    hook_name: hook.hook_name.clone(),
                    hook_event: hook.hook_event.clone(),
                    stdout: hook.stdout.clone(),
                    stderr: hook.stderr.clone(),
                    exit_code: hook.exit_code.unwrap_or(-1),
                    timed_out,
                });
            }
        }

        responses
    }

    /// Get the number of pending (undelivered) async hooks.
    pub async fn pending_count(&self) -> usize {
        self.pending
            .lock()
            .await
            .values()
            .filter(|h| !h.delivered)
            .count()
    }

    /// Remove all delivered hooks from the registry.
    pub async fn cleanup_delivered(&self) {
        self.pending.lock().await.retain(|_, h| !h.delivered);
    }

    /// Finalize all pending hooks (shutdown path).
    ///
    /// TS: finalizePendingAsyncHooks() — called on session cleanup.
    pub async fn finalize_all(&self) -> Vec<AsyncHookResponse> {
        let mut pending = self.pending.lock().await;
        let mut responses = Vec::new();

        for hook in pending.values_mut() {
            if !hook.delivered {
                hook.delivered = true;
                responses.push(AsyncHookResponse {
                    hook_id: hook.hook_id.clone(),
                    hook_name: hook.hook_name.clone(),
                    hook_event: hook.hook_event.clone(),
                    stdout: hook.stdout.clone(),
                    stderr: hook.stderr.clone(),
                    exit_code: hook.exit_code.unwrap_or(-1),
                    timed_out: hook.exit_code.is_none(),
                });
            }
        }

        responses
    }
}

#[cfg(test)]
#[path = "async_registry.test.rs"]
mod tests;
