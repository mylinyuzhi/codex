//! SDK hook callback bridge.
//!
//! Wires the agent-side hook orchestration to the SDK client over the
//! control-protocol transport. The bridge:
//!
//! 1. Installs a runtime callback on `HookRegistry` that, when invoked,
//!    sends a `hook/callback` server request and awaits the reply.
//! 2. Translates the SDK's typed [`coco_types::SdkHookOutput`] reply
//!    into a JSON `Value` for the existing hook-orchestration parser.
//!    There is **no behaviour collapse** — the SDK speaks the same
//!    canonical TS shape (`hookSpecificOutput` + flat sync fields) the
//!    orchestrator already understands.
//!
//! TS source: `cli/structuredIO.ts::createHookCallback`.
//!
//! Concurrency note: every hook invocation gets a fresh JSON-RPC
//! `request_id` (issued by `send_server_request`). Two parallel
//! invocations of the same `callback_id` cannot consume each other's
//! responses — TS parity.

use std::sync::Arc;

use tracing::warn;

use crate::sdk_server::handlers::SdkServerState;

pub fn install_runtime_callback(
    state: Arc<SdkServerState>,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
) {
    let callback: coco_hooks::SdkHookCallback = Arc::new(move |request| {
        let state = state.clone();
        Box::pin(async move { route_hook_callback(state, request).await })
    });
    runtime.hook_registry().set_sdk_hook_callback(callback);
}

pub fn register_initialize_hooks(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    hooks: &std::collections::HashMap<
        coco_types::HookEventType,
        Vec<coco_types::HookCallbackMatcher>,
    >,
) -> usize {
    let registry = runtime.hook_registry();
    let mut count = 0;
    for (event, matchers) in hooks {
        for matcher in matchers {
            for callback_id in &matcher.hook_callback_ids {
                let timeout_ms = matcher.timeout.map(|seconds| seconds * 1000);
                let hook = coco_hooks::HookDefinition {
                    event: *event,
                    matcher: matcher.matcher.clone(),
                    handler: coco_hooks::HookHandler::SdkCallback {
                        callback_id: callback_id.clone(),
                        timeout_ms,
                    },
                    priority: 0,
                    scope: coco_types::HookScope::Session,
                    if_condition: None,
                    once: false,
                    is_async: false,
                    async_rewake: false,
                    status_message: None,
                };
                if registry.register_deduped(hook) {
                    count += 1;
                }
            }
        }
    }
    count
}

async fn route_hook_callback(
    state: Arc<SdkServerState>,
    request: coco_hooks::SdkHookCallbackRequest,
) -> coco_hooks::Result<coco_types::SdkHookOutput> {
    let transport = {
        let guard = state.transport.read().await;
        guard.as_ref().cloned()
    }
    .ok_or_else(|| coco_hooks::HooksError::generic("SDK hook bridge transport not initialized"))?;

    let params = coco_types::ServerHookCallbackParams {
        callback_id: request.callback_id,
        event_type: request.event,
        input: request.input,
        tool_use_id: request.tool_use_id,
    };
    let params_json = serde_json::to_value(params).map_err(|e| {
        coco_hooks::HooksError::generic(format!("serialize hook/callback params: {e}"))
    })?;

    let reply = state
        .send_server_request(&transport, "hook/callback", params_json)
        .await
        .map_err(|e| coco_hooks::HooksError::generic(format!("send hook/callback: {e}")))?;

    match reply {
        coco_types::JsonRpcMessage::Response(response) => {
            // Strict typed parse — bad payload fails here instead of
            // getting silently re-interpreted by the legacy
            // `parse_hook_output` permissive parser. The typed output
            // flows end-to-end: callback → orchestration spawn loop
            // → `HookExecutionResult::SdkOutput` → `apply_sdk_hook_output`.
            // No JSON `Value` round-trip on this path.
            let result: coco_types::HookCallbackResult = serde_json::from_value(response.result)
                .map_err(|e| {
                    coco_hooks::HooksError::generic(format!("parse hook/callback response: {e}"))
                })?;
            Ok(result.output)
        }
        coco_types::JsonRpcMessage::Error(error) => Err(coco_hooks::HooksError::generic(format!(
            "SDK client returned hook/callback error: {} ({})",
            error.message, error.code
        ))),
        other => {
            warn!(?other, "unexpected hook/callback reply");
            Err(coco_hooks::HooksError::generic(
                "unexpected hook/callback reply",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use coco_types::HookCallbackResult;
    use coco_types::HookDecision;
    use coco_types::HookPermissionDecision;
    use coco_types::HookSpecificOutput;
    use coco_types::SdkHookOutput;
    use pretty_assertions::assert_eq;

    /// PreToolUse deny round-trips through the same TS-canonical
    /// `hookSpecificOutput` shape that orchestration's
    /// `aggregate_results_for_event` understands.
    #[test]
    fn pre_tool_use_deny_round_trips_through_hook_specific_output() {
        let result = HookCallbackResult {
            output: SdkHookOutput {
                hook_specific_output: Some(HookSpecificOutput::PreToolUse {
                    permission_decision: Some(HookPermissionDecision::Deny),
                    permission_decision_reason: Some("sdk denied".into()),
                    updated_input: None,
                    additional_context: None,
                }),
                ..Default::default()
            },
        };

        let wire = serde_json::to_value(&result).unwrap();
        // SDK-canonical wire shape: `{output}`, where
        // `output.hookSpecificOutput.hookEventName` discriminates.
        let specific = &wire["output"]["hookSpecificOutput"];
        assert_eq!(specific["hookEventName"], "PreToolUse");
        assert_eq!(specific["permissionDecision"], "deny");
        assert_eq!(specific["permissionDecisionReason"], "sdk denied");

        // Round-trip is lossless: parsing the wire JSON back recovers
        // the typed enum, not a string.
        let parsed: HookCallbackResult = serde_json::from_value(wire).unwrap();
        match parsed.output.hook_specific_output.unwrap() {
            HookSpecificOutput::PreToolUse {
                permission_decision,
                ..
            } => {
                assert_eq!(permission_decision, Some(HookPermissionDecision::Deny));
            }
            other => panic!("expected PreToolUse, got {other:?}"),
        }
    }

    /// Top-level `continue: false` propagates through SdkHookOutput as
    /// a sync-mode stop signal. Tests TS parity with hooks that want
    /// to halt the loop without using `hookSpecificOutput`.
    #[test]
    fn top_level_continue_false_serializes_as_async_omitted() {
        let output = SdkHookOutput {
            r#continue: Some(false),
            stop_reason: Some("policy".into()),
            ..Default::default()
        };
        let wire = serde_json::to_value(&output).unwrap();
        // `async` is omitted (TS sync-mode default) when not set.
        assert!(wire.get("async").is_none());
        assert_eq!(wire["continue"], false);
        assert_eq!(wire["stopReason"], "policy");
    }

    /// Async hooks carry `async: true` and optionally `asyncTimeout`;
    /// every sync field is omitted by serde when None.
    #[test]
    fn async_hook_serializes_async_discriminator() {
        let output = SdkHookOutput {
            r#async: Some(true),
            async_timeout: Some(5_000),
            ..Default::default()
        };
        let wire = serde_json::to_value(&output).unwrap();
        assert_eq!(wire["async"], true);
        assert_eq!(wire["asyncTimeout"], 5_000);
        assert!(wire.get("hookSpecificOutput").is_none());
    }

    /// Top-level `decision: "block"` is wire-canonical (lowercase) for
    /// the TS `HookDecision` enum.
    #[test]
    fn hook_decision_serializes_lowercase() {
        let output = SdkHookOutput {
            decision: Some(HookDecision::Block),
            reason: Some("nope".into()),
            ..Default::default()
        };
        let wire = serde_json::to_value(&output).unwrap();
        assert_eq!(wire["decision"], "block");
        assert_eq!(wire["reason"], "nope");
    }
}
