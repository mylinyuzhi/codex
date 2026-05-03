//! TUI runner — orchestrates TUI ↔ QueryEngine ↔ FileHistory.
//!
//! TS equivalent: REPL.tsx is the orchestrator (React component owns QueryEngine,
//! messages, file history, and permission state). In Rust we use an explicit
//! async task (`run_agent_driver`) since ratatui is not a reactive framework.
//!
//! Architecture:
//! ```text
//! ┌─────────────┐  UserCommand   ┌────────────────┐  LLM / tools  ┌────────────┐
//! │  TUI App    │ ──────────────>│  agent_driver   │ ──────────────>│ QueryEngine│
//! │  (ratatui)  │ <──────────────│  (tokio task)   │ <──────────────│            │
//! └─────────────┘ ServerNotif.   └────────────────┘  QueryEvent    └────────────┘
//!                                       │
//!                                 FileHistoryState
//! ```

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tracing::info;
use tracing::warn;

use coco_config::EnvKey;
use coco_config::env;
use coco_context::FileHistoryState;
use coco_context::attachment::Attachment;
use coco_inference::ApiClient;
use coco_query::CoreEvent;
use coco_query::ServerNotification;
use coco_tool_runtime::ToolRegistry;
use coco_tui::App;
use coco_tui::UserCommand;
use coco_tui::app::create_channels;
use coco_types::TuiOnlyEvent;
use tokio_util::sync::CancellationToken;

use crate::Cli;

/// Run the interactive TUI mode.
///
/// TS: launchRepl() → <REPL /> (React/Ink component).
/// Rust: spawns agent_driver as background task, runs TUI in foreground.
pub async fn run_tui(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;

    // Spawn the hot-reload loop FIRST. The reloader watches the four
    // settings layers + `providers.json` / `models.json` and publishes
    // a fresh `Arc<RuntimeConfig>` via `RuntimePublisher` on debounced
    // change. We take its initial snapshot as the canonical
    // `runtime_config` for this session so `RuntimeConfig` is built
    // exactly once at startup. Drop on `_reloader` aborts the spawned
    // task when `run_tui` returns.
    //
    // **Subscriber wiring is deferred.** The QueryEngine integration
    // that re-reads `tool_overrides` + `api_client` per turn off the
    // publisher lands as a separate change. Until then the published
    // updates are observed only via tracing.
    //
    // **Reloader spawn failure → fall back to a one-shot static
    // build.** Outside a Tokio runtime `RuntimeReloader::spawn`
    // returns Err; in that case (which shouldn't happen here, but
    // surface gracefully if it does) we build the config directly.
    let reload_opts = coco_config_reload::ReloadOptions::new(cwd.clone())
        .with_overrides(crate::cli_runtime_overrides(cli)?);
    let reload_opts = if let Some(path) = cli.settings.as_deref() {
        reload_opts.with_flag_settings(path)
    } else {
        reload_opts
    };
    let (_reloader, runtime_config) = match coco_config_reload::RuntimeReloader::spawn(reload_opts)
    {
        Ok(reloader) => {
            let snapshot = reloader.current();
            (Some(reloader), Arc::unwrap_or_clone(snapshot))
        }
        Err(e) => {
            tracing::warn!(error = %e, "config hot-reload disabled; using one-shot build");
            (None, crate::build_runtime_config_for_cli(cli, &cwd)?)
        }
    };
    let settings = &runtime_config.settings;

    // Resolve initial mode + bypass capability + run sudo/sandbox guard
    // in one shot. TS parity: `initialPermissionModeFromCLI` +
    // `isBypassPermissionsModeAvailable` + `setup.ts:395-442`.
    let startup = crate::resolve_startup_permission_state(cli, &settings.merged)?;
    let permission_mode = startup.mode;
    let bypass_permissions_available = startup.bypass_available;
    // `startup.notification` is surfaced in the TUI as a toast below,
    // once `app.state` exists. Headless paths (run_chat, run_sdk_mode)
    // eprintln it instead.

    // Model + client. `create_api_client` computes a real
    // `ProviderClientFingerprint` from the resolved `ProviderConfig`
    // (multi-provider-plan §11.1) — only the mock fallback uses the
    // test-grade default fingerprint.
    let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
    let (client, provider_api, model_id) = crate::create_api_client(&runtime_config, retry.clone());
    let mode = provider_api.map_or("mock", |api| api.as_str());

    // Main role fallback chain — populated from CLI `--fallback-model`
    // flags (repeatable) OR settings.models.main.fallbacks, whichever
    // the resolver produced. Fail-fast on any tier that can't build:
    // silently dropping a fallback would only surface under outage.
    let fallback_clients = crate::build_fallback_clients_for_role(
        &runtime_config,
        coco_types::ModelRole::Main,
        retry,
    )?;
    // Optional half-open recovery policy for Main. Defaults to
    // None (sticky fallback) unless settings.models.main.recovery
    // is configured.
    let recovery_policy = runtime_config
        .model_roles
        .recovery(coco_types::ModelRole::Main);

    // Tools
    let registry = ToolRegistry::new();
    coco_tools::register_all_tools(&registry);
    let tools = Arc::new(registry);

    // System prompt
    let system_prompt =
        crate::build_system_prompt_for_model(&cwd, &runtime_config, client.provider(), &model_id);

    // Session manager for auto-title persistence (F5). Built here so
    // `SessionRuntime::build` can borrow it and the cleanup task can
    // own it.
    let sessions_dir = coco_config::global_config::config_home().join("sessions");
    let session_manager = Arc::new(coco_session::SessionManager::new(sessions_dir));
    let _ = session_manager.create(&model_id, &cwd);
    {
        // Background housekeeping: prune session files older than the
        // default retention period. Mirrors TS `utils/cleanup.ts`
        // `DEFAULT_CLEANUP_PERIOD_DAYS = 30`. Fire-and-forget.
        let mgr = session_manager.clone();
        tokio::spawn(async move {
            let period = coco_session::default_cleanup_period();
            match tokio::task::spawn_blocking(move || mgr.cleanup_older_than(period)).await {
                Ok(Ok(n)) if n > 0 => {
                    tracing::info!(
                        target: "coco::session::cleanup",
                        removed = n,
                        "pruned old session files"
                    );
                }
                Ok(Err(e)) => tracing::warn!(
                    target: "coco::session::cleanup",
                    error = %e,
                    "cleanup_older_than failed"
                ),
                _ => {}
            }
        });
    }

    // Fast-role ModelSpec for auto-title generation (F5). Prefer the
    // JSON-first runtime config; keep the Anthropic Haiku fallback for
    // users who only configured an API key.
    let fast_model_spec = runtime_config
        .model_roles
        .get(coco_types::ModelRole::Fast)
        .cloned()
        .or_else(|| {
            runtime_config
                .providers
                .get("anthropic")
                .and_then(coco_config::ProviderConfig::resolve_api_key)
                .map(|_| coco_types::ModelSpec {
                    provider: "anthropic".to_string(),
                    api: coco_types::ProviderApi::Anthropic,
                    model_id: "claude-haiku-4-5-20251001".to_string(),
                    display_name: "Claude Haiku 4.5".to_string(),
                })
        });

    // P0: build channels FIRST so the TUI permission bridge can
    // capture the notification sender. Without this, the engine's
    // `PermissionDecision::Ask` path falls back to legacy auto-allow
    // (permission_controller.rs:100-107), which is the wrong default
    // for interactive sessions.
    let (command_tx, command_rx, notification_tx, notification_rx) = create_channels();
    let pending_approvals = coco_cli::tui_permission_bridge::new_pending_map();
    let tui_permission_bridge: coco_tool_runtime::ToolPermissionBridgeRef =
        Arc::new(coco_cli::tui_permission_bridge::TuiPermissionBridge::new(
            notification_tx.clone(),
            pending_approvals.clone(),
        ));

    // SessionRuntime owns every per-session subsystem (FileReadState,
    // SessionMemoryService, FileHistoryState, ToolAppState,
    // CompactionObserverRegistry, HookRegistry, history Mutex, etc.).
    // Both runners (TUI + SDK) share this construction; the per-turn
    // engine assembly below routes through `runtime.build_engine()`.
    let runtime = crate::session_runtime::SessionRuntime::build(
        crate::session_runtime::SessionRuntimeBuildOpts {
            cli,
            runtime_config: Arc::new(runtime_config),
            cwd: cwd.clone(),
            model_id: model_id.clone(),
            system_prompt,
            bypass_permissions_available,
            permission_mode,
            client,
            fallback_clients,
            recovery_policy,
            tools,
            session_manager,
            fast_model_spec,
            permission_bridge: Some(tui_permission_bridge),
        },
    )
    .await?;

    // P1: install agent-team wiring (SwarmAgentHandle + QueryEngineAdapter
    // factory) when `Feature::AgentTeams` is enabled. No-op otherwise.
    coco_cli::agent_handle_factory::install_agent_team(runtime.clone(), cwd.display().to_string())
        .await?;

    // TS parity: TUI users opt into per-spawn periodic AgentSummary
    // timers via `COCO_AGENT_SUMMARY_ENABLE` (TS uses an SDK control
    // message — `agentProgressSummaries: true` — that TUI sessions
    // can't send). Default off keeps LLM cost off the hot path.
    // Coordinator mode auto-enables independently and ignores this
    // flag (matches `AgentTool.tsx:750`).
    if coco_config::env::is_env_truthy(coco_config::EnvKey::CocoAgentSummaryEnable) {
        runtime
            .app_state
            .write()
            .await
            .agent_progress_summaries_enabled = true;
    }

    // Create TUI app
    let mut app = App::new(command_tx, notification_rx)
        .map_err(|e| anyhow::anyhow!("Failed to create TUI: {e}"))?;

    // Wire file_history_enabled into TUI session state so the rewind
    // overlay knows whether to show code restore options.
    app.state_mut().session.file_history_enabled = runtime.file_history.is_some();

    // Seed the capability gate that controls both Shift+Tab cycle
    // (`PermissionMode::next_in_cycle`) and the plan-mode exit
    // overlay's "Bypass" option. Matches engine_config below so the
    // engine and TUI share one truth. Static for session lifetime.
    app.state_mut().session.bypass_permissions_available = bypass_permissions_available;
    app.state_mut().session.permission_mode = permission_mode;

    // Surface the startup downgrade notification (if any) as a toast
    // so interactive users see it. Headless paths eprintln it; the
    // TUI swallows stderr.
    if let Some(msg) = startup.notification {
        app.state_mut()
            .ui
            .add_toast(coco_tui::state::ui::Toast::warning(msg));
    }

    // Spawn agent driver — owns the SessionRuntime + transports.
    let driver_handle = tokio::spawn(run_agent_driver(
        command_rx,
        notification_tx,
        runtime,
        pending_approvals,
    ));

    eprintln!("coco-rs TUI ({mode} mode) — model: {model_id}\n");

    // Run TUI (blocks until exit)
    let tui_result = app.run().await;

    // Wait for agent driver
    let _ = driver_handle.await;

    tui_result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))
}

/// Agent driver — consumes UserCommands, drives QueryEngine, emits CoreEvents.
///
/// TS: REPL.tsx's onSubmit → query() → onQueryEvent() loop.
/// Runs as a background tokio task alongside the TUI event loop.
///
/// Events flow directly as `CoreEvent` from QueryEngine → TUI (no mapping layer).
async fn run_agent_driver(
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<CoreEvent>,
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    pending_approvals: coco_cli::tui_permission_bridge::PendingApprovals,
) {
    // One-shot gate: title gen runs at most once per driver instance.
    // `Arc<AtomicBool>` because the SubmitInput body now runs in a
    // spawned task; the outer-scope flag must stay reachable across
    // task boundaries so subsequent turns observe the latch.
    let title_gen_attempted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    info!("Agent driver started");

    // Active-turn tracker. SubmitInput spawns the engine work into a
    // dedicated task and stores its `JoinHandle` + `CancellationToken`
    // here; the dispatch loop continues to `recv()` so interrupting
    // commands (`Interrupt`, `ClearConversation`, `Compact`, `Rewind`,
    // `Shutdown`) reach their arms without waiting for the engine to
    // finish. TS parity: REPL.tsx's `query()` runs in the same single-
    // threaded React event loop, so its keyboard `useInput` hook fires
    // `abortController.abort()` "concurrently" with engine work — JS
    // cooperative-async makes that natural; Rust needs an explicit
    // `tokio::spawn` to free the recv loop.
    struct ActiveTurn {
        task: tokio::task::JoinHandle<()>,
        cancel: CancellationToken,
    }
    let active_turn: Arc<Mutex<Option<ActiveTurn>>> = Arc::new(Mutex::new(None));

    /// Cancel the in-flight turn (if any) and await its completion.
    /// Used by every arm whose semantics conflict with a concurrent
    /// turn (Clear / Compact / Rewind / Shutdown / next SubmitInput).
    async fn drain_active_turn(slot: &Arc<Mutex<Option<ActiveTurn>>>) {
        let state = { slot.lock().await.take() };
        if let Some(s) = state {
            s.cancel.cancel();
            let _ = s.task.await;
        }
    }

    while let Some(command) = command_rx.recv().await {
        // Re-read each turn so `/clear` regen picks up the new id.
        let session_id = runtime.current_session_id().await;
        match command {
            UserCommand::SubmitInput {
                user_message_id,
                content,
                images,
                ..
            } => {
                if content.is_empty() {
                    continue;
                }

                // Defensive drain: TUI input layer gates submit on
                // `running` state, but a slow gate could still let a
                // second SubmitInput through. Cancel + await the prior
                // turn before starting the new one — last-write-wins
                // semantics, matches TS REPL.tsx behavior where a new
                // onSubmit aborts the previous query() generator.
                drain_active_turn(&active_turn).await;

                let turn_cancel = CancellationToken::new();
                let cancel_for_state = turn_cancel.clone();

                let runtime_t = runtime.clone();
                let event_tx_t = event_tx.clone();
                let title_gen_attempted_t = title_gen_attempted.clone();
                let session_id_t = session_id.clone();

                let task = tokio::spawn(async move {
                    process_submit_turn(
                        user_message_id,
                        content,
                        images,
                        runtime_t,
                        event_tx_t,
                        title_gen_attempted_t,
                        session_id_t,
                        turn_cancel,
                    )
                    .await;
                });

                *active_turn.lock().await = Some(ActiveTurn {
                    task,
                    cancel: cancel_for_state,
                });
            }

            UserCommand::Rewind {
                message_id,
                restore_type,
                rewound_turn,
            } => {
                // Drain first — rewind reads file_history snapshots
                // and rewrites runtime.history; an in-flight turn that
                // mutates either would race.
                drain_active_turn(&active_turn).await;
                handle_rewind(
                    &restore_type,
                    &message_id,
                    rewound_turn,
                    &runtime.file_history,
                    &runtime.config_home,
                    &session_id,
                    &event_tx,
                    &runtime.history,
                    &runtime.client,
                )
                .await;
            }

            UserCommand::RequestDiffStats { message_id } => {
                // Async diff stats computation.
                // TS: fileHistoryGetDiffStats() in MessageSelector useEffect.
                // Emitted as CoreEvent::Tui since this is a UI-only event.
                if let Some(fh) = &runtime.file_history {
                    let fh = fh.read().await;
                    let (files, ins, del, paths) = match fh
                        .get_diff_stats(&message_id, &runtime.config_home, &session_id)
                        .await
                    {
                        Ok(stats) => {
                            let paths: Vec<String> = stats
                                .files_changed
                                .iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect();
                            (paths.len() as i32, stats.insertions, stats.deletions, paths)
                        }
                        Err(_) => (0, 0, 0, Vec::new()),
                    };
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::DiffStatsReady {
                            message_id,
                            files_changed: files,
                            insertions: ins,
                            deletions: del,
                            file_paths: paths,
                        }))
                        .await;
                }
            }

            UserCommand::Interrupt => {
                // Mid-turn cancel: read the active turn's cancel token
                // and fire it. The spawned turn task observes the
                // token at the next `.await` point inside
                // `engine.run_with_messages` (LLM streaming, tool
                // execution, hook orchestration all check the parent
                // CancellationToken) and exits cleanly. The task slot
                // stays Some until the task naturally completes — the
                // next SubmitInput (or driver shutdown) drains it.
                // TS parity: REPL.tsx Esc/Ctrl+C → abortController
                // .abort() → query() generator yields and returns.
                if let Some(state) = active_turn.lock().await.as_ref() {
                    state.cancel.cancel();
                    info!("Interrupt: cancelled active turn");
                }
            }

            UserCommand::Compact {
                custom_instructions,
            } => {
                // Manual `/compact [instructions]` from the TUI.
                // TS: commands/compact/compact.ts:40 — `args.trim()`
                // becomes `customInstructions`. Build a transient engine
                // sharing the same registries / state and drive
                // `run_manual_compact`. The session memory short-circuit
                // and PreCompact/PostCompact hooks are owned inside that
                // method.
                info!(
                    session_id = %session_id,
                    has_instructions = custom_instructions.is_some(),
                    "TUI: manual /compact"
                );
                // Drain any active turn before compacting — compact
                // mutates the same `runtime.history` and runs an LLM
                // call that races with the in-flight engine.
                drain_active_turn(&active_turn).await;
                let compact_cancel = CancellationToken::new();
                let engine = runtime.build_engine(compact_cancel).await;
                let history_msgs = runtime.history.lock().await.clone();
                let mut history = coco_messages::MessageHistory::new();
                for m in history_msgs {
                    history.push(m);
                }
                let event_tx_opt = Some(event_tx.clone());
                engine
                    .run_manual_compact(&mut history, &event_tx_opt, custom_instructions)
                    .await;
                {
                    let mut h = runtime.history.lock().await;
                    *h = history.messages;
                }
            }

            UserCommand::SetPermissionMode { mode } => {
                let cur_session_id = runtime.current_session_id().await;
                let cfg = runtime.current_engine_config().await;
                if mode == coco_types::PermissionMode::BypassPermissions
                    && !cfg.bypass_permissions_available
                {
                    warn!(
                        session_id = %cur_session_id,
                        requested = ?mode,
                        "TUI SetPermissionMode denied: bypass capability gate is off"
                    );
                    continue;
                }
                let prev_mode = cfg.permission_mode;
                runtime
                    .update_engine_config(|cfg| cfg.permission_mode = mode)
                    .await;
                {
                    let mut guard = runtime.app_state.write().await;
                    guard.permission_mode = Some(mode);
                    coco_permissions::apply_auto_transition_to_app_state(
                        &mut guard, prev_mode, mode,
                    );
                }
                info!(
                    session_id = %cur_session_id,
                    from = ?prev_mode,
                    to = ?mode,
                    "TUI SetPermissionMode propagated to engine_config + app_state",
                );
            }

            UserCommand::ClearConversation { scope } => {
                // Drain first — clear_conversation mutates session_id
                // and resets file_read_state / SM / cache-break; an
                // in-flight turn writing into those would observe a
                // half-cleared state.
                drain_active_turn(&active_turn).await;
                if let Err(e) = runtime.clear_conversation(scope).await {
                    warn!(error = %e, "/clear failed");
                }
            }

            UserCommand::PlanApprovalResponse {
                request_id,
                teammate_agent,
                approved,
                feedback,
            } => {
                // Leader responding to a teammate's plan-approval
                // request. Write a `PlanApprovalResponse` envelope into
                // the teammate's inbox; their `poll_teammate_approval`
                // picks it up on the next turn boundary. TS parity:
                // leader-side resolution of `ExitPlanModeV2Tool.ts:137-141`.
                let team_name = match env::var(EnvKey::CocoTeamName) {
                    Ok(t) if !t.is_empty() => t,
                    _ => {
                        info!(%request_id, "PlanApprovalResponse: no COCO_TEAM_NAME; dropping");
                        continue;
                    }
                };
                let agent_name =
                    env::var(EnvKey::CocoAgentName).unwrap_or_else(|_| "team-lead".to_string());
                let mailbox: coco_tool_runtime::MailboxHandleRef =
                    Arc::new(coco_coordinator::mailbox::SwarmMailboxHandle);

                let response = coco_tool_runtime::PlanApprovalMessage::PlanApprovalResponse(
                    coco_tool_runtime::PlanApprovalResponse {
                        request_id: request_id.clone(),
                        approved,
                        feedback: feedback.clone(),
                        permission_mode: None,
                    },
                );
                let envelope = coco_tool_runtime::MailboxEnvelope {
                    text: serde_json::to_string(&response).unwrap_or_default(),
                    from: agent_name.clone(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                if let Err(e) = mailbox
                    .write_to_mailbox(&teammate_agent, &team_name, envelope)
                    .await
                {
                    info!(%request_id, error = %e, "failed to write PlanApprovalResponse");
                } else {
                    // Clear the leader-side awaiting flag so the
                    // reminder can stop nagging about this request.
                    let mut guard = runtime.app_state.write().await;
                    if guard.awaiting_plan_approval_request_id.as_deref()
                        == Some(request_id.as_str())
                    {
                        guard.awaiting_plan_approval = false;
                        guard.awaiting_plan_approval_request_id = None;
                    }
                }
            }

            UserCommand::ApprovalResponse {
                request_id,
                approved,
                always_allow: _, // TS persists rule via permission_updates; today we route the boolean
                feedback,
                updated_input: _, // TS edits the tool input pre-approval; that path lands later
                permission_updates: _, // applied separately via the permission ruleset
            } => {
                // P0: route the user's Approve / Deny back to the
                // pending oneshot the `TuiPermissionBridge` is awaiting.
                // Stale request_ids (already resolved or timed-out)
                // are logged and dropped — TS does the same when an
                // overlay closes after the engine moved on.
                let resolved = coco_cli::tui_permission_bridge::resolve_pending(
                    &pending_approvals,
                    &request_id,
                    approved,
                    feedback,
                )
                .await;
                if !resolved {
                    info!(
                        %request_id,
                        approved,
                        "ApprovalResponse for unknown request_id (already resolved or stale)"
                    );
                }
            }

            UserCommand::Shutdown => {
                // Drain in-flight turn before emitting SessionEnded so
                // the engine stops promptly and any pending events
                // flush through `event_tx` ahead of the lifecycle
                // notification.
                drain_active_turn(&active_turn).await;
                let _ = event_tx
                    .send(CoreEvent::Protocol(ServerNotification::SessionEnded(
                        coco_types::SessionEndedParams {
                            reason: "User shutdown".into(),
                        },
                    )))
                    .await;
                break;
            }

            // Other commands: log and skip for now
            other => {
                info!(?other, "Unhandled UserCommand in agent driver");
            }
        }
    }

    // Driver loop exited (sender dropped or Shutdown). Drain any
    // turn that's still running so we don't leak a JoinHandle.
    drain_active_turn(&active_turn).await;
    info!("Agent driver stopped");
}

/// Body of `UserCommand::SubmitInput` extracted into an async fn so
/// it can be `tokio::spawn`ed. The dispatch loop stores the
/// `JoinHandle` in `active_turn` and continues to recv the next
/// command — letting `Interrupt` / `ClearConversation` / `Compact` /
/// `Rewind` / `Shutdown` reach their arms while the engine runs.
///
/// All session-scoped Arcs are read out of `runtime` inside the body —
/// the only data piped in are the per-turn user inputs, the cancel
/// token, the cross-turn `title_gen_attempted` latch, and the snapshot
/// of `session_id` taken on the dispatcher side (so the title-gen path
/// uses the same id the rest of the turn observed, not a later
/// `/clear`-regenerated one).
#[allow(clippy::too_many_arguments)]
async fn process_submit_turn(
    user_message_id: String,
    content: String,
    images: Vec<coco_tui::paste::ImageData>,
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    event_tx: mpsc::Sender<CoreEvent>,
    title_gen_attempted: Arc<std::sync::atomic::AtomicBool>,
    session_id: String,
    turn_cancel: CancellationToken,
) {
    // Resolve @mentions into attachments.
    let processed = coco_context::process_user_input(&content);
    let cwd = std::env::current_dir().unwrap_or_default();

    let (file_attachments, changed_file_attachments) = {
        let mut frs = runtime.file_read_state.write().await;
        let file_attachments = coco_context::resolve_mentions(
            &processed.mentions,
            &mut frs,
            &coco_context::MentionResolveOptions {
                cwd: &cwd,
                max_dir_entries: 1000,
            },
        )
        .await;
        let changed_file_attachments = coco_context::detect_changed_files(&mut frs).await;
        (file_attachments, changed_file_attachments)
    };

    let user_uuid =
        uuid::Uuid::parse_str(&user_message_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
    let new_turn_messages = build_turn_messages_with_uuid(
        user_uuid,
        &content,
        &images,
        &file_attachments,
        &changed_file_attachments,
    );

    // Persist user message immediately so engine errors don't lose it.
    let messages: Vec<coco_messages::Message> = {
        let mut h = runtime.history.lock().await;
        h.extend(new_turn_messages.iter().cloned());
        h.clone()
    };

    let engine = runtime.build_engine(turn_cancel.clone()).await;

    // Mention priority for post-compact restoration.
    let mentioned_abs: Vec<std::path::PathBuf> = file_attachments
        .iter()
        .filter_map(|att| match att {
            coco_context::attachment::Attachment::File(f) => {
                Some(std::path::PathBuf::from(&f.filename))
            }
            coco_context::attachment::Attachment::AlreadyReadFile(f) => {
                Some(std::path::PathBuf::from(&f.filename))
            }
            _ => None,
        })
        .collect();
    if !mentioned_abs.is_empty() {
        engine.note_mentioned_paths(mentioned_abs).await;
    }

    let (core_event_tx, mut core_event_rx) = mpsc::channel::<CoreEvent>(256);
    let event_tx_clone = event_tx.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(ev) = core_event_rx.recv().await {
            let _ = event_tx_clone.send(ev).await;
        }
    });

    match engine.run_with_messages(messages, core_event_tx).await {
        Ok(result) => {
            let mut h = runtime.history.lock().await;
            *h = result.final_messages;
        }
        Err(e) => {
            // User message stays in `runtime.history` from the
            // pre-engine push above. Surface failure as TurnFailed so
            // TUI can render it.
            let _ = event_tx
                .send(CoreEvent::Protocol(ServerNotification::TurnFailed(
                    coco_types::TurnFailedParams {
                        error: e.to_string(),
                    },
                )))
                .await;
        }
    }

    let _ = forward_handle.await;

    maybe_spawn_auto_title(&runtime, &title_gen_attempted, &session_id).await;
}

/// One-shot, fire-and-forget title generation. Returns immediately
/// without spawning if any precondition (auto-title disabled, already
/// attempted, no Fast spec, plan not exited, plan empty) fails.
async fn maybe_spawn_auto_title(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    title_gen_attempted: &Arc<std::sync::atomic::AtomicBool>,
    session_id: &str,
) {
    let plan_exited = runtime.app_state.read().await.has_exited_plan_mode;
    let plans_dir = coco_context::resolve_plans_directory(
        &runtime.config_home,
        /*project_dir*/ None,
        /*setting*/ None,
    );
    let plan_text = coco_context::get_plan(session_id, &plans_dir, /*agent_id*/ None);
    let plan_non_empty = plan_text
        .as_deref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    let already_attempted = title_gen_attempted.load(std::sync::atomic::Ordering::Acquire);
    if !should_trigger_title_gen(
        runtime.auto_title_enabled,
        already_attempted,
        runtime.fast_model_spec.is_some(),
        plan_exited,
        plan_non_empty,
    ) {
        return;
    }
    let (Some(spec), Some(text)) = (runtime.fast_model_spec.clone(), plan_text) else {
        return;
    };
    title_gen_attempted.store(true, std::sync::atomic::Ordering::Release);
    spawn_auto_title_task(
        spec,
        text,
        runtime.session_manager.clone(),
        session_id.to_string(),
        runtime.runtime_config.clone(),
    );
}

/// Handle a rewind command.
///
/// TS: REPL.tsx rewindConversationTo() + fileHistoryRewind()
/// - Code rewind: calls file_history.rewind() to restore files
/// - Conversation rewind: truncates the agent-side history_handle
///   AND emits RewindCompleted so the TUI truncates its display.
/// - Both: does both
#[allow(clippy::too_many_arguments)]
async fn handle_rewind(
    restore_type: &coco_tui::state::RestoreType,
    message_id: &str,
    rewound_turn: i32,
    file_history: &Option<Arc<RwLock<FileHistoryState>>>,
    config_home: &std::path::Path,
    session_id: &str,
    event_tx: &mpsc::Sender<CoreEvent>,
    history_handle: &Arc<Mutex<Vec<coco_messages::Message>>>,
    client: &Arc<ApiClient>,
) {
    use coco_tui::state::RestoreType;

    let mut files_changed = 0i32;
    let mut messages_removed = 0i32;

    // Summarize variants: dispatch to partial_compact_conversation
    // and replace the history with the resulting messages. TS:
    // `screens/REPL.tsx:4918-4988` (`onSummarize` branch).
    if matches!(
        restore_type,
        RestoreType::SummarizeFrom { .. } | RestoreType::SummarizeUpTo { .. }
    ) {
        handle_summarize_rewind(restore_type, message_id, history_handle, client, event_tx).await;
        return;
    }

    // Code rewind (file restore)
    // TS: fileHistoryRewind() in REPL.tsx onRestoreCode prop
    // CodeOnly + Both restore files; Summarize variants do NOT
    // restore files (TS parity: summarize keeps the workspace
    // intact, only the conversation is rewritten).
    if matches!(restore_type, RestoreType::Both | RestoreType::CodeOnly)
        && let Some(fh) = file_history
    {
        let fh = fh.read().await;
        match fh.rewind(message_id, config_home, session_id).await {
            Ok(changed) => {
                files_changed = changed.len() as i32;
                info!(files_changed, message_id, "File history rewind completed");
            }
            Err(e) => {
                warn!("File history rewind failed: {e}");
                let _ = event_tx
                    .send(CoreEvent::Protocol(ServerNotification::Error(
                        coco_types::ErrorParams {
                            message: format!("File rewind failed: {e}"),
                            category: Some("rewind".into()),
                            retryable: false,
                        },
                    )))
                    .await;
                return;
            }
        }
    }

    // Conversation rewind: truncate the agent-side history at the
    // target message, emit TuiOnlyEvent so the TUI mirrors the
    // truncate on its display side.
    // TS: rewindConversationTo() + restoreMessageSync() in REPL.tsx
    let should_truncate = matches!(
        restore_type,
        RestoreType::Both | RestoreType::ConversationOnly
    );

    if should_truncate {
        let mut h = history_handle.lock().await;
        if let Some(idx) = h.iter().position(|m| match m {
            coco_messages::Message::User(u) => u.uuid.to_string() == message_id,
            _ => false,
        }) {
            let pre_count = h.len() as i32;
            messages_removed = (pre_count - idx as i32).max(0);
            h.truncate(idx);
            // TS `tengu_conversation_rewind` (`screens/REPL.tsx:3665-3670`).
            coco_otel::events::emit_conversation_rewind(
                pre_count as i64,
                h.len() as i64,
                messages_removed as i64,
                idx as i64,
            );
        }
    }

    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::RewindCompleted {
            target_message_id: if should_truncate {
                message_id.to_string()
            } else {
                String::new()
            },
            files_changed,
        }))
        .await;

    // Protocol-level event for SDK consumers (Phase 3.2). Coco-rs ext
    // — TS doesn't emit a wire event for rewind because the React
    // state-update is the source of truth.
    let _ = event_tx
        .send(CoreEvent::Protocol(ServerNotification::RewindCompleted(
            coco_types::RewindCompletedParams {
                rewound_turn,
                restored_files: files_changed,
                messages_removed,
            },
        )))
        .await;
}

/// Run `partial_compact_conversation` for SummarizeFrom / SummarizeUpTo
/// rewind options, replace the agent history with the result, and
/// emit a TUI signal to mirror the truncation in the display.
///
/// TS: `screens/REPL.tsx:4918-4988` (`onSummarize`). Direction
/// mapping: `SummarizeFrom` ↔ TS `'from'` (== `Newest` in coco-rs);
/// `SummarizeUpTo` ↔ TS `'up_to'` (== `Oldest` in coco-rs).
async fn handle_summarize_rewind(
    restore_type: &coco_tui::state::RestoreType,
    message_id: &str,
    history_handle: &Arc<Mutex<Vec<coco_messages::Message>>>,
    client: &Arc<ApiClient>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    use coco_messages::PartialCompactDirection;
    use coco_tui::state::RestoreType;

    let (direction, feedback) = match restore_type {
        RestoreType::SummarizeFrom { feedback } => (PartialCompactDirection::Newest, feedback),
        RestoreType::SummarizeUpTo { feedback } => (PartialCompactDirection::Oldest, feedback),
        _ => return,
    };

    let messages = {
        let h = history_handle.lock().await;
        h.clone()
    };

    // Pivot index: position of the picked user message in the
    // history vec.
    let pivot_index = match messages.iter().position(|m| match m {
        coco_messages::Message::User(u) => u.uuid.to_string() == message_id,
        _ => false,
    }) {
        Some(i) => i,
        None => {
            warn!(
                message_id,
                "summarize-rewind: target message not found in history"
            );
            let _ = event_tx
                .send(CoreEvent::Protocol(coco_query::ServerNotification::Error(
                    coco_types::ErrorParams {
                        message: "summarize: message not in active history".into(),
                        category: Some("rewind".into()),
                        retryable: false,
                    },
                )))
                .await;
            return;
        }
    };

    // Summarize closure — same shape as engine.rs's full-compact path.
    let summarize_fn = |prompt: String| {
        let client = client.clone();
        async move {
            use coco_inference::QueryParams;
            use coco_messages::AssistantContent;
            use coco_messages::LlmMessage;
            let params = QueryParams {
                prompt: vec![LlmMessage::user_text(&prompt)],
                max_tokens: Some(coco_compact::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY),
                thinking_level: None,
                fast_mode: false,
                tools: None,
                context_management: None,
                query_source: None,
                agent_id: None,
                time_since_last_assistant_ms: None,
            };
            match client.query(&params).await {
                Ok(result) => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(t) => Some(t.text.as_str()),
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

    match coco_compact::partial_compact_conversation(
        &messages,
        pivot_index,
        direction,
        feedback.as_deref(),
        /*custom_instructions*/ None,
        summarize_fn,
        /*attachment_fn*/ None,
    )
    .await
    {
        Ok(result) => {
            let new_messages = coco_compact::build_post_compact_messages(&result);
            // Persist the summarized history back so the next turn
            // sees it. TS: setMessages(postCompact).
            {
                let mut h = history_handle.lock().await;
                *h = new_messages;
            }

            // Emit a RewindCompleted with empty target so the TUI
            // dismisses the overlay + shows a toast, but does NOT try
            // to truncate by message_id (the message is gone after
            // summarization).
            let _ = event_tx
                .send(CoreEvent::Tui(TuiOnlyEvent::RewindCompleted {
                    target_message_id: String::new(),
                    files_changed: 0,
                }))
                .await;

            // Protocol-level event so SDK consumers see it too.
            let _ = event_tx
                .send(CoreEvent::Protocol(
                    coco_query::ServerNotification::ContextCompacted(
                        coco_types::ContextCompactedParams {
                            removed_messages: 0,
                            summary_tokens: result.post_compact_tokens as i32,
                            trigger: coco_types::CompactTrigger::Manual,
                            pre_tokens: Some(result.pre_compact_tokens),
                            post_tokens: Some(result.post_compact_tokens),
                        },
                    ),
                ))
                .await;
        }
        Err(e) => {
            warn!(error = %e, "partial-compact rewind failed");
            let _ = event_tx
                .send(CoreEvent::Protocol(coco_query::ServerNotification::Error(
                    coco_types::ErrorParams {
                        message: format!("Summarize failed: {e}"),
                        category: Some("rewind".into()),
                        retryable: false,
                    },
                )))
                .await;
        }
    }
}

/// Build a turn's messages with a caller-supplied user-message UUID.
///
/// The caller (TUI submit path) mints the UUID at input time so the
/// agent driver, file-history snapshot, and rewind picker all share
/// one identity for the turn's user message.
fn build_turn_messages_with_uuid(
    user_uuid: uuid::Uuid,
    text: &str,
    images: &[coco_tui::ImageData],
    file_attachments: &[Attachment],
    changed_file_attachments: &[Attachment],
) -> Vec<coco_messages::Message> {
    use coco_inference::UserContentPart;

    let mut messages = Vec::new();

    // 1. User message: text + clipboard images
    if images.is_empty() {
        messages.push(coco_messages::create_user_message_with_uuid(
            user_uuid, text,
        ));
    } else {
        let mut parts: Vec<UserContentPart> = vec![UserContentPart::text(text)];
        for img in images {
            parts.push(UserContentPart::image(img.bytes.clone(), &img.mime));
        }
        messages.push(coco_messages::create_user_message_with_parts_and_uuid(
            user_uuid, parts,
        ));
    }

    // 2. @mention attachment messages (separate, wrapped in system-reminder)
    for att in file_attachments {
        if let Some(msg) = attachment_to_message(att) {
            messages.push(msg);
        }
    }

    // 3. Changed file notification messages
    for att in changed_file_attachments {
        if let Some(msg) = changed_file_to_message(att) {
            messages.push(msg);
        }
    }

    messages
}

/// Convert a resolved @mention attachment into a system-reminder message.
///
/// TS: `normalizeAttachmentForAPI()` — wraps file content in synthetic
/// tool-use/tool-result pairs inside `<system-reminder>` tags.
fn attachment_to_message(att: &Attachment) -> Option<coco_messages::Message> {
    let read_tool = coco_types::ToolName::Read.as_str();
    let bash_tool = coco_types::ToolName::Bash.as_str();

    match att {
        Attachment::File(f) => {
            let text = format!(
                "Called the {read_tool} tool with the following input: \
                 {{\"file_path\":\"{}\"}}\n\
                 Result of calling the {read_tool} tool:\n{}",
                f.filename, f.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        Attachment::Image(img) => {
            if let Some(b64) = &img.base64_data {
                use coco_inference::FilePart;
                use coco_inference::UserContentPart;
                let parts = vec![
                    UserContentPart::text(coco_messages::wrapping::wrap_in_system_reminder(
                        &format!(
                            "Called the {read_tool} tool with the following input: \
                             {{\"file_path\":\"{}\"}}",
                            img.filename
                        ),
                    )),
                    UserContentPart::File(FilePart::image_base64(b64, &img.media_type)),
                ];
                Some(coco_messages::create_user_message_with_parts(parts))
            } else {
                None
            }
        }
        Attachment::Directory(d) => {
            let text = format!(
                "Called the {bash_tool} tool with the following input: \
                 {{\"command\":\"ls {}\",\"description\":\"Lists files in {}\"}}\n\
                 Result of calling the {bash_tool} tool:\n{}",
                d.display_path, d.display_path, d.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        Attachment::AlreadyReadFile(_) | Attachment::AgentMention(_) => None,
        _ => None,
    }
}

/// Convert a changed-file attachment into a notification message.
///
/// Decide whether the driver should fire an auto-title task this turn.
///
/// Pure gate function factored out of the driver loop so we can unit
/// test the precedence without spinning up a real engine. All five
/// conditions must hold; missing any single one short-circuits.
fn should_trigger_title_gen(
    auto_title_enabled: bool,
    already_attempted: bool,
    fast_spec_present: bool,
    plan_has_exited: bool,
    plan_text_non_empty: bool,
) -> bool {
    auto_title_enabled
        && !already_attempted
        && fast_spec_present
        && plan_has_exited
        && plan_text_non_empty
}

/// Spawn a detached tokio task that generates a session title from the
/// approved plan text via the Fast-role model, then persists it.
///
/// TS parity: `sessionTitle.ts::generateSessionTitle` + the REPL's
/// post-ExitPlanMode fire-and-forget invocation. Silent on any failure
/// (no Fast model, LLM error, schema mismatch) — the user can always
/// rename manually with `/rename`.
fn spawn_auto_title_task(
    spec: coco_types::ModelSpec,
    plan_text: String,
    session_manager: Arc<coco_session::SessionManager>,
    session_id: String,
    runtime: Arc<coco_config::RuntimeConfig>,
) {
    use coco_inference::QueryParams;
    use coco_inference::RetryConfig;
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;

    tokio::spawn(async move {
        let Ok(client) = crate::build_api_client(&runtime, &spec, RetryConfig::default()) else {
            // Provider dispatch failed (e.g. missing API key) — silently
            // abandon; `auto_title` is an advisory feature.
            return;
        };

        let (system, user) = coco_session::title_generator::build_title_prompt(&plan_text);
        // Compose prompt as a single user message with the system text
        // appended. LlmMessage::System exists but the provider-agnostic
        // query path accepts user text most reliably across providers.
        let combined = format!("{system}\n\n{user}");
        let params = QueryParams {
            prompt: vec![LlmMessage::user_text(&combined)],
            max_tokens: Some(150),
            thinking_level: None,
            fast_mode: false,
            tools: None,
            context_management: None,
            query_source: None,
            agent_id: None,
            time_since_last_assistant_ms: None,
        };

        let raw = match client.query(&params).await {
            Ok(result) => result
                .content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            Err(_) => return,
        };

        let Some(title) = coco_session::title_generator::parse_title_response(&raw) else {
            return;
        };
        // `apply_title` is idempotent + refuses to overwrite a
        // user-set title — safe to always call.
        let _ = coco_session::title_generator::apply_title(&session_manager, &session_id, title);
    });
}

/// TS: `normalizeAttachmentForAPI()` for `edited_text_file` type — sends a
/// note explaining the file was modified externally, with a diff snippet.
fn changed_file_to_message(att: &Attachment) -> Option<coco_messages::Message> {
    match att {
        Attachment::File(f) => {
            let text = format!(
                "Note: {} was modified, either by the user or by a linter. \
                 This change was intentional, so make sure to take it into \
                 account as you proceed (ie. don't revert it unless the user \
                 asks you to). Don't tell the user this, since they are already \
                 aware. Here are the relevant changes (shown with line numbers):\n{}",
                f.display_path, f.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tui_runner.test.rs"]
mod tests;
