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

use coco_messages::MessageHistory;
use coco_query::QueryEngineConfig;
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
/// Holds an `Arc<SessionRuntime>` — the same per-session state container
/// the TUI runner uses. Per-turn engine assembly routes through
/// `runtime.build_engine_from_config(...)` so SDK and TUI share the
/// `with_*` install list.
pub struct QueryEngineRunner {
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    /// Max output tokens per turn. Pulled from CLI flags at startup.
    max_output_tokens: i64,
    /// Max internal agent turns (tool-use iterations) per SDK turn.
    max_turns: i32,
    /// Optional system prompt. When None, the engine uses its default.
    system_prompt: Option<String>,
}

impl QueryEngineRunner {
    /// Build a runner from a pre-constructed [`SessionRuntime`] (which
    /// already owns the client / tools / fallbacks / hook registry / all
    /// session subsystems).
    pub fn new(
        runtime: Arc<crate::session_runtime::SessionRuntime>,
        max_output_tokens: i64,
        max_turns: i32,
        system_prompt: Option<String>,
    ) -> Self {
        Self {
            runtime,
            max_output_tokens,
            max_turns,
            system_prompt,
        }
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
        let runtime = self.runtime.clone();
        let history_handle = handoff.history.clone();
        // Keep our own handle on the cancel token. The engine consumes
        // its copy; we still need to know post-run whether the user
        // requested an interrupt so the wire stream gets `turn/interrupted`
        // rather than `turn/failed`.
        let cancel_for_terminal = cancel.clone();
        Box::pin(async move {
            info!(
                session_id = %handoff.session_id,
                model = %handoff.model,
                cwd = %handoff.cwd,
                "QueryEngineRunner: run_turn"
            );

            // Resolve the permission mode. Priority:
            //   1. `params.permission_mode` (turn-scoped, TS parity).
            //   2. `handoff.permission_mode` (session-scoped, set by
            //      `control/setPermissionMode`).
            //   3. `PermissionMode::default()`.
            let permission_mode = params
                .permission_mode
                .or(handoff.permission_mode)
                .unwrap_or_default();

            // Re-use the SessionRuntime's already-loaded `RuntimeConfig`
            // instead of re-running `RuntimeConfigBuilder::from_process`
            // per turn. The runtime's config is the canonical session-
            // scoped resolution (incl. CLI overrides + flag settings);
            // rebuilding from `from_process` would lose them and slow
            // every turn down by re-walking settings layers.
            let runtime_config = runtime.runtime_config.as_ref();
            let config = QueryEngineConfig {
                model_id: handoff.model.clone(),
                permission_mode,
                context_window: 200_000,
                max_output_tokens,
                max_turns: if max_turns > 0 {
                    max_turns
                } else {
                    runtime_config.loop_config.max_turns.unwrap_or(max_turns)
                },
                max_tokens: runtime_config.loop_config.max_tokens.map(i64::from),
                system_prompt,
                streaming_tool_execution: runtime_config.loop_config.enable_streaming_tools,
                session_id: handoff.session_id.clone(),
                tool_config: runtime_config.tool.clone(),
                sandbox_config: runtime_config.sandbox.clone(),
                memory_config: runtime_config.memory.clone(),
                shell_config: runtime_config.shell.clone(),
                web_fetch_config: runtime_config.web_fetch.clone(),
                web_search_config: runtime_config.web_search.clone(),
                compact: runtime_config.compact.clone(),
                features: std::sync::Arc::new(runtime_config.features.clone()),
                tool_overrides: runtime_config.tool_overrides.clone(),
                ..Default::default()
            };

            // SDK pre-builds an engine_config with handoff overrides
            // (model / session_id / cwd may differ from runtime
            // defaults). `build_engine_from_config` installs every
            // per-session subsystem via `wire_engine`, and the
            // `app_state_override` argument keeps the compaction
            // observers' app_state pointer aligned with the engine's —
            // critical so post-compact resets reach the actual flags
            // the engine reads, not a sibling runtime copy.
            let engine = runtime
                .build_engine_from_config(config, cancel, Some(handoff.app_state.clone()))
                .await;

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
            // SDK-side `/compact` short-circuit. If the prompt arrives as
            // a sentinel-prefixed string (slash-command handler output),
            // run manual compaction directly rather than sending the
            // sentinel text to the LLM as a user message.
            // TS parity: REPL.tsx command dispatcher routes /compact
            // through `compactConversation` rather than chat input.
            if let Some(req) = coco_commands::handlers::compact::parse_compact_sentinel(&prompt) {
                let combined: Vec<coco_messages::Message> = {
                    let h = history_handle.lock().await;
                    h.clone()
                };
                let mut history = MessageHistory::new();
                for m in combined {
                    history.push(m);
                }
                let custom_instructions = if req.custom_instructions.is_empty() {
                    None
                } else {
                    Some(req.custom_instructions)
                };
                let event_tx_opt = Some(event_tx.clone());
                engine
                    .run_manual_compact(&mut history, &event_tx_opt, custom_instructions)
                    .await;
                {
                    let mut h = history_handle.lock().await;
                    *h = history.messages;
                }
                return Ok(());
            }

            // SDK-side `/dream` short-circuit — fire auto-memory
            // consolidation directly. When the engine has no
            // `MemoryRuntime` (Feature::AutoMemory off), we silently
            // no-op. TS parity: `/dream` slash command.
            if coco_commands::handlers::dream::parse_dream_sentinel(&prompt).is_some() {
                if let Some(runtime) = engine.memory_runtime() {
                    let transcript_dir = std::path::PathBuf::from(".");
                    let now_ms = coco_memory::service::dream::DreamService::now_ms();
                    let _ = runtime
                        .dream
                        .maybe_consolidate(&transcript_dir, &[], now_ms)
                        .await;
                }
                return Ok(());
            }

            // SDK-side `/summary` short-circuit — force a 9-section
            // session-memory update.
            if coco_commands::handlers::summary::parse_summary_sentinel(&prompt).is_some() {
                if let Some(runtime) = engine.memory_runtime() {
                    let combined: Vec<coco_messages::Message> = {
                        let h = history_handle.lock().await;
                        h.clone()
                    };
                    let tokens = coco_compact::estimate_tokens(&combined);
                    let _ = runtime.session_memory.force(tokens).await;
                }
                return Ok(());
            }

            // SDK-side `/btw` short-circuit (D1). When the prompt is
            // the BTW sentinel emitted by `handlers::btw::handler`,
            // dispatch a one-shot fork via the runtime's
            // [`ForkDispatcher`] instead of mutating the parent
            // engine. The dispatcher builds a *fresh* engine, runs a
            // single turn against it, and returns the response text;
            // the parent's history and cache slot are untouched.
            //
            // TS parity: `commands/btw.ts` calls `runForkedAgent`
            // which constructs an `AgentQueryConfig` with
            // `lastCacheSafeParams`, runs one turn, and surfaces the
            // result as a meta message.
            if let Some(req) = coco_commands::handlers::btw::parse_btw_sentinel(&prompt) {
                let cache = engine.last_cache_safe_params().await;
                let response_text = match cache {
                    None => format!(
                        "{}\n(no parent turn yet — run a regular prompt first so /btw can share its cache)",
                        req.display_text
                    ),
                    Some(cache) => match runtime.current_fork_dispatcher().await {
                        None => format!(
                            "{}\n(fork dispatcher not installed — /btw requires CLI bootstrap)",
                            req.display_text
                        ),
                        Some(dispatcher) => {
                            let options = coco_query::forked_agent::one_shot_options("/btw");
                            match dispatcher
                                .dispatch(&cache, &options, &req.question, None)
                                .await
                            {
                                Ok(result) => {
                                    format!("{}\n\n{}", req.display_text, result.text)
                                }
                                Err(e) => {
                                    format!("{}\n(side-question failed: {e})", req.display_text)
                                }
                            }
                        }
                    },
                };
                // Surface the answer as a meta message — visible to
                // SDK consumers but flagged so future compaction
                // passes know it's not part of the main conversation.
                {
                    let mut h = history_handle.lock().await;
                    h.push(coco_messages::create_meta_message(&response_text));
                }
                return Ok(());
            }

            let new_user_msg = coco_messages::create_user_message(&prompt);
            let combined: Vec<coco_messages::Message> = {
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
                    {
                        let mut h = history_handle.lock().await;
                        *h = result.final_messages;
                    }
                    // Cancellation is reported as `Ok(QueryResult{cancelled:true})`
                    // by the engine, NOT as Err — see engine.rs's top-of-loop
                    // cancel check. Emit `TurnInterrupted` here so the SDK
                    // wire stream produces a terminal turn event in this
                    // path; without it, clients waiting for `turn/completed`
                    // / `turn/interrupted` would hang forever after a
                    // mid-flight `control/interrupt`.
                    if cancel_for_terminal.is_cancelled() {
                        let _ = event_tx_for_error
                            .send(CoreEvent::Protocol(
                                coco_types::ServerNotification::TurnInterrupted(
                                    coco_types::TurnInterruptedParams { turn_id: None },
                                ),
                            ))
                            .await;
                    }
                    Ok(())
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "QueryEngineRunner: engine returned error; \
                         user message already persisted to session history"
                    );
                    // Emit a wire-level terminal notification BEFORE the
                    // synthetic SessionResult. Without this the SDK
                    // client never sees `turn/failed` or `turn/interrupted`
                    // on the engine-bail path — `TurnCompleted` is only
                    // emitted on the Ok path, so the client would hang
                    // waiting for a terminator that never arrives.
                    let was_cancelled = cancel_for_terminal.is_cancelled();
                    let terminal = if was_cancelled {
                        coco_types::ServerNotification::TurnInterrupted(
                            coco_types::TurnInterruptedParams { turn_id: None },
                        )
                    } else {
                        coco_types::ServerNotification::TurnFailed(coco_types::TurnFailedParams {
                            error: e.to_string(),
                        })
                    };
                    let _ = event_tx_for_error.send(CoreEvent::Protocol(terminal)).await;

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
                        stop_reason: if was_cancelled {
                            "interrupted".into()
                        } else {
                            "engine_error".into()
                        },
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
