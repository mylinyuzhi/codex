//! Elicitation/ElicitationResult hook wrappers around `SendElicitation`.
//!
//! TS parity: `services/mcp/elicitationHandler.ts:runElicitationHooks` +
//! `runElicitationResultHooks`. The TS handler:
//!
//! 1. Receives an MCP `elicit/create` request.
//! 2. Fires `executeElicitationHooks` BEFORE showing the dialog. If the
//!    hook returns an `action` (accept/decline/cancel) the dialog is
//!    skipped and the hook's response goes back to the server.
//! 3. Otherwise shows the dialog, awaits the user's response.
//! 4. Fires `executeElicitationResultHooks` AFTER. The hook can override
//!    the action/content or block (forces decline).
//! 5. Fires `Notification` with `notification_type: "elicitation_response"`.
//!
//! Coco-rs has no dialog UI yet — every `SendElicitation` call site
//! returns `Err`. By wrapping the closure with this helper, hooks fire
//! regardless of whether a UI exists, so users can program-respond to
//! elicitations through hooks even on headless / SDK paths.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use coco_hooks::HookRegistry;
use coco_hooks::orchestration::ElicitationAction as HookElicitationAction;
use coco_hooks::orchestration::ElicitationMode;
use coco_hooks::orchestration::OrchestrationContext;

/// Build a `SendElicitation` closure that fires `Elicitation` and
/// `ElicitationResult` hooks around the supplied `inner` fallback.
///
/// `inner` is invoked only when the `Elicitation` hook does NOT return
/// a decision — matching TS, where the dialog only renders when no hook
/// short-circuits. Failures in `inner` propagate as elicitation errors.
///
/// `ctx_factory` produces an `OrchestrationContext` per fire. Each call
/// captures session_id / cwd at the time of firing rather than at
/// closure-creation time so a `/clear` doesn't leave a stale snapshot.
pub fn wrap_send_elicitation_with_hooks(
    server_name: String,
    registry: Arc<HookRegistry>,
    ctx_factory: Arc<dyn Fn() -> OrchestrationContext + Send + Sync>,
    inner: coco_mcp::SendElicitation,
) -> coco_mcp::SendElicitation {
    let inner = std::sync::Arc::new(tokio::sync::Mutex::new(inner));
    Box::new(
        move |request_id,
              elicitation|
              -> Pin<
            Box<
                dyn Future<
                        Output = std::result::Result<
                            coco_mcp::ElicitationResponse,
                            coco_mcp::RmcpClientError,
                        >,
                    > + Send,
            >,
        > {
            let server_name = server_name.clone();
            let registry = registry.clone();
            let ctx_factory = ctx_factory.clone();
            let inner = inner.clone();
            Box::pin(async move {
                let message = elicitation.message.clone();
                let requested_schema = serde_json::to_value(&elicitation.requested_schema).ok();

                // Pre-dialog hook (TS `runElicitationHooks`).
                let ctx = (ctx_factory)();
                if !ctx.disable_all_hooks {
                    match coco_hooks::orchestration::execute_elicitation(
                        &registry,
                        &ctx,
                        &server_name,
                        &message,
                        Some(ElicitationMode::Form),
                        /*url*/ None,
                        /*elicitation_id*/ None,
                        requested_schema.as_ref(),
                    )
                    .await
                    {
                        Ok(agg) => {
                            // TS: `blockingError` ⇒ decline.
                            if agg.is_blocked() {
                                tracing::debug!(
                                    %server_name,
                                    "Elicitation hook blocked; auto-declining"
                                );
                                return run_result_hook_and(
                                    &registry,
                                    &ctx_factory,
                                    &server_name,
                                    coco_mcp::ElicitationResponse {
                                        action: coco_mcp::RmcpElicitationAction::Decline,
                                        content: None,
                                    },
                                    /*hook_overrode*/ true,
                                )
                                .await;
                            }
                            // TS: hook returned `action` ⇒ use it as the response.
                            if let Some(resp) = agg.elicitation_response {
                                let response =
                                    build_elicitation_response(&resp.action, resp.content);
                                return run_result_hook_and(
                                    &registry,
                                    &ctx_factory,
                                    &server_name,
                                    response,
                                    /*hook_overrode*/ true,
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, %server_name, "Elicitation hook failed; falling through to dialog");
                        }
                    }
                }

                // No hook decision — fall through to the underlying
                // `SendElicitation` (which is a no-op stub today; will
                // become the TUI dialog once that bridge lands).
                let raw_response = {
                    let guard = inner.lock().await;
                    (guard)(request_id, elicitation).await
                }?;

                // Post-dialog hook (TS `runElicitationResultHooks`).
                run_result_hook_and(
                    &registry,
                    &ctx_factory,
                    &server_name,
                    raw_response,
                    /*hook_overrode*/ false,
                )
                .await
            })
        },
    )
}

/// Run `ElicitationResult` hook + emit the `elicitation_response`
/// notification, returning the (potentially overridden) response.
///
/// `hook_overrode` is `true` when the response came from the
/// `Elicitation` hook itself (so the result hook still fires for
/// observability, matching TS `runElicitationResultHooks`).
async fn run_result_hook_and(
    registry: &Arc<HookRegistry>,
    ctx_factory: &Arc<dyn Fn() -> OrchestrationContext + Send + Sync>,
    server_name: &str,
    response: coco_mcp::ElicitationResponse,
    _hook_overrode: bool,
) -> std::result::Result<coco_mcp::ElicitationResponse, coco_mcp::RmcpClientError> {
    let ctx = (ctx_factory)();
    let action = action_from_rmcp(&response.action);
    let content = response.content.clone();

    if !ctx.disable_all_hooks {
        match coco_hooks::orchestration::execute_elicitation_result(
            registry,
            &ctx,
            server_name,
            /*elicitation_id*/ None,
            Some(ElicitationMode::Form),
            action,
            content.as_ref(),
        )
        .await
        {
            Ok(agg) => {
                let final_response = if agg.is_blocked() {
                    coco_mcp::ElicitationResponse {
                        action: coco_mcp::RmcpElicitationAction::Decline,
                        content: None,
                    }
                } else if let Some(override_resp) = agg.elicitation_result_response {
                    build_elicitation_response(
                        &override_resp.action,
                        override_resp.content.or(content),
                    )
                } else {
                    response
                };

                // TS fires `elicitation_response` Notification at the end
                // (elicitationHandler.ts:283-286 / 297-301). Mirror that.
                let final_action = format!("{:?}", final_response.action).to_lowercase();
                let _ = coco_hooks::orchestration::execute_notification(
                    registry,
                    &ctx,
                    "elicitation_response",
                    &format!("Elicitation response for server \"{server_name}\": {final_action}"),
                    /*title*/ None,
                )
                .await;

                Ok(final_response)
            }
            Err(e) => {
                tracing::warn!(error = %e, "ElicitationResult hook failed");
                let _ = coco_hooks::orchestration::execute_notification(
                    registry,
                    &ctx,
                    "elicitation_response",
                    &format!("Elicitation response for server \"{server_name}\": (hook error)"),
                    /*title*/ None,
                )
                .await;
                Ok(response)
            }
        }
    } else {
        Ok(response)
    }
}

fn action_from_rmcp(action: &coco_mcp::RmcpElicitationAction) -> HookElicitationAction {
    match action {
        coco_mcp::RmcpElicitationAction::Accept => HookElicitationAction::Accept,
        coco_mcp::RmcpElicitationAction::Decline => HookElicitationAction::Decline,
        coco_mcp::RmcpElicitationAction::Cancel => HookElicitationAction::Cancel,
    }
}

fn build_elicitation_response(
    action: &str,
    content: Option<serde_json::Value>,
) -> coco_mcp::ElicitationResponse {
    let action = match action {
        "accept" => coco_mcp::RmcpElicitationAction::Accept,
        "decline" => coco_mcp::RmcpElicitationAction::Decline,
        _ => coco_mcp::RmcpElicitationAction::Cancel,
    };
    coco_mcp::ElicitationResponse { action, content }
}
