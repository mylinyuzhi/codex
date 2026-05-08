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
use coco_inference::ApiClient;
use coco_query::CoreEvent;
use coco_query::ServerNotification;
use coco_tui::App;
use coco_tui::ClearScope;
use coco_tui::UserCommand;
use coco_tui::app::create_channels;
use coco_types::SlashCommandStatusKind;
use coco_types::TuiOnlyEvent;
use tokio_util::sync::CancellationToken;

use coco_cli::session_bootstrap::build_engine_resources;
use coco_cli::session_bootstrap::install_session_late_binds;

use coco_cli::resume_resolver::ResumePlan;

use crate::Cli;

/// Run the interactive TUI mode.
///
/// TS: launchRepl() → <REPL /> (React/Ink component).
/// Rust: spawns agent_driver as background task, runs TUI in foreground.
///
/// `resume_plan`: resolved by the binary entry from
/// `--resume` / `--continue` / `--fork-session` flags. When `Some`,
/// the runtime is repointed at the source session id and `runtime.history`
/// is seeded with the loaded messages so the first turn picks up where
/// the prior session left off. Pre-populating the transcript dedup set
/// prevents the per-turn append from re-writing already-persisted
/// messages.
pub async fn run_tui(cli: &Cli, resume_plan: Option<ResumePlan>) -> Result<()> {
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
        .with_overrides(coco_cli::headless::cli_runtime_overrides(cli)?);
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
            (
                None,
                coco_cli::headless::build_runtime_config_for_cli(cli, &cwd)?,
            )
        }
    };
    // Capture a fresh ConfigChange receiver from the reloader (when
    // available) so the SessionRuntime can drive the `ConfigChange`
    // hook on every settings/catalog file change. Borrowed before
    // `runtime_config` is moved into the bootstrap below.
    let config_change_rx = _reloader
        .as_ref()
        .map(coco_config_reload::RuntimeReloader::subscribe_changes);
    // Engine resources (client, fallbacks, recovery, tools, system
    // prompt, command registry, startup-permission state) shared with
    // SDK / headless via `session_bootstrap::build_engine_resources`.
    // The slash-command registry uses the full TS-parity load order
    // (builtins → extended → skills → plugin contributions → TS-parity
    // P1 handlers), so `dispatch_slash_command` and the SDK
    // `initialize.commands` advertisement share one Arc.
    let resources = build_engine_resources(cli, &runtime_config, &cwd)?;
    let mode = resources
        .provider_api
        .map_or("mock", coco_types::ProviderApi::as_str);
    let model_id = resources.model_id.clone();
    let permission_mode = resources.startup.mode;
    let bypass_permissions_available = resources.startup.bypass_available;
    let startup_notification = resources.startup.notification.clone();
    let client = resources.client;
    let fallback_clients = resources.fallback_clients;
    let recovery_policy = resources.recovery_policy;
    let tools = resources.tools;
    let system_prompt = resources.system_prompt;
    let command_registry = resources.command_registry.clone();
    let skill_manager = resources.skill_manager.clone();

    // Session manager for auto-title persistence (F5). Built here so
    // `SessionRuntime::build` can borrow it and the cleanup task can
    // own it.
    let session_manager = Arc::new(coco_session::SessionManager::new(
        coco_cli::paths::sessions_dir(),
    ));
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
    // Keep a concrete `Arc<TuiPermissionBridge>` alongside the trait
    // object so we can install the SessionRuntime weak-ref after
    // `SessionRuntime::build` returns (used to fire the Notification
    // hook on permission prompts — TS parity with
    // `PermissionRequest.tsx:190`).
    let tui_permission_bridge_concrete =
        Arc::new(coco_cli::tui_permission_bridge::TuiPermissionBridge::new(
            notification_tx.clone(),
            pending_approvals.clone(),
        ));
    let tui_permission_bridge: coco_tool_runtime::ToolPermissionBridgeRef =
        tui_permission_bridge_concrete.clone();

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
            command_registry: command_registry.clone(),
            skill_manager: skill_manager.clone(),
        },
    )
    .await?;

    // Post-build late-binds shared with SDK: task runtime, agent
    // transcript persistence, agent-team wiring, fork dispatcher.
    // Without this TUI used to silently miss background AgentTool,
    // resume, and `/btw`. MCP handle is `None` until TUI grows its
    // own `McpConnectionManager` bootstrap.
    install_session_late_binds(runtime.clone(), &cwd, None).await?;

    // Install the SessionRuntime weak-ref on the permission bridge so
    // `Notification` hooks (TS `permission_prompt`) fire when the
    // user is asked to approve a tool. Weak avoids extending the
    // runtime's lifetime through the bridge.
    tui_permission_bridge_concrete
        .set_notification_runtime(Arc::downgrade(&runtime))
        .await;

    // Spawn the ConfigChange watcher (TS
    // `executeConfigChangeHooks(source, path)` from
    // `utils/settings/changeDetector.ts:292/344`). The watcher's
    // join-handle is leaked: it terminates on its own when the
    // reloader's broadcast channel closes (reloader drop) or when
    // `runtime.cancel` fires.
    if let Some(rx) = config_change_rx {
        std::mem::drop(runtime.spawn_config_change_watcher(rx));
    }

    // Spawn the sandbox hot-reload subscriber so settings.json edits
    // touching `sandbox.*` re-flow into the live `SandboxState`. Skipped
    // when the reloader spawn failed (one-shot build) or sandbox isn't
    // bootstrapped (feature off / FullAccess / gates failed). The task
    // exits on its own when the reloader (and its publisher) drops.
    if let (Some(reloader), Some(state)) = (_reloader.as_ref(), runtime.sandbox_state()) {
        std::mem::drop(coco_cli::sandbox_reload::spawn_sandbox_reload(
            state,
            &reloader.publisher(),
            cwd.clone(),
        ));
    }

    // Honor `--resume` / `--continue` / `--fork-session`. The binary
    // entry has already loaded the source transcript; here we repoint
    // every session-id-keyed subsystem at the resume target and seed
    // the in-memory history so the first user prompt sees the prior
    // chain. Pre-populating the transcript dedup set with the loaded
    // uuids prevents `record_transcript_tail` from re-appending
    // entries that are already on disk. TS parity:
    // `processResumedConversation()` + `adoptResumedSessionFile()`.
    if let Some(plan) = resume_plan {
        runtime.start_new_session(plan.session_id.clone()).await;
        {
            let mut history = runtime.history.lock().await;
            *history = plan.prior_messages.clone();
        }
        runtime
            .seed_transcript_dedup(plan.prior_messages.iter().filter_map(|m| m.uuid().copied()))
            .await;
        eprintln!(
            "{} session {} ({} prior message(s))",
            if plan.is_fork { "Forked" } else { "Resumed" },
            plan.source_session_id,
            plan.prior_messages.len(),
        );
    }

    // TS parity (`main.tsx:2437/2577/2607`): fire SessionStart hooks
    // once at session bootstrap. Output queues onto the shared
    // sync-hook buffer and surfaces as `hook_*` reminders on the
    // first turn's reminder pass.
    runtime.fire_session_start_hooks("startup").await;

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
    if let Some(msg) = startup_notification {
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
    let active_turn: Arc<Mutex<Option<ActiveTurn>>> = Arc::new(Mutex::new(None));

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

                // Slash-command interception. When the user typed `/foo args`,
                // resolve through `runtime.command_registry` BEFORE handing
                // raw text to the model. TS parity:
                // `utils/processUserInput/processSlashCommand.tsx`.
                let mut effective_content = content;
                if let Some((name, args)) = parse_slash_command(&effective_content) {
                    match dispatch_slash_command(name, args, &runtime, &event_tx).await {
                        SlashOutcome::Handled => continue,
                        SlashOutcome::RunEngine { content: rendered } => {
                            effective_content = rendered;
                        }
                        SlashOutcome::NotFound => {
                            // Fall through with original content — unknown
                            // command goes to the model as raw text.
                        }
                        SlashOutcome::TriggerCompact {
                            custom_instructions,
                        } => {
                            run_manual_compact(
                                &runtime,
                                &event_tx,
                                custom_instructions,
                                &active_turn,
                            )
                            .await;
                            continue;
                        }
                        SlashOutcome::TriggerClear { scope } => {
                            run_clear_conversation(&runtime, scope, &active_turn).await;
                            continue;
                        }
                        SlashOutcome::TriggerDream => {
                            run_dream_consolidation(&runtime).await;
                            continue;
                        }
                        SlashOutcome::TriggerSummary => {
                            run_session_memory_force(&runtime).await;
                            continue;
                        }
                        SlashOutcome::TriggerRename { name } => {
                            run_session_rename(&runtime, &event_tx, &name).await;
                            continue;
                        }
                        SlashOutcome::TriggerTag { tag } => {
                            run_session_tag(&runtime, &event_tx, &tag).await;
                            continue;
                        }
                        SlashOutcome::TriggerAddDir { path } => {
                            run_add_working_dir(&runtime, &path).await;
                            continue;
                        }
                        SlashOutcome::TriggerReloadPlugins => {
                            run_reload_plugins(&runtime, &event_tx).await;
                            continue;
                        }
                        SlashOutcome::TriggerReloadHooks => {
                            run_reload_hooks(&runtime, &event_tx).await;
                            continue;
                        }
                    }
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
                        effective_content,
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

            UserCommand::ExecuteSkill { name, args } => {
                // Command-palette dispatch (`update/overlay.rs::Submit`).
                // Same registry lookup as the typed path, but with no
                // user-supplied chat message — for `Prompt` outcomes we
                // mint a fresh user-message UUID so file-history /
                // rewind keys line up.
                let args_str = args.unwrap_or_default();
                match dispatch_slash_command(&name, &args_str, &runtime, &event_tx).await {
                    SlashOutcome::Handled => {}
                    SlashOutcome::RunEngine { content } => {
                        drain_active_turn(&active_turn).await;
                        let turn_cancel = CancellationToken::new();
                        let cancel_for_state = turn_cancel.clone();
                        let runtime_t = runtime.clone();
                        let event_tx_t = event_tx.clone();
                        let title_gen_attempted_t = title_gen_attempted.clone();
                        let session_id_t = session_id.clone();
                        let synth_id = uuid::Uuid::new_v4().to_string();
                        let task = tokio::spawn(async move {
                            process_submit_turn(
                                synth_id,
                                content,
                                Vec::new(),
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
                    SlashOutcome::NotFound => {
                        warn!(%name, "ExecuteSkill: command not registered");
                    }
                    SlashOutcome::TriggerCompact {
                        custom_instructions,
                    } => {
                        run_manual_compact(&runtime, &event_tx, custom_instructions, &active_turn)
                            .await;
                    }
                    SlashOutcome::TriggerClear { scope } => {
                        run_clear_conversation(&runtime, scope, &active_turn).await;
                    }
                    SlashOutcome::TriggerDream => {
                        run_dream_consolidation(&runtime).await;
                    }
                    SlashOutcome::TriggerSummary => {
                        run_session_memory_force(&runtime).await;
                    }
                    SlashOutcome::TriggerRename { name } => {
                        run_session_rename(&runtime, &event_tx, &name).await;
                    }
                    SlashOutcome::TriggerTag { tag } => {
                        run_session_tag(&runtime, &event_tx, &tag).await;
                    }
                    SlashOutcome::TriggerAddDir { path } => {
                        run_add_working_dir(&runtime, &path).await;
                    }
                    SlashOutcome::TriggerReloadPlugins => {
                        run_reload_plugins(&runtime, &event_tx).await;
                    }
                    SlashOutcome::TriggerReloadHooks => {
                        run_reload_hooks(&runtime, &event_tx).await;
                    }
                }
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
                // becomes `customInstructions`.
                info!(
                    session_id = %session_id,
                    has_instructions = custom_instructions.is_some(),
                    "TUI: manual /compact"
                );
                run_manual_compact(&runtime, &event_tx, custom_instructions, &active_turn).await;
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
                run_clear_conversation(&runtime, scope, &active_turn).await;
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

            UserCommand::FireIdleNotification { message } => {
                // TS parity (`REPL.tsx:3934-3937` →
                // `services/notifier.ts::sendNotification`): the TUI
                // detected an idle window past
                // `messageIdleNotifThresholdMs`; route through the
                // hook orchestrator so registered `Notification`
                // hooks fire with `notification_type = "idle_prompt"`.
                let registry = runtime.hook_registry();
                let factory = runtime.orchestration_ctx_factory();
                let ctx = (factory)();
                if ctx.disable_all_hooks {
                    continue;
                }
                if let Err(e) = coco_hooks::orchestration::execute_notification(
                    &registry,
                    &ctx,
                    "idle_prompt",
                    &message,
                    /*title*/ None,
                )
                .await
                {
                    tracing::warn!(error = %e, "idle_prompt notification hook failed");
                }
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
/// Outcome of slash-command resolution against `runtime.command_registry`.
///
/// `dispatch_slash_command` is the single source of truth for routing
/// `/foo` regardless of whether the user typed it (`SubmitInput`) or
/// picked it from the palette (`ExecuteSkill`).
enum SlashOutcome {
    /// Command consumed locally (Text / Compact / OpenDialog / Skip).
    /// The caller should NOT run the engine.
    Handled,
    /// Re-feed `content` into the engine as the user message
    /// (Prompt / InjectPrompt). For typed commands the original `/foo`
    /// is replaced with the rendered prompt body so the model sees the
    /// expansion, not the slash.
    RunEngine { content: String },
    /// No command with this name is registered. Caller should fall
    /// through to the existing path (model receives raw text).
    NotFound,
    /// Trigger the same flow as `UserCommand::Compact`. Emitted when
    /// the slash dispatcher detects `COMPACT_SENTINEL` (palette path)
    /// or intercepts `/compact` / `/compact <args>` directly. The agent
    /// driver runs `engine.run_manual_compact` so the model actually
    /// summarizes — not just print "Compacting…".
    TriggerCompact { custom_instructions: Option<String> },
    /// Trigger the same flow as `UserCommand::ClearConversation`.
    /// Emitted for the palette path of `/clear` / `/clear all` /
    /// `/clear history`. The agent driver calls
    /// `runtime.clear_conversation(scope)` which actually wipes
    /// transcript, plan slugs, file caches, etc.
    TriggerClear { scope: ClearScope },
    /// Trigger auto-memory consolidation (when the runtime has a
    /// `MemoryRuntime`). Emitted when the dispatcher sees `DREAM_SENTINEL`.
    TriggerDream,
    /// Trigger a session-memory force update (9-section). Emitted when
    /// the dispatcher sees `SUMMARY_SENTINEL`.
    TriggerSummary,
    /// Rename the current session to `name`. Dispatcher calls
    /// `runtime.session_manager.set_title(session_id, &name)`.
    TriggerRename { name: String },
    /// Toggle a tag on the current session. Dispatcher calls
    /// `runtime.session_manager.toggle_tag(session_id, &tag)`.
    TriggerTag { tag: String },
    /// Push `path` onto the engine's `session_additional_dirs` so the
    /// next turn's permission context sees the wider scope. TS:
    /// `useWorkingDirectories` REPL hook reacting to `/add-dir`.
    TriggerAddDir { path: String },
    /// Rebuild the slash-command registry from disk and atomically
    /// swap. Triggered by `/reload-plugins`. TS:
    /// `useManagePlugins.refreshActivePlugins`.
    TriggerReloadPlugins,
    /// Reload the live `HookRegistry` from the latest `RuntimeConfig`
    /// snapshot. Triggered by `/hooks reload`. TS:
    /// `updateHooksConfigSnapshot()` (`utils/hooks/hooksConfigSnapshot.ts`).
    /// Slash commands run only at turn boundaries (`QueryGuard::Idle`),
    /// so PreToolUse/PostToolUse for an in-flight call cannot see
    /// different hook sets.
    TriggerReloadHooks,
}

/// Split `/<name> <args>` into `(name, args)`. Returns `None` when
/// `text` does not start with `/` or has no name. Whitespace-trimmed.
fn parse_slash_command(text: &str) -> Option<(&str, &str)> {
    let stripped = text.trim().strip_prefix('/')?;
    if stripped.is_empty() {
        return None;
    }
    Some(match stripped.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, rest.trim_start()),
        None => (stripped, ""),
    })
}

/// Decision-tree classifier for sentinel-prefixed handler output.
/// Pure, no side-effects — used by `dispatch_slash_command` to decide
/// whether the Text result actually carries a request to fire a real
/// feature (compact / dream / summary / rename / tag). Extracted as a
/// free function so the routing logic is testable without a full
/// `SessionRuntime`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SentinelTrigger {
    Compact { custom_instructions: Option<String> },
    Dream,
    Summary,
    Rename { name: String },
    Tag { tag: String },
    AddDir { path: String },
    ReloadPlugins,
    ReloadHooks,
}

fn classify_sentinel_trigger(text: &str) -> Option<SentinelTrigger> {
    use coco_commands::handlers::compact::COMPACT_SENTINEL;
    use coco_commands::handlers::compact::parse_compact_sentinel;
    use coco_commands::handlers::dream::DREAM_SENTINEL;
    use coco_commands::handlers::dream::parse_dream_sentinel;
    use coco_commands::handlers::summary::SUMMARY_SENTINEL;
    use coco_commands::handlers::summary::parse_summary_sentinel;
    if text.starts_with(COMPACT_SENTINEL) {
        let req = parse_compact_sentinel(text)?;
        let trimmed = req.custom_instructions.trim();
        let custom_instructions = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        return Some(SentinelTrigger::Compact {
            custom_instructions,
        });
    }
    if text.starts_with(DREAM_SENTINEL) && parse_dream_sentinel(text).is_some() {
        return Some(SentinelTrigger::Dream);
    }
    if text.starts_with(SUMMARY_SENTINEL) && parse_summary_sentinel(text).is_some() {
        return Some(SentinelTrigger::Summary);
    }
    if text.starts_with(coco_commands::RENAME_SENTINEL)
        && let Some(name) = coco_commands::parse_rename_sentinel(text)
    {
        return Some(SentinelTrigger::Rename { name });
    }
    if text.starts_with(coco_commands::TAG_SENTINEL)
        && let Some(tag) = coco_commands::parse_tag_sentinel(text)
    {
        return Some(SentinelTrigger::Tag { tag });
    }
    if text.starts_with(coco_commands::ADD_DIR_SENTINEL)
        && let Some(path) = coco_commands::parse_add_dir_sentinel(text)
    {
        return Some(SentinelTrigger::AddDir { path });
    }
    if text.starts_with(coco_commands::RELOAD_PLUGINS_SENTINEL)
        && coco_commands::parse_reload_plugins_sentinel(text).is_some()
    {
        return Some(SentinelTrigger::ReloadPlugins);
    }
    if text.starts_with(coco_commands::RELOAD_HOOKS_SENTINEL)
        && coco_commands::parse_reload_hooks_sentinel(text).is_some()
    {
        return Some(SentinelTrigger::ReloadHooks);
    }
    None
}

/// Map `/clear` args to a `ClearScope`. `None` for unknown args, which
/// the dispatcher surfaces as a usage hint. Pure helper extracted from
/// `dispatch_slash_command` to keep routing logic testable.
fn parse_clear_scope(args: &str) -> Option<ClearScope> {
    match args.trim() {
        "" | "all" => Some(ClearScope::Conversation),
        "history" => Some(ClearScope::History),
        _ => None,
    }
}

/// Mutating subcommand of `/permissions`. `None` for the read-only
/// (`list` / no-arg) path, which falls through to the registry handler.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionsMutation {
    Allow(String),
    Deny(String),
    Reset,
}

fn parse_permissions_mutation(args: &str) -> Option<PermissionsMutation> {
    let trimmed = args.trim();
    if trimmed == "reset" {
        return Some(PermissionsMutation::Reset);
    }
    if let Some(tool) = trimmed.strip_prefix("allow ") {
        let tool = tool.trim();
        if tool.is_empty() {
            return None;
        }
        return Some(PermissionsMutation::Allow(tool.to_string()));
    }
    if let Some(tool) = trimmed.strip_prefix("deny ") {
        let tool = tool.trim();
        if tool.is_empty() {
            return None;
        }
        return Some(PermissionsMutation::Deny(tool.to_string()));
    }
    None
}

/// Resolve `/<name> <args>` through the registry and route the result.
async fn dispatch_slash_command(
    name: &str,
    args: &str,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> SlashOutcome {
    // Runtime-state-aware commands intercepted before registry lookup:
    // their behavior depends on per-session state (session_id, plan
    // file, app_state) that the static registry can't carry. TS:
    // `commands/plan/plan.tsx` reads `appState.toolPermissionContext`
    // + `getPlan()` / `getPlanFilePath()` directly.
    if matches!(name, "plan" | "planning") {
        return dispatch_plan(args, runtime, event_tx).await;
    }
    // `/permissions allow|deny|reset` — the registry handler can't
    // mutate `engine_config.allow_rules / deny_rules`. Intercept the
    // mutating subcommands so they actually take effect; the `list`
    // / no-arg / `list` path keeps falling through to the registry
    // handler that reads settings.json.
    if name == "permissions"
        && let Some(outcome) = dispatch_permissions_mutation(args, runtime, event_tx).await
    {
        return outcome;
    }
    // `/color <name|default>` mutates `app_state.agent_color`. The
    // registry handler is sync + has no runtime context, so the
    // intercept owns the teammate guard + state write. Falls through
    // to the registry (handler lists colors) when args are empty.
    if name == "color"
        && let Some(outcome) = dispatch_color(args, runtime, event_tx).await
    {
        return outcome;
    }
    // `/clear` from the palette: typed `/clear` is intercepted in
    // `update/edit.rs::try_local_clear`, but ExecuteSkill flows
    // through here. Without this short-circuit the registry handler's
    // text — which says "Conversation cleared" — would print without
    // any actual clearing.
    if name == "clear" {
        return match parse_clear_scope(args) {
            Some(scope) => SlashOutcome::TriggerClear { scope },
            None => {
                emit_slash_text(
                    event_tx,
                    name,
                    &format!(
                        "Unknown clear subcommand: {}\n\n\
                         Usage:\n\
                         /clear           Conversation + plan state + caches\n\
                         /clear all       Alias of /clear\n\
                         /clear history   Lighter: clear transcript only",
                        args.trim()
                    ),
                )
                .await;
                SlashOutcome::Handled
            }
        };
    }
    // `/rewind` / `/checkpoint` from the palette: emit a TuiOnlyEvent
    // so the TUI builds the picker overlay from current session state.
    // Typed paths are intercepted earlier in the TUI.
    if matches!(name, "rewind" | "checkpoint") {
        let _ = event_tx
            .send(CoreEvent::Tui(TuiOnlyEvent::OpenRewindPicker))
            .await;
        return SlashOutcome::Handled;
    }

    // Snapshot once per dispatch — `/reload-plugins` may swap the
    // registry mid-call, but the snapshot keeps the resolved command
    // valid through the handler's await chain.
    let registry_snapshot = runtime.current_command_registry().await;
    let Some(cmd) = registry_snapshot.get(name) else {
        return SlashOutcome::NotFound;
    };
    let Some(handler) = cmd.handler.as_ref() else {
        // Registered shell with no handler. For Prompt-type commands the
        // safe default is to fall through to the model so it sees the
        // raw `/foo` — TS does the same when the loader returns nothing.
        // Local-type commands genuinely need a handler; surface a
        // breadcrumb so the user knows the command is mis-wired.
        if matches!(cmd.command_type, coco_types::CommandType::Prompt(_)) {
            return SlashOutcome::NotFound;
        }
        emit_slash_status(event_tx, name, SlashCommandStatusKind::NoHandler).await;
        return SlashOutcome::Handled;
    };

    let result = match handler.execute_command(args).await {
        Ok(r) => r,
        Err(e) => {
            emit_slash_status(
                event_tx,
                name,
                SlashCommandStatusKind::Failed {
                    error: e.to_string(),
                },
            )
            .await;
            return SlashOutcome::Handled;
        }
    };

    use coco_commands::CommandResult;
    use coco_commands::DialogSpec;
    use coco_commands::PromptPart;
    match result {
        CommandResult::Skip => SlashOutcome::Handled,
        CommandResult::Text(text) => {
            // Sentinel detection — handlers like `/compact`, `/dream`,
            // `/summary` produce a sentinel-prefixed string instead of
            // having direct access to the runtime. Convert the sentinel
            // into a structured `SlashOutcome` so the agent driver runs
            // the real feature (compaction, consolidation, extraction).
            // Mirrors the SDK runner's sentinel detection
            // (`sdk_runner.rs:170,199,213`).
            if let Some(trigger) = classify_sentinel_trigger(&text) {
                return match trigger {
                    SentinelTrigger::Compact {
                        custom_instructions,
                    } => SlashOutcome::TriggerCompact {
                        custom_instructions,
                    },
                    SentinelTrigger::Dream => SlashOutcome::TriggerDream,
                    SentinelTrigger::Summary => SlashOutcome::TriggerSummary,
                    SentinelTrigger::Rename { name } => SlashOutcome::TriggerRename { name },
                    SentinelTrigger::Tag { tag } => SlashOutcome::TriggerTag { tag },
                    SentinelTrigger::AddDir { path } => SlashOutcome::TriggerAddDir { path },
                    SentinelTrigger::ReloadPlugins => SlashOutcome::TriggerReloadPlugins,
                    SentinelTrigger::ReloadHooks => SlashOutcome::TriggerReloadHooks,
                };
            }
            emit_slash_text(event_tx, name, &text).await;
            SlashOutcome::Handled
        }
        CommandResult::InjectPrompt(text) => SlashOutcome::RunEngine { content: text },
        CommandResult::Prompt { parts, .. } => {
            // Concatenate text parts. `File` parts are not yet wired —
            // none of the in-tree Prompt handlers emit them today.
            let mut buf = String::new();
            for part in parts {
                match part {
                    PromptPart::Text { text } => {
                        if !buf.is_empty() {
                            buf.push('\n');
                        }
                        buf.push_str(&text);
                    }
                    PromptPart::File { .. } => {
                        warn!(%name, "Prompt::File parts not yet rendered to engine input");
                    }
                }
            }
            if buf.is_empty() {
                emit_slash_status(event_tx, name, SlashCommandStatusKind::EmptyPrompt).await;
                SlashOutcome::Handled
            } else {
                SlashOutcome::RunEngine { content: buf }
            }
        }
        CommandResult::Compact {
            display_text,
            summary,
        } => {
            // Pre-computed summary path: a handler that already ran
            // compaction (or has a summary in hand) returns the summary
            // string + display text. We push the summary as a
            // `is_compact_summary: true` user message so the next turn
            // sees it as a compact boundary; the LLM-summarized engine
            // path is unchanged (it's still the entry-point for typed
            // `/compact` from the TUI fast-path).
            //
            // Truncation of pre-summary rounds is intentionally left to
            // the handler — when no handler emits this today, we err on
            // the side of preserving history rather than dropping it.
            if !summary.trim().is_empty() {
                let mut h = runtime.history.lock().await;
                h.push(coco_compact::build_compact_summary_message(&summary));
            }
            emit_slash_text(event_tx, name, &display_text).await;
            SlashOutcome::Handled
        }
        CommandResult::OpenDialog(spec) => {
            // Wired dialogs route to TuiOnlyEvent so the TUI opens the
            // overlay; unwired dialogs emit a localized breadcrumb.
            // Typed `/rewind` etc. are intercepted earlier in
            // `update/edit.rs::try_local_command`; this path covers the
            // command-palette (ExecuteSkill) flow.
            match spec {
                DialogSpec::MessageSelector => {
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::OpenRewindPicker))
                        .await;
                }
                DialogSpec::MemoryFileSelector { entries } => {
                    // Convert from coco_commands::MemoryFileEntry to the
                    // wire-payload struct in coco-types so the TUI can
                    // consume the event without depending on coco-commands.
                    let wire_entries: Vec<coco_types::MemoryDialogEntry> = entries
                        .into_iter()
                        .map(|e| coco_types::MemoryDialogEntry {
                            path: e.path.display().to_string(),
                            label: e.label,
                            scope: match e.scope {
                                coco_commands::MemoryScope::Managed => {
                                    coco_types::MemoryDialogScope::Managed
                                }
                                coco_commands::MemoryScope::User => {
                                    coco_types::MemoryDialogScope::User
                                }
                                coco_commands::MemoryScope::Project => {
                                    coco_types::MemoryDialogScope::Project
                                }
                                coco_commands::MemoryScope::ProjectLocal => {
                                    coco_types::MemoryDialogScope::ProjectLocal
                                }
                                coco_commands::MemoryScope::Subdir => {
                                    coco_types::MemoryDialogScope::Subdir
                                }
                            },
                        })
                        .collect();
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::OpenMemoryDialog {
                            entries: wire_entries,
                        }))
                        .await;
                }
                DialogSpec::PluginPicker
                | DialogSpec::McpbConfig { .. }
                | DialogSpec::Confirm { .. } => {
                    let dialog_kind = match spec {
                        DialogSpec::PluginPicker => "plugin picker",
                        DialogSpec::McpbConfig { .. } => "MCPB config form",
                        DialogSpec::Confirm { .. } => "confirm dialog",
                        DialogSpec::MessageSelector | DialogSpec::MemoryFileSelector { .. } => {
                            unreachable!()
                        }
                    }
                    .to_string();
                    emit_slash_status(
                        event_tx,
                        name,
                        SlashCommandStatusKind::DialogPending { dialog_kind },
                    )
                    .await;
                }
            }
            SlashOutcome::Handled
        }
    }
}

/// `/plan` dispatch with full session-runtime context.
///
/// Mirrors TS `commands/plan/plan.tsx`:
/// - `""` → show current plan content (or "no plan yet" hint)
/// - `"open"` → ensure file exists, launch `$EDITOR` (or `vi`) on it
/// - `"<description>"` → emit a Prompt that asks the model to call
///   EnterPlanMode and plan for the description (TS sets app-state
///   directly + triggers a query; coco-rs routes this through the
///   EnterPlanMode tool, which is the canonical mode-entry path)
async fn dispatch_plan(
    args: &str,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> SlashOutcome {
    let args = args.trim();
    let session_id = runtime.current_session_id().await;
    let plans_dir = coco_context::resolve_plans_directory(
        &runtime.config_home,
        /*project_dir*/ None,
        /*setting*/ None,
    );

    if args.is_empty() {
        let path =
            coco_context::get_plan_file_path(&session_id, &plans_dir, /*agent_id*/ None);
        let content = coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
        let text = match content {
            Some(body) if !body.trim().is_empty() => format!(
                "## Current Plan\n\n*{}*\n\n{}\n\nRun `/plan open` to edit in $EDITOR.",
                path.display(),
                body
            ),
            _ => format!(
                "No plan written yet for this session.\n\n\
                 Plan file: `{}`\n\n\
                 Run `/plan <description>` to ask the model to enter plan mode \
                 for a task, or `/plan open` to start an empty plan in $EDITOR.",
                path.display()
            ),
        };
        emit_slash_text(event_tx, "plan", &text).await;
        return SlashOutcome::Handled;
    }

    if args == "open" {
        let path = coco_context::get_plan_file_path(&session_id, &plans_dir, None);
        if let Some(parent) = path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            emit_slash_text(
                event_tx,
                "plan",
                &format!("Failed to create plans directory: {e}"),
            )
            .await;
            return SlashOutcome::Handled;
        }
        if !path.exists() {
            let _ = tokio::fs::write(&path, "").await;
        }
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        let text = match tokio::process::Command::new(&editor).arg(&path).spawn() {
            Ok(_) => format!("Opened plan in {editor}: {}", path.display()),
            Err(e) => format!(
                "Failed to launch editor `{editor}`: {e}\n\nPlan file: {}",
                path.display()
            ),
        };
        emit_slash_text(event_tx, "plan", &text).await;
        return SlashOutcome::Handled;
    }

    // /plan <description> — TS sets plan-mode + triggers a query. coco-rs
    // analog: feed the description back as a user message asking the
    // model to use EnterPlanMode (the canonical entry path) and plan
    // for the task.
    let body =
        format!("Use the EnterPlanMode tool to enter plan mode, then create a plan for: {args}");
    SlashOutcome::RunEngine { content: body }
}

/// In-flight turn handle. Each `SubmitInput` / `ExecuteSkill` spawns
/// the engine call into a child task so the `command_rx` recv loop stays
/// responsive (Interrupt / ClearConversation / Compact / Rewind / Shutdown
/// can reach their arms while the engine runs). TS:
/// `screens/REPL.tsx`'s React event loop fires `abortController.abort()`
/// "concurrently" with engine work — JS cooperative-async makes that
/// natural; Rust needs an explicit `tokio::spawn`.
struct ActiveTurn {
    task: tokio::task::JoinHandle<()>,
    cancel: CancellationToken,
}

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

/// Run a manual full LLM compaction. Used by `UserCommand::Compact` and
/// the slash dispatcher's `TriggerCompact` outcome — both routes feed
/// through here so typed `/compact` and palette `/compact` behave
/// identically. TS: `commands/compact/compact.ts:40`.
async fn run_manual_compact(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
    custom_instructions: Option<String>,
    active_turn: &Arc<Mutex<Option<ActiveTurn>>>,
) {
    // Drain any active turn before compacting — compact mutates
    // `runtime.history` and runs an LLM call that races with the
    // in-flight engine.
    drain_active_turn(active_turn).await;
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

/// Run the same clear flow as `UserCommand::ClearConversation`. Drains
/// any active turn first since clear mutates session_id + resets several
/// per-session caches. TS: `clearConversation()`.
async fn run_clear_conversation(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    scope: ClearScope,
    active_turn: &Arc<Mutex<Option<ActiveTurn>>>,
) {
    drain_active_turn(active_turn).await;
    if let Err(e) = runtime.clear_conversation(scope).await {
        warn!(error = %e, "/clear failed");
    }
}

/// Force auto-memory consolidation now (skips the three-gate scheduler).
/// Mirrors the SDK runner's `/dream` short-circuit (`sdk_runner.rs:199`).
/// Silently no-ops when `Feature::AutoMemory` is off — matches TS.
async fn run_dream_consolidation(runtime: &Arc<crate::session_runtime::SessionRuntime>) {
    let Some(memory_runtime) = runtime.memory_runtime().cloned() else {
        info!("/dream: no MemoryRuntime (Feature::AutoMemory off); skipping");
        return;
    };
    let transcript_dir = std::path::PathBuf::from(".");
    let now_ms = coco_memory::service::dream::DreamService::now_ms();
    let _ = memory_runtime
        .dream
        .maybe_consolidate(&transcript_dir, &[], now_ms)
        .await;
}

/// Force a 9-section session-memory update. Mirrors the SDK runner's
/// `/summary` short-circuit (`sdk_runner.rs:213`). Silently no-ops when
/// the runtime has no `MemoryRuntime`.
async fn run_session_memory_force(runtime: &Arc<crate::session_runtime::SessionRuntime>) {
    let Some(memory_runtime) = runtime.memory_runtime().cloned() else {
        info!("/summary: no MemoryRuntime; skipping");
        return;
    };
    let history_msgs = runtime.history.lock().await.clone();
    let tokens = coco_compact::estimate_tokens(&history_msgs);
    let _ = memory_runtime.session_memory.force(tokens).await;
}

/// `/rename <name>` runner — sets the session title via `SessionManager`
/// and surfaces a system-line confirmation. Silently no-ops when the
/// session id hasn't been minted yet (rare: only between fresh launch
/// and first turn).
async fn run_session_rename(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
    name: &str,
) {
    let session_id = runtime.current_session_id().await;
    let manager = runtime.session_manager.clone();
    let name_owned = name.to_string();
    let session_id_owned = session_id.clone();
    let result =
        tokio::task::spawn_blocking(move || manager.set_title(&session_id_owned, &name_owned))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|inner| inner.map_err(anyhow::Error::from));
    let text = match result {
        Ok(_) => format!("Conversation renamed to: {name}"),
        Err(e) => format!("Failed to rename conversation ({session_id}): {e}"),
    };
    emit_slash_text(event_tx, "rename", &text).await;
}

/// `/reload-plugins` runner — rescans plugin + skill dirs and
/// atomically swaps the active `CommandRegistry`. Snapshots taken by
/// in-flight dispatches stay valid (they hold the prior `Arc`); the
/// swap is observed by the next dispatch. TS:
/// `useManagePlugins.refreshActivePlugins`.
async fn run_reload_plugins(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let count = runtime.reload_plugins(&cwd).await;
    let body = format!("Reloaded — {count} commands now registered.");
    emit_slash_text(event_tx, "reload-plugins", &body).await;
}

/// `/hooks reload` runner — rebuild the live `HookRegistry` from the
/// latest `RuntimeConfig` snapshot. TS parity:
/// `updateHooksConfigSnapshot()`.
async fn run_reload_hooks(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    let body = match runtime.reload_hooks().await {
        Ok(count) => format!("Reloaded — {count} hook(s) registered from current settings."),
        Err(e) => format!("Hook reload failed: {e}"),
    };
    emit_slash_text(event_tx, "hooks", &body).await;
}

/// `/add-dir <abs-path>` runner — pushes the (already-validated)
/// absolute path onto `engine_config.session_additional_dirs` so the
/// next turn's `ToolPermissionContext.additional_dirs` carries it.
/// Source is `Session` (TS parity) — never persisted to settings.json.
async fn run_add_working_dir(runtime: &Arc<crate::session_runtime::SessionRuntime>, path: &str) {
    let path_owned = path.to_string();
    runtime
        .update_engine_config(move |cfg| {
            cfg.session_additional_dirs.insert(
                path_owned.clone(),
                coco_types::AdditionalWorkingDir {
                    path: path_owned,
                    source: coco_types::PermissionUpdateDestination::Session,
                },
            );
        })
        .await;
}

/// `/tag <name>` runner — toggles the tag via `SessionManager`. Reports
/// "added" or "removed" so the user knows the new state.
async fn run_session_tag(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
    tag: &str,
) {
    let session_id = runtime.current_session_id().await;
    let manager = runtime.session_manager.clone();
    let tag_owned = tag.to_string();
    let session_id_owned = session_id.clone();
    let result =
        tokio::task::spawn_blocking(move || manager.toggle_tag(&session_id_owned, &tag_owned))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|inner| inner.map_err(anyhow::Error::from));
    let text = match result {
        Ok((_, true)) => format!("Tag added: {tag}"),
        Ok((_, false)) => format!("Tag removed: {tag}"),
        Err(e) => format!("Failed to toggle tag `{tag}` on session {session_id}: {e}"),
    };
    emit_slash_text(event_tx, "tag", &text).await;
}

/// `/permissions allow|deny|reset` dispatch with engine-config mutation.
///
/// The static registry handler can return text but can't mutate
/// `engine_config.allow_rules / deny_rules`. This intercepts the three
/// mutating subcommands so they take real effect; `list` / no-arg fall
/// through to the registry handler that reads settings.json. Returns
/// `None` for non-mutating args so the caller falls through.
/// `/color <name|default>` — set the prompt bar color for this session.
///
/// TS parity: `commands/color/color.ts` — the same `RESET_ALIASES`, the
/// same teammate guard, the same error messages. Persists to the live
/// `ToolAppState.agent_color` so the prompt-bar UI sees the change
/// without a session restart. Returns `None` for the empty-args case so
/// the registry handler still produces the "Available colors: …"
/// listing.
async fn dispatch_color(
    args: &str,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> Option<SlashOutcome> {
    use coco_coordinator::identity::is_teammate;
    use coco_types::AgentColorName;

    if is_teammate() {
        emit_slash_text(
            event_tx,
            "color",
            "Cannot set color: This session is a swarm teammate. \
             Teammate colors are assigned by the team leader.",
        )
        .await;
        return Some(SlashOutcome::Handled);
    }

    let trimmed = args.trim();
    if trimmed.is_empty() {
        // Empty args fall through to the registry handler, which
        // produces the canonical "Please provide a color..." listing
        // (identical to TS empty-args output).
        return None;
    }

    // Reset aliases mirror `commands/color/color.ts:18`.
    const RESET_ALIASES: &[&str] = &["default", "reset", "none", "gray", "grey"];
    let lower = trimmed.to_ascii_lowercase();
    if RESET_ALIASES.contains(&lower.as_str()) {
        runtime.app_state.write().await.agent_color = None;
        emit_slash_text(event_tx, "color", "Session color reset to default").await;
        return Some(SlashOutcome::Handled);
    }

    match lower.parse::<AgentColorName>() {
        Ok(color) => {
            runtime.app_state.write().await.agent_color = Some(color);
            emit_slash_text(event_tx, "color", &format!("Session color set to: {color}")).await;
            Some(SlashOutcome::Handled)
        }
        Err(_) => {
            let list = AgentColorName::ALL
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            emit_slash_text(
                event_tx,
                "color",
                &format!("Invalid color \"{lower}\". Available colors: {list}, default"),
            )
            .await;
            Some(SlashOutcome::Handled)
        }
    }
}

async fn dispatch_permissions_mutation(
    args: &str,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> Option<SlashOutcome> {
    use coco_types::PermissionBehavior;
    use coco_types::PermissionRule;
    use coco_types::PermissionRuleSource;
    use coco_types::PermissionRuleValue;

    // Empty `allow` / `deny` (no tool name) is a usage error — surface
    // the hint without falling through to the registry handler. The
    // pure parser returns `None` in that case (vs. None for read-only
    // / unrecognized which DO fall through).
    let trimmed = args.trim();
    if trimmed == "allow" || trimmed.starts_with("allow  ") || trimmed == "allow " {
        // Route through the typed status enum so the TUI translates via
        // `slash.permissions.usage_allow` (i18n parity with the other
        // dispatcher status messages).
        emit_slash_status(
            event_tx,
            "permissions",
            SlashCommandStatusKind::PermissionsUsageAllow,
        )
        .await;
        return Some(SlashOutcome::Handled);
    }
    if trimmed == "deny" || trimmed.starts_with("deny  ") || trimmed == "deny " {
        emit_slash_status(
            event_tx,
            "permissions",
            SlashCommandStatusKind::PermissionsUsageDeny,
        )
        .await;
        return Some(SlashOutcome::Handled);
    }

    let mutation = parse_permissions_mutation(args)?;

    // Push a `command_permissions` reminder body so the next turn's
    // system-reminder pipeline informs the model that permission rules
    // changed. TS parity: `processSlashCommand.tsx:909` — the model
    // sees a brief "permission rule added/removed" hint without
    // re-rendering the full rule set.
    let mailbox = runtime.reminder_mailbox_handle();

    let confirmation = match &mutation {
        PermissionsMutation::Allow(tool) => {
            let rule = PermissionRule {
                source: PermissionRuleSource::Session,
                behavior: PermissionBehavior::Allow,
                value: PermissionRuleValue {
                    tool_pattern: tool.clone(),
                    rule_content: None,
                },
            };
            runtime
                .update_engine_config(|cfg| {
                    cfg.allow_rules
                        .entry(PermissionRuleSource::Session)
                        .or_default()
                        .push(rule);
                })
                .await;
            mailbox.put_command_permissions(format!(
                "Permission rule added: allow `{tool}` (session scope)."
            ));
            format!(
                "Added allow rule for `{tool}`.\n\nSource: Session (highest priority — \
                 active until end of session or `/permissions reset`)."
            )
        }
        PermissionsMutation::Deny(tool) => {
            let rule = PermissionRule {
                source: PermissionRuleSource::Session,
                behavior: PermissionBehavior::Deny,
                value: PermissionRuleValue {
                    tool_pattern: tool.clone(),
                    rule_content: None,
                },
            };
            runtime
                .update_engine_config(|cfg| {
                    cfg.deny_rules
                        .entry(PermissionRuleSource::Session)
                        .or_default()
                        .push(rule);
                })
                .await;
            mailbox.put_command_permissions(format!(
                "Permission rule added: deny `{tool}` (session scope)."
            ));
            format!(
                "Added deny rule for `{tool}`.\n\nSource: Session (highest priority — \
                 active until end of session or `/permissions reset`)."
            )
        }
        PermissionsMutation::Reset => {
            runtime
                .update_engine_config(|cfg| {
                    cfg.allow_rules.remove(&PermissionRuleSource::Session);
                    cfg.deny_rules.remove(&PermissionRuleSource::Session);
                })
                .await;
            mailbox.put_command_permissions(
                "Session permission rules reset (cleared all session-scope allow/deny entries)."
                    .to_string(),
            );
            "Session permission rules cleared. File-based rules \
             (.claude/settings.json, ~/.cocode/settings.json) are unchanged — \
             edit those files directly to modify persistent rules."
                .to_string()
        }
    };
    emit_slash_text(event_tx, "permissions", &confirmation).await;
    Some(SlashOutcome::Handled)
}

/// Emit a `TuiOnlyEvent::SlashCommandResult` so the TUI appends a
/// system-role chat message carrying handler-rendered content (verbatim,
/// no translation).
async fn emit_slash_text(event_tx: &mpsc::Sender<CoreEvent>, name: &str, text: &str) {
    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::SlashCommandResult {
            name: name.to_string(),
            text: text.to_string(),
        }))
        .await;
}

/// Emit a `TuiOnlyEvent::SlashCommandStatus` so the TUI renders a
/// localized dispatcher breadcrumb (handler missing, handler error,
/// empty Prompt body, dialog wiring pending).
async fn emit_slash_status(
    event_tx: &mpsc::Sender<CoreEvent>,
    name: &str,
    kind: SlashCommandStatusKind,
) {
    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::SlashCommandStatus {
            name: name.to_string(),
            kind,
        }))
        .await;
}

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
    // Resolve @-mentions through the shared cross-path helper.
    // TS parity: `processUserInput.ts:504` calls `getAttachmentMessages`
    // which produces both file-attachment system-reminders and
    // changed-file notifications. The same pipeline now feeds headless
    // and SDK paths via `coco_cli::at_mention_turn::resolve_turn_inputs`.
    let cwd = std::env::current_dir().unwrap_or_default();
    let user_uuid =
        uuid::Uuid::parse_str(&user_message_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
    let inputs = coco_cli::at_mention_turn::resolve_turn_inputs(
        &content,
        &images,
        &cwd,
        user_uuid,
        &runtime.file_read_state,
    )
    .await;

    // TS parity (`processUserInput.ts:182-263`): fire UserPromptSubmit
    // hooks BEFORE building the engine. Output queues onto the shared
    // sync-hook buffer so the next turn surfaces `hook_*` reminders;
    // a blocking_error suppresses the turn and surfaces a TurnFailed;
    // prevent_continuation keeps the prompt but skips the engine.
    let prompt_hook_result = runtime.fire_user_prompt_submit_hooks(&content).await;
    if let Some(blocking) = &prompt_hook_result.blocking_error {
        let warning = format!(
            "UserPromptSubmit hook blocked the turn: {}\n\nOriginal prompt: {content}",
            blocking.blocking_error,
        );
        let _ = event_tx
            .send(CoreEvent::Protocol(ServerNotification::TurnFailed(
                coco_types::TurnFailedParams { error: warning },
            )))
            .await;
        return;
    }
    if prompt_hook_result.prevent_continuation {
        let stop_msg = prompt_hook_result
            .stop_reason
            .clone()
            .map(|r| format!("Operation stopped by hook: {r}"))
            .unwrap_or_else(|| "Operation stopped by hook".to_string());
        // Persist the prompt + system warning so the user sees it in
        // the transcript even though no LLM call follows.
        {
            let mut h = runtime.history.lock().await;
            h.push(coco_messages::create_user_message(&content));
            h.push(coco_messages::create_user_message(&stop_msg));
        }
        return;
    }

    let new_turn_messages = coco_cli::at_mention_turn::build_messages_for_turn(&inputs);

    // Persist user message immediately so engine errors don't lose it.
    let messages: Vec<coco_messages::Message> = {
        let mut h = runtime.history.lock().await;
        h.extend(new_turn_messages.iter().cloned());
        h.clone()
    };

    let engine = runtime.build_engine(turn_cancel.clone()).await;

    // Mention priority for post-compact restoration.
    if !inputs.mentioned_paths.is_empty() {
        engine
            .note_mentioned_paths(inputs.mentioned_paths.clone())
            .await;
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
                // Compaction summarizer helper — not the agent loop.
                agentic: false,
                cache: None,
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
        let Ok(client) = coco_inference::model_factory::build_api_client(
            &runtime,
            &spec,
            RetryConfig::default(),
        ) else {
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
            // Title-generation helper — not the agent loop.
            agentic: false,
            cache: None,
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

#[cfg(test)]
#[path = "tui_runner.test.rs"]
mod tests;
