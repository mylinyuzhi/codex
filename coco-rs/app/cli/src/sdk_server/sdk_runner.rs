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
            // SDK turns honor the same settings-layered permission rules
            // as TUI / headless. Mirrors TS `loadPermissionRules()`;
            // before this wiring SDK turns ran with empty rule maps.
            let (allow_rules, deny_rules, ask_rules) =
                crate::permission_rule_loader::typed_permission_rules(&runtime_config.settings);
            let permission_rule_source_roots =
                crate::permission_rule_loader::permission_rule_source_roots(
                    &runtime_config.settings,
                    &runtime.original_cwd,
                );
            let config = QueryEngineConfig {
                model_id: handoff.model.clone(),
                permission_mode,
                context_window: 200_000,
                allow_rules,
                deny_rules,
                ask_rules,
                permission_rule_source_roots,
                max_output_tokens,
                max_turns: if max_turns > 0 {
                    max_turns
                } else {
                    runtime_config.loop_config.max_turns.unwrap_or(max_turns)
                },
                max_tokens: runtime_config.loop_config.max_tokens.map(i64::from),
                prompt_cache: runtime
                    .main_client()
                    .await
                    .supports_prompt_cache()
                    .then(|| coco_types::PromptCacheConfig {
                        mode: coco_types::PromptCacheMode::Auto,
                        ttl: coco_types::CacheTtl::OneHour,
                        scope: None,
                        requested_betas: Default::default(),
                        skip_cache_write: false,
                    }),
                system_prompt,
                streaming_tool_execution: runtime_config.loop_config.enable_streaming_tools,
                session_id: handoff.session_id.clone(),
                tool_config: runtime_config.tool.clone(),
                sandbox_config: runtime_config.sandbox.clone(),
                sandbox_state: runtime.sandbox_state(),
                memory_config: runtime_config.memory.clone(),
                shell_config: runtime_config.shell.clone(),
                web_fetch_config: runtime_config.web_fetch.clone(),
                web_search_config: runtime_config.web_search.clone(),
                compact: runtime_config.compact.clone(),
                features: std::sync::Arc::new(runtime_config.features.clone()),
                skill_overrides: std::sync::Arc::new(runtime_config.skill_overrides.clone()),
                tool_overrides: runtime_config.tool_overrides.clone(),
                // Inherit `--include-hook-events` from the runtime's
                // stored engine config so SDK turns honour the flag the
                // session was started with.
                include_hook_events: runtime.current_engine_config().await.include_hook_events,
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
                let combined: Vec<std::sync::Arc<coco_messages::Message>> = {
                    let h = history_handle.lock().await;
                    h.clone()
                };
                let mut history = MessageHistory::new();
                for arc in combined {
                    history.push_arc(arc);
                }
                let command_args = req.custom_instructions;
                let custom_instructions = if command_args.is_empty() {
                    None
                } else {
                    Some(command_args.clone())
                };
                let event_tx_opt = Some(event_tx.clone());
                let request = coco_query::ManualCompactRequest {
                    custom_instructions,
                    command_args,
                };
                engine
                    .run_manual_compact(&mut history, &event_tx_opt, request)
                    .await;
                {
                    let mut h = history_handle.lock().await;
                    *h = history.to_vec();
                }
                return Ok(());
            }

            // SDK-side `/dream` short-circuit — fire auto-memory
            // consolidation directly. When the engine has no
            // `MemoryRuntime` (Feature::AutoMemory off), we silently
            // no-op. TS parity: `/dream` slash command. Uses `force`
            // so the time / session / scan-throttle gates are
            // bypassed; the lock is still acquired.
            if coco_commands::handlers::dream::parse_dream_sentinel(&prompt).is_some() {
                if let Some(runtime) = engine.memory_runtime() {
                    let transcript_dir = runtime
                        .transcript_dir()
                        .map(std::path::Path::to_path_buf)
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let now_ms = coco_memory::service::dream::DreamService::now_ms();
                    let _ = runtime.dream.force(&transcript_dir, Vec::new, now_ms).await;
                }
                return Ok(());
            }

            // SDK-side `/rename [name]` short-circuit. The sentinel
            // arrives as the slash handler's first line; we resolve
            // the name (LLM-generated when `Auto`) and persist via
            // the shared helpers, then return without sending the
            // sentinel text to the LLM as a user message. TS parity:
            // `commands/rename/rename.ts` runs entirely client-side.
            if let Some(req) = coco_commands::parse_rename_sentinel(&prompt) {
                // Teammates can't rename — silently no-op for SDK
                // (no user-visible transcript) to mirror the TUI
                // guard without surfacing an error that wasn't
                // requested by an interactive user. Logged so the
                // call still leaves a trail.
                if coco_coordinator::identity::is_teammate() {
                    tracing::warn!("SDK rename ignored: session is a swarm teammate");
                    return Ok(());
                }

                let name = match req {
                    coco_commands::ParsedRename::Explicit(n) => n,
                    coco_commands::ParsedRename::Auto => {
                        match crate::session_rename::auto_generate_session_name(&runtime).await {
                            Ok(n) => n,
                            Err(err) => {
                                tracing::warn!(
                                    reason = ?err,
                                    "SDK rename auto-gen failed"
                                );
                                return Ok(());
                            }
                        }
                    }
                };
                if let Err(e) = crate::session_rename::persist_rename(&runtime, name.clone()).await
                {
                    tracing::warn!(
                        error = %e,
                        "SDK rename persist failed"
                    );
                }
                return Ok(());
            }

            // SDK-side `/summary` short-circuit — force a 9-section
            // session-memory update.
            if coco_commands::handlers::summary::parse_summary_sentinel(&prompt).is_some() {
                if let Some(runtime) = engine.memory_runtime() {
                    let combined: Vec<std::sync::Arc<coco_messages::Message>> = {
                        let h = history_handle.lock().await;
                        h.clone()
                    };
                    let tokens = coco_messages::estimate_tokens_for_messages(&combined);
                    // TS parity: walk history for the orphan-safe
                    // cursor signals (`sessionMemory.ts:441-442`).
                    let last_msg_id = combined
                        .last()
                        .and_then(|m| m.uuid())
                        .map(uuid::Uuid::to_string);
                    let had_tool_calls =
                        coco_messages::count_tool_calls_in_last_assistant_turn(&combined) > 0;
                    let _ = runtime
                        .session_memory
                        .force(tokens, last_msg_id, had_tool_calls)
                        .await;
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
                            let mut options =
                                coco_query::forked_agent::ForkedAgentOptions::for_label(
                                    coco_types::ForkLabel::SideQuestion,
                                );
                            options.can_use_tool = Some(coco_query::forked_agent::deny_all_handle(
                                "side question: tools disabled",
                            ));
                            match dispatcher
                                .dispatch(&cache, &options, &req.question, None)
                                .await
                            {
                                Ok(result) => {
                                    // P1 single-message walk; PR 4a will
                                    // promote this to the full
                                    // multi-message text walk pattern.
                                    let text = result
                                        .messages
                                        .iter()
                                        .rev()
                                        .find_map(|m| match m.as_ref() {
                                            coco_messages::Message::Assistant(a) => {
                                                match &a.message {
                                                    coco_llm_types::LlmMessage::Assistant {
                                                        content,
                                                        ..
                                                    } => content.iter().rev().find_map(|p| {
                                                        match p {
                                                    coco_llm_types::AssistantContentPart::Text(
                                                        t,
                                                    ) => Some(t.text.clone()),
                                                    _ => None,
                                                }
                                                    }),
                                                    _ => None,
                                                }
                                            }
                                            _ => None,
                                        })
                                        .unwrap_or_default();
                                    format!("{}\n\n{}", req.display_text, text)
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
                    h.push(std::sync::Arc::new(coco_messages::create_meta_message(
                        &response_text,
                    )));
                }
                return Ok(());
            }

            // TS parity (`processUserInput.ts:182-263`): fire
            // UserPromptSubmit hooks BEFORE the LLM call. Output
            // surfaces as `hook_*` reminders on the next reminder pass;
            // a blocking_error suppresses the turn (warns instead);
            // prevent_continuation keeps the prompt but skips the
            // engine.
            let prompt_hook_result = runtime.fire_user_prompt_submit_hooks(&prompt).await;
            if let Some(blocking) = &prompt_hook_result.blocking_error {
                let warning = format!(
                    "UserPromptSubmit hook blocked the turn: {}\n\nOriginal prompt: {prompt}",
                    blocking.blocking_error,
                );
                let warning_msg = std::sync::Arc::new(coco_messages::create_user_message(&warning));
                {
                    let mut h = history_handle.lock().await;
                    h.push(warning_msg.clone());
                }
                // I-1: emit so SDK observers see the warning row.
                let _ = event_tx
                    .send(CoreEvent::Protocol(
                        coco_types::ServerNotification::MessageAppended {
                            message: warning_msg,
                            session_id: String::new(),
                            agent_id: None,
                        },
                    ))
                    .await;
                let _ = event_tx
                    .send(CoreEvent::Protocol(
                        coco_types::ServerNotification::TurnFailed(coco_types::TurnFailedParams {
                            error: warning.clone(),
                        }),
                    ))
                    .await;
                return Ok(());
            }
            if prompt_hook_result.prevent_continuation {
                let stop_msg = prompt_hook_result
                    .stop_reason
                    .clone()
                    .map(|r| format!("Operation stopped by hook: {r}"))
                    .unwrap_or_else(|| "Operation stopped by hook".to_string());
                let prompt_msg = std::sync::Arc::new(coco_messages::create_user_message(&prompt));
                let stop_msg_obj =
                    std::sync::Arc::new(coco_messages::create_user_message(&stop_msg));
                {
                    let mut h = history_handle.lock().await;
                    h.push(prompt_msg.clone());
                    h.push(stop_msg_obj.clone());
                }
                // I-1: emit so SDK observers see both rows.
                let _ = event_tx
                    .send(CoreEvent::Protocol(
                        coco_types::ServerNotification::MessageAppended {
                            message: prompt_msg,
                            session_id: String::new(),
                            agent_id: None,
                        },
                    ))
                    .await;
                let _ = event_tx
                    .send(CoreEvent::Protocol(
                        coco_types::ServerNotification::MessageAppended {
                            message: stop_msg_obj,
                            session_id: String::new(),
                            agent_id: None,
                        },
                    ))
                    .await;
                return Ok(());
            }

            // Resolve `@`-mentions in the prompt to file-content
            // system-reminder messages. TS parity:
            // `getAttachmentMessages` from `processUserInput.ts:504` /
            // `query.ts:1580`. Shared helper now drives TUI / headless / SDK
            // identically — without this, headless and SDK clients
            // sending `@path/to/file` got the literal string instead of
            // the file's contents (the `at_mentioned_files` reminder
            // body claims content is "loaded into context" — this is
            // what makes that true).
            let cwd_path = std::path::Path::new(&handoff.cwd);
            let inputs = crate::at_mention_turn::resolve_turn_inputs_text_only(
                &prompt,
                cwd_path,
                &runtime.file_read_state,
            )
            .await;
            let new_msgs = crate::at_mention_turn::build_messages_for_turn(&inputs);
            // I-1 (Authority) — D2: emit MessageAppended for the new
            // turn messages BEFORE invoking the engine. The engine no
            // longer re-emits its initial turn_messages load (would
            // double-fire on every turn). Engines only emit for
            // newly-produced content (assistant turns, tool results,
            // system pushes) within the loop. See
            // `engine-tui-unified-transcript-plan.md` §5.2.
            for m in new_msgs.iter().cloned() {
                let _ = event_tx
                    .send(CoreEvent::Protocol(
                        coco_types::ServerNotification::MessageAppended {
                            message: std::sync::Arc::new(m),
                            session_id: String::new(),
                            agent_id: None,
                        },
                    ))
                    .await;
            }
            let combined: Vec<std::sync::Arc<coco_messages::Message>> = {
                let mut h = history_handle.lock().await;
                h.extend(new_msgs.iter().cloned().map(std::sync::Arc::new));
                h.clone()
            };
            if !inputs.mentioned_paths.is_empty() {
                engine
                    .note_mentioned_paths(inputs.mentioned_paths.clone())
                    .await;
            }

            // Clone the event channel so we can still emit on the
            // error path (the engine takes ownership of the original).
            let event_tx_for_error = event_tx.clone();
            let session_id_for_error = handoff.session_id.clone();

            match engine.run_with_messages(combined, event_tx).await {
                Ok(result) => {
                    info!(
                        turns = result.turns,
                        input_tokens = result.total_usage.input_tokens.total,
                        output_tokens = result.total_usage.output_tokens.total,
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
                        // SDK mode's only cancel entry is the
                        // `control/interrupt` client request, which is
                        // a user-initiated cancel. Mirrors TS where the
                        // SDK-control path also corresponds to
                        // `abortController.abort('user-cancel')`.
                        let _ = event_tx_for_error
                            .send(CoreEvent::Protocol(
                                coco_types::ServerNotification::TurnInterrupted(
                                    coco_types::TurnInterruptedParams {
                                        turn_id: None,
                                        reason: Some(coco_types::CancelReason::UserCancel),
                                    },
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
                            coco_types::TurnInterruptedParams {
                                turn_id: None,
                                reason: Some(coco_types::CancelReason::UserCancel),
                            },
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
                    Err(anyhow::anyhow!("{e}"))
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "sdk_runner.test.rs"]
mod tests;
