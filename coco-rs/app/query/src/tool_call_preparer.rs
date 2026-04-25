use std::collections::HashMap;
use std::sync::Arc;

use coco_hooks::HookExecutionEvent;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration::OrchestrationContext;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_messages::MessageHistory;
use coco_permissions::AutoModeRules;
use coco_tool_runtime::PendingToolCall;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::CoreEvent;
use coco_types::Message;
use coco_types::PermissionDecision;
use coco_types::PermissionDenialInfo;
use coco_types::ToolId;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::ToolCallPart;

use crate::helpers::complete_tool_call_with_error;
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
    let prepared =
        prepare_committed_tool_call(args.event_tx, args.history, args.tools, args.ctx, tc).await?;

    let tool_id = prepared.tool_id;
    let tool = prepared.tool;

    let hook_controller =
        HookController::new(args.hooks, args.orchestration_ctx.clone(), args.hook_tx_opt);
    let pre_tool_outcome = hook_controller
        .run_pre_tool_use(args.event_tx, args.history, tc, &tool_id)
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
        )
        .await?;

    let decision = resolve_permission_decision(
        tc,
        &tool,
        &effective_input,
        args.ctx,
        &args.history.messages,
        (hook_permission_behavior, hook_permission_reason),
        args.auto_mode_state,
        args.denial_tracker,
        args.client,
        args.auto_mode_rules,
    )
    .await;

    let permission_outcome = PermissionController::new(
        args.event_tx,
        args.history,
        args.permission_denials,
        args.state_tracker,
        args.permission_bridge,
        args.session_id,
        args.cancel,
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

async fn resolve_effective_input_from_pre_hook(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    pre_tool_outcome: PreToolUseOutcome,
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
                )
                .await
                .map(|input| (input, permission_behavior, reason));
            }
            Some((tool_call.input.clone(), permission_behavior, reason))
        }
    }
}

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
    let mut decision = match hook_permission_behavior {
        Some(coco_types::PermissionBehavior::Allow) => PermissionDecision::Allow {
            updated_input: None,
            feedback: hook_permission_reason,
        },
        Some(coco_types::PermissionBehavior::Ask) => PermissionDecision::Ask {
            message: hook_permission_reason
                .unwrap_or_else(|| "PreToolUse hook requested approval".into()),
            suggestions: Vec::new(),
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
        None => tool.check_permissions(effective_input, ctx).await,
    };

    if matches!(decision, PermissionDecision::Ask { .. })
        && let (Some(state), Some(tracker)) = (auto_mode_state, denial_tracker)
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
            let prompt: vercel_ai_provider::LanguageModelV4Prompt = vec![
                vercel_ai_provider::LanguageModelV4Message::System {
                    content: req.system_prompt,
                    provider_options: None,
                },
                vercel_ai_provider::LanguageModelV4Message::User {
                    content: vec![vercel_ai_provider::UserContentPart::Text(
                        vercel_ai_provider::TextPart {
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
            };
            match client.query(&params).await {
                Ok(result) => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            vercel_ai_provider::AssistantContentPart::Text(t) => {
                                Some(t.text.as_str())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    Ok(text)
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

async fn resolve_effective_input_from_permission(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    permission_outcome: PermissionOutcome,
    effective_input: Value,
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
                )
                .await;
            }
            Some(effective_input)
        }
    }
}

async fn validate_effective_input_or_complete_error(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    tool_id: &ToolId,
    tool: &Arc<dyn Tool>,
    input: Value,
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
    if let Some(validator) = ctx.tool_schema_validator.as_ref() {
        if let Err(e) = validator.validate(tool.as_ref(), &input).await {
            let message = format!("Invalid input: {e}");
            complete_tool_call_with_error(
                event_tx,
                history,
                &tool_call.tool_call_id,
                &tool_call.tool_name,
                tool_id,
                &message,
            )
            .await;
            return None;
        }
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
    complete_tool_call_with_error(
        event_tx,
        history,
        &tool_call.tool_call_id,
        &tool_call.tool_name,
        tool_id,
        &message,
    )
    .await;
    None
}
