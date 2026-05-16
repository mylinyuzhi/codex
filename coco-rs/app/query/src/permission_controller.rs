use std::sync::Arc;

use coco_hooks::HookRegistry;
use coco_hooks::orchestration::OrchestrationContext;
use coco_hooks::orchestration::PermissionRequestDecision;
use coco_inference::ToolCallPart;
use coco_messages::MessageHistory;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_types::CoreEvent;
use coco_types::PermissionDecision;
use coco_types::PermissionDenialInfo;
use coco_types::SessionState;
use coco_types::ToolId;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::helpers::ToolCompletionEventMode;
use crate::helpers::complete_tool_call_with_error_mode;
use crate::session_state::SessionStateTracker;

pub(crate) enum PermissionOutcome {
    Allow {
        updated_input: Option<serde_json::Value>,
    },
    Denied,
}

pub(crate) struct PermissionController<'a> {
    event_tx: &'a Option<mpsc::Sender<CoreEvent>>,
    history: &'a mut MessageHistory,
    permission_denials: &'a mut Vec<PermissionDenialInfo>,
    state_tracker: &'a SessionStateTracker,
    permission_bridge: Option<&'a ToolPermissionBridgeRef>,
    session_id: &'a str,
    cancel: &'a CancellationToken,
    /// Hook registry + orchestration context for firing
    /// `PermissionRequest` hooks before the dialog. TS:
    /// `executePermissionRequestHooks` runs in the dialog gate so
    /// hooks can override the user prompt with allow/deny.
    hooks: Option<&'a Arc<HookRegistry>>,
    orchestration_ctx: Option<&'a OrchestrationContext>,
    completion_event_mode: ToolCompletionEventMode,
}

impl<'a> PermissionController<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        event_tx: &'a Option<mpsc::Sender<CoreEvent>>,
        history: &'a mut MessageHistory,
        permission_denials: &'a mut Vec<PermissionDenialInfo>,
        state_tracker: &'a SessionStateTracker,
        permission_bridge: Option<&'a ToolPermissionBridgeRef>,
        session_id: &'a str,
        cancel: &'a CancellationToken,
        hooks: Option<&'a Arc<HookRegistry>>,
        orchestration_ctx: Option<&'a OrchestrationContext>,
        completion_event_mode: ToolCompletionEventMode,
    ) -> Self {
        Self {
            event_tx,
            history,
            permission_denials,
            state_tracker,
            permission_bridge,
            session_id,
            cancel,
            hooks,
            orchestration_ctx,
            completion_event_mode,
        }
    }

    pub(crate) async fn resolve(
        &mut self,
        decision: PermissionDecision,
        tool_call: &ToolCallPart,
        tool_input: &serde_json::Value,
        tool_id: &ToolId,
    ) -> PermissionOutcome {
        match decision {
            PermissionDecision::Allow { updated_input, .. } => {
                PermissionOutcome::Allow { updated_input }
            }
            PermissionDecision::Deny { message, .. } => {
                warn!(tool = tool_call.tool_name, %message, "tool permission denied");
                self.record_denial(tool_call, tool_input);
                let output = format!("Permission denied: {message}");
                complete_tool_call_with_error_mode(
                    self.event_tx,
                    self.history,
                    &tool_call.tool_call_id,
                    &tool_call.tool_name,
                    tool_id,
                    &output,
                    self.completion_event_mode,
                )
                .await;
                PermissionOutcome::Denied
            }
            PermissionDecision::Ask { choices, .. } => {
                self.resolve_ask(tool_call, tool_input, tool_id, choices)
                    .await
            }
        }
    }

    async fn resolve_ask(
        &mut self,
        tool_call: &ToolCallPart,
        tool_input: &serde_json::Value,
        tool_id: &ToolId,
        choices: Option<Vec<coco_types::PermissionAskChoice>>,
    ) -> PermissionOutcome {
        // TS reference: notifySessionStateChanged('requires_action') on
        // can_use_tool entry, then transition back to running when the
        // approval path resolves. No bridge preserves legacy headless
        // auto-allow behavior.
        self.state_tracker
            .transition_to(SessionState::RequiresAction, self.event_tx)
            .await;

        // PermissionRequest hook: TS `executePermissionRequestHooks`
        // (`utils/hooks.ts:4157`) — fires before the dialog. If the
        // hook returns a `decision` (allow/deny), it short-circuits
        // the prompt entirely.
        if let (Some(registry), Some(ctx)) = (self.hooks, self.orchestration_ctx)
            && !ctx.disable_all_hooks
        {
            match coco_hooks::orchestration::execute_permission_request(
                registry,
                ctx,
                &tool_call.tool_name,
                tool_input,
                /*permission_suggestions*/ None,
            )
            .await
            {
                Ok(agg) => {
                    if let Some(decision) = agg.permission_request_result {
                        match decision {
                            PermissionRequestDecision::Allow { updated_input } => {
                                self.state_tracker
                                    .transition_to(SessionState::Running, self.event_tx)
                                    .await;
                                return PermissionOutcome::Allow { updated_input };
                            }
                            PermissionRequestDecision::Deny { message, .. } => {
                                let feedback = message
                                    .unwrap_or_else(|| "Permission denied by hook".to_string());
                                warn!(
                                    tool = tool_call.tool_name,
                                    "PermissionRequest hook denied tool execution"
                                );
                                self.record_denial(tool_call, tool_input);
                                let output = format!("Permission denied: {feedback}");
                                complete_tool_call_with_error_mode(
                                    self.event_tx,
                                    self.history,
                                    &tool_call.tool_call_id,
                                    &tool_call.tool_name,
                                    tool_id,
                                    &output,
                                    self.completion_event_mode,
                                )
                                .await;
                                self.state_tracker
                                    .transition_to(SessionState::Running, self.event_tx)
                                    .await;
                                return PermissionOutcome::Denied;
                            }
                        }
                    }
                    // No decision → fall through to the dialog as TS
                    // does when `hookSpecificOutput.decision` is absent.
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        tool = tool_call.tool_name,
                        "PermissionRequest hook failed; proceeding with dialog"
                    );
                }
            }
        }

        let Some(bridge) = self.permission_bridge else {
            self.state_tracker
                .transition_to(SessionState::Running, self.event_tx)
                .await;
            return PermissionOutcome::Allow {
                updated_input: None,
            };
        };

        let request = coco_tool_runtime::ToolPermissionRequest {
            id: format!("approval-{}", uuid::Uuid::new_v4()),
            tool_use_id: tool_call.tool_call_id.clone(),
            agent_id: self.session_id.to_string(),
            tool_name: tool_call.tool_name.clone(),
            description: format!("Approval required for {}", tool_call.tool_name),
            input: tool_input.clone(),
            choices,
        };

        let bridge_result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => {
                Err("Turn cancelled while waiting for permission approval".to_string())
            }
            r = bridge.request_permission(request) => r,
        };

        match bridge_result {
            Ok(resolution) => match resolution.decision {
                coco_tool_runtime::ToolPermissionDecision::Approved => {
                    self.state_tracker
                        .transition_to(SessionState::Running, self.event_tx)
                        .await;
                    // Forward `updated_input` from the bridge so
                    // `tool_call_preparer::resolve_effective_input_from_permission`
                    // can substitute it for the original tool input. Used
                    // by `AskUserQuestion` to splice user-selected
                    // `answers` into the tool's data envelope. TS parity:
                    // `permissionDecision.updatedInput` →
                    // `processedInput = permissionDecision.updatedInput`
                    // at `services/tools/toolExecution.ts:1130-1131`.
                    PermissionOutcome::Allow {
                        updated_input: resolution.updated_input,
                    }
                }
                coco_tool_runtime::ToolPermissionDecision::Rejected => {
                    let feedback = resolution
                        .feedback
                        .unwrap_or_else(|| "Permission denied by client".into());
                    warn!(tool = tool_call.tool_name, "approval bridge: rejected");
                    self.record_denial(tool_call, tool_input);
                    let output = format!("Permission denied: {feedback}");
                    complete_tool_call_with_error_mode(
                        self.event_tx,
                        self.history,
                        &tool_call.tool_call_id,
                        &tool_call.tool_name,
                        tool_id,
                        &output,
                        self.completion_event_mode,
                    )
                    .await;
                    self.state_tracker
                        .transition_to(SessionState::Running, self.event_tx)
                        .await;
                    PermissionOutcome::Denied
                }
            },
            Err(e) => {
                warn!(
                    error = %e,
                    tool = tool_call.tool_name,
                    "approval bridge failed; auto-denying"
                );
                self.record_denial(tool_call, tool_input);
                let output = format!("Approval bridge error: {e}");
                complete_tool_call_with_error_mode(
                    self.event_tx,
                    self.history,
                    &tool_call.tool_call_id,
                    &tool_call.tool_name,
                    tool_id,
                    &output,
                    self.completion_event_mode,
                )
                .await;
                self.state_tracker
                    .transition_to(SessionState::Running, self.event_tx)
                    .await;
                PermissionOutcome::Denied
            }
        }
    }

    fn record_denial(&mut self, tool_call: &ToolCallPart, tool_input: &serde_json::Value) {
        self.permission_denials.push(PermissionDenialInfo {
            tool_name: tool_call.tool_name.clone(),
            tool_use_id: tool_call.tool_call_id.clone(),
            tool_input: tool_input.clone(),
        });
    }
}
