use coco_messages::MessageHistory;
use coco_tool::ToolPermissionBridgeRef;
use coco_types::CoreEvent;
use coco_types::PermissionDecision;
use coco_types::PermissionDenialInfo;
use coco_types::SessionState;
use coco_types::ToolId;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use vercel_ai_provider::ToolCallPart;

use crate::helpers::complete_tool_call_with_error;
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
}

impl<'a> PermissionController<'a> {
    pub(crate) fn new(
        event_tx: &'a Option<mpsc::Sender<CoreEvent>>,
        history: &'a mut MessageHistory,
        permission_denials: &'a mut Vec<PermissionDenialInfo>,
        state_tracker: &'a SessionStateTracker,
        permission_bridge: Option<&'a ToolPermissionBridgeRef>,
        session_id: &'a str,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            event_tx,
            history,
            permission_denials,
            state_tracker,
            permission_bridge,
            session_id,
            cancel,
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
                complete_tool_call_with_error(
                    self.event_tx,
                    self.history,
                    &tool_call.tool_call_id,
                    &tool_call.tool_name,
                    tool_id,
                    &output,
                )
                .await;
                PermissionOutcome::Denied
            }
            PermissionDecision::Ask { .. } => {
                self.resolve_ask(tool_call, tool_input, tool_id).await
            }
        }
    }

    async fn resolve_ask(
        &mut self,
        tool_call: &ToolCallPart,
        tool_input: &serde_json::Value,
        tool_id: &ToolId,
    ) -> PermissionOutcome {
        // TS reference: notifySessionStateChanged('requires_action') on
        // can_use_tool entry, then transition back to running when the
        // approval path resolves. No bridge preserves legacy headless
        // auto-allow behavior.
        self.state_tracker
            .transition_to(SessionState::RequiresAction, self.event_tx)
            .await;

        let Some(bridge) = self.permission_bridge else {
            self.state_tracker
                .transition_to(SessionState::Running, self.event_tx)
                .await;
            return PermissionOutcome::Allow {
                updated_input: None,
            };
        };

        let request = coco_tool::ToolPermissionRequest {
            id: format!("approval-{}", uuid::Uuid::new_v4()),
            tool_use_id: tool_call.tool_call_id.clone(),
            agent_id: self.session_id.to_string(),
            tool_name: tool_call.tool_name.clone(),
            description: format!("Approval required for {}", tool_call.tool_name),
            input: tool_input.clone(),
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
                coco_tool::ToolPermissionDecision::Approved => {
                    self.state_tracker
                        .transition_to(SessionState::Running, self.event_tx)
                        .await;
                    PermissionOutcome::Allow {
                        updated_input: None,
                    }
                }
                coco_tool::ToolPermissionDecision::Rejected => {
                    let feedback = resolution
                        .feedback
                        .unwrap_or_else(|| "Permission denied by client".into());
                    warn!(tool = tool_call.tool_name, "approval bridge: rejected");
                    self.record_denial(tool_call, tool_input);
                    let output = format!("Permission denied: {feedback}");
                    complete_tool_call_with_error(
                        self.event_tx,
                        self.history,
                        &tool_call.tool_call_id,
                        &tool_call.tool_name,
                        tool_id,
                        &output,
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
                complete_tool_call_with_error(
                    self.event_tx,
                    self.history,
                    &tool_call.tool_call_id,
                    &tool_call.tool_name,
                    tool_id,
                    &output,
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
