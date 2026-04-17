//! Production [`TurnRunner`] backed by [`coco_query::QueryEngine`].
//!
//! This is the bridge between the SDK dispatch layer (which knows only
//! about the `TurnRunner` trait) and the real agent loop. The CLI entry
//! point in `main.rs` constructs one of these per-process and hands it
//! to `SdkServer::with_turn_runner`.
//!
//! Scope:
//! - One QueryEngine per turn (fresh config). Multi-turn context is
//!   threaded forward via `SessionHandle.history`: the runner locks
//!   the shared history, builds
//!   `prior_history + [create_user_message(prompt)]`, calls
//!   `run_with_messages`, and replaces the history with
//!   `result.final_messages` on completion.
//! - Forwards CoreEvents emitted by the engine directly onto the SDK
//!   server's `event_tx`. The server's notification forwarder then
//!   translates protocol events into JSON-RPC notifications on the wire.
//!
//! TS reference: `src/cli/print.ts runHeadless()` — creates a single
//! QueryEngine per headless invocation. coco-rs lets the SDK client
//! drive the cadence via multiple `turn/start` calls per session.

use std::pin::Pin;
use std::sync::Arc;

use coco_inference::ApiClient;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_tool::ToolPermissionBridgeRef;
use coco_tool::ToolRegistry;
use coco_types::CoreEvent;
use coco_types::TurnStartParams;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

use crate::sdk_server::handlers::TurnHandoff;
use crate::sdk_server::handlers::TurnRunner;

/// `TurnRunner` implementation that spawns a fresh `QueryEngine` per
/// turn.
///
/// Holds shared process-level resources (model client, tool registry)
/// so each turn can construct an engine cheaply without reloading
/// providers or re-registering tools.
pub struct QueryEngineRunner {
    client: Arc<ApiClient>,
    tools: Arc<ToolRegistry>,
    /// Max output tokens per turn. Pulled from CLI flags at startup.
    max_output_tokens: i64,
    /// Max internal agent turns (tool-use iterations) per SDK turn.
    max_turns: i32,
    /// Optional system prompt. When None, the engine uses its default.
    system_prompt: Option<String>,
    /// Optional permission bridge installed on each turn's `QueryEngine`.
    /// Wire `SdkPermissionBridge` here to route `PermissionDecision::Ask`
    /// to the SDK client via `approval/askForApproval`.
    permission_bridge: Option<ToolPermissionBridgeRef>,
}

impl QueryEngineRunner {
    /// Build a runner from pre-constructed shared resources.
    pub fn new(
        client: Arc<ApiClient>,
        tools: Arc<ToolRegistry>,
        max_output_tokens: i64,
        max_turns: i32,
        system_prompt: Option<String>,
    ) -> Self {
        Self {
            client,
            tools,
            max_output_tokens,
            max_turns,
            system_prompt,
            permission_bridge: None,
        }
    }

    /// Install a `ToolPermissionBridge` that every per-turn `QueryEngine`
    /// will consult when hitting `PermissionDecision::Ask`.
    pub fn with_permission_bridge(mut self, bridge: ToolPermissionBridgeRef) -> Self {
        self.permission_bridge = Some(bridge);
        self
    }
}

impl TurnRunner for QueryEngineRunner {
    fn run_turn<'a>(
        &'a self,
        params: TurnStartParams,
        handoff: TurnHandoff,
        event_tx: mpsc::Sender<CoreEvent>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        let prompt = params.prompt;
        let system_prompt = self.system_prompt.clone();
        let max_output_tokens = self.max_output_tokens;
        let max_turns = self.max_turns;
        let client = self.client.clone();
        let tools = self.tools.clone();
        let permission_bridge = self.permission_bridge.clone();
        let history_handle = handoff.history.clone();
        Box::pin(async move {
            info!(
                session_id = %handoff.session_id,
                model = %handoff.model,
                cwd = %handoff.cwd,
                "QueryEngineRunner: run_turn"
            );

            // Resolve the permission mode from the turn params if
            // provided, otherwise fall back to the session's default
            // (Default permission mode) — same behavior as the TS
            // headless flow.
            let permission_mode = params.permission_mode.unwrap_or_default();

            let config = QueryEngineConfig {
                model_name: handoff.model.clone(),
                permission_mode,
                context_window: 200_000,
                max_output_tokens,
                max_turns,
                max_tokens: None,
                system_prompt,
                ..Default::default()
            };

            let mut engine = QueryEngine::new(config, client, tools, cancel, /*hooks*/ None);
            if let Some(bridge) = permission_bridge {
                engine = engine.with_permission_bridge(bridge);
            }

            // Snapshot the prior history, append a fresh user message,
            // and **persist the combined history back to shared state
            // BEFORE calling the engine**. This way, even if the engine
            // returns `Err(...)` (e.g. transport crash, unrecoverable
            // tool failure), the user's prompt is still recorded and
            // the next `turn/start` sees it. On `Ok`, we overwrite with
            // the engine's more up-to-date `final_messages`, which also
            // includes any tool calls + the assistant reply.
            //
            // The engine's `run_session_loop` finds the LAST user
            // message in the list and keys the file history snapshot
            // against it, so passing the whole combined list works
            // for both single and multi-turn scenarios.
            let new_user_msg = coco_messages::create_user_message(&prompt);
            let combined: Vec<coco_types::Message> = {
                let mut h = history_handle.lock().await;
                h.push(new_user_msg);
                h.clone()
            };

            // Clone the event channel so we can still emit on the
            // error path (the engine takes ownership of the original).
            let event_tx_for_error = event_tx.clone();
            let session_id_for_error = handoff.session_id.clone();

            match engine.run_with_messages(combined, event_tx).await {
                Ok(result) => {
                    info!(
                        turns = result.turns,
                        input_tokens = result.total_usage.input_tokens,
                        output_tokens = result.total_usage.output_tokens,
                        history_len = result.final_messages.len(),
                        "QueryEngineRunner: turn complete"
                    );
                    // Overwrite with the engine's final history — this
                    // includes tool calls, tool results, and the
                    // assistant reply in addition to the user message
                    // we pre-persisted above.
                    let mut h = history_handle.lock().await;
                    *h = result.final_messages;
                    Ok(())
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "QueryEngineRunner: engine returned error; \
                         user message already persisted to session history"
                    );
                    // Emit a synthetic `SessionResult` with `is_error=true`
                    // so the forwarder's `accumulate_session_result` folds
                    // the failure into `SessionHandle.stats`. Without
                    // this, true engine-bail paths (compaction failure,
                    // transport crash, etc.) don't surface in the final
                    // aggregated `SessionResult` emitted by `session/archive`.
                    //
                    // Fields are minimal — we don't have usage/cost
                    // because the engine didn't reach `make_result`. The
                    // forwarder handles missing fields gracefully (default
                    // usage is zero; cost is 0.0; errors list is the one
                    // message we provide).
                    let error_params = coco_types::SessionResultParams {
                        session_id: session_id_for_error,
                        total_turns: 1,
                        duration_ms: 0,
                        duration_api_ms: 0,
                        is_error: true,
                        stop_reason: "engine_error".into(),
                        total_cost_usd: 0.0,
                        usage: coco_types::TokenUsage::default(),
                        model_usage: std::collections::HashMap::new(),
                        permission_denials: Vec::new(),
                        result: None,
                        errors: vec![e.to_string()],
                        structured_output: None,
                        fast_mode_state: None,
                        num_api_calls: None,
                    };
                    let _ = event_tx_for_error
                        .send(CoreEvent::Protocol(
                            coco_types::ServerNotification::SessionResult(Box::new(error_params)),
                        ))
                        .await;
                    Err(e)
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "sdk_runner.test.rs"]
mod tests;
