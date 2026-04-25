//! Streaming tool executor — real-time concurrent tool execution.
//!
//! TS: `services/tools/StreamingToolExecutor.ts`
//!
//! Tools execute DURING API streaming, not after. As the API streams
//! tool_use blocks, `add_tool()` queues them immediately. Safe tools
//! start executing while the API is still streaming. Results and
//! progress messages are yielded in real-time.

use coco_config::EnvKey;
use coco_config::env;
use coco_types::ToolId;
use coco_types::ToolName;
use coco_types::ToolResult;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::call_plan::PreparedToolCall;
use crate::call_plan::RunOneRuntime;
use crate::call_plan::ToolCallOutcome;
use crate::call_plan::ToolCallPlan;
use crate::call_plan::ToolSideEffects;
use crate::call_plan::UnstampedToolCallOutcome;
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
    /// Shared app_state write handle — the executor owns the **only
    /// write-capable reference** visible from the tool pipeline.
    /// Tools see `ctx.app_state` as an `AppStateReadHandle` (no
    /// write method); they return mutations via
    /// `ToolResult::app_state_patch` which we apply here, under a
    /// single write lock per batch. TS parity:
    /// `orchestration.ts:queuedContextModifiers` applied after the
    /// concurrent batch finishes.
    app_state: Option<Arc<RwLock<coco_types::ToolAppState>>>,
    /// Optional protocol-event sink used to broadcast
    /// `TaskPanelChanged` after every applied `app_state_patch`. Keeps
    /// the TUI in sync with V2 plan-item and V1 todo snapshots.
    event_tx: Option<mpsc::Sender<coco_types::CoreEvent>>,
}

impl StreamingToolExecutor {
    pub fn new() -> Self {
        let max_concurrency = env::env_opt(EnvKey::CocoMaxToolUseConcurrency)
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
            app_state: None,
            event_tx: None,
        }
    }

    /// Attach the shared app_state write handle. Must match the Arc
    /// wrapped inside `ToolUseContext.app_state` (a read-only view
    /// of the same store). Without this, patches returned by tools
    /// are silently dropped (the executor has nowhere to apply them).
    pub fn with_app_state(mut self, arc: Arc<RwLock<coco_types::ToolAppState>>) -> Self {
        self.app_state = Some(arc);
        self
    }

    /// Install a protocol-event sink so the executor can emit
    /// `TaskPanelChanged` after applying task-related `app_state_patch`
    /// closures. Optional; omission drops the notifications silently
    /// (tests + SDK-only paths don't need UI refreshes).
    pub fn with_event_sink(mut self, tx: mpsc::Sender<coco_types::CoreEvent>) -> Self {
        self.event_tx = Some(tx);
        self
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
            app_state: None,
            event_tx: None,
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

    /// Execute a single tool call and apply any resulting app-state patch.
    ///
    /// Hook and permission lifecycle decisions are owned by the query layer.
    /// The executor only schedules and runs already-approved calls.
    async fn execute_single(&self, call: PendingToolCall, ctx: &ToolUseContext) -> ToolCallResult {
        let tool_id = call.tool.id();
        let tool_use_id = call.tool_use_id.clone();
        let start = std::time::Instant::now();
        let input = call.input;

        // Track in-progress
        {
            let mut ids = ctx.in_progress_tool_use_ids.write().await;
            ids.insert(tool_use_id.clone());
        }

        let result = tokio::select! {
            r = call.tool.execute(input, ctx) => r,
            () = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
        };

        // Remove from in-progress
        {
            let mut ids = ctx.in_progress_tool_use_ids.write().await;
            ids.remove(&tool_use_id);
        }

        // Apply queued app_state patch (if any) under a write lock
        // before returning. This is the serial-tool equivalent of
        // TS's "`currentContext = update.newContext(currentContext)`"
        // reassign-between-tools pattern — because serial unsafe
        // tools run one-per-batch, we can apply immediately and the
        // next batch's `create_tool_context` sees the update.
        // Patches also get stripped from the ToolCallResult so the
        // engine's downstream code (which flows the result across
        // `.await` points) doesn't need `Sync` on `AppStatePatch`.
        let result = match result {
            Ok(mut tr) => {
                if let Some(patch) = tr.app_state_patch.take()
                    && let Some(state) = self.app_state.as_ref()
                {
                    let snapshot = {
                        let mut guard = state.write().await;
                        patch(&mut guard);
                        coco_types::TaskPanelChangedParams {
                            plan_tasks: guard.plan_tasks.clone(),
                            todos_by_agent: guard.todos_by_agent.clone(),
                            expanded_view: guard.expanded_view,
                            verification_nudge_pending: guard.verification_nudge_pending,
                        }
                    };
                    if let Some(tx) = self.event_tx.as_ref() {
                        let _ = tx
                            .send(coco_types::CoreEvent::Protocol(
                                coco_types::ServerNotification::TaskPanelChanged(snapshot),
                            ))
                            .await;
                    }
                }
                // No patch or no shared state wired: nothing to emit.
                Ok(tr)
            }
            Err(e) => Err(e),
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
    ///
    /// **app_state invariant**: tools that return
    /// `is_concurrency_safe == true` MUST NOT write `ctx.app_state`
    /// during `execute` — see the `Tool::is_concurrency_safe`
    /// contract. `shared_ctx` is an `Arc<ToolUseContext>` holding the
    /// SAME `Arc<RwLock<ToolAppState>>` as every sibling, so any write
    /// would be visible mid-batch to the others (vs. TS which queues
    /// writes until batch-end). Rust relies on convention: concurrent
    /// tools are read-only (Read/Glob/Grep/LSP). Serial unsafe tools
    /// — the only writers — never hit this path.
    async fn execute_concurrent(
        &self,
        calls: Vec<PendingToolCall>,
        ctx: &ToolUseContext,
    ) -> Vec<ToolCallResult> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let shared_ctx = Arc::new(ctx.clone_for_concurrent());
        let sibling_cancel = CancellationToken::new();
        let call_count = calls.len();
        let model_order_tool_use_ids: Vec<String> =
            calls.iter().map(|call| call.tool_use_id.clone()).collect();
        let mut handles = FuturesUnordered::new();

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

                // Track in-progress
                {
                    let mut ids = ctx_clone.in_progress_tool_use_ids.write().await;
                    ids.insert(tool_use_id.clone());
                }

                let result = tokio::select! {
                    r = tool.execute(input, &ctx_clone) => r,
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

            handles.push(async move { (saved_tool_use_id, saved_tool_id, handle.await) });
        }

        // Collect in completion order. Shared app-state patches are still
        // applied after the batch; see the patch block below.
        let mut results = Vec::with_capacity(call_count);
        while let Some((saved_tool_use_id, saved_tool_id, joined)) = handles.next().await {
            match joined {
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

        // Apply any queued app_state patches in model/submission order
        // under a single write lock. Concurrent tools by convention
        // don't return patches (they're read-only), but the plumbing
        // exists uniformly so Tool authors don't have to special-case
        // which path their tool is on. TS parity:
        // `orchestration.ts:queuedContextModifiers` applied after the
        // concurrent batch finishes — exact same timing + ordering.
        //
        // Patches also get stripped here so the returned
        // `ToolCallResult` values don't carry a `FnOnce` across
        // `.await` points in the engine (which would require `Sync`).
        if let Some(state) = self.app_state.as_ref() {
            let any_patch = results
                .iter()
                .any(|r| matches!(&r.result, Ok(tr) if tr.app_state_patch.is_some()));
            if any_patch {
                let snapshot = {
                    let mut guard = state.write().await;
                    for tool_use_id in &model_order_tool_use_ids {
                        if let Some(r) = results
                            .iter_mut()
                            .find(|r| r.tool_use_id.as_str() == tool_use_id)
                            && let Ok(tr) = r.result.as_mut()
                            && let Some(patch) = tr.app_state_patch.take()
                        {
                            patch(&mut guard);
                        }
                    }
                    coco_types::TaskPanelChangedParams {
                        plan_tasks: guard.plan_tasks.clone(),
                        todos_by_agent: guard.todos_by_agent.clone(),
                        expanded_view: guard.expanded_view,
                        verification_nudge_pending: guard.verification_nudge_pending,
                    }
                };
                if let Some(tx) = self.event_tx.as_ref() {
                    let _ = tx
                        .send(coco_types::CoreEvent::Protocol(
                            coco_types::ServerNotification::TaskPanelChanged(snapshot),
                        ))
                        .await;
                }
            }
        } else {
            // No shared state → drop any patches silently to strip
            // `FnOnce` from the result for Sync-safety downstream.
            for r in results.iter_mut() {
                if let Ok(tr) = r.result.as_mut() {
                    tr.app_state_patch = None;
                }
            }
        }

        results
    }

    // -- Phase 4d Scheduler API (plans + callback-driven surfacing) --
    //
    // `execute_with` is the TS-parity scheduler that the refactor plan
    // calls for: the runner hands in pre-validated `ToolCallPlan`
    // values and a `run_one` callback, and the executor surfaces each
    // outcome through `on_outcome` the moment it is ready. No
    // pre-allocated result-slot vector — history grows in completion
    // order for concurrent-safe batches, execution order for serial
    // unsafe tools, and partition order for `EarlyOutcome` barriers.
    //
    // This coexists with the legacy `execute_all` / `execute_batch`
    // API. The engine migrates call-site by call-site; the legacy
    // path stays until every caller has been ported.

    /// Drive a plan list and surface each outcome through `on_outcome`
    /// as soon as it is available.
    ///
    /// Ordering contract (I12):
    ///
    /// - `ToolCallPlan::EarlyOutcome` acts as a single-tool barrier.
    ///   It splits the surrounding `Runnable` plans into separate
    ///   concurrent-safe batches (TS parity:
    ///   `toolOrchestration.ts:91-115` where schema-invalid calls have
    ///   `isConcurrencySafe = false`).
    /// - Within a concurrent-safe batch, runnable plans dispatch
    ///   through a `FuturesUnordered` so a slow earlier tool does not
    ///   block a faster later tool. The executor stamps
    ///   `completion_seq` at surface time and calls `on_outcome`
    ///   immediately with the patch-free `ToolCallOutcome`.
    /// - Within a concurrent-safe batch, queued `app_state_patch`es
    ///   apply post-batch in **model_index** order under one write
    ///   lock (TS `toolOrchestration.ts:54-62`).
    /// - Serial unsafe plans apply their patch before building the
    ///   next tool's context (TS `toolOrchestration.ts:130-141`).
    /// - `EarlyOutcome` plans stamp when the partitioner reaches that
    ///   plan's block — not globally before all Runnables — so the
    ///   resulting completion sequence interleaves correctly with
    ///   surrounding batches.
    ///
    /// This does **not** emit `ToolUseStarted` / `ToolUseCompleted`
    /// yet — Phase 4d wires event emission to the runner boundary once
    /// `ToolCallRunner::run_one` is the sole semantic lifecycle owner.
    /// Today the engine still owns those events for the legacy path.
    pub async fn execute_with<F, Fut, H>(
        &self,
        plans: Vec<ToolCallPlan>,
        run_one: F,
        mut on_outcome: H,
    ) where
        F: Fn(PreparedToolCall, RunOneRuntime) -> Fut + Sync,
        Fut: std::future::Future<Output = UnstampedToolCallOutcome> + Send,
        H: FnMut(ToolCallOutcome),
    {
        let mut completion_seq: usize = 0;

        // Partition plans into blocks. Each Runnable sequence of
        // concurrency-safe tools becomes one batch; an EarlyOutcome or
        // unsafe Runnable breaks the batch and forms its own block.
        let blocks = partition_plans(plans);

        for block in blocks {
            match block {
                PlanBlock::EarlyOutcome(unstamped) => {
                    // Pre-execution failure: no scheduling, no patch
                    // — but still emit one Completed outcome so the
                    // per-call invariant (one Queued → one Completed)
                    // holds.
                    let (outcome, _effects) = unstamped.stamp_and_extract_effects(completion_seq);
                    completion_seq += 1;
                    on_outcome(outcome);
                }
                PlanBlock::SerialUnsafe(prepared) => {
                    let runtime = self.make_runtime(prepared.model_index);
                    let unstamped = run_one(prepared, runtime).await;
                    let (outcome, effects) = unstamped.stamp_and_extract_effects(completion_seq);
                    completion_seq += 1;
                    // Apply patch BEFORE the next tool's context build
                    // (TS `toolOrchestration.ts:130-141`). This is the
                    // serial-tool equivalent of "update.newContext()".
                    self.apply_side_effects(effects).await;
                    on_outcome(outcome);
                }
                PlanBlock::ConcurrentSafe(prepared_calls) => {
                    self.run_concurrent_batch(
                        prepared_calls,
                        &run_one,
                        &mut on_outcome,
                        &mut completion_seq,
                    )
                    .await;
                }
            }
        }
    }

    /// Build a fresh per-tool runtime for one `run_one` invocation.
    pub(crate) fn make_runtime(&self, model_index: usize) -> RunOneRuntime {
        RunOneRuntime {
            // Child token of the turn cancel (the caller seeds
            // `self.sibling_cancel` to match the current turn). Using
            // a child keeps per-tool cancellation independent of
            // siblings unless sibling-abort explicitly fires.
            cancellation: self.sibling_cancel.child_token(),
            sibling_abort: Some(self.sibling_cancel.clone()),
            progress_tx: None,
            model_index,
        }
    }

    /// Run one concurrent-safe batch end-to-end.
    ///
    /// Surfaces each outcome through `on_outcome` the moment
    /// `run_one` resolves (completion-order history). Queues
    /// `app_state_patch`es keyed by `model_index` and applies them in
    /// model-index order post-batch.
    pub(crate) async fn run_concurrent_batch<F, Fut, H>(
        &self,
        prepared_calls: Vec<PreparedToolCall>,
        run_one: &F,
        on_outcome: &mut H,
        completion_seq: &mut usize,
    ) where
        F: Fn(PreparedToolCall, RunOneRuntime) -> Fut + Sync,
        Fut: std::future::Future<Output = UnstampedToolCallOutcome> + Send,
        H: FnMut(ToolCallOutcome),
    {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut pending: FuturesUnordered<_> = FuturesUnordered::new();

        for prepared in prepared_calls {
            let runtime = self.make_runtime(prepared.model_index);
            let sem = semaphore.clone();
            let fut = run_one(prepared, runtime);
            pending.push(async move {
                // Semaphore acquisition bounds the concurrent tool
                // count but does not block the driver future from
                // progressing — a permit holder returns the permit
                // when it drops.
                let _permit = sem.acquire().await.ok();
                fut.await
            });
        }

        let mut queued_effects: Vec<(usize, ToolSideEffects)> = Vec::new();

        while let Some(unstamped) = pending.next().await {
            let model_index = unstamped.model_index;
            let (outcome, effects) = unstamped.stamp_and_extract_effects(*completion_seq);
            *completion_seq += 1;
            queued_effects.push((model_index, effects));
            on_outcome(outcome);
        }

        // Apply queued patches in model_index order under one write
        // lock — TS `toolOrchestration.ts:54-62`. This mirrors the
        // legacy `execute_concurrent` post-batch apply but keys on
        // `model_index` rather than tool_use_id.
        queued_effects.sort_by_key(|(idx, _)| *idx);
        let combined = ToolSideEffects {
            app_state_patch: coalesce_patches(
                queued_effects
                    .into_iter()
                    .filter_map(|(_, e)| e.app_state_patch),
            ),
        };
        self.apply_side_effects(combined).await;
    }

    /// Apply a `ToolSideEffects` under one write lock, emitting a
    /// `TaskPanelChanged` snapshot to the event sink when a patch
    /// actually ran. Matches the existing legacy-path invariants:
    /// patch `FnOnce` runs exactly once, event is best-effort
    /// delivery (dropped if no sink is configured).
    pub(crate) async fn apply_side_effects(&self, effects: ToolSideEffects) {
        let Some(patch) = effects.app_state_patch else {
            return;
        };
        let Some(state) = self.app_state.as_ref() else {
            // No shared state → drop the patch. TS parity: the
            // context modifier is never invoked when there's no
            // context to modify.
            return;
        };
        let snapshot = {
            let mut guard = state.write().await;
            patch(&mut guard);
            coco_types::TaskPanelChangedParams {
                plan_tasks: guard.plan_tasks.clone(),
                todos_by_agent: guard.todos_by_agent.clone(),
                expanded_view: guard.expanded_view,
                verification_nudge_pending: guard.verification_nudge_pending,
            }
        };
        if let Some(tx) = self.event_tx.as_ref() {
            let _ = tx
                .send(coco_types::CoreEvent::Protocol(
                    coco_types::ServerNotification::TaskPanelChanged(snapshot),
                ))
                .await;
        }
    }
}

/// One block in the executor's plan-partition output.
///
/// `ConcurrentSafe` holds one-or-more `Runnable` plans that can run
/// in parallel; `SerialUnsafe` holds a single non-concurrency-safe
/// `Runnable`; `EarlyOutcome` passes a pre-built outcome straight to
/// the stamp path. TS parity: `toolOrchestration.ts:91-115`
/// `partitionToolCalls` returns the same shape.
enum PlanBlock {
    ConcurrentSafe(Vec<PreparedToolCall>),
    SerialUnsafe(PreparedToolCall),
    EarlyOutcome(UnstampedToolCallOutcome),
}

/// Partition a flat plan list into batches.
///
/// Rules (TS parity):
///
/// - `EarlyOutcome` is never concurrency-safe — it ends the preceding
///   safe batch and forms its own block.
/// - `Runnable` whose `is_concurrency_safe(input)` returns `false`
///   forms its own `SerialUnsafe` block.
/// - `Runnable` whose `is_concurrency_safe(input)` returns `true`
///   accumulates into a `ConcurrentSafe` batch, flushed whenever an
///   `EarlyOutcome` or unsafe Runnable interrupts.
/// - If `is_concurrency_safe` panics (defensive), treat as unsafe.
fn partition_plans(plans: Vec<ToolCallPlan>) -> Vec<PlanBlock> {
    let mut blocks: Vec<PlanBlock> = Vec::new();
    let mut safe_batch: Vec<PreparedToolCall> = Vec::new();
    let flush_safe = |safe_batch: &mut Vec<PreparedToolCall>, blocks: &mut Vec<PlanBlock>| {
        if !safe_batch.is_empty() {
            blocks.push(PlanBlock::ConcurrentSafe(std::mem::take(safe_batch)));
        }
    };
    for plan in plans {
        match plan {
            ToolCallPlan::Runnable(prepared) => {
                let is_safe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepared.tool.is_concurrency_safe(&prepared.parsed_input)
                }))
                .unwrap_or(false);
                if is_safe {
                    safe_batch.push(prepared);
                } else {
                    flush_safe(&mut safe_batch, &mut blocks);
                    blocks.push(PlanBlock::SerialUnsafe(prepared));
                }
            }
            ToolCallPlan::EarlyOutcome(unstamped) => {
                flush_safe(&mut safe_batch, &mut blocks);
                blocks.push(PlanBlock::EarlyOutcome(unstamped));
            }
        }
    }
    flush_safe(&mut safe_batch, &mut blocks);
    blocks
}

/// Coalesce several `FnOnce` patches into a single one that invokes
/// each in turn. Used so the end-of-batch apply is still exactly one
/// `write().await` + one TaskPanelChanged snapshot, regardless of how
/// many tools in the batch emitted patches.
pub(crate) fn coalesce_patches(
    patches: impl IntoIterator<Item = coco_types::AppStatePatch>,
) -> Option<coco_types::AppStatePatch> {
    let patches: Vec<coco_types::AppStatePatch> = patches.into_iter().collect();
    if patches.is_empty() {
        return None;
    }
    Some(Box::new(move |state| {
        for patch in patches {
            patch(state);
        }
    }))
}

impl Default for StreamingToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "executor.test.rs"]
mod tests;
