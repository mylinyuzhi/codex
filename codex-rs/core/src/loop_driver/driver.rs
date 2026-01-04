use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

use super::condition::LoopCondition;
use super::prompt::LoopPromptBuilder;
use crate::codex::Codex;
use crate::spawn_task::LogFileSink;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;

/// Progress information for callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopProgress {
    /// Current iteration number (0-indexed, after completion).
    pub iteration: i32,
    /// Number of iterations that succeeded.
    pub succeeded: i32,
    /// Number of iterations that failed.
    pub failed: i32,
    /// Elapsed time in seconds.
    pub elapsed_seconds: i64,
}

/// Result of loop execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopResult {
    /// Number of iterations attempted.
    pub iterations_attempted: i32,
    /// Number of iterations that succeeded.
    pub iterations_succeeded: i32,
    /// Number of iterations that failed.
    pub iterations_failed: i32,
    /// Reason the loop stopped.
    pub stop_reason: LoopStopReason,
    /// Total elapsed time in seconds.
    pub elapsed_seconds: i64,
}

/// Reason why the loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStopReason {
    /// Completed all iterations (count mode).
    Completed,
    /// Duration elapsed (time mode).
    DurationElapsed,
    /// Cancelled via CancellationToken.
    Cancelled,
    /// Task returned None (aborted internally).
    TaskAborted,
}

/// Driver for loop-based agent execution.
///
/// Wraps the standard run_task() with loop/time-based execution control.
/// **Key behavior:** Continue-on-error - iterations continue after failure.
///
/// # Example
///
/// ```rust,ignore
/// let condition = LoopCondition::Iters { count: 5 };
/// let mut driver = LoopDriver::new(condition, cancellation_token);
///
/// while driver.should_continue() {
///     let query = driver.build_query("original query");
///     // Execute iteration...
///     driver.mark_iteration_complete(success);
/// }
///
/// let result = driver.finish();
/// println!("Completed {} of {} iterations", result.iterations_succeeded, result.iterations_attempted);
/// ```
pub struct LoopDriver {
    condition: LoopCondition,
    start_time: Instant,
    iteration: i32,
    iterations_failed: i32,
    cancellation_token: CancellationToken,
    custom_loop_prompt: Option<String>,
    /// Optional progress callback for real-time updates.
    progress_callback: Option<Box<dyn Fn(LoopProgress) + Send + Sync>>,
}

impl LoopDriver {
    /// Create a new LoopDriver.
    ///
    /// # Arguments
    ///
    /// * `condition` - Loop termination condition
    /// * `token` - Cancellation token for graceful shutdown
    pub fn new(condition: LoopCondition, token: CancellationToken) -> Self {
        Self {
            condition,
            start_time: Instant::now(),
            iteration: 0,
            iterations_failed: 0,
            cancellation_token: token,
            custom_loop_prompt: None,
            progress_callback: None,
        }
    }

    /// Set custom loop prompt (instead of default git-based prompt).
    pub fn with_custom_prompt(mut self, prompt: String) -> Self {
        self.custom_loop_prompt = Some(prompt);
        self
    }

    /// Set progress callback for real-time iteration updates.
    ///
    /// The callback is invoked after each iteration completes (success or failure).
    /// Use this to persist progress to metadata or update UI.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let driver = LoopDriver::new(condition, token)
    ///     .with_progress_callback(|progress| {
    ///         println!("Iteration {}: {} succeeded, {} failed",
    ///             progress.iteration, progress.succeeded, progress.failed);
    ///     });
    /// ```
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(LoopProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Current iteration number (0-indexed).
    pub fn current_iteration(&self) -> i32 {
        self.iteration
    }

    /// Elapsed time since driver started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Get the loop condition.
    pub fn condition(&self) -> &LoopCondition {
        &self.condition
    }

    /// Get the cancellation token.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Check if loop should continue.
    ///
    /// Returns false if:
    /// - Cancellation token is cancelled
    /// - Iteration count reached (iters mode)
    /// - Duration elapsed (time mode)
    pub fn should_continue(&self) -> bool {
        // 1. Check cancellation first
        if self.cancellation_token.is_cancelled() {
            return false;
        }

        // 2. Check condition
        match &self.condition {
            LoopCondition::Iters { count } => self.iteration < *count,
            LoopCondition::Duration { seconds } => {
                (self.start_time.elapsed().as_secs() as i64) < *seconds
            }
        }
    }

    /// Build query for current iteration.
    pub fn build_query(&self, original: &str) -> String {
        LoopPromptBuilder::build_with_custom(
            original,
            self.iteration,
            self.custom_loop_prompt.as_deref(),
        )
    }

    /// Mark iteration as complete.
    ///
    /// # Arguments
    /// * `success` - Whether the iteration succeeded
    ///
    /// # Returns
    /// Current progress after this iteration
    pub fn mark_iteration_complete(&mut self, success: bool) -> LoopProgress {
        if !success {
            self.iterations_failed += 1;
            warn!(
                iteration = self.iteration,
                "Iteration failed, continuing to next iteration..."
            );
        } else {
            info!(
                iteration = self.iteration,
                elapsed_secs = self.start_time.elapsed().as_secs(),
                "Iteration succeeded"
            );
        }

        self.iteration += 1;

        let progress = LoopProgress {
            iteration: self.iteration,
            succeeded: self.iteration - self.iterations_failed,
            failed: self.iterations_failed,
            elapsed_seconds: self.start_time.elapsed().as_secs() as i64,
        };

        // Trigger progress callback
        if let Some(ref callback) = self.progress_callback {
            callback(progress.clone());
        }

        progress
    }

    /// Finish the loop and return the result.
    pub fn finish(self) -> LoopResult {
        let result = LoopResult {
            iterations_attempted: self.iteration,
            iterations_succeeded: self.iteration - self.iterations_failed,
            iterations_failed: self.iterations_failed,
            stop_reason: self.determine_stop_reason(),
            elapsed_seconds: self.start_time.elapsed().as_secs() as i64,
        };

        info!(
            attempted = result.iterations_attempted,
            succeeded = result.iterations_succeeded,
            failed = result.iterations_failed,
            elapsed_secs = result.elapsed_seconds,
            reason = ?result.stop_reason,
            "Loop execution complete"
        );

        result
    }

    /// Run task with loop driver.
    ///
    /// Executes codex.submit() in a loop until condition is met.
    /// Uses continue-on-error: if iteration fails, logs and continues.
    ///
    /// # Arguments
    ///
    /// * `codex` - Codex instance to submit queries to
    /// * `original_query` - Original user query (enhanced for iterations > 0)
    /// * `sink` - Optional LogFileSink for event logging
    ///
    /// # Returns
    ///
    /// Loop execution result with iteration count and stop reason.
    pub async fn run_with_loop(
        &mut self,
        codex: &Codex,
        original_query: &str,
        sink: Option<&LogFileSink>,
    ) -> LoopResult {
        info!(
            condition = %self.condition.display(),
            "Starting loop execution"
        );

        while self.should_continue() {
            let query = self.build_query(original_query);
            let input = vec![UserInput::Text { text: query }];

            if let Some(s) = sink {
                s.log(&format!("Iteration {}: Starting...", self.iteration));
            }

            info!(
                iteration = self.iteration,
                elapsed_secs = self.start_time.elapsed().as_secs(),
                "Starting iteration"
            );

            // Submit via Codex API
            if let Err(e) = codex.submit(Op::UserInput { items: input }).await {
                warn!(
                    iteration = self.iteration,
                    error = %e,
                    "Iteration failed to submit, continuing to next iteration..."
                );
                if let Some(s) = sink {
                    s.log(&format!(
                        "Iteration {} failed to submit: {e}",
                        self.iteration
                    ));
                }
                self.iterations_failed += 1;
                self.iteration += 1;
                continue; // Continue-on-error
            }

            // Wait for task completion
            let success = self.wait_for_task_complete(codex, sink).await;

            if success {
                info!(
                    iteration = self.iteration,
                    elapsed_secs = self.start_time.elapsed().as_secs(),
                    "Iteration succeeded"
                );
            } else {
                warn!(
                    iteration = self.iteration,
                    "Iteration task aborted, continuing to next iteration..."
                );
                self.iterations_failed += 1;
            }

            self.iteration += 1;

            // Trigger progress callback
            if let Some(ref callback) = self.progress_callback {
                callback(LoopProgress {
                    iteration: self.iteration,
                    succeeded: self.iteration - self.iterations_failed,
                    failed: self.iterations_failed,
                    elapsed_seconds: self.start_time.elapsed().as_secs() as i64,
                });
            }
        }

        let result = LoopResult {
            iterations_attempted: self.iteration,
            iterations_succeeded: self.iteration - self.iterations_failed,
            iterations_failed: self.iterations_failed,
            stop_reason: self.determine_stop_reason(),
            elapsed_seconds: self.start_time.elapsed().as_secs() as i64,
        };

        info!(
            attempted = result.iterations_attempted,
            succeeded = result.iterations_succeeded,
            failed = result.iterations_failed,
            elapsed_secs = result.elapsed_seconds,
            reason = ?result.stop_reason,
            "Loop execution complete"
        );

        result
    }

    /// Wait for TaskComplete or TurnAborted event.
    ///
    /// Returns true if TaskComplete was received, false if aborted or error.
    async fn wait_for_task_complete(&self, codex: &Codex, sink: Option<&LogFileSink>) -> bool {
        loop {
            // Check cancellation
            if self.cancellation_token.is_cancelled() {
                if let Some(s) = sink {
                    s.log("Cancelled by user");
                }
                return false;
            }

            // Get next event from Codex
            match codex.next_event().await {
                Ok(event) => {
                    // Log event to sink if provided (only key events, not all)
                    match &event.msg {
                        EventMsg::TaskComplete(_) => {
                            if let Some(s) = sink {
                                s.log("TaskComplete received");
                            }
                            return true;
                        }
                        EventMsg::TurnAborted(aborted) => {
                            if let Some(s) = sink {
                                s.log(&format!("TurnAborted: {:?}", aborted.reason));
                            }
                            return false;
                        }
                        // Continue processing other events
                        _ => continue,
                    }
                }
                Err(e) => {
                    if let Some(s) = sink {
                        s.log(&format!("Error receiving event: {e}"));
                    }
                    warn!(error = %e, "Error receiving event");
                    return false;
                }
            }
        }
    }

    /// Determine why loop stopped.
    fn determine_stop_reason(&self) -> LoopStopReason {
        if self.cancellation_token.is_cancelled() {
            return LoopStopReason::Cancelled;
        }

        match &self.condition {
            LoopCondition::Iters { count } => {
                if self.iteration >= *count {
                    LoopStopReason::Completed
                } else {
                    LoopStopReason::TaskAborted
                }
            }
            LoopCondition::Duration { seconds } => {
                if (self.start_time.elapsed().as_secs() as i64) >= *seconds {
                    LoopStopReason::DurationElapsed
                } else {
                    LoopStopReason::TaskAborted
                }
            }
        }
    }
}

impl std::fmt::Debug for LoopDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopDriver")
            .field("condition", &self.condition)
            .field("iteration", &self.iteration)
            .field("iterations_failed", &self.iterations_failed)
            .field("elapsed_secs", &self.start_time.elapsed().as_secs())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_continue_iters() {
        let token = CancellationToken::new();
        let mut driver = LoopDriver::new(LoopCondition::Iters { count: 3 }, token);

        assert!(driver.should_continue()); // iteration 0 < 3
        driver.iteration = 2;
        assert!(driver.should_continue()); // iteration 2 < 3
        driver.iteration = 3;
        assert!(!driver.should_continue()); // iteration 3 >= 3
    }

    #[test]
    fn should_continue_cancelled() {
        let token = CancellationToken::new();
        let driver = LoopDriver::new(LoopCondition::Iters { count: 100 }, token.clone());

        assert!(driver.should_continue());
        token.cancel();
        assert!(!driver.should_continue());
    }

    #[test]
    fn build_query_iterations() {
        let token = CancellationToken::new();
        let mut driver = LoopDriver::new(LoopCondition::Iters { count: 5 }, token);

        let original = "Fix the bug";

        // Iteration 0: unchanged
        assert_eq!(driver.build_query(original), original);

        // Iteration 1+: enhanced
        driver.iteration = 1;
        let enhanced = driver.build_query(original);
        assert!(enhanced.contains(original));
        assert!(enhanced.contains("git log"));
    }

    #[test]
    fn loop_result_tracks_failures() {
        let result = LoopResult {
            iterations_attempted: 5,
            iterations_succeeded: 3,
            iterations_failed: 2,
            stop_reason: LoopStopReason::Completed,
            elapsed_seconds: 100,
        };

        assert_eq!(result.iterations_attempted, 5);
        assert_eq!(result.iterations_succeeded, 3);
        assert_eq!(result.iterations_failed, 2);
    }

    #[test]
    fn mark_iteration_complete() {
        let token = CancellationToken::new();
        let mut driver = LoopDriver::new(LoopCondition::Iters { count: 5 }, token);

        // First iteration succeeds
        let progress = driver.mark_iteration_complete(true);
        assert_eq!(progress.iteration, 1);
        assert_eq!(progress.succeeded, 1);
        assert_eq!(progress.failed, 0);

        // Second iteration fails
        let progress = driver.mark_iteration_complete(false);
        assert_eq!(progress.iteration, 2);
        assert_eq!(progress.succeeded, 1);
        assert_eq!(progress.failed, 1);

        // Third iteration succeeds
        let progress = driver.mark_iteration_complete(true);
        assert_eq!(progress.iteration, 3);
        assert_eq!(progress.succeeded, 2);
        assert_eq!(progress.failed, 1);
    }

    #[test]
    fn finish_returns_correct_result() {
        let token = CancellationToken::new();
        let mut driver = LoopDriver::new(LoopCondition::Iters { count: 3 }, token);

        driver.mark_iteration_complete(true);
        driver.mark_iteration_complete(false);
        driver.mark_iteration_complete(true);

        let result = driver.finish();
        assert_eq!(result.iterations_attempted, 3);
        assert_eq!(result.iterations_succeeded, 2);
        assert_eq!(result.iterations_failed, 1);
        assert_eq!(result.stop_reason, LoopStopReason::Completed);
    }
}
