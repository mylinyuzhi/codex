use std::collections::HashMap;
use std::sync::Arc;

use coco_hooks::HookExecutionEvent;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_llm_types::ToolCallPart;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_permissions::AutoModeRules;
use coco_tool_runtime::CanUseToolDecision;
use coco_tool_runtime::DecisionReason;
use coco_tool_runtime::PendingToolCall;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::CoreEvent;
use coco_types::PermissionDecision;
use coco_types::PermissionDenialInfo;
use coco_types::ToolId;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::helpers::ToolCompletionEventMode;
use crate::helpers::complete_tool_call_with_error_mode;
use crate::hook_controller::HookController;
use crate::hook_controller::PreToolUseOutcome;
use crate::permission_controller::PermissionController;
use crate::permission_controller::PermissionOutcome;
use crate::session_state::SessionStateTracker;
use crate::tool_runner::prepare_committed_tool_call;

/// Per-call data carried from preparation → run_one → on_outcome.
///
/// Keyed by `tool_use_id`, this lets the runner retrieve the
/// post-hook `tool_name` + effective input that the preparer
/// resolved, without re-deriving them in `run_one`. `is_mcp` is
/// asked directly from the tool via `Tool::is_mcp()` at outcome-
/// build time (single source of truth), so it does not travel
/// through this struct.
#[derive(Debug, Clone)]
pub(crate) struct ToolResultContext {
    pub tool_name: String,
    pub effective_input: Value,
}

pub(crate) struct PendingToolPreparation<'a> {
    pub event_tx: &'a Option<mpsc::Sender<CoreEvent>>,
    pub history: &'a mut MessageHistory,
    pub ctx: &'a ToolUseContext,
    pub tool_calls: &'a [ToolCallPart],
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
    pub completion_event_mode: ToolCompletionEventMode,
}

pub(crate) async fn prepare_pending_tool_calls(
    mut args: PendingToolPreparation<'_>,
) -> (Vec<PendingToolCall>, HashMap<String, ToolResultContext>) {
    let mut pending = Vec::new();
    let mut tool_result_contexts = HashMap::new();

    // Ownership gymnastics: `prepare_one_pending_tool_call` borrows
    // the args struct mutably for per-call state (history +
    // permission_denials). We split the tool_calls slice out first
    // so the inner loop can re-borrow args freely.
    let tool_calls = args.tool_calls;
    for tc in tool_calls {
        if let Some((pending_call, ctx)) = prepare_one_pending_tool_call(&mut args, tc).await {
            tool_result_contexts.insert(tc.tool_call_id.clone(), ctx);
            pending.push(pending_call);
        }
    }

    (pending, tool_result_contexts)
}

/// Run the full per-tool preparation pipeline (validate → pre-hook →
/// input rewrite → re-validate → permission → bridge) against one
/// committed assistant tool_use.
///
/// Returns `Some((PendingToolCall, ToolResultContext))` when the
/// call made it through all gates and is ready for execution;
/// `None` when preparation failed — in which case an error
/// tool_result has already been pushed to history via
/// `complete_tool_call_with_error` and the caller should simply
/// skip the call.
///
/// This is the reusable per-call body used by both:
/// - `prepare_pending_tool_calls` (batch-at-end non-streaming path)
/// - Phase 9 streaming path (to be wired — call once per
///   `ToolCallEnd` event so safe tools can start mid-stream)
///
/// Extracting this makes the streaming integration trivial: the
/// engine's stream consumer calls this function as each tool_use
/// block arrives, feeding the result into `StreamingHandle`. The
/// preparation semantics (pre-hook order, permission path, audit
/// records) stay byte-identical between streaming and
/// non-streaming, which is the plan's I2 invariant.
pub(crate) async fn prepare_one_pending_tool_call(
    args: &mut PendingToolPreparation<'_>,
    tc: &ToolCallPart,
) -> Option<(PendingToolCall, ToolResultContext)> {
    let prepared = prepare_committed_tool_call(
        args.event_tx,
        args.history,
        args.tools,
        args.ctx,
        tc,
        args.completion_event_mode,
    )
    .await?;

    let tool_id = prepared.tool_id;
    let tool = prepared.tool;
    // `tc.input` is already the observable input — both engine paths run
    // `normalize_observable_tool_input` when building this `ToolCallPart`.

    let hook_controller =
        HookController::new(args.hooks, args.orchestration_ctx.clone(), args.hook_tx_opt);
    let pre_tool_outcome = hook_controller
        .run_pre_tool_use(
            args.event_tx,
            args.history,
            tc,
            &tool_id,
            args.completion_event_mode,
        )
        .await;

    let (effective_input, hook_permission_behavior, hook_permission_reason) =
        resolve_effective_input_from_pre_hook(
            args.event_tx,
            args.history,
            args.ctx,
            tc,
            &tool_id,
            &tool,
            pre_tool_outcome,
            args.completion_event_mode,
        )
        .await?;

    let decision = resolve_permission_decision(
        tc,
        &tool,
        &effective_input,
        args.ctx,
        args.history.as_slice(),
        (hook_permission_behavior, hook_permission_reason),
        args.auto_mode_state,
        args.denial_tracker,
        args.client,
        args.auto_mode_rules,
    )
    .await;

    // TS `toolExecution.ts:1075-1101`: when an auto-mode classifier
    // denial lands, fire `PermissionDenied` hooks. If any hook returns
    // `retry: true`, the model is hinted that it may retry. We extend
    // the deny message in-place so the existing controller path stays
    // unchanged.
    let decision =
        maybe_fire_permission_denied_hook(&hook_controller, tc, &effective_input, decision).await;

    let permission_outcome = PermissionController::new(
        args.event_tx,
        args.history,
        args.permission_denials,
        args.state_tracker,
        args.permission_bridge,
        args.session_id,
        args.cancel,
        args.hooks,
        Some(&args.orchestration_ctx),
        args.completion_event_mode,
    )
    .resolve(decision, tc, &effective_input, &tool_id)
    .await;

    let effective_input = resolve_effective_input_from_permission(
        args.event_tx,
        args.history,
        args.ctx,
        tc,
        &tool_id,
        &tool,
        permission_outcome,
        effective_input,
        args.completion_event_mode,
    )
    .await?;

    Some((
        PendingToolCall {
            tool_use_id: tc.tool_call_id.clone(),
            tool: tool.clone(),
            input: effective_input.clone(),
        },
        ToolResultContext {
            tool_name: tc.tool_name.clone(),
            effective_input,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
async fn resolve_effective_input_from_pre_hook(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    pre_tool_outcome: PreToolUseOutcome,
    completion_event_mode: ToolCompletionEventMode,
) -> Option<(
    Value,
    Option<coco_types::PermissionBehavior>,
    Option<String>,
)> {
    match pre_tool_outcome {
        PreToolUseOutcome::Blocked => None,
        PreToolUseOutcome::Continue {
            updated_input,
            permission_behavior,
            reason,
        } => {
            if let Some(updated_input) = updated_input {
                return validate_effective_input_or_complete_error(
                    event_tx,
                    history,
                    ctx,
                    tool_call,
                    tool_id,
                    tool,
                    updated_input,
                    completion_event_mode,
                )
                .await
                .map(|input| (input, permission_behavior, reason));
            }
            Some((tool_call.input.clone(), permission_behavior, reason))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn resolve_permission_decision(
    tool_call: &ToolCallPart,
    tool: &Arc<dyn Tool>,
    effective_input: &Value,
    ctx: &ToolUseContext,
    history_messages: &[Message],
    hook_permission: (Option<coco_types::PermissionBehavior>, Option<String>),
    auto_mode_state: Option<&Arc<coco_permissions::AutoModeState>>,
    denial_tracker: Option<&Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>>,
    client: &Arc<ApiClient>,
    auto_mode_rules: &AutoModeRules,
) -> PermissionDecision {
    let (hook_permission_behavior, hook_permission_reason) = hook_permission;
    let mut hook_permission_behavior = hook_permission_behavior;

    if let Some(gate) =
        resolve_can_use_tool_decision(tool_call, effective_input, ctx, hook_permission_behavior)
            .await
    {
        match gate {
            CanUseToolResolution::Decision(decision) => return decision,
            CanUseToolResolution::Ask => {
                if matches!(
                    hook_permission_behavior,
                    Some(coco_types::PermissionBehavior::Allow)
                ) && ctx.require_can_use_tool
                {
                    hook_permission_behavior = None;
                }
            }
        }
    }

    let mut decision = match hook_permission_behavior {
        Some(coco_types::PermissionBehavior::Allow) => PermissionDecision::Allow {
            updated_input: None,
            feedback: hook_permission_reason,
        },
        Some(coco_types::PermissionBehavior::Ask) => PermissionDecision::Ask {
            message: hook_permission_reason
                .unwrap_or_else(|| "PreToolUse hook requested approval".into()),
            suggestions: Vec::new(),
            choices: None,
        },
        Some(coco_types::PermissionBehavior::Deny) => PermissionDecision::Deny {
            message: hook_permission_reason
                .clone()
                .unwrap_or_else(|| "PreToolUse hook denied tool execution".into()),
            reason: coco_types::PermissionDecisionReason::Hook {
                hook_name: "PreToolUse".into(),
                reason: hook_permission_reason,
            },
        },
        None => evaluate_with_rules(tool, effective_input, ctx).await,
    };

    // Subagent/fork isolation: prefer `ctx.local_denial_tracking` over the
    // engine-level session tracker. TS parity (`permissions.ts:553-558`):
    //   `context.localDenialTracking ?? appState.denialTracking`.
    // Without this, a fork's denials would bump the parent's
    // consecutive-denial circuit breaker.
    let chosen_tracker: Option<Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>> = ctx
        .local_denial_tracking
        .clone()
        .or_else(|| denial_tracker.cloned());

    if matches!(decision, PermissionDecision::Ask { .. })
        && let (Some(state), Some(tracker)) = (auto_mode_state, chosen_tracker.as_ref())
        && state.is_active()
    {
        let is_read_only = tool.is_read_only(effective_input);
        let mut tracker_guard = tracker.lock().await;
        let classifier_decision = try_classify_in_auto_mode(
            &tool_call.tool_name,
            effective_input,
            is_read_only,
            state,
            &mut tracker_guard,
            history_messages,
            client,
            auto_mode_rules,
        )
        .await;
        drop(tracker_guard);
        if let Some(d) = classifier_decision {
            decision = d;
        }
    }

    decision
}

enum CanUseToolResolution {
    Decision(PermissionDecision),
    Ask,
}

async fn resolve_can_use_tool_decision(
    tool_call: &ToolCallPart,
    effective_input: &Value,
    ctx: &ToolUseContext,
    hook_permission_behavior: Option<coco_types::PermissionBehavior>,
) -> Option<CanUseToolResolution> {
    let should_run = match hook_permission_behavior {
        Some(coco_types::PermissionBehavior::Deny) => false,
        Some(coco_types::PermissionBehavior::Allow) => ctx.require_can_use_tool,
        Some(coco_types::PermissionBehavior::Ask) | None => true,
    };
    if !should_run {
        return None;
    }

    let handle = ctx.can_use_tool.clone()?;
    let cb_ctx = coco_tool_runtime::CanUseToolCallContext {
        tool_use_id: tool_call.tool_call_id.clone(),
        abort: ctx.cancel.clone(),
        require_can_use_tool: ctx.require_can_use_tool,
        messages: ctx.messages.clone(),
    };
    match handle
        .check(&tool_call.tool_name, effective_input, &cb_ctx)
        .await
    {
        CanUseToolDecision::Deny {
            message,
            decision_reason,
        } => {
            tracing::info!(
                tool_use_id = %tool_call.tool_call_id,
                tool_name = %tool_call.tool_name,
                decision_reason = ?decision_reason,
                "fork canUseTool denied call"
            );
            Some(CanUseToolResolution::Decision(PermissionDecision::Deny {
                message,
                reason: coco_types::PermissionDecisionReason::AsyncAgent {
                    reason: can_use_tool_reason_label(&decision_reason),
                },
            }))
        }
        CanUseToolDecision::Allow {
            updated_input,
            decision_reason,
        } => {
            tracing::debug!(
                tool_use_id = %tool_call.tool_call_id,
                tool_name = %tool_call.tool_name,
                decision_reason = ?decision_reason,
                updated = updated_input.is_some(),
                "fork canUseTool allowed call"
            );
            Some(CanUseToolResolution::Decision(PermissionDecision::Allow {
                updated_input,
                feedback: Some(can_use_tool_reason_label(&decision_reason)),
            }))
        }
        CanUseToolDecision::Ask { decision_reason } => {
            tracing::debug!(
                tool_use_id = %tool_call.tool_call_id,
                tool_name = %tool_call.tool_name,
                decision_reason = ?decision_reason,
                "fork canUseTool abstained; falling through"
            );
            Some(CanUseToolResolution::Ask)
        }
    }
}

fn can_use_tool_reason_label(reason: &DecisionReason) -> String {
    match reason {
        DecisionReason::Other { reason } => reason.clone(),
        DecisionReason::RuleAllow { rule_kind } => format!("rule_allow:{rule_kind}"),
        DecisionReason::RuleDeny { rule_kind } => format!("rule_deny:{rule_kind}"),
        DecisionReason::ModeAllow => "mode_allow".into(),
        DecisionReason::UserAccept => "user_accept".into(),
        DecisionReason::UserReject => "user_reject".into(),
        DecisionReason::Speculation { boundary } => format!("speculation:{boundary:?}"),
    }
}

/// Run the central rule evaluator against a tool call.
///
/// TS parity: `hasPermissionsToUseToolInner` in `permissions.ts`.
/// The tool's own opinion (`Tool::check_permissions`) is captured
/// once and supplied as the step-1c slot to
/// [`coco_permissions::PermissionEvaluator::evaluate_with_tool_check`],
/// so the same `ToolCheckResult` passes through deny rules → tool
/// opinion → allow rules → ask rules → path safety → MCP server
/// rules → mode fallthrough exactly as TS does.
///
/// Returning `Allow { updated_input: Some(_) }` from the tool's
/// opinion survives an evaluator-side `Allow` decision — TS keeps
/// `updatedInput` on downstream allows so a tool can normalize
/// input even when a user-allow rule is present.
async fn evaluate_with_rules(
    tool: &Arc<dyn Tool>,
    effective_input: &Value,
    ctx: &ToolUseContext,
) -> PermissionDecision {
    use coco_types::ToolCheckResult;

    let tool_opinion = tool.check_permissions(effective_input, ctx).await;
    let tool_check = move |_id: &ToolId,
                           _input: &Value,
                           _pc: &coco_types::ToolPermissionContext|
          -> ToolCheckResult { tool_opinion.clone() };

    let tool_id = tool.id();
    // `evaluate_with_tool_check` step 1c short-circuits with the
    // tool's own `Allow { updated_input, feedback }` before any rule
    // evaluation, so the returned decision already preserves
    // `updated_input` when the tool returned one. No post-processing
    // needed here.
    coco_permissions::PermissionEvaluator::evaluate_with_tool_check(
        &tool_id,
        effective_input,
        &ctx.permission_context,
        Some(&tool_check),
    )
}

#[allow(clippy::too_many_arguments)]
async fn try_classify_in_auto_mode(
    tool_name: &str,
    input: &Value,
    is_read_only: bool,
    state: &coco_permissions::AutoModeState,
    tracker: &mut coco_permissions::DenialTracker,
    messages: &[Message],
    client: &Arc<ApiClient>,
    auto_mode_rules: &AutoModeRules,
) -> Option<PermissionDecision> {
    let client = Arc::clone(client);
    let classify_fn = move |req: coco_permissions::ClassifyRequest| {
        let client = Arc::clone(&client);
        async move {
            let prompt: coco_llm_types::LlmPrompt = vec![
                coco_llm_types::LlmMessage::System {
                    content: vec![coco_llm_types::UserContentPart::Text(
                        coco_llm_types::TextPart {
                            text: req.system_prompt,
                            provider_metadata: None,
                        },
                    )],
                    provider_options: None,
                },
                coco_llm_types::LlmMessage::User {
                    content: vec![coco_llm_types::UserContentPart::Text(
                        coco_llm_types::TextPart {
                            text: req.user_prompt,
                            provider_metadata: None,
                        },
                    )],
                    provider_options: None,
                },
            ];
            let params = QueryParams {
                prompt,
                max_tokens: Some(req.max_tokens),
                thinking_level: None,
                fast_mode: req.stage == 1,
                tools: None,
                context_management: None,
                query_source: None,
                agent_id: None,
                time_since_last_assistant_ms: None,
                // Auto-mode classifier helper call — not the agent loop.
                agentic: false,
                cache: None,
                // Stage 1 in `both` mode passes ["</block>"] so the model
                // terminates immediately after the verdict tag, saving
                // tokens and latency. Stage 2 leaves this `None` so it can
                // emit `<thinking>` and `<reason>` freely. TS parity:
                // `yoloClassifier.ts:792`.
                stop_sequences: req.stop_sequences,
            };
            match client.query(&params).await {
                Ok(result) => {
                    // auto-mode classifier input — preserve tool-call
                    // boundary markers so permission decisions see the
                    // structural transitions (otherwise multi-text +
                    // tool calls collapse to a single blob and the
                    // classifier can misclassify).
                    let mut chunks: Vec<String> = Vec::new();
                    for p in &result.content {
                        match p {
                            coco_llm_types::AssistantContentPart::Text(t) if !t.text.is_empty() => {
                                chunks.push(t.text.clone());
                            }
                            coco_llm_types::AssistantContentPart::ToolCall(tc) => {
                                chunks.push(format!("[tool: {}]", tc.tool_name));
                            }
                            _ => {}
                        }
                    }
                    // Stage 1 uses `["</block>"]` as a stop sequence, so a
                    // `stop_sequence` stop_reason is expected and stays
                    // in the happy-path set of `is_abnormal_stop_reason`.
                    // The danger is `length` (verdict truncated mid-XML)
                    // or `content-filter` — both yield a structurally
                    // incomplete classifier output that downstream
                    // permission parsing may silently mis-interpret as
                    // "allow". Warn so the permission misroute is
                    // discoverable.
                    let stop = result.stop_reason;
                    if stop.is_some_and(coco_messages::StopReason::is_abnormal) || chunks.is_empty()
                    {
                        tracing::warn!(
                            stop_reason = ?stop,
                            tokens_out = result.usage.output_tokens,
                            chunks = chunks.len(),
                            stage = req.stage,
                            "auto-mode classifier unexpected outcome — \
                             permission decision may use a truncated verdict"
                        );
                    }
                    Ok(chunks.join("\n"))
                }
                Err(e) => Err(e.to_string()),
            }
        }
    };

    coco_permissions::can_use_tool_in_auto_mode(
        tool_name,
        input,
        is_read_only,
        state,
        tracker,
        messages,
        auto_mode_rules,
        classify_fn,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn resolve_effective_input_from_permission(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    permission_outcome: PermissionOutcome,
    effective_input: Value,
    completion_event_mode: ToolCompletionEventMode,
) -> Option<Value> {
    match permission_outcome {
        PermissionOutcome::Denied => None,
        PermissionOutcome::Allow { updated_input } => {
            if let Some(updated_input) = updated_input {
                return validate_effective_input_or_complete_error(
                    event_tx,
                    history,
                    ctx,
                    tool_call,
                    tool_id,
                    tool,
                    updated_input,
                    completion_event_mode,
                )
                .await;
            }
            Some(effective_input)
        }
    }
}

/// TS `executePermissionDeniedHooks` wiring — only fires when the
/// decision is a classifier-driven `Deny`. Returns the (possibly
/// rewritten) decision; on `retry: true` we append a hint so the model
/// learns the hook approved the retry.
async fn maybe_fire_permission_denied_hook(
    hook_controller: &HookController<'_>,
    tool_call: &ToolCallPart,
    effective_input: &Value,
    decision: PermissionDecision,
) -> PermissionDecision {
    let PermissionDecision::Deny { message, reason } = decision else {
        return decision;
    };
    let coco_types::PermissionDecisionReason::Classifier {
        reason: classifier_reason,
        ..
    } = &reason
    else {
        return PermissionDecision::Deny { message, reason };
    };

    let retry = hook_controller
        .run_permission_denied(
            &tool_call.tool_name,
            &tool_call.tool_call_id,
            effective_input,
            classifier_reason,
        )
        .await;
    if !retry {
        return PermissionDecision::Deny { message, reason };
    }

    let updated_message = format!(
        "{message}\n\nThe PermissionDenied hook indicated this command is now \
         approved. You may retry it if you would like."
    );
    PermissionDecision::Deny {
        message: updated_message,
        reason,
    }
}

#[allow(clippy::too_many_arguments)]
async fn validate_effective_input_or_complete_error(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    input: Value,
    completion_event_mode: ToolCompletionEventMode,
) -> Option<Value> {
    // Schema validation (plan I3 Rust-side tightening): check the
    // (possibly hook-rewritten) input against the tool's JSON
    // schema BEFORE running `tool.validate_input`. A hook that
    // returns malformed input produces a synthetic validation
    // error here, not silently downstream.
    //
    // The validator is session-scoped via
    // `ctx.tool_schema_validator` when present; a null validator
    // short-circuits to the legacy path (no schema check). Cache
    // hits across validations within a turn are free.
    if let Some(validator) = ctx.tool_schema_validator.as_ref()
        && let Err(e) = validator.validate(tool.as_ref(), &input).await
    {
        let message = format!("Invalid input: {e}");
        complete_tool_call_with_error_mode(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            tool_id,
            &message,
            completion_event_mode,
        )
        .await;
        return None;
    }

    let validation = tool.validate_input(&input, ctx);
    if validation.is_valid() {
        return Some(input);
    }

    let message = match validation {
        coco_tool_runtime::ValidationResult::Invalid { message, .. } => {
            format!("Invalid input: {message}")
        }
        coco_tool_runtime::ValidationResult::Valid => "Invalid input".to_string(),
    };
    complete_tool_call_with_error_mode(
        event_tx,
        history,
        &tool_call.tool_call_id,
        &tool_call.tool_name,
        tool_id,
        &message,
        completion_event_mode,
    )
    .await;
    None
}

#[cfg(test)]
#[path = "tool_call_preparer.test.rs"]
mod tests;
