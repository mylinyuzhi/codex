//! Streaming tool executor — real-time concurrent tool execution.
//!
//! TS: `services/tools/StreamingToolExecutor.ts`
//!
//! Tools execute DURING API streaming, not after. As the API streams
//! tool_use blocks, `add_tool()` queues them immediately. Safe tools
//! start executing while the API is still streaming. Results and
//! progress messages are yielded in real-time.

use coco_types::ToolId;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::context::ToolUseContext;
use crate::error::SyntheticToolError;
use crate::error::ToolError;
use crate::traits::Tool;
use crate::traits::ToolProgress;

/// Default maximum concurrent tool executions.
const DEFAULT_MAX_CONCURRENCY: usize = 10;

/// Status of a tracked tool in the streaming executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Waiting to be executed.
    Queued,
    /// Currently executing.
    Executing,
    /// Execution completed (result ready).
    Completed,
    /// Result has been yielded to the caller.
    Yielded,
}

/// An update from the streaming executor.
#[derive(Debug)]
pub enum StreamingToolUpdate {
    /// A tool progress message (yielded immediately).
    Progress(ToolProgress),
    /// A completed tool result.
    Result(ToolCallResult),
}

/// A batch of tool calls to execute.
/// Either a single unsafe (non-concurrent) tool, or multiple concurrent-safe tools.
#[derive(Debug)]
pub enum ToolBatch {
    /// Single non-concurrent tool -- runs exclusively.
    SingleUnsafe(PendingToolCall),
    /// Multiple concurrent-safe tools -- run in parallel.
    ConcurrentSafe(Vec<PendingToolCall>),
}

/// A pending tool call waiting for execution.
pub struct PendingToolCall {
    pub tool_use_id: String,
    pub tool: Arc<dyn Tool>,
    pub input: Value,
}

impl std::fmt::Debug for PendingToolCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingToolCall")
            .field("tool_use_id", &self.tool_use_id)
            .field("tool_name", &self.tool.name())
            .finish()
    }
}

/// Result of executing a batch of tool calls.
#[derive(Debug)]
pub struct BatchResult {
    pub results: Vec<ToolCallResult>,
}

/// Result of a single tool call.
pub struct ToolCallResult {
    pub tool_use_id: String,
    pub tool_id: ToolId,
    pub result: Result<ToolResult<Value>, ToolError>,
    pub duration_ms: i64,
}

impl std::fmt::Debug for ToolCallResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolCallResult")
            .field("tool_use_id", &self.tool_use_id)
            .field("tool_id", &self.tool_id)
            .field("is_ok", &self.result.is_ok())
            .field("duration_ms", &self.duration_ms)
            .finish()
    }
}

/// Internal tracking for a tool in the streaming executor.
#[allow(dead_code)]
struct TrackedTool {
    tool_use_id: String,
    tool: Arc<dyn Tool>,
    input: Value,
    status: ToolStatus,
    is_concurrency_safe: bool,
}

/// Streaming tool executor — matches TS StreamingToolExecutor.
///
/// Tools are added via `add_tool()` as the API streams tool_use blocks.
/// Safe tools start executing immediately while the API is still streaming.
/// Results are yielded in tool-received order for determinism.
///
/// Progress messages are forwarded immediately from the tool's progress_tx
/// channel to the update_tx output channel.
///
/// Sibling abort: When a tool fails in a concurrent batch, its siblings
/// are cancelled via a shared CancellationToken.
///
/// Streaming fallback: `discard()` generates SyntheticToolError::StreamingFallback
/// for all unfinished tools when the stream fails and retries non-streaming.
pub struct StreamingToolExecutor {
    max_concurrency: usize,
    tracked: Vec<TrackedTool>,
    discarded: bool,
    /// Child token for sibling abort. Cancelled when any tool in a
    /// concurrent batch fails.
    sibling_cancel: CancellationToken,
    /// Output channel for streaming updates (results + progress).
    update_tx: mpsc::UnboundedSender<StreamingToolUpdate>,
    /// Receive end of the update channel.
    update_rx: mpsc::UnboundedReceiver<StreamingToolUpdate>,
}

impl StreamingToolExecutor {
    pub fn new() -> Self {
        let max_concurrency = std::env::var("CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONCURRENCY);
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        Self {
            max_concurrency,
            tracked: Vec::new(),
            discarded: false,
            sibling_cancel: CancellationToken::new(),
            update_tx,
            update_rx,
        }
    }

    pub fn with_max_concurrency(max_concurrency: usize) -> Self {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        Self {
            max_concurrency,
            tracked: Vec::new(),
            discarded: false,
            sibling_cancel: CancellationToken::new(),
            update_tx,
            update_rx,
        }
    }

    /// Queue a tool for execution. Called as API streams tool_use blocks.
    ///
    /// TS: `addTool(block, assistantMessage)` — safe tools start immediately
    /// if only safe tools are currently running.
    pub fn add_tool(&mut self, tool_use_id: String, tool: Arc<dyn Tool>, input: Value) {
        let is_concurrency_safe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tool.is_concurrency_safe(&input)
        }))
        .unwrap_or(false);

        self.tracked.push(TrackedTool {
            tool_use_id,
            tool,
            input,
            status: ToolStatus::Queued,
            is_concurrency_safe,
        });
    }

    /// Whether a tool with the given safety level can start now.
    ///
    /// TS: `canExecuteTool(isConcurrencySafe)`:
    /// - Concurrent-safe: can run if no non-safe tools are executing
    /// - Non-safe: can run only if nothing is executing
    #[allow(dead_code)]
    fn can_execute(&self, is_concurrency_safe: bool) -> bool {
        let has_executing = self
            .tracked
            .iter()
            .any(|t| t.status == ToolStatus::Executing);
        let has_unsafe_executing = self
            .tracked
            .iter()
            .any(|t| t.status == ToolStatus::Executing && !t.is_concurrency_safe);

        if is_concurrency_safe {
            !has_unsafe_executing
        } else {
            !has_executing
        }
    }

    /// Whether there are still tools executing or queued.
    pub fn has_pending(&self) -> bool {
        self.tracked
            .iter()
            .any(|t| t.status == ToolStatus::Queued || t.status == ToolStatus::Executing)
    }

    /// Whether all tools have been yielded.
    pub fn is_complete(&self) -> bool {
        self.tracked.iter().all(|t| t.status == ToolStatus::Yielded)
    }

    /// Drain completed results (non-blocking).
    ///
    /// Returns all updates (progress + results) that are ready now.
    pub fn drain_completed(&mut self) -> Vec<StreamingToolUpdate> {
        let mut updates = Vec::new();
        while let Ok(update) = self.update_rx.try_recv() {
            updates.push(update);
        }
        updates
    }

    /// Wait for the next update (progress or result).
    pub async fn next_update(&mut self) -> Option<StreamingToolUpdate> {
        self.update_rx.recv().await
    }

    /// Abandon all pending tools, generating synthetic errors.
    ///
    /// TS: `discard()` — called on streaming fallback when stream fails
    /// and the engine retries without streaming.
    pub fn discard(&mut self) {
        self.discarded = true;
        self.sibling_cancel.cancel();

        for tracked in &mut self.tracked {
            if tracked.status == ToolStatus::Queued || tracked.status == ToolStatus::Executing {
                let _ = self
                    .update_tx
                    .send(StreamingToolUpdate::Result(ToolCallResult {
                        tool_use_id: tracked.tool_use_id.clone(),
                        tool_id: tracked.tool.id(),
                        result: Err(ToolError::ExecutionFailed {
                            message: SyntheticToolError::StreamingFallback.to_string(),
                            source: None,
                        }),
                        duration_ms: 0,
                    }));
                tracked.status = ToolStatus::Completed;
            }
        }
    }

    // -- Batch API (backward-compatible) --

    /// Partition tool calls into execution batches.
    ///
    /// Rules:
    /// - Consecutive concurrent-safe tools -> one ConcurrentSafe batch
    /// - Each non-concurrent tool -> its own SingleUnsafe batch
    /// - If is_concurrency_safe() panics, treat as unsafe (conservative)
    pub fn partition(&self, tool_calls: Vec<PendingToolCall>) -> Vec<ToolBatch> {
        let mut batches: Vec<ToolBatch> = Vec::new();
        let mut safe_accumulator: Vec<PendingToolCall> = Vec::new();

        for call in tool_calls {
            let is_safe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.tool.is_concurrency_safe(&call.input)
            }))
            .unwrap_or(false);

            if is_safe {
                safe_accumulator.push(call);
            } else {
                // Flush any accumulated safe tools first
                if !safe_accumulator.is_empty() {
                    batches.push(ToolBatch::ConcurrentSafe(std::mem::take(
                        &mut safe_accumulator,
                    )));
                }
                batches.push(ToolBatch::SingleUnsafe(call));
            }
        }

        // Flush remaining safe tools
        if !safe_accumulator.is_empty() {
            batches.push(ToolBatch::ConcurrentSafe(safe_accumulator));
        }

        batches
    }

    /// Execute a batch of tool calls.
    pub async fn execute_batch(&self, batch: ToolBatch, ctx: &ToolUseContext) -> BatchResult {
        match batch {
            ToolBatch::SingleUnsafe(call) => {
                let result = self.execute_single(call, ctx).await;
                BatchResult {
                    results: vec![result],
                }
            }
            ToolBatch::ConcurrentSafe(calls) => {
                let results = self.execute_concurrent(calls, ctx).await;
                BatchResult { results }
            }
        }
    }

    /// Execute all batches in sequence, yielding results.
    pub async fn execute_all(
        &self,
        tool_calls: Vec<PendingToolCall>,
        ctx: &ToolUseContext,
    ) -> Vec<ToolCallResult> {
        let batches = self.partition(tool_calls);
        let mut all_results = Vec::new();

        for batch in batches {
            if ctx.cancel.is_cancelled() {
                break;
            }
            let batch_result = self.execute_batch(batch, ctx).await;
            all_results.extend(batch_result.results);
        }

        all_results
    }

    /// Execute a single tool call through the full lifecycle pipeline:
    ///
    /// 1. PreToolUse hook — may rewrite input, override permission, or block
    /// 2. `tool.execute()` — the actual work
    /// 3. PostToolUse / PostToolUseFailure hook — may replace output or stop loop
    ///
    /// TS: `services/tools/toolExecution.ts:800-862` pipeline flow.
    async fn execute_single(&self, call: PendingToolCall, ctx: &ToolUseContext) -> ToolCallResult {
        let tool_id = call.tool.id();
        let tool_use_id = call.tool_use_id.clone();
        let tool_name = call.tool.name().to_string();
        let start = std::time::Instant::now();
        let mut input = call.input;

        // ── Stage 1: PreToolUse hook ──
        // Run PreToolUse hooks if a handle is configured. Hooks may rewrite
        // the input, override permission (most-restrictive-wins: deny > ask >
        // allow > passthrough — already aggregated in `PreToolUseOutcome`),
        // or hard-block the call via `blocking_reason`.
        if let Some(handle) = ctx.hook_handle.as_ref() {
            let pre = handle
                .run_pre_tool_use(&tool_name, &tool_use_id, &input)
                .await;

            if pre.is_blocked() {
                let reason = pre
                    .blocking_reason
                    .unwrap_or_else(|| "PreToolUse hook denied tool execution".into());
                return ToolCallResult {
                    tool_use_id,
                    tool_id,
                    result: Err(ToolError::PermissionDenied { message: reason }),
                    duration_ms: start.elapsed().as_millis() as i64,
                };
            }

            // Apply input rewrite if the hook emitted one.
            if let Some(updated) = pre.updated_input {
                input = updated;
            }
            // Ask/Allow overrides are forwarded to the upper permission layer
            // via the outcome; the executor doesn't re-check permissions here
            // because `check_permissions()` was already run upstream.
        }

        // ── Stage 2: Execute ──
        // Track in-progress
        {
            let mut ids = ctx.in_progress_tool_use_ids.write().await;
            ids.insert(tool_use_id.clone());
        }

        let exec_result = tokio::select! {
            r = call.tool.execute(input.clone(), ctx) => r,
            () = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
        };

        // Remove from in-progress
        {
            let mut ids = ctx.in_progress_tool_use_ids.write().await;
            ids.remove(&tool_use_id);
        }

        // ── Stage 3: PostToolUse / PostToolUseFailure hook ──
        let result = if let Some(handle) = ctx.hook_handle.as_ref() {
            match exec_result {
                Ok(mut tool_result) => {
                    let post = handle
                        .run_post_tool_use(&tool_name, &tool_use_id, &input, &tool_result.data)
                        .await;

                    // Hard-block: replace result with error (TS: PostToolUse
                    // Reject path, `toolHooks.ts:237-244`).
                    if let Some(reason) = post.blocking_reason {
                        Err(ToolError::PermissionDenied { message: reason })
                    } else {
                        // Output rewrite: use hook-modified data. Preserves
                        // any `new_messages` from the original tool result.
                        if let Some(updated) = post.updated_output {
                            tool_result.data = updated;
                        }
                        Ok(tool_result)
                    }
                }
                Err(e) => {
                    // Failure path: run PostToolUseFailure hook but don't
                    // let it rewrite the error (TS doesn't allow that either
                    // — failure hooks can only inject context, not recover).
                    let _ = handle
                        .run_post_tool_use_failure(&tool_name, &tool_use_id, &input, &e.to_string())
                        .await;
                    Err(e)
                }
            }
        } else {
            exec_result
        };

        let duration_ms = start.elapsed().as_millis() as i64;

        ToolCallResult {
            tool_use_id,
            tool_id,
            result,
            duration_ms,
        }
    }

    /// Execute multiple concurrent-safe tool calls in parallel.
    ///
    /// Uses sibling abort: if any tool fails, siblings are cancelled.
    /// Results are collected in submission order (not completion order).
    async fn execute_concurrent(
        &self,
        calls: Vec<PendingToolCall>,
        ctx: &ToolUseContext,
    ) -> Vec<ToolCallResult> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let shared_ctx = Arc::new(ctx.clone_for_concurrent());
        let sibling_cancel = CancellationToken::new();
        let mut handles = Vec::with_capacity(calls.len());

        for call in calls {
            let sem = semaphore.clone();
            let ctx_clone = shared_ctx.clone();
            let tool = call.tool;
            let input = call.input;
            let tool_use_id = call.tool_use_id;
            let tool_id = tool.id();
            let tool_name = tool.name().to_string();
            let sibling_tok = sibling_cancel.clone();

            // Capture IDs before spawn for join-error fallback
            let saved_tool_use_id = tool_use_id.clone();
            let saved_tool_id = tool_id.clone();

            let handle = tokio::spawn(async move {
                let Ok(_permit) = sem.acquire().await else {
                    return ToolCallResult {
                        tool_use_id,
                        tool_id,
                        result: Err(ToolError::Cancelled),
                        duration_ms: 0,
                    };
                };
                let start = std::time::Instant::now();

                // ── Stage 1: PreToolUse hook ──
                // Concurrent path runs the same lifecycle hooks as single
                // path. See `execute_single` docstring for the rationale.
                // All three stages must run inside the spawned task so
                // each tool gets its own hook invocation sequence.
                let mut input = input;
                if let Some(handle) = ctx_clone.hook_handle.as_ref() {
                    let pre = handle
                        .run_pre_tool_use(&tool_name, &tool_use_id, &input)
                        .await;
                    if pre.is_blocked() {
                        let reason = pre
                            .blocking_reason
                            .unwrap_or_else(|| "PreToolUse hook denied tool execution".into());
                        return ToolCallResult {
                            tool_use_id,
                            tool_id,
                            result: Err(ToolError::PermissionDenied { message: reason }),
                            duration_ms: start.elapsed().as_millis() as i64,
                        };
                    }
                    if let Some(updated) = pre.updated_input {
                        input = updated;
                    }
                }

                // ── Stage 2: Execute ──
                // Track in-progress
                {
                    let mut ids = ctx_clone.in_progress_tool_use_ids.write().await;
                    ids.insert(tool_use_id.clone());
                }

                let exec_result = tokio::select! {
                    r = tool.execute(input.clone(), &ctx_clone) => r,
                    () = ctx_clone.cancel.cancelled() => Err(ToolError::Cancelled),
                    () = sibling_tok.cancelled() => Err(ToolError::ExecutionFailed {
                        message: SyntheticToolError::SiblingError {
                            failed_tool: "sibling".into(),
                        }.to_string(),
                        source: None,
                    }),
                };

                // Remove from in-progress
                {
                    let mut ids = ctx_clone.in_progress_tool_use_ids.write().await;
                    ids.remove(&tool_use_id);
                }

                // ── Stage 3: PostToolUse / PostToolUseFailure hook ──
                let result = if let Some(handle) = ctx_clone.hook_handle.as_ref() {
                    match exec_result {
                        Ok(mut tool_result) => {
                            let post = handle
                                .run_post_tool_use(
                                    &tool_name,
                                    &tool_use_id,
                                    &input,
                                    &tool_result.data,
                                )
                                .await;
                            if let Some(reason) = post.blocking_reason {
                                Err(ToolError::PermissionDenied { message: reason })
                            } else {
                                if let Some(updated) = post.updated_output {
                                    tool_result.data = updated;
                                }
                                Ok(tool_result)
                            }
                        }
                        Err(e) => {
                            let _ = handle
                                .run_post_tool_use_failure(
                                    &tool_name,
                                    &tool_use_id,
                                    &input,
                                    &e.to_string(),
                                )
                                .await;
                            Err(e)
                        }
                    }
                } else {
                    exec_result
                };

                // If a shell tool failed, cancel siblings.
                // TS: only Bash errors trigger sibling abort (StreamingToolExecutor.ts:354-363).
                // Shell tools (Bash, PowerShell) can leave the system in an inconsistent
                // state, so we abort siblings. Non-shell tool failures (Read, WebFetch)
                // are isolated and don't affect siblings.
                if result.is_err()
                    && (tool_name.as_str() == ToolName::Bash.as_str()
                        || tool_name.as_str() == ToolName::PowerShell.as_str())
                {
                    sibling_tok.cancel();
                }

                let duration_ms = start.elapsed().as_millis() as i64;

                ToolCallResult {
                    tool_use_id,
                    tool_id,
                    result,
                    duration_ms,
                }
            });

            handles.push((saved_tool_use_id, saved_tool_id, handle));
        }

        // Collect in submission order for determinism
        let mut results = Vec::with_capacity(handles.len());
        for (saved_tool_use_id, saved_tool_id, handle) in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(ToolCallResult {
                        tool_use_id: saved_tool_use_id,
                        tool_id: saved_tool_id,
                        result: Err(ToolError::ExecutionFailed {
                            message: format!("task join error: {e}"),
                            source: None,
                        }),
                        duration_ms: 0,
                    });
                }
            }
        }

        results
    }
}

impl Default for StreamingToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "executor.test.rs"]
mod tests;
