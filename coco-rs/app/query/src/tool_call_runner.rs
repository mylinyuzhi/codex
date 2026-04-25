//! `ToolCallRunner` — owns the tool-call lifecycle for a single
//! assistant batch.
//!
//! Phase 4d-β rewires this to drive
//! [`coco_tool::StreamingToolExecutor::execute_with`] so outcomes
//! surface through `on_outcome` in TS-parity I12 order:
//!
//! - Concurrent-safe batches surface in completion order.
//! - Serial unsafe tools surface in execution order.
//! - `EarlyOutcome` barriers (unknown tool / schema / permission /
//!   pre-hook block) land in partition order, never globally first.
//!
//! The per-tool semantic lifecycle is split across:
//!
//! 1. `tool_call_preparer::prepare_pending_tool_calls` — pre-hook,
//!    input rewrite, re-validate, permission (including auto-mode
//!    classifier + bridge).
//! 2. `run_one` (here) — `tool.execute` + PostToolUse /
//!    PostToolUseFailure hooks, via
//!    [`tool_outcome_builder::build_outcome_from_execution`].
//!
//! `on_outcome` then appends the pre-flattened `ordered_messages` to
//! history and surfaces `permission_denial` / `prevent_continuation`
//! back to the engine.

use std::sync::Arc;

use coco_hooks::HookExecutionEvent;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::ApiClient;
use coco_messages::MessageHistory;
use coco_permissions::AutoModeRules;
use coco_tool::PreparedToolCall;
use coco_tool::StreamingToolExecutor;
use coco_tool::ToolCallPlan;
use coco_tool::ToolError;
use coco_tool::ToolPermissionBridgeRef;
use coco_tool::ToolRegistry;
use coco_tool::ToolUseContext;
use coco_types::CoreEvent;
use coco_types::PermissionDenialInfo;
use coco_types::ToolAppState;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use vercel_ai_provider::ToolCallPart;

use crate::emit::emit_stream;
use crate::session_state::SessionStateTracker;
use crate::tool_call_preparer::PendingToolPreparation;
use crate::tool_call_preparer::prepare_pending_tool_calls;
use crate::tool_outcome_builder::RunOneTail;
use crate::tool_outcome_builder::build_outcome_from_execution;

#[derive(Debug, Default)]
pub(crate) struct ToolCallRunOutcome {
    pub continue_after_tools: bool,
    pub stop_reason_override: Option<String>,
}

pub(crate) struct ToolCallRunner<'a> {
    pub event_tx: &'a Option<mpsc::Sender<CoreEvent>>,
    pub history: &'a mut MessageHistory,
    pub ctx: &'a ToolUseContext,
    pub tool_calls: &'a [ToolCallPart],
    pub turn: i32,
    pub tools: &'a ToolRegistry,
    pub hooks: Option<&'a Arc<HookRegistry>>,
    pub orchestration_ctx: OrchestrationContext,
    pub hook_tx_opt: Option<&'a mpsc::Sender<HookExecutionEvent>>,
    pub permission_denials: &'a mut Vec<PermissionDenialInfo>,
    pub state_tracker: &'a SessionStateTracker,
    pub permission_bridge: Option<&'a ToolPermissionBridgeRef>,
    pub session_id: &'a str,
    pub cancel: &'a CancellationToken,
    pub auto_mode_state: Option<&'a Arc<coco_permissions::AutoModeState>>,
    pub denial_tracker: Option<&'a Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>>,
    pub client: &'a Arc<ApiClient>,
    pub auto_mode_rules: &'a AutoModeRules,
    pub app_state: Option<&'a Arc<RwLock<ToolAppState>>>,
}

impl<'a> ToolCallRunner<'a> {
    pub(crate) async fn run(self) -> ToolCallRunOutcome {
        info!(
            turn = self.turn,
            tool_count = self.tool_calls.len(),
            "executing tool calls"
        );

        // 1. Per-tool preparation (pre-hook + permission + re-validate).
        //    `prepare_pending_tool_calls` emits ToolUseQueued for each
        //    committed tool_use and completes error tool results for
        //    any call that fails preparation (unknown tool / invalid
        //    input / hook block / permission denial).
        let (pending, tool_result_contexts) = prepare_pending_tool_calls(PendingToolPreparation {
            event_tx: self.event_tx,
            history: self.history,
            ctx: self.ctx,
            tool_calls: self.tool_calls,
            tools: self.tools,
            hooks: self.hooks,
            orchestration_ctx: self.orchestration_ctx.clone(),
            hook_tx_opt: self.hook_tx_opt,
            permission_denials: self.permission_denials,
            state_tracker: self.state_tracker,
            permission_bridge: self.permission_bridge,
            session_id: self.session_id,
            cancel: self.cancel,
            auto_mode_state: self.auto_mode_state,
            denial_tracker: self.denial_tracker,
            client: self.client,
            auto_mode_rules: self.auto_mode_rules,
        })
        .await;

        // 2. Build `Vec<ToolCallPlan>` from the pre-validated pending
        //    calls. Every plan here is `Runnable` — calls that failed
        //    preparation have already been completed by the preparer
        //    and do not reach this point.
        //
        //    `model_index` is the call's position in the pending
        //    list, which matches the tool_use position in the
        //    assistant message (the preparer iterates
        //    `self.tool_calls` in model order and skips only
        //    early-completed entries).
        let mut plans: Vec<ToolCallPlan> = Vec::with_capacity(pending.len());
        for (idx, pending_call) in pending.into_iter().enumerate() {
            let tool_id = pending_call.tool.id();
            plans.push(ToolCallPlan::Runnable(PreparedToolCall {
                tool_use_id: pending_call.tool_use_id,
                tool_id,
                tool: pending_call.tool,
                parsed_input: pending_call.input,
                model_index: idx,
            }));
        }

        // 3. Emit ToolUseStarted for each Runnable plan before
        //    dispatching. This keeps the SDK stream shape unchanged
        //    from the legacy path — consumers see Started before
        //    Completed as before.
        for plan in &plans {
            if let ToolCallPlan::Runnable(prepared) = plan {
                let tool_name = tool_result_contexts
                    .get(&prepared.tool_use_id)
                    .map(|c| c.tool_name.clone())
                    .unwrap_or_else(|| prepared.tool.name().to_string());
                let _delivered = emit_stream(
                    self.event_tx,
                    crate::AgentStreamEvent::ToolUseStarted {
                        call_id: prepared.tool_use_id.clone(),
                        name: tool_name,
                        batch_id: None,
                    },
                )
                .await;
            }
        }

        // 4. Drive the scheduler. `run_one` executes the tool +
        //    post-hook and builds `UnstampedToolCallOutcome`;
        //    `on_outcome` appends ordered_messages to history and
        //    records prevent_continuation.
        //
        //    `on_outcome` is `FnMut` (single-threaded, called
        //    synchronously from the executor), so it captures
        //    mutable references directly — no Mutex ceremony, and
        //    no `.unwrap()` for lock acquisition. `run_one` is
        //    `Fn + Sync` (called concurrently via
        //    `FuturesUnordered`), so it only captures immutable
        //    data.
        let executor = create_executor(self.app_state);
        let shared_ctx = self.ctx;
        let hooks = self.hooks;
        let orchestration_ctx = self.orchestration_ctx.clone();
        let hook_tx = self.hook_tx_opt;
        let contexts = &tool_result_contexts;
        let event_tx = self.event_tx;

        let history_ref: &mut MessageHistory = self.history;
        let mut control = Control::default();
        let mut event_log: Vec<PendingCompletedEvent> = Vec::new();

        executor
            .execute_with(
                plans,
                |prepared, _runtime| {
                    let hooks = hooks;
                    let orchestration_ctx = orchestration_ctx.clone();
                    let hook_tx = hook_tx;
                    let contexts = contexts;
                    async move {
                        let ctx_entry = contexts.get(&prepared.tool_use_id);
                        let tool_name = ctx_entry
                            .map(|c| c.tool_name.clone())
                            .unwrap_or_else(|| prepared.tool.name().to_string());
                        let effective_input = ctx_entry
                            .map(|c| c.effective_input.clone())
                            .unwrap_or_else(|| prepared.parsed_input.clone());

                        // Execute the tool under cancellation.
                        let execute_result = tokio::select! {
                            r = prepared.tool.execute(effective_input.clone(), shared_ctx) => r,
                            () = shared_ctx.cancel.cancelled() => Err(ToolError::Cancelled),
                        };

                        build_outcome_from_execution(RunOneTail {
                            tool_use_id: prepared.tool_use_id.clone(),
                            tool_id: prepared.tool_id.clone(),
                            tool_name,
                            model_index: prepared.model_index,
                            tool: prepared.tool,
                            effective_input,
                            execute_result,
                            hooks,
                            orchestration_ctx,
                            hook_tx,
                        })
                        .await
                    }
                },
                |outcome| {
                    let ctx_entry = contexts.get(outcome.tool_use_id()).cloned();
                    let (tool_name, is_error) = match (ctx_entry.as_ref(), outcome.error_kind()) {
                        (Some(c), _) => (c.tool_name.clone(), outcome.error_kind().is_some()),
                        (None, _) => (
                            outcome.tool_id().to_string(),
                            outcome.error_kind().is_some(),
                        ),
                    };

                    // Capture Completed event data for emission after
                    // the executor finishes driving — we can't
                    // `.await` from the sync `on_outcome` callback.
                    let output_text = render_completed_output(&outcome);
                    event_log.push(PendingCompletedEvent {
                        call_id: outcome.tool_use_id().to_string(),
                        tool_name,
                        output: output_text,
                        is_error,
                    });

                    // Update control signals from prevent_continuation.
                    if let Some(reason) = outcome.prevent_continuation() {
                        control.continue_after_tools = false;
                        if control.stop_reason_override.is_none() {
                            control.stop_reason_override = Some(reason.to_string());
                        }
                    }

                    // Append ordered_messages verbatim — the runner
                    // has already flattened per MCP / non-MCP / path
                    // rules.
                    let parts = outcome.into_parts();
                    for msg in parts.ordered_messages {
                        history_ref.push(msg);
                    }
                },
            )
            .await;

        // 5. Drain the Completed event log now that we're outside
        //    the executor's sync on_outcome boundary.
        for event in event_log {
            let _delivered = emit_stream(
                event_tx,
                crate::AgentStreamEvent::ToolUseCompleted {
                    call_id: event.call_id,
                    name: event.tool_name,
                    output: event.output,
                    is_error: event.is_error,
                },
            )
            .await;
        }

        control.into_outcome()
    }
}

/// Render the text payload surfaced in `ToolUseCompleted.output`
/// events so SDK consumers see the same string the legacy path
/// emitted.
///
/// Success paths carry the flattened tool_result text; failure /
/// early-return paths carry the synthetic error message.
fn render_completed_output(outcome: &coco_tool::ToolCallOutcome) -> String {
    // Extract the tool_result body from the first ordered message
    // that is a ToolResult. Matches the legacy processor which
    // serialized the tool output into the event.
    for msg in outcome.ordered_messages() {
        if let coco_types::Message::ToolResult(tr) = msg
            && let coco_types::LlmMessage::Tool { content, .. } = &tr.message
        {
            for part in content {
                if let coco_types::ToolContent::ToolResult(r) = part {
                    match &r.output {
                        vercel_ai_provider::ToolResultContent::Text { value, .. } => {
                            return value.clone();
                        }
                        vercel_ai_provider::ToolResultContent::ErrorText { value, .. } => {
                            return value.clone();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    String::new()
}

#[derive(Debug)]
struct PendingCompletedEvent {
    call_id: String,
    tool_name: String,
    output: String,
    is_error: bool,
}

#[derive(Debug)]
struct Control {
    continue_after_tools: bool,
    stop_reason_override: Option<String>,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            continue_after_tools: true,
            stop_reason_override: None,
        }
    }
}

impl Control {
    fn into_outcome(self) -> ToolCallRunOutcome {
        ToolCallRunOutcome {
            continue_after_tools: self.continue_after_tools,
            stop_reason_override: self.stop_reason_override,
        }
    }
}

fn create_executor(app_state: Option<&Arc<RwLock<ToolAppState>>>) -> StreamingToolExecutor {
    match app_state {
        Some(arc) => StreamingToolExecutor::new().with_app_state(arc.clone()),
        None => StreamingToolExecutor::new(),
    }
}
