//! Session lifecycle impl for [`QueryEngine`].
//!
//! Owns the public entry points (`run`, `run_with_events`,
//! `run_with_messages`) and the orchestration around `run_session_loop`:
//! emitting `SessionStarted` / `SessionStateChanged(Running)` / `Idle` /
//! `SessionResult`, plus the hook-event forwarder bridge that carries
//! `HookExecutionEvent` from `coco-hooks` into `CoreEvent::Protocol`.
//!
//! TS parity: this is the analog of `print.ts` + `runHeadless()` —
//! every consumer of the SDK / TUI sees the same envelope of session
//! events, regardless of whether the inner loop succeeded or errored.
//!
//! Extracted from `engine.rs` so the multi-turn loop file can stay
//! focused on per-turn mechanics.

use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

use coco_hooks::orchestration::OrchestrationContext;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_messages::create_user_message;
use coco_types::TokenUsage;

use crate::CoreEvent;
use crate::ServerNotification;
use crate::config::QueryResult;
use crate::emit::emit_protocol;
use crate::emit::emit_protocol_owned;
use crate::engine::QueryEngine;
use crate::helpers::extract_last_assistant_text;
use crate::helpers::hook_outcome_to_status;
use crate::session_state::SessionStateTracker;

impl QueryEngine {
    /// Run the agent loop with event streaming from a text prompt.
    pub async fn run_with_events(
        &self,
        user_prompt: &str,
        event_tx: tokio::sync::mpsc::Sender<CoreEvent>,
    ) -> Result<QueryResult, coco_error::BoxedError> {
        let user_msg = std::sync::Arc::new(create_user_message(user_prompt));
        self.run_internal_with_messages(vec![user_msg], Some(event_tx))
            .await
    }

    /// Run the agent loop with pre-built messages (user + attachment messages).
    pub async fn run_with_messages(
        &self,
        messages: Vec<std::sync::Arc<Message>>,
        event_tx: tokio::sync::mpsc::Sender<CoreEvent>,
    ) -> Result<QueryResult, coco_error::BoxedError> {
        if messages.is_empty() {
            return Err(Box::new(coco_error::PlainError::new(
                "No messages to process",
                coco_error::StatusCode::InvalidArguments,
            )));
        }
        self.run_internal_with_messages(messages, Some(event_tx))
            .await
    }

    /// Run the agent loop with an initial user prompt (no event streaming).
    pub async fn run(&self, user_prompt: &str) -> Result<QueryResult, coco_error::BoxedError> {
        let user_msg = std::sync::Arc::new(create_user_message(user_prompt));
        self.run_internal_with_messages(vec![user_msg], None).await
    }

    /// Core internal implementation: user + attachment messages.
    ///
    /// First message is the user message (used for file history snapshot UUID).
    /// Subsequent messages are attachment messages (is_meta=true, system-reminder wrapped).
    ///
    /// Session lifecycle sequence (matches TS print.ts + QueryEngine.ts):
    /// 1. SessionStarted  (if bootstrap attached)   — TS: buildSystemInitMessage
    /// 2. SessionStateChanged(Running)              — TS: notifySessionStateChanged('running')
    /// 3. run_session_loop: turn-by-turn work       — TS: query() generator loop
    /// 4. SessionStateChanged(Idle)                 — TS: notifySessionStateChanged('idle')
    /// 5. SessionResult (success or error subtype)  — TS: SDKResultMessage
    ///
    /// Steps 1/2/4/5 fire regardless of success or error so SDK consumers
    /// always see a complete session envelope.
    #[tracing::instrument(
        skip_all,
        name = "session",
        fields(
            session_id = %self.config.session_id,
            agent_id = ?self.config.agent_id,
            model_id = %self.config.model_id,
            permission_mode = ?self.config.permission_mode,
        ),
    )]
    pub(crate) async fn run_internal_with_messages(
        &self,
        turn_messages: Vec<std::sync::Arc<Message>>,
        event_tx: Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) -> Result<QueryResult, coco_error::BoxedError> {
        info!(
            turn_message_count = turn_messages.len(),
            streaming_tools = self.config.streaming_tool_execution,
            max_turns = self.config.max_turns,
            query_source = %self.query_source_label(),
            fork_label = ?self.config.fork_label,
            "session entering agent loop"
        );

        // Single choke point for all SessionStateChanged emissions. Dedupes
        // consecutive identical states so the wire stream sees exactly one
        // emission per real edge. See plan file WS-4.
        let state_tracker = SessionStateTracker::new();

        // SessionStarted must happen before anything that can error.
        self.emit_session_started(&event_tx).await;

        // Running — agent is actively processing.
        state_tracker
            .transition_to(coco_types::SessionState::Running, &event_tx)
            .await;

        // Set up the Hook → CoreEvent forwarder as a structured child task.
        //
        // The forwarder is a `JoinHandle` owned by this function, cancelled
        // via a child `CancellationToken` off `self.cancel`, and drained at
        // the single exit point below. See plan file WS-5.
        //
        // TS: print.ts emits SDKHookStartedMessage/etc. directly from the
        // hook execution path; in Rust we use this child task so
        // orchestration stays independent of the coco-query event type.
        let hook_cancel = self.cancel.child_token();
        // TS `--include-hook-events` opt-in: only emit
        // `SDKHookStarted/Progress/Response` to the SDK stream when the
        // session was started with the flag. When disabled, skip the
        // forwarder channel entirely so the orchestration layer never
        // sees a sender (cheaper than emitting + dropping).
        let (hook_tx_opt, hook_forwarder_handle) =
            if event_tx.is_some() && self.config.include_hook_events {
                let (hook_event_tx, hook_event_rx) =
                    tokio::sync::mpsc::channel::<coco_hooks::HookExecutionEvent>(64);
                let core_tx = event_tx.clone();
                let handle = tokio::spawn(Self::forward_hook_events(
                    hook_event_rx,
                    core_tx,
                    hook_cancel.clone(),
                ));
                (Some(hook_event_tx), Some(handle))
            } else {
                (None, None)
            };

        // History is owned here so StopFailure can carry the last assistant
        // message text on the error path (TS parity: `executeStopFailureHooks`
        // pulls the text out of `messages` at the call site). On success the
        // QueryResult already exposes `response_text`.
        let mut history = MessageHistory::new();
        // Stamp F9 envelope so every emit from this engine invocation
        // carries the active session + agent identity.
        history.set_envelope(self.config.session_id.clone(), self.config.agent_id.clone());
        let result = self
            .run_session_loop(
                turn_messages,
                event_tx.clone(),
                &state_tracker,
                hook_tx_opt.clone(),
                &mut history,
            )
            .await;

        // Drain the hook forwarder before emitting Idle/SessionResult so any
        // in-flight hook events land on the wire before the session
        // terminator. Order matters: drop the sender FIRST so the
        // forwarder's `rx.recv()` sees channel-closed; then await the
        // handle with a bounded timeout so a runaway hook can't wedge
        // shutdown.
        drop(hook_tx_opt);
        if let Some(handle) = hook_forwarder_handle {
            const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
            match tokio::time::timeout(DRAIN_TIMEOUT, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(join_err)) => {
                    warn!(error = %join_err, "hook forwarder task panicked");
                }
                Err(_) => {
                    warn!("hook forwarder drain timed out; cancelling");
                    hook_cancel.cancel();
                }
            }
        }

        // Idle — turn-over signal; emit regardless of outcome.
        state_tracker
            .transition_to(coco_types::SessionState::Idle, &event_tx)
            .await;

        // Subagent finalize: drop this agent's tracking entry from the
        // shared `CacheBreakDetector` so a long-running parent session
        // doesn't accumulate stale subagent snapshots that would push
        // out the main thread's entry under the LRU cap. TS:
        // runAgent.ts:18 `cleanupAgentTracking(agentId)`.
        if let Some(agent_id) = self.config.agent_id.as_deref() {
            self.client.cache_break_cleanup_agent(agent_id).await;
        }

        // StopFailure — fire-and-forget hooks when the turn ended in an
        // API / runtime error rather than a clean stop. TS:
        // `executeStopFailureHooks()` (`utils/hooks.ts:3594`). Output
        // and exit codes are intentionally ignored — this is observability
        // only, not a recovery path. We swallow registry-level failures
        // so a misconfigured hook can't suppress the user-visible error.
        if let (Err(e), Some(hooks)) = (&result, &self.hooks) {
            let err_msg = e.to_string();
            let hook_ctx = self.orchestration_ctx();
            let last_text = extract_last_assistant_text(&history);
            let last_assistant_message = (!last_text.is_empty()).then_some(last_text);
            // TS classifies via a small enum (`rate_limit` / `auth` / …).
            // Without classification infrastructure here we pass a single
            // bucket; users match on `error_details` for the raw text.
            if let Err(hook_err) = coco_hooks::orchestration::execute_stop_failure(
                hooks,
                &hook_ctx,
                /*error_label*/ "unknown",
                Some(err_msg.as_str()),
                last_assistant_message.as_deref(),
            )
            .await
            {
                warn!(error = %hook_err, "StopFailure hook execution failed");
            }
        }

        // SessionResult — always emitted. On Err, we synthesize a minimal
        // QueryResult-like view so SDK consumers see a terminal `result`
        // event matching TS SDKResultErrorMessage.
        let params = match &result {
            Ok(qr) => self.build_session_result_params(qr, /*error_messages*/ Vec::new()),
            Err(e) => self.build_session_error_params(e.to_string()),
        };
        match &result {
            Ok(qr) => info!(
                turns = qr.turns,
                duration_ms = qr.duration_ms,
                duration_api_ms = qr.duration_api_ms,
                tokens_in = qr.total_usage.input_tokens,
                tokens_out = qr.total_usage.output_tokens,
                cancelled = qr.cancelled,
                budget_exhausted = qr.budget_exhausted,
                stop_reason = ?qr.stop_reason,
                "session complete"
            ),
            Err(e) => warn!(
                error = %e,
                "session terminated with error"
            ),
        }
        let _delivered = emit_protocol(
            &event_tx,
            ServerNotification::SessionResult(Box::new(params)),
        )
        .await;

        result
    }

    /// Emit the `SessionStarted` protocol event from attached bootstrap data.
    /// No-op if the engine was not built with `with_session_bootstrap()`.
    pub(crate) async fn emit_session_started(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) {
        let Some(bootstrap) = &self.session_bootstrap else {
            return;
        };
        // Wire format is whatever `PermissionMode`'s serde serialization
        // produces — now camelCase matching TS `PermissionModeSchema`.
        let permission_mode = serde_json::to_value(self.config.permission_mode)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "default".into());
        let tools = if bootstrap.tools.is_empty() {
            let stub_ctx = coco_tool_runtime::ToolUseContext::stub_for_filtering(
                self.config.features.clone(),
                self.config.tool_overrides.clone(),
                self.config.tool_filter.clone(),
                self.config.permission_mode,
            );
            self.tools
                .loaded_tools(&stub_ctx)
                .iter()
                .map(|t| t.name().to_string())
                .collect()
        } else {
            bootstrap.tools.clone()
        };
        // Snapshot LSP connectivity for the status bar. Sync read — the
        // adapter's `is_connected()` is backed by an AtomicBool refined
        // by bootstrap prewarm, so this reflects accurate running-state
        // at session-start time. Subsequent runtime changes (server
        // crash, reload) would need a separate notification to update.
        let lsp_active = self.lsp_handle.as_ref().is_some_and(|h| h.is_connected());

        let _delivered = emit_protocol(
            event_tx,
            ServerNotification::SessionStarted(coco_types::SessionStartedParams {
                session_id: self.config.session_id.clone(),
                protocol_version: bootstrap.protocol_version.clone(),
                cwd: bootstrap.cwd.clone(),
                model: self.config.model_id.clone(),
                permission_mode,
                tools,
                slash_commands: bootstrap.slash_commands.clone(),
                agents: bootstrap.agents.clone(),
                skills: bootstrap.skills.clone(),
                mcp_servers: bootstrap.mcp_servers.clone(),
                plugins: bootstrap.plugins.clone(),
                api_key_source: bootstrap.api_key_source.clone(),
                betas: bootstrap.betas.clone(),
                version: bootstrap.version.clone(),
                output_style: bootstrap.output_style.clone(),
                fast_mode_state: bootstrap.fast_mode_state,
                lsp_active,
            }),
        )
        .await;
    }

    /// Synthesize a `SessionResultParams` for the error path (when
    /// `run_session_loop` returned `Err`). Matches TS `SDKResultErrorSchema`.
    pub(crate) fn build_session_error_params(
        &self,
        error_msg: String,
    ) -> coco_types::SessionResultParams {
        coco_types::SessionResultParams {
            session_id: self.config.session_id.clone(),
            total_turns: 0,
            duration_ms: 0,
            duration_api_ms: 0,
            is_error: true,
            stop_reason: "error_during_execution".into(),
            total_cost_usd: 0.0,
            usage: TokenUsage::default(),
            model_usage: Default::default(),
            permission_denials: Vec::new(),
            result: None,
            errors: vec![error_msg],
            structured_output: None,
            fast_mode_state: None,
            num_api_calls: None,
        }
    }

    /// Build a `SessionResultParams` from a completed `QueryResult`.
    /// Matches TS `SDKResultMessage` shape (coreSchemas.ts:1407-1451).
    ///
    /// `error_messages` is propagated into the `errors` field (for TS
    /// `SDKResultErrorSchema` parity); success results pass an empty Vec.
    pub(crate) fn build_session_result_params(
        &self,
        qr: &QueryResult,
        error_messages: Vec<String>,
    ) -> coco_types::SessionResultParams {
        // Per-model usage aggregated from CostTracker.
        let model_usage = qr
            .cost_tracker
            .per_model
            .iter()
            .map(|(model, usage)| {
                (
                    model.clone(),
                    coco_types::SessionModelUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cache_read_input_tokens: usage.cache_read_input_tokens,
                        cache_creation_input_tokens: usage.cache_creation_input_tokens,
                        web_search_requests: usage.web_search_requests,
                        cost_usd: usage.cost_usd,
                        context_window: self.config.context_window,
                        max_output_tokens: self.config.max_output_tokens,
                    },
                )
            })
            .collect();

        let stop_reason = qr
            .stop_reason
            .clone()
            .unwrap_or_else(|| "end_turn".to_string());
        let is_error = qr.cancelled || qr.budget_exhausted || !error_messages.is_empty();

        coco_types::SessionResultParams {
            session_id: self.config.session_id.clone(),
            total_turns: qr.turns,
            duration_ms: qr.duration_ms,
            duration_api_ms: qr.duration_api_ms,
            is_error,
            stop_reason,
            total_cost_usd: qr.cost_tracker.total_cost_usd(),
            usage: qr.total_usage,
            model_usage,
            // Accumulated across PermissionDecision::Deny branches.
            permission_denials: qr.permission_denials.clone(),
            result: if is_error {
                None
            } else {
                Some(qr.response_text.clone())
            },
            errors: error_messages,
            structured_output: None,
            fast_mode_state: None,
            num_api_calls: Some(qr.cost_tracker.total_api_calls as i32),
        }
    }

    /// Build an orchestration context from the engine's config.
    pub(crate) fn orchestration_ctx(&self) -> OrchestrationContext {
        OrchestrationContext {
            session_id: self.config.session_id.clone(),
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: self.config.project_dir.clone(),
            permission_mode: Some(format!("{:?}", self.config.permission_mode)),
            // Main-thread orchestration: no subagent identity. Per-spawn
            // contexts are constructed in `coco-coordinator` via
            // `hook_ctx_for_subagent`.
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: self.config.disable_all_hooks,
            allow_managed_hooks_only: self.config.allow_managed_hooks_only,
            attachment_emitter: self.attachment_emitter(),
            sync_event_sink: self.sync_hook_buffer.clone(),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: self.async_hook_registry.clone(),
            llm_handle: self.hook_llm_handle.clone(),
            workspace_trust_accepted: None,
        }
    }

    /// Consume `HookExecutionEvent` from the orchestration layer and forward
    /// them as `CoreEvent::Protocol(HookStarted/Progress/Response)`.
    ///
    /// Associated function (no `&self`) so callers can drive a standalone
    /// task with `tokio::spawn(QueryEngine::forward_hook_events(...))`. Tests
    /// rely on this calling convention.
    ///
    /// TS: print.ts emits these directly from the hook execution path; in
    /// Rust we use a child task so orchestration stays independent of
    /// the coco-query event type.
    ///
    /// Graceful shutdown: the normal exit path is for the caller to drop
    /// the matching sender, which makes `rx.recv()` return `None` and
    /// drains any queued events before returning. The `cancel` token is
    /// a fast-path escape hatch for crash scenarios (e.g. the drain
    /// timeout in `run_internal_with_messages` has expired); when
    /// cancelled, pending events are discarded. See plan file WS-5.
    pub(crate) async fn forward_hook_events(
        mut rx: tokio::sync::mpsc::Receiver<coco_hooks::HookExecutionEvent>,
        core_tx: Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        cancel: CancellationToken,
    ) {
        let Some(core_tx) = core_tx else {
            return;
        };
        loop {
            let evt = tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(evt) => evt,
                    None => break,
                },
            };
            let notif = match evt {
                coco_hooks::HookExecutionEvent::Started {
                    hook_id,
                    hook_name,
                    hook_event,
                } => ServerNotification::HookStarted(coco_types::HookStartedParams {
                    hook_id,
                    hook_name,
                    hook_event,
                }),
                coco_hooks::HookExecutionEvent::Progress {
                    hook_id,
                    hook_name,
                    stdout,
                    stderr,
                } => ServerNotification::HookProgress(coco_types::HookProgressParams {
                    hook_id,
                    hook_name,
                    // The orchestration-layer event doesn't carry the
                    // hook event name on Progress; consumers can correlate
                    // via `hook_id` against the preceding Started event.
                    hook_event: String::new(),
                    stdout,
                    stderr,
                    output: String::new(),
                }),
                coco_hooks::HookExecutionEvent::Response {
                    hook_id,
                    hook_name,
                    exit_code,
                    stdout,
                    stderr,
                    outcome,
                } => ServerNotification::HookResponse(coco_types::HookResponseParams {
                    hook_id,
                    hook_name,
                    hook_event: String::new(),
                    // orchestration layer merges stdout into output on
                    // the raw event; expose both fields separately for
                    // SDK consumers.
                    output: stdout.clone(),
                    stdout,
                    stderr,
                    exit_code,
                    outcome: hook_outcome_to_status(outcome),
                }),
            };
            if !emit_protocol_owned(&core_tx, notif).await {
                break;
            }
        }
    }
}
