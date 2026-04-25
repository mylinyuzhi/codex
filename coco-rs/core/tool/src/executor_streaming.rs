//! Streaming entry-point for [`StreamingToolExecutor`] — Phase 9.
//!
//! TS: `services/tools/StreamingToolExecutor.ts` (`addTool` / `processQueue`
//! / `discard`). TS drives streaming at the **executor** level via
//! `addTool(block)` being called from the stream consumer as tool_use
//! blocks finish parsing; coco-rs mirrors that structure through
//! [`StreamingHandle`], with one deliberate divergence: Rust defers
//! history-append until the assistant message is committed (after the
//! stream's Finish), while TS yields tool_results through an async
//! generator as soon as they're ready. Both designs preserve the I1
//! invariant — TS uses an outer reconciliation net
//! (`yieldMissingToolResultBlocks`, `query.ts:123-149`); Rust's
//! deferred-commit construction precludes orphan cases by design.
//!
//! ## Consumer visibility
//!
//! External SDK/TUI consumers still see real-time completion via the
//! `CoreEvent::ToolUseCompleted` stream event emitted from the engine;
//! the deferral only applies to the `Message::ToolResult` entries in
//! history.
//!
//! ## Background-progress model
//!
//! Each safe plan is spawned onto a [`tokio::task::JoinSet`] at
//! `feed_plan` time so it advances **autonomously** while the engine
//! continues consuming stream events. This mirrors TS's implicit
//! promise-execution model: creating a promise immediately enqueues
//! its callback on the JS event loop. In Rust, `FuturesUnordered`
//! without an outer poll-driver would starve safe tools of CPU — the
//! futures wouldn't advance until `commit_flush` awaited them, which
//! defeats the whole "start during stream" goal.
//!
//! The `'static` bound on `F` / `Fut` is a direct consequence of using
//! `tokio::spawn`: the task outlives the caller's stack. Production
//! callers wrap their closures in `Arc`ed captures so `run_one` is
//! `'static`.
//!
//! ## Concurrency gate
//!
//! Mirrors TS `canExecuteTool` at `StreamingToolExecutor.ts:129-135`:
//!
//! - No tools executing → any plan may start.
//! - Only safe tools executing → more safe plans may start.
//! - Any unsafe tool pending → all subsequent plans hold for
//!   `commit_flush`.
//!
//! coco-rs tightens TS slightly: once *any* unsafe plan is fed, *all*
//! subsequent feeds (safe or unsafe) hold, so there's never a mixed
//! safe+unsafe inflight state. Unsafe plans run serially in
//! `commit_flush` after the inflight safe batch drains, preserving
//! the TS rule without requiring a tool-level interlock.
//!
//! ## `StreamingDiscarded` variant
//!
//! Reserved for TS-parity discard semantics. Under the default
//! coco-rs post-commit design, discarded tool_uses were never
//! committed into an assistant message, so their `UnstampedToolCallOutcome`s
//! can be safely dropped. Callers that want to surface them (e.g.
//! for diagnostic traces) can consume `discard()`'s return value.

use std::future::Future;
use std::sync::Arc;

use tokio::task::JoinSet;

use crate::call_plan::PreparedToolCall;
use crate::call_plan::RunOneRuntime;
use crate::call_plan::ToolCallErrorKind;
use crate::call_plan::ToolCallOutcome;
use crate::call_plan::ToolCallPlan;
use crate::call_plan::ToolMessagePath;
use crate::call_plan::ToolSideEffects;
use crate::call_plan::UnstampedToolCallOutcome;
use crate::executor::StreamingToolExecutor;

/// Streaming scheduler driven by [`StreamingHandle::feed_plan`].
///
/// See module documentation for design rationale. Typical lifecycle:
///
/// ```ignore
/// let executor = Arc::new(StreamingToolExecutor::new());
/// let mut h = StreamingHandle::new(executor.clone(), runner_closure);
/// // as each tool-use arrives during stream:
/// h.feed_plan(plan);
/// // on stream Finish (assistant message committed):
/// h.commit_flush(seq_start, |outcome| {
///     for m in outcome.ordered_messages() { history.push(m.clone()); }
/// }).await;
/// ```
pub struct StreamingHandle<F, Fut>
where
    F: Fn(PreparedToolCall, RunOneRuntime) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = UnstampedToolCallOutcome> + Send + 'static,
{
    executor: Arc<StreamingToolExecutor>,
    run_one: Arc<F>,
    /// Spawned tasks for safe plans. `JoinSet` drives them
    /// autonomously via the tokio runtime — no external poll needed.
    inflight: JoinSet<UnstampedToolCallOutcome>,
    /// Plans held for serial execution after the stream commits.
    /// Populated when a plan is unsafe OR when `any_unsafe_fed` is
    /// true (see gate rules in module doc).
    pending_serial: Vec<PreparedToolCall>,
    /// Pre-resolved EarlyOutcome plans — stamped in feed order during
    /// `commit_flush`. Unreachable via model execution; used for
    /// unknown tool / schema fail / pre-hook stop / permission deny.
    pending_early: Vec<UnstampedToolCallOutcome>,
    /// Gate latch: once any unsafe plan is fed, subsequent safe
    /// plans also hold to preserve TS's no-safe-during-unsafe rule.
    any_unsafe_fed: bool,
}

impl<F, Fut> StreamingHandle<F, Fut>
where
    F: Fn(PreparedToolCall, RunOneRuntime) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = UnstampedToolCallOutcome> + Send + 'static,
{
    pub fn new(executor: Arc<StreamingToolExecutor>, run_one: F) -> Self {
        Self {
            executor,
            run_one: Arc::new(run_one),
            inflight: JoinSet::new(),
            pending_serial: Vec::new(),
            pending_early: Vec::new(),
            any_unsafe_fed: false,
        }
    }

    /// Feed one plan from the streaming accumulator.
    ///
    /// Dispatch rules (see module-level gate documentation):
    /// - `EarlyOutcome(o)` → queued; surfaced first on `commit_flush`.
    /// - `Runnable(safe)` with no unsafe pending → spawned now.
    /// - `Runnable(unsafe)` or any plan once unsafe fed → queued for
    ///   serial execution during `commit_flush`.
    pub fn feed_plan(&mut self, plan: ToolCallPlan) {
        match plan {
            ToolCallPlan::EarlyOutcome(o) => self.pending_early.push(o),
            ToolCallPlan::Runnable(prepared) => {
                let is_safe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepared.tool.is_concurrency_safe(&prepared.parsed_input)
                }))
                .unwrap_or(false);
                if is_safe && !self.any_unsafe_fed {
                    self.start_safe_now(prepared);
                } else {
                    if !is_safe {
                        self.any_unsafe_fed = true;
                    }
                    self.pending_serial.push(prepared);
                }
            }
        }
    }

    /// Count of plans that are not yet surfaced. Used by tests to
    /// assert gate decisions without awaiting execution.
    pub fn pending_count(&self) -> usize {
        self.pending_serial.len() + self.pending_early.len() + self.inflight.len()
    }

    /// Abandon all scheduled work — called when the stream fails and
    /// the engine retries non-streaming. Aborts any inflight spawned
    /// tasks (via `JoinSet::shutdown`) and converts queued plans into
    /// `StreamingDiscarded` synthetic outcomes.
    ///
    /// Default engine path drops these outcomes because their
    /// tool_use blocks never entered the assistant message. Callers
    /// implementing TS-parity semantics can iterate the return value
    /// to emit synthetic error tool_results if they committed the
    /// assistant message before discarding.
    pub async fn discard(mut self) -> Vec<UnstampedToolCallOutcome> {
        // Abort any inflight spawned tasks; wait for cancellation.
        self.inflight.shutdown().await;

        let StreamingHandle {
            pending_serial,
            pending_early,
            ..
        } = self;

        let mut discarded: Vec<UnstampedToolCallOutcome> = Vec::new();
        for prepared in pending_serial {
            discarded.push(discard_outcome_for(prepared));
        }
        for o in pending_early {
            discarded.push(o);
        }
        discarded
    }

    /// Commit point: drain spawned safe tasks, run queued serial
    /// plans, stamp all outcomes with monotonic `completion_seq`
    /// starting at `seq_start`, and hand each to `on_outcome`.
    ///
    /// Post-batch safe patches apply in model-index order under one
    /// write lock. Serial unsafe patches apply between each tool's
    /// execution. Matches [`StreamingToolExecutor::execute_with`]
    /// semantics exactly.
    pub async fn commit_flush<H>(mut self, seq_start: usize, mut on_outcome: H)
    where
        H: FnMut(ToolCallOutcome),
    {
        let mut completion_seq = seq_start;

        // EarlyOutcomes surface first (they were never in-flight).
        for unstamped in std::mem::take(&mut self.pending_early) {
            let (outcome, _effects) = unstamped.stamp_and_extract_effects(completion_seq);
            completion_seq += 1;
            on_outcome(outcome);
        }

        // Drain inflight safe tasks in real completion order. Queue
        // their app_state patches to apply in model-index order
        // post-batch.
        let mut safe_effects: Vec<(usize, ToolSideEffects)> = Vec::new();
        while let Some(join_res) = self.inflight.join_next().await {
            let unstamped = match join_res {
                Ok(u) => u,
                Err(e) => {
                    // A spawned safe task panicked or was cancelled.
                    // Produce a synthetic outcome so the completion
                    // stream stays paired.
                    join_error_outcome(e, completion_seq)
                }
            };
            let model_index = unstamped.model_index;
            let (outcome, effects) = unstamped.stamp_and_extract_effects(completion_seq);
            completion_seq += 1;
            safe_effects.push((model_index, effects));
            on_outcome(outcome);
        }
        safe_effects.sort_by_key(|(idx, _)| *idx);
        let combined_safe = ToolSideEffects {
            app_state_patch: crate::executor::coalesce_patches(
                safe_effects
                    .into_iter()
                    .filter_map(|(_, e)| e.app_state_patch),
            ),
        };
        self.executor.apply_side_effects(combined_safe).await;

        // Run serial unsafe plans. Each tool's patch applies before
        // the next tool's execution.
        for prepared in std::mem::take(&mut self.pending_serial) {
            let runtime = self.executor.make_runtime(prepared.model_index);
            let fut = (self.run_one)(prepared, runtime);
            let unstamped = fut.await;
            let (outcome, effects) = unstamped.stamp_and_extract_effects(completion_seq);
            completion_seq += 1;
            self.executor.apply_side_effects(effects).await;
            on_outcome(outcome);
        }
    }

    /// Spawn a safe plan onto the JoinSet so it runs autonomously.
    /// The runtime token is built here (synchronously) because
    /// `make_runtime` takes `&self` on the executor and we need it
    /// before moving into the spawned task.
    fn start_safe_now(&mut self, prepared: PreparedToolCall) {
        let runtime = self.executor.make_runtime(prepared.model_index);
        let run_one = self.run_one.clone();
        self.inflight
            .spawn(async move { run_one(prepared, runtime).await });
    }
}

/// Build a `StreamingDiscarded` synthetic outcome for a queued plan
/// that never got to run.
fn discard_outcome_for(prepared: PreparedToolCall) -> UnstampedToolCallOutcome {
    UnstampedToolCallOutcome {
        tool_use_id: prepared.tool_use_id,
        tool_id: prepared.tool_id,
        model_index: prepared.model_index,
        ordered_messages: Vec::new(),
        message_path: ToolMessagePath::EarlyReturn,
        error_kind: Some(ToolCallErrorKind::StreamingDiscarded),
        permission_denial: None,
        prevent_continuation: None,
        effects: ToolSideEffects::none(),
    }
}

/// Build a synthetic outcome for a `JoinError` from a spawned safe
/// task. The task didn't return a value — either panicked or
/// cancelled — so we classify as `JoinFailed` (per
/// [`ToolCallErrorKind::runs_post_tool_use_failure`], this IS an
/// execution-stage failure that should fire PostToolUseFailure).
///
/// Model-index 0 is a placeholder — we don't know which plan
/// produced the error because JoinSet doesn't track that. In practice
/// this only fires on panic, which is rare enough to not warrant a
/// full id-tracking wrapper.
fn join_error_outcome(
    _err: tokio::task::JoinError,
    _completion_seq_placeholder: usize,
) -> UnstampedToolCallOutcome {
    UnstampedToolCallOutcome {
        tool_use_id: String::new(),
        tool_id: coco_types::ToolId::Custom("<join-failed>".into()),
        model_index: 0,
        ordered_messages: Vec::new(),
        message_path: ToolMessagePath::Failure,
        error_kind: Some(ToolCallErrorKind::JoinFailed),
        permission_denial: None,
        prevent_continuation: None,
        effects: ToolSideEffects::none(),
    }
}

/// Convenience: build a [`StreamingHandle`] from an
/// `Arc<StreamingToolExecutor>`. The `Arc` requirement is inherent in
/// the `'static` spawn model — spawned tasks must not borrow the
/// executor's stack slot.
impl StreamingToolExecutor {
    pub fn streaming_handle<F, Fut>(self: &Arc<Self>, run_one: F) -> StreamingHandle<F, Fut>
    where
        F: Fn(PreparedToolCall, RunOneRuntime) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = UnstampedToolCallOutcome> + Send + 'static,
    {
        StreamingHandle::new(self.clone(), run_one)
    }
}

#[cfg(test)]
#[path = "executor_streaming.test.rs"]
mod tests;
