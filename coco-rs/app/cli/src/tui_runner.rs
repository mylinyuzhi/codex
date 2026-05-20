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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Result;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use tracing::warn;

use coco_config::EnvKey;
use coco_config::env;
use coco_context::FileHistoryState;
use coco_query::CoreEvent;
use coco_query::QueuePriority;
use coco_query::QueuedCommand;
use coco_query::QueuedImage;
use coco_query::ServerNotification;
use coco_system_reminder::QueueOrigin;
use coco_tui::App;
use coco_tui::ClearScope;
use coco_tui::UserCommand;
use coco_tui::app::create_channels;
use coco_types::CancelReason;
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
    coco_cli::model_card_refresh::spawn_if_enabled(&runtime_config);
    // Capture a fresh ConfigChange receiver from the reloader (when
    // available) so the SessionRuntime can drive the `ConfigChange`
    // hook on every settings/catalog file change. Borrowed before
    // `runtime_config` is moved into the bootstrap below.
    let config_change_rx = _reloader
        .as_ref()
        .map(coco_config_reload::RuntimeReloader::subscribe_changes);
    let display_settings_rx = _reloader
        .as_ref()
        .map(|reloader| spawn_display_settings_reload(reloader.publisher().subscribe()));
    let config_reload_errors_rx = _reloader
        .as_ref()
        .map(coco_config_reload::RuntimeReloader::subscribe_errors)
        .map(spawn_config_reload_error_toasts);
    // Engine resources (client, fallbacks, recovery, tools, system
    // prompt, command registry, startup-permission state) shared with
    // SDK / headless via `session_bootstrap::build_engine_resources`.
    // The slash-command registry uses the full TS-parity load order
    // (builtins → extended → skills → plugin contributions → TS-parity
    // P1 handlers), so `dispatch_slash_command` and the SDK
    // `initialize.commands` advertisement share one Arc.
    let resources = build_engine_resources(cli, &runtime_config, &cwd)?;
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
        coco_config::global_config::config_home(),
    ));
    let _ = session_manager.create(&model_id, &cwd);
    {
        // Background housekeeping: prune session files older than the
        // default retention period. Mirrors TS `utils/cleanup.ts`
        // `DEFAULT_CLEANUP_PERIOD_DAYS = 30`. Fire-and-forget.
        let mgr = session_manager.clone();
        let transcript_store =
            coco_session::TranscriptStore::new(coco_cli::paths::project_paths(&cwd));
        tokio::spawn(async move {
            let period = coco_session::default_cleanup_period();
            match tokio::task::spawn_blocking(move || -> coco_session::Result<(i32, i32)> {
                let removed_sessions = mgr.cleanup_older_than(period)?;
                let removed_tool_results =
                    transcript_store.cleanup_tool_results_older_than(period)?;
                Ok((removed_sessions, removed_tool_results))
            })
            .await
            {
                Ok(Ok((removed_sessions, removed_tool_results)))
                    if removed_sessions > 0 || removed_tool_results > 0 =>
                {
                    tracing::info!(
                        target: "coco::session::cleanup",
                        removed_sessions,
                        removed_tool_results,
                        "pruned old session artifacts"
                    );
                }
                Ok(Err(e)) => tracing::warn!(
                    target: "coco::session::cleanup",
                    error = %e,
                    "session cleanup failed"
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
            // TS parity: load `~/.coco/agents` + `<cwd>/.claude/agents`
            // and surface them in AgentTool's per-turn dynamic prompt
            // listing. Worktree fallback is applied inside
            // `standard_agent_search_paths`.
            agent_search_paths: coco_cli::paths::standard_agent_search_paths(
                &coco_config::global_config::config_home(),
                &cwd,
            ),
            builtin_agent_catalog: coco_subagent::BuiltinAgentCatalog::interactive(),
        },
    )
    .await?;

    // Post-build late-binds shared with SDK: task runtime, agent
    // transcript persistence, agent-team wiring, fork dispatcher.
    // Without this TUI used to silently miss background AgentTool,
    // resume, and `/btw`. MCP handle is `None` until TUI grows its
    // own `McpConnectionManager` bootstrap.
    let lsp_handle = coco_cli::session_bootstrap::build_lsp_handle_if_enabled(
        &runtime.runtime_config,
        &coco_config::global_config::config_home(),
        &cwd,
    )
    .await;
    install_session_late_binds(runtime.clone(), &cwd, None, lsp_handle).await?;

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

    // Plugin change detector — TS parity:
    // `useManagePlugins.ts:293-300`. Lifecycle: held by
    // `_plugin_watcher_guard` so the `Arc` lives until this function
    // returns (TUI shutdown). The wrapped `FileWatcher` drops with the
    // Arc, shutting its notify thread + throttle task down cleanly.
    let _plugin_watcher_guard = coco_cli::plugin_watch::spawn(
        notification_tx.clone(),
        &cwd,
        &coco_config::global_config::config_home(),
    );

    // Honor `--resume` / `--continue` / `--fork-session`. The binary
    // entry has already loaded the source transcript; here we repoint
    // every session-id-keyed subsystem at the resume target and seed
    // the in-memory history so the first user prompt sees the prior
    // chain. Pre-populating the transcript dedup set with the loaded
    // uuids prevents `record_transcript_tail` from re-appending
    // entries that are already on disk. TS parity:
    // `processResumedConversation()` + `adoptResumedSessionFile()`.
    if let Some(plan) = resume_plan {
        tracing::info!(
            target: "coco_cli::resume",
            session_id = %plan.session_id,
            source_session_id = %plan.source_session_id,
            prior_messages = plan.prior_messages.len(),
            is_fork = plan.is_fork,
            "resume: hydrating session",
        );
        runtime.start_new_session(plan.session_id.clone()).await;
        {
            let mut history = runtime.history.lock().await;
            history.clear();
            for m in plan.prior_messages.iter().cloned() {
                history.push(m);
            }
        }
        runtime
            .seed_transcript_dedup(plan.prior_messages.iter().filter_map(|m| m.uuid().copied()))
            .await;
        runtime
            .seed_tool_result_replacement_state(&plan.prior_messages, &plan.session_id)
            .await;
        // Phase 4 hydration. Two events:
        //   1. `SessionResetForResume` rotates the conversation id and
        //      clears the prior session's UI-only state (streaming
        //      overlay, tool widgets, side-caches).
        //   2. `HistoryReplaced` carries the loaded JSONL transcript
        //      in one shot so the TUI does a single cache-rebuild pass
        //      instead of N `MessageAppended` round-trips. For a 5k-
        //      message transcript that's the difference between one
        //      vec extend and ~20 channel-bounded yields.
        // Live appends after this still go through `MessageAppended` —
        // the bulk path is modeled as a separate event because it IS
        // a different operation (full replace vs. incremental
        // append). See `engine-tui-unified-transcript-plan.md` §7.3.
        let _ = notification_tx
            .send(CoreEvent::Protocol(
                coco_types::ServerNotification::SessionResetForResume {
                    session_id: plan.session_id.clone(),
                    agent_id: None,
                },
            ))
            .await;
        let _ = notification_tx
            .send(CoreEvent::Protocol(
                coco_types::ServerNotification::HistoryReplaced {
                    messages: plan
                        .prior_messages
                        .iter()
                        .cloned()
                        .map(std::sync::Arc::new)
                        .collect(),
                    session_id: plan.session_id.clone(),
                    agent_id: None,
                },
            ))
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
    app.state_mut()
        .ui
        .apply_display_settings(coco_tui::DisplaySettings::from_runtime_config(
            &runtime.runtime_config,
        ));
    app.state_mut().ui.coordinator_mode_active =
        coco_subagent::is_coordinator_mode(&runtime.runtime_config.features);
    if let Some(rx) = display_settings_rx {
        app = app.with_display_settings_reload(rx);
    }
    if let Some(rx) = config_reload_errors_rx {
        app = app.with_config_reload_errors(rx);
    }

    // Wire file_history_enabled into TUI session state so the rewind
    // modal knows whether to show code restore options.
    app.state_mut().session.file_history_enabled = runtime.file_history.is_some();

    // Seed the capability gate that controls both Shift+Tab cycle
    // (`PermissionMode::next_in_cycle`) and the plan-mode exit
    // modal's "Bypass" option. Matches engine_config below so the
    // engine and TUI share one truth. Static for session lifetime.
    app.state_mut().session.bypass_permissions_available = bypass_permissions_available;
    app.state_mut().session.permission_mode = permission_mode;
    // Seed the model + provider for the status bar. Production TUI
    // doesn't currently install a `SessionBootstrap`, so the engine's
    // `emit_session_started` is a no-op and the model field would
    // otherwise stay empty until a fallback fires. Provider is the
    // authoritative id from the resolved Main role; the picker keeps
    // a prefix-match fallback for unregistered builtins.
    app.state_mut().session.model = model_id.clone();
    app.state_mut().session.provider = runtime
        .runtime_config
        .model_roles
        .get(coco_types::ModelRole::Main)
        .map(|spec| spec.provider.clone())
        .unwrap_or_default();
    // Seed cwd + git branch so the header's "where am I" rows render on
    // the first frame. Production TUI doesn't install `SessionBootstrap`,
    // so the engine's `emit_session_started` never fires the
    // `ServerNotification::SessionStarted` that would populate these via
    // `protocol::handle`. Without this seed the rows stay empty for the
    // session's lifetime.
    app.state_mut().session.working_dir = Some(cwd.to_string_lossy().into_owned());
    app.state_mut().session.git_branch = coco_git::get_current_branch(&cwd).ok().flatten();
    // Mirror `SessionStarted`'s thinking-level seed: read the model's
    // registered default so the header's effort dial reflects the real
    // starting state, not the `ReasoningEffort::Auto` fallback.
    if let Some(default_effort) = coco_config::builtin_models_partial()
        .get(&model_id)
        .and_then(|info| info.default_thinking_level)
    {
        app.state_mut().session.thinking_effort = default_effort;
    }

    // Seed `model_catalog` and `model_by_role` from the resolved
    // `ModelRegistry`. The TUI picker and Ctrl+T cycle both consult
    // these — using the registry view (rather than the L0-only
    // `builtin_models_partial`) means L1 `~/.coco/models.json` entries
    // and L2 `providers.<n>.models.<id>` overrides are visible.
    {
        let mut catalog = build_model_catalog(&runtime.runtime_config);
        let provider_statuses = build_provider_statuses(&runtime.runtime_config);
        let by_role = build_model_by_role(&runtime.runtime_config);
        let state = app.state_mut();
        state.session.model_catalog = std::mem::take(&mut catalog);
        state.session.provider_statuses = provider_statuses;
        state.session.model_by_role = by_role;
    }

    // Seed `available_commands` so the `/` autocomplete popup and the
    // `Ctrl+Shift+P` command palette resolve against the live registry
    // (builtins + extended + skills + plugin contributions). Without
    // this snapshot the popup silently shows nothing because the field
    // defaults to an empty Vec. TS parity: `commands.ts::getCommands`
    // is the catalog source for `commandSuggestions.ts`.
    //
    // Two seed paths:
    //   * **Startup (here)** — direct mutation. The event loop hasn't
    //     started yet, so emitting on `notification_tx` would just
    //     queue the event behind `App::run()`'s first iteration —
    //     adds latency without simplifying anything.
    //   * **Reload (`/reload-plugins`)** — see [`run_reload_plugins`].
    //     Emits [`TuiOnlyEvent::AvailableCommandsRefreshed`] through
    //     the same event channel the agent driver uses; the TUI
    //     handler at `server_notification_handler::tui_only` overwrites
    //     the slot and re-runs `refresh_suggestions`.
    {
        let snapshot = command_registry.read().await.snapshot_for_ui();
        app.state_mut().session.available_commands = snapshot;
    }

    // Surface the startup downgrade notification (if any) as a toast
    // so interactive users see it. Headless paths eprintln it; the
    // TUI swallows stderr.
    if let Some(msg) = startup_notification {
        app.state_mut()
            .ui
            .add_toast(coco_tui::state::ui::Toast::warning(msg));
    }

    // Boot the TUI theme stack from ~/.coco/theme.json. This is TUI-local
    // config, separate from RuntimeConfig, so user palette edits can hot-reload
    // without rebuilding the agent runtime.
    let _theme_watcher_guard = {
        let coco_tui::theme::ThemeSetup {
            watcher,
            reload_rx,
            initial,
            watch_error,
        } = coco_tui::theme::install_theme().await;
        app.state_mut().ui.apply_theme_runtime(initial.state);
        if let Some(error) = initial.error {
            app.state_mut()
                .ui
                .add_toast(coco_tui::state::ui::Toast::warning(error));
        }
        if let Some(error) = watch_error {
            app.state_mut()
                .ui
                .add_toast(coco_tui::state::ui::Toast::warning(error));
        }
        app = app.with_theme_reload(reload_rx);
        watcher
    };

    // Boot the keybindings stack via the TUI helper: builds a
    // watcher-backed handle (which hot-reloads on file changes via
    // `KeybindingsWatcher`) and gives back a channel of post-startup
    // validation warnings to plumb into the App's event loop.
    let kb_setup = coco_tui::keybinding_setup::install_keybindings().await;

    // Surface **startup** warnings as toasts immediately (subsequent
    // reloads flow through the `kb_setup.warnings_rx` channel below).
    for issue in &kb_setup.initial.warnings {
        let line = coco_keybindings::format_issue_oneline(issue);
        let toast = match issue.severity {
            coco_keybindings::Severity::Error => coco_tui::state::ui::Toast::error(line),
            coco_keybindings::Severity::Warning => coco_tui::state::ui::Toast::warning(line),
        };
        app.state_mut().ui.add_toast(toast);
    }

    // Install the watcher-backed handle into AppState — replaces the
    // defaults-only handle `UiState::new()` initialized. Reads + chord
    // state both flow through this clone.
    app.state_mut().ui.kb_handle = kb_setup.handle;

    // Plug the warnings receiver into the App so post-startup reloads
    // (user edits `keybindings.json` while the TUI is running) also
    // surface as toasts.
    app = app.with_keybinding_warnings(kb_setup.warnings_rx);

    // Hold onto the watcher for the TUI's lifetime — dropping it
    // stops the hot-reload background task.
    let _kb_watcher_guard = kb_setup.watcher;

    // Spawn agent driver — owns the SessionRuntime + transports.
    let driver_handle = tokio::spawn(run_agent_driver(
        command_rx,
        notification_tx,
        runtime,
        pending_approvals,
    ));

    // Run TUI (blocks until exit)
    let tui_result = app.run().await;

    // Wait for agent driver
    let _ = driver_handle.await;

    tui_result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))
}

fn spawn_display_settings_reload(
    mut rx: tokio::sync::watch::Receiver<Arc<coco_config::RuntimeConfig>>,
) -> mpsc::Receiver<coco_tui::DisplaySettings> {
    let (tx, out_rx) = mpsc::channel(16);
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let display_settings = coco_tui::DisplaySettings::from_runtime_config(&rx.borrow());
            if tx.send(display_settings).await.is_err() {
                break;
            }
        }
    });
    out_rx
}

fn spawn_config_reload_error_toasts(
    mut rx: tokio::sync::broadcast::Receiver<coco_config_reload::ConfigReloadError>,
) -> mpsc::Receiver<String> {
    let (tx, out_rx) = mpsc::channel(16);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(err) => {
                    let source = err.kind.as_str();
                    let detail = err.message;
                    let message = format!("{source}: {detail}");
                    if tx.send(message).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    out_rx
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
    // commands (`Interrupt`, `Compact`, `Rewind`,
    // `Shutdown`) reach their arms without waiting for the engine to
    // finish. TS parity: REPL.tsx's `query()` runs in the same single-
    // threaded React event loop, so its keyboard `useInput` hook fires
    // `abortController.abort()` "concurrently" with engine work — JS
    // cooperative-async makes that natural; Rust needs an explicit
    // `tokio::spawn` to free the recv loop.
    let active_turn: Arc<Mutex<Option<ActiveTurn>>> = Arc::new(Mutex::new(None));
    let mut pending_editor_requests: HashMap<String, PendingEditorRequest> = HashMap::new();

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
                            run_clear_conversation(&runtime, scope, &active_turn, &event_tx).await;
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
                        SlashOutcome::TriggerOpenPlanEditor { path } => {
                            prepare_external_editor_request(
                                &mut pending_editor_requests,
                                PendingEditorRequest::Plan { path },
                                &event_tx,
                            )
                            .await;
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
                let cancel_reason: Arc<OnceLock<CancelReason>> = Arc::new(OnceLock::new());
                let cancel_reason_for_state = cancel_reason.clone();

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
                        cancel_reason,
                    )
                    .await;
                });

                *active_turn.lock().await = Some(ActiveTurn {
                    task,
                    cancel: cancel_for_state,
                    cancel_reason: cancel_reason_for_state,
                });
            }

            UserCommand::SubmitBash {
                user_message_id,
                command,
            } => {
                let event_tx_t = event_tx.clone();
                let runtime_t = runtime.clone();
                // Run from the process's current dir — shell prompt
                // commands inherit the same cwd the agent is using.
                // `runtime_config.paths.project_dir` is the explicit
                // project root when configured, but is optional, so
                // we fall back to `current_dir()` (always defined).
                let cwd = runtime
                    .runtime_config
                    .paths
                    .project_dir
                    .clone()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                tokio::spawn(async move {
                    run_prompt_mode_bash(&cwd, user_message_id, command, runtime_t, event_tx_t)
                        .await;
                });
            }

            UserCommand::OpenMemoryFile { path } => {
                prepare_external_editor_request(
                    &mut pending_editor_requests,
                    PendingEditorRequest::Memory { path },
                    &event_tx,
                )
                .await;
            }

            UserCommand::OpenPlanEditor => {
                let path = runtime_session_plan_file_path(&runtime).await;
                prepare_external_editor_request(
                    &mut pending_editor_requests,
                    PendingEditorRequest::Plan { path },
                    &event_tx,
                )
                .await;
            }

            UserCommand::OpenPromptEditor { initial_content } => {
                prepare_external_editor_request(
                    &mut pending_editor_requests,
                    PendingEditorRequest::Prompt { initial_content },
                    &event_tx,
                )
                .await;
            }

            UserCommand::ExternalEditorTerminalReady { request_id } => {
                let Some(request) = pending_editor_requests.remove(&request_id) else {
                    warn!(%request_id, "terminal ready for unknown external editor request");
                    continue;
                };
                match request {
                    PendingEditorRequest::Memory { path } => {
                        run_open_memory_file(path, event_tx.clone()).await;
                    }
                    PendingEditorRequest::Plan { path } => {
                        run_open_plan_file(path, event_tx.clone()).await;
                    }
                    PendingEditorRequest::Prompt { initial_content } => {
                        run_prompt_editor(initial_content, event_tx.clone()).await;
                    }
                }
            }

            UserCommand::ExternalEditorTerminalPrepareFailed { request_id, error } => {
                let Some(request) = pending_editor_requests.remove(&request_id) else {
                    warn!(%request_id, "terminal prepare failed for unknown editor request");
                    continue;
                };
                emit_editor_prepare_failed(request, error, event_tx.clone()).await;
            }

            UserCommand::SetModelRole {
                role,
                provider,
                model_id,
                effort,
            } => {
                let runtime_t = runtime.clone();
                let event_tx_t = event_tx.clone();
                tokio::spawn(async move {
                    apply_role_in_memory(runtime_t, role, provider, model_id, effort, event_tx_t)
                        .await;
                });
            }

            UserCommand::SetThinkingLevel { level } => {
                // coco-rs Ctrl+T cycle path. Updates the Main role's
                // effort in-memory and emits `ModelRoleChanged` so the
                // TUI mirror stays consistent across status bar +
                // picker. No file write — see `apply_role_in_memory`.
                let runtime_t = runtime.clone();
                let event_tx_t = event_tx.clone();
                tokio::spawn(async move {
                    apply_main_effort_in_memory(runtime_t, level, event_tx_t).await;
                });
            }

            UserCommand::ExecuteSkill { name, args } => {
                // Command-palette dispatch.
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
                        let cancel_reason: Arc<OnceLock<CancelReason>> = Arc::new(OnceLock::new());
                        let cancel_reason_for_state = cancel_reason.clone();
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
                                cancel_reason,
                            )
                            .await;
                        });
                        *active_turn.lock().await = Some(ActiveTurn {
                            task,
                            cancel: cancel_for_state,
                            cancel_reason: cancel_reason_for_state,
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
                        run_clear_conversation(&runtime, scope, &active_turn, &event_tx).await;
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
                    SlashOutcome::TriggerOpenPlanEditor { path } => {
                        prepare_external_editor_request(
                            &mut pending_editor_requests,
                            PendingEditorRequest::Plan { path },
                            &event_tx,
                        )
                        .await;
                    }
                    SlashOutcome::TriggerReloadPlugins => {
                        run_reload_plugins(&runtime, &event_tx).await;
                    }
                    SlashOutcome::TriggerReloadHooks => {
                        run_reload_hooks(&runtime, &event_tx).await;
                    }
                }
            }

            UserCommand::ExecuteSlashCommand { name, args } => {
                match dispatch_slash_command(name.as_str(), &args, &runtime, &event_tx).await {
                    SlashOutcome::Handled => {}
                    SlashOutcome::RunEngine { content } => {
                        drain_active_turn(&active_turn).await;
                        let turn_cancel = CancellationToken::new();
                        let cancel_for_state = turn_cancel.clone();
                        let cancel_reason: Arc<OnceLock<CancelReason>> = Arc::new(OnceLock::new());
                        let cancel_reason_for_state = cancel_reason.clone();
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
                                cancel_reason,
                            )
                            .await;
                        });
                        *active_turn.lock().await = Some(ActiveTurn {
                            task,
                            cancel: cancel_for_state,
                            cancel_reason: cancel_reason_for_state,
                        });
                    }
                    SlashOutcome::NotFound => {
                        emit_slash_status(
                            &event_tx,
                            name.as_str(),
                            SlashCommandStatusKind::NoHandler,
                        )
                        .await;
                    }
                    SlashOutcome::TriggerCompact {
                        custom_instructions,
                    } => {
                        run_manual_compact(&runtime, &event_tx, custom_instructions, &active_turn)
                            .await;
                    }
                    SlashOutcome::TriggerClear { scope } => {
                        run_clear_conversation(&runtime, scope, &active_turn, &event_tx).await;
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
                    SlashOutcome::TriggerOpenPlanEditor { path } => {
                        prepare_external_editor_request(
                            &mut pending_editor_requests,
                            PendingEditorRequest::Plan { path },
                            &event_tx,
                        )
                        .await;
                    }
                    SlashOutcome::TriggerReloadPlugins => {
                        run_reload_plugins(&runtime, &event_tx).await;
                    }
                    SlashOutcome::TriggerReloadHooks => {
                        run_reload_hooks(&runtime, &event_tx).await;
                    }
                }
            }

            UserCommand::Rewind { message_id, mode } => {
                // Drain first — rewind reads file_history snapshots
                // and rewrites runtime.history; an in-flight turn that
                // mutates either would race.
                drain_active_turn(&active_turn).await;
                match mode {
                    coco_tui::command::RewindMode::Explicit {
                        restore_type,
                        rewound_turn,
                    } => {
                        handle_rewind(
                            &restore_type,
                            &message_id,
                            rewound_turn,
                            &runtime.file_history,
                            &runtime.config_home,
                            &session_id,
                            &event_tx,
                            &runtime,
                        )
                        .await;
                    }
                    coco_tui::command::RewindMode::AutoRestore => {
                        handle_auto_truncate(&message_id, &event_tx, &runtime).await;
                    }
                }
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
                // .abort('user-cancel') → query() generator yields,
                // .finally reads `signal.reason` and may auto-restore.
                //
                // Record `UserCancel` BEFORE firing `.cancel()` so the
                // turn task (which races at every `.await` point) is
                // guaranteed to see it. `OnceLock::set` returning Err
                // means a prior writer raced and won — keep going; the
                // first reason wins regardless of who wrote it.
                if let Some(state) = active_turn.lock().await.as_ref() {
                    let _ = state.cancel_reason.set(CancelReason::UserCancel);
                    state.cancel.cancel();
                    info!("Interrupt: cancelled active turn");
                }
            }

            UserCommand::InterruptAgentCurrentWork { agent_id } => {
                match runtime.interrupt_agent_current_work(&agent_id).await {
                    Ok(true) => {
                        info!(%agent_id, "Interrupt: cancelled teammate current turn");
                    }
                    Ok(false) => {
                        info!(%agent_id, "Interrupt: teammate had no active turn to cancel");
                    }
                    Err(error) => {
                        tracing::warn!(%agent_id, %error, "Interrupt: teammate current turn failed");
                    }
                }
            }

            UserCommand::QueueCommand { prompt, images } => {
                // User typed Enter while the agent was streaming.
                // Push onto the session-scoped command queue so the
                // running engine sees it at its next drain point
                // (mid-turn `Now` drain or end-of-turn full drain).
                // TS parity: `handlePromptSubmit.ts:336-343` — when
                // `queryGuard.isActive`, the prompt is enqueued
                // instead of starting a fresh turn.
                if prompt.trim().is_empty() {
                    continue;
                }
                let queued = QueuedCommand::new(prompt, QueuePriority::Next)
                    .with_origin(QueueOrigin::Human)
                    .with_images(image_data_to_queued(&images));
                let id = queued.id;
                let preview = queued.preview();
                runtime.command_queue().enqueue(queued).await;
                // Round-trip notify: the TUI display
                // (`SessionState::queued_commands`) is a projection of
                // engine state and waits for this event to update —
                // see `update.rs::QueueInput` (no optimistic push).
                let _ = event_tx
                    .send(CoreEvent::Protocol(ServerNotification::CommandQueued {
                        id: id.to_string(),
                        preview,
                    }))
                    .await;
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
                let cfg_mode = cfg.permission_mode;
                runtime
                    .update_engine_config(|cfg| cfg.permission_mode = mode)
                    .await;
                let prev_mode;
                {
                    let mut guard = runtime.app_state.write().await;
                    prev_mode = guard.permission_mode.unwrap_or(cfg_mode);
                    coco_permissions::apply_permission_mode_transition_to_app_state(
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
                always_allow,
                feedback,
                updated_input,
                mut permission_updates,
                content_blocks,
            } => {
                let pending_entry =
                    coco_cli::tui_permission_bridge::take_pending(&pending_approvals, &request_id)
                        .await;

                let always_allow_options_allowed =
                    coco_cli::tui_permission_bridge::settings_allow_always_allow_options(
                        &runtime.runtime_config.settings,
                    );
                if pending_entry.is_some()
                    && !always_allow_options_allowed
                    && !permission_updates.is_empty()
                {
                    warn!(
                        %request_id,
                        "dropping permission updates because managed policy disables always-allow"
                    );
                    permission_updates.clear();
                }

                // Apply any rule additions the user authorized
                // ("Always Allow" or future destination-picker
                // selections) BEFORE resolving the bridge. Order
                // matches TS `applyPermissionUpdate` →
                // `persistPermissionUpdates` so subsequent same-tool
                // calls within the turn pick up the rule.
                if pending_entry.is_some() && approved && !permission_updates.is_empty() {
                    let updates_for_apply = permission_updates.clone();
                    runtime
                        .update_engine_config(move |cfg| {
                            // Build a transient `ToolPermissionContext`
                            // view over the engine config's rule maps,
                            // run the typed apply helper, write the
                            // mutated maps back. `apply_permission_updates`
                            // is the single source of truth for rule
                            // mutation (TS `PermissionUpdate.ts`); we
                            // never edit the maps inline so audit logs
                            // and persistence consumers see one shape.
                            let ctx = coco_types::ToolPermissionContext {
                                mode: cfg.permission_mode,
                                additional_dirs: cfg.session_additional_dirs.clone(),
                                allow_rules: cfg.allow_rules.clone(),
                                deny_rules: cfg.deny_rules.clone(),
                                ask_rules: cfg.ask_rules.clone(),
                                bypass_available: cfg.bypass_permissions_available,
                                pre_plan_mode: None,
                                stripped_dangerous_rules: None,
                                session_plan_file: None,
                                permission_rule_source_roots: cfg
                                    .permission_rule_source_roots
                                    .clone(),
                            };
                            let updated =
                                coco_permissions::apply_permission_updates(ctx, &updates_for_apply);
                            cfg.allow_rules = updated.allow_rules;
                            cfg.deny_rules = updated.deny_rules;
                            cfg.ask_rules = updated.ask_rules;
                            cfg.session_additional_dirs = updated.additional_dirs;
                            // Mode updates are normally driven by the
                            // `/permission-mode` slash command path,
                            // not the dialog. But if a future caller
                            // bundles `SetMode` into the same update
                            // batch, honor it on the engine_config so
                            // subsequent turns see the change.
                            cfg.permission_mode = updated.mode;
                        })
                        .await;

                    // Persist updates whose destination wires to a
                    // settings.json layer (User / Project / Local).
                    // Session / CliArg / Command destinations are
                    // in-memory only — matches TS
                    // `persistPermissionUpdates` which no-ops on
                    // non-persistable destinations.
                    //
                    // Phase A: TUI dialog only emits Session-scoped
                    // updates today, so the persist branch is
                    // exercised once Phase B adds the destination
                    // sub-picker. The store is constructed cheaply
                    // per-call (just holds cwd + optional flag-
                    // settings path) so we don't need to thread an
                    // `Arc<PermissionStore>` through SessionRuntime
                    // until Phase B.
                    let cwd =
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let store = coco_permissions::SettingsPermissionStore::new(cwd);
                    use coco_permissions::permissions_store::PermissionStore;
                    for update in &permission_updates {
                        let Some(dest) = update.destination() else {
                            continue;
                        };
                        if !coco_permissions::permission_updates::supports_persistence(dest) {
                            continue;
                        }
                        if let Err(e) = store.persist_update(update) {
                            warn!(error = %e, "failed to persist permission update");
                        }
                    }

                    // Mirror `dispatch_permissions_mutation`
                    // (`/permissions allow|deny|reset`) and push a
                    // `command_permissions` system-reminder so the
                    // next turn's prompt informs the model that the
                    // permission ruleset changed. Without this, a
                    // dialog "Always Allow Bash" would silently take
                    // effect — the slash-command path already does
                    // this for symmetry.
                    let mailbox = runtime.reminder_mailbox_handle();
                    for update in &permission_updates {
                        if let coco_types::PermissionUpdate::AddRules { rules, destination } =
                            update
                        {
                            for rule in rules {
                                let scope = match destination {
                                    coco_types::PermissionUpdateDestination::Session => {
                                        "session scope"
                                    }
                                    coco_types::PermissionUpdateDestination::UserSettings => {
                                        "user settings"
                                    }
                                    coco_types::PermissionUpdateDestination::ProjectSettings => {
                                        "project settings"
                                    }
                                    coco_types::PermissionUpdateDestination::LocalSettings => {
                                        "local settings"
                                    }
                                    coco_types::PermissionUpdateDestination::CliArg => "CLI flag",
                                    coco_types::PermissionUpdateDestination::Command => {
                                        "command scope"
                                    }
                                };
                                let behavior = match rule.behavior {
                                    coco_types::PermissionBehavior::Allow => "allow",
                                    coco_types::PermissionBehavior::Deny => "deny",
                                    coco_types::PermissionBehavior::Ask => "ask",
                                };
                                mailbox.put_command_permissions(format!(
                                    "Permission rule added: {behavior} `{tool}` ({scope}).",
                                    tool = rule.value.tool_pattern,
                                ));
                            }
                        }
                    }
                }

                // Always-allow with empty `permission_updates` is the
                // legacy path (pre-Phase A). Treat as one-shot approve
                // — the rule plumbing the prompt produced was lost
                // somewhere between TUI and runner. Log and move
                // on rather than failing.
                if always_allow && permission_updates.is_empty() {
                    debug!(
                        %request_id,
                        "always_allow set without permission_updates; treating as one-shot approve"
                    );
                }

                // Route the user's Approve / Deny back to the pending
                // oneshot the `TuiPermissionBridge` is awaiting.
                // `applied_updates` are forwarded so audit/logging
                // downstream sees the user's intent. Stale request_ids
                // (already resolved or timed-out) are logged and
                // dropped — TS does the same when a prompt closes
                // after the engine moved on.
                if let Some(entry) = pending_entry {
                    let resolved = coco_cli::tui_permission_bridge::send_resolution(
                        entry,
                        approved,
                        feedback,
                        permission_updates,
                        updated_input,
                        content_blocks,
                    );
                    if !resolved {
                        info!(
                            %request_id,
                            approved,
                            "ApprovalResponse receiver dropped after request was taken"
                        );
                    }
                } else {
                    info!(
                        %request_id,
                        approved,
                        "ApprovalResponse for unknown request_id (already resolved or stale)"
                    );
                }
            }

            UserCommand::Shutdown { reason } => {
                info!(%reason, "Shutdown requested by TUI");
                // Drain in-flight turn before emitting SessionEnded so
                // the engine stops promptly and any pending events
                // flush through `event_tx` ahead of the lifecycle
                // notification.
                drain_active_turn(&active_turn).await;
                drain_pending_memory_extraction(&runtime).await;
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

            UserCommand::PushSystemMessage { kind } => {
                // TUI-originated transcript content (slash output,
                // file-open notices, plan-rejected body, …) round-trips
                // through engine `MessageHistory` so every observer
                // (TUI transcript view, SDK consumers, JSONL transcript)
                // sees it via the same `MessageAppended` event stream as
                // engine-pushed content. See
                // `engine-tui-unified-transcript-plan.md` §3 Commit 2.
                let msg = build_system_message_from_push_kind(kind);
                let mut h = runtime.history.lock().await;
                let event_tx_opt = Some(event_tx.clone());
                coco_query::history_sync::history_push_and_emit(&mut h, msg, &event_tx_opt).await;
            }

            // Other commands: log and skip for now
            other => {
                info!(?other, "Unhandled UserCommand in agent driver");
            }
        }
    }

    // Driver loop exited (sender dropped or Shutdown). Drain any
    // turn that's still running so we don't leak a JoinHandle, and
    // wait briefly on any pending auto-memory extraction so partial
    // writes don't get cut off.
    drain_active_turn(&active_turn).await;
    drain_pending_memory_extraction(&runtime).await;
    info!("Agent driver stopped");
}

/// Wait up to `coco_memory::service::extract::DEFAULT_DRAIN_TIMEOUT`
/// (60s) for an in-flight extraction fork to finish before the session
/// shuts down. TS parity: `print.ts` awaits
/// `drainPendingExtraction(60_000)` before emitting the lifecycle exit.
/// Silently no-ops when `Feature::AutoMemory` is off (no runtime).
async fn drain_pending_memory_extraction(runtime: &Arc<crate::session_runtime::SessionRuntime>) {
    let Some(memory_runtime) = runtime.memory_runtime() else {
        return;
    };
    if !memory_runtime
        .extract
        .drain(coco_memory::service::extract::DEFAULT_DRAIN_TIMEOUT)
        .await
    {
        warn!("auto-memory extraction did not drain within timeout — continuing shutdown");
    }
}

/// Body of `UserCommand::SubmitInput` extracted into an async fn so
/// it can be `tokio::spawn`ed. The dispatch loop stores the
/// `JoinHandle` in `active_turn` and continues to recv the next
/// command — letting `Interrupt` / `Compact` /
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
    /// Trigger the clear flow for `/clear` / `/clear all` /
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
    /// Open a concrete session plan file through the same external
    /// editor terminal handoff used by prompt and memory editing.
    TriggerOpenPlanEditor { path: std::path::PathBuf },
    /// Rebuild the slash-command registry from disk and atomically
    /// swap. Triggered by `/reload-plugins`. TS:
    /// `useManagePlugins.refreshActivePlugins`.
    TriggerReloadPlugins,
    /// Reload the live `HookRegistry` from the latest `RuntimeConfig`
    /// snapshot. Triggered by `/hooks reload`. TS:
    /// `updateHooksConfigSnapshot()` (`utils/hooks/hooksConfigSnapshot.ts`).
    /// Slash commands run only at turn boundaries (the dispatch loop
    /// `drain_active_turn`s before invoking them), so
    /// PreToolUse/PostToolUse for an in-flight call cannot see
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

fn session_plans_dir(
    config_home: &std::path::Path,
    project_dir: Option<&std::path::Path>,
    plans_directory_setting: Option<&str>,
) -> std::path::PathBuf {
    coco_context::resolve_plans_directory(config_home, project_dir, plans_directory_setting)
}

fn session_plan_file_path(
    config_home: &std::path::Path,
    project_dir: Option<&std::path::Path>,
    plans_directory_setting: Option<&str>,
    session_id: &str,
) -> std::path::PathBuf {
    let plans_dir = session_plans_dir(config_home, project_dir, plans_directory_setting);
    coco_context::get_plan_file_path(session_id, &plans_dir, /*agent_id*/ None)
}

async fn runtime_session_plan_file_path(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
) -> std::path::PathBuf {
    let session_id = runtime.current_session_id().await;
    session_plan_file_path(
        &runtime.config_home,
        runtime.runtime_config.paths.project_dir.as_deref(),
        runtime
            .runtime_config
            .settings
            .merged
            .plans_directory
            .as_deref(),
        &session_id,
    )
}

async fn prepare_external_editor_request(
    pending_editor_requests: &mut HashMap<String, PendingEditorRequest>,
    request: PendingEditorRequest,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    let request_id = uuid::Uuid::new_v4().to_string();
    pending_editor_requests.insert(request_id.clone(), request);
    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::ExternalEditorPrepare {
            request_id,
        }))
        .await;
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
    // `/clear` mutates runtime state. Keep it in the command layer so
    // typed and palette dispatch both run the real clear flow instead
    // of letting a registry text handler print without clearing.
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
    // `/rewind` / `/checkpoint` need current TUI session state for the
    // picker, so the command layer asks the TUI to open the modal.
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
                // I-1 (Authority): pre-computed compact summary push
                // goes through history_push_and_emit so the TUI
                // TranscriptView and SDK observers see the new
                // boundary marker, not just the slash text echo.
                let mut h = runtime.history.lock().await;
                let event_tx_opt = Some(event_tx.clone());
                coco_query::history_sync::history_push_and_emit(
                    &mut h,
                    coco_compact::build_compact_summary_message(&summary),
                    &event_tx_opt,
                )
                .await;
            }
            emit_slash_text(event_tx, name, &display_text).await;
            SlashOutcome::Handled
        }
        CommandResult::OpenDialog(spec) => {
            // Wired dialogs route to TuiOnlyEvent so the TUI opens the
            // modal; unwired dialogs emit a localized breadcrumb.
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
                        .map(|e| {
                            let exists = e.path.exists();
                            coco_types::MemoryDialogEntry {
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
                                row_kind: coco_types::MemoryDialogRowKind::File {
                                    exists,
                                    read_only: false,
                                },
                            }
                        })
                        .collect();
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::OpenMemoryDialog {
                            entries: wire_entries,
                        }))
                        .await;
                }
                DialogSpec::ModelPicker => {
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::OpenModelPicker))
                        .await;
                }
                DialogSpec::PluginPicker
                | DialogSpec::McpbConfig { .. }
                | DialogSpec::Confirm { .. } => {
                    let dialog_kind = match spec {
                        DialogSpec::PluginPicker => "plugin picker",
                        DialogSpec::McpbConfig { .. } => "MCPB config form",
                        DialogSpec::Confirm { .. } => "confirm dialog",
                        DialogSpec::MessageSelector
                        | DialogSpec::MemoryFileSelector { .. }
                        | DialogSpec::ModelPicker => unreachable!(),
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

/// Pure decision used by `dispatch_plan`: after a `/plan <description>`
/// successfully flips into plan mode, should the slash command fire a
/// query for the description? TS parity: `commands/plan/plan.tsx:84-89`
/// — `description && description !== 'open'` selects `shouldQuery: true`.
/// Returns `Some(trimmed_description)` when a query should fire, else
/// `None`. Pure so the TS-parity rule is regression-tested without a
/// `SessionRuntime` fixture.
fn plan_command_query_after_flip(args: &str) -> Option<&str> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed == "open" {
        None
    } else {
        Some(trimmed)
    }
}

/// `/plan` dispatch with full session-runtime context.
///
/// Mirrors TS `commands/plan/plan.tsx:64-121` byte-for-byte intent:
/// typing `/plan` IS the consent to enter plan mode, so the dispatcher
/// flips state directly via the same dual-write path
/// `UserCommand::SetPermissionMode` uses (engine_config + app_state)
/// plus the plan-mode-specific patch (`pre_plan_mode`,
/// `plan_mode_entry_ms`, `needs_plan_mode_exit_attachment` cleared).
/// The model never sees a redundant `EnterPlanMode` Yes/No dialog.
///
/// Per-arg behaviour matches TS:
/// - `""`         → flip if needed, then show current plan or hint
/// - `"open"`     → flip if needed, ensure file, launch `$EDITOR`/`vi`
/// - `<description>` → flip if needed; if state changed, fire a query
///   with the description (TS `shouldQuery: true`); if already in
///   plan mode, ignore the description and show the plan (TS lines
///   92-119).
async fn dispatch_plan(
    args: &str,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> SlashOutcome {
    let args = args.trim();
    let session_id = runtime.current_session_id().await;
    let project_dir = runtime.runtime_config.paths.project_dir.as_deref();
    let plans_directory_setting = runtime
        .runtime_config
        .settings
        .merged
        .plans_directory
        .as_deref();
    let plans_dir = session_plans_dir(&runtime.config_home, project_dir, plans_directory_setting);

    // TS `commands/plan/plan.tsx:70-91` reads `appState.toolPermissionContext.mode`
    // first; coco-rs does the same — live cross-turn state
    // (`app_state.permission_mode`) wins when present, else fall
    // back to the engine_config value (covers the "app_state not yet
    // primed" case at the start of a fresh session).
    let live_app_mode = runtime.app_state.read().await.permission_mode;
    let prev_mode = match live_app_mode {
        Some(m) => m,
        None => runtime.current_engine_config().await.permission_mode,
    };
    let was_in_plan = prev_mode == coco_types::PermissionMode::Plan;

    // TS `commands/plan/plan.tsx:73-82` flips state for ALL `/plan`
    // invocations when not already in plan mode — bare `/plan`,
    // `/plan open`, and `/plan <description>` all consent to plan
    // mode equally.
    if !was_in_plan {
        runtime
            .update_engine_config(|cfg| cfg.permission_mode = coco_types::PermissionMode::Plan)
            .await;
        let patch = coco_tools::build_enter_plan_mode_patch(prev_mode);
        {
            let mut guard = runtime.app_state.write().await;
            patch(&mut guard);
        }
        info!(
            session_id = %session_id,
            from = ?prev_mode,
            to = ?coco_types::PermissionMode::Plan,
            "TUI /plan: direct-toggle to Plan mode (TS commands/plan/plan.tsx parity)",
        );
    }

    // Path to the (resolved) session plan file — used by every arm.
    let plan_path =
        coco_context::get_plan_file_path(&session_id, &plans_dir, /*agent_id*/ None);

    if args.is_empty() {
        let content = coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
        let body = match content {
            Some(body) if !body.trim().is_empty() => format!(
                "## Current Plan\n\n*{}*\n\n{}\n\nRun `/plan open` to edit in $EDITOR.",
                plan_path.display(),
                body
            ),
            _ => format!(
                "No plan written yet for this session.\n\n\
                 Plan file: `{}`\n\n\
                 Run `/plan <description>` to plan for a task in plan mode, \
                 or `/plan open` to start an empty plan in $EDITOR.",
                plan_path.display()
            ),
        };
        let text = if was_in_plan {
            body
        } else {
            format!("Enabled plan mode.\n\n{body}")
        };
        emit_slash_text(event_tx, "plan", &text).await;
        return SlashOutcome::Handled;
    }

    if args == "open" {
        let text = if was_in_plan {
            format!("Opening plan file: {}", plan_path.display())
        } else {
            format!(
                "Enabled plan mode.\n\nOpening plan file: {}",
                plan_path.display()
            )
        };
        emit_slash_text(event_tx, "plan", &text).await;
        return SlashOutcome::TriggerOpenPlanEditor { path: plan_path };
    }

    // `/plan <description>` —
    // - TS lines 73-91: flipped to plan mode → fire query with the
    //   user input (`shouldQuery: true`). coco-rs returns
    //   `RunEngine { content: <description> }`.
    // - TS lines 92-119: already in plan mode → ignore the
    //   description, just show the plan. coco-rs matches.
    if was_in_plan {
        let content = coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
        let text = match content {
            Some(body) if !body.trim().is_empty() => format!(
                "Already in plan mode.\n\n## Current Plan\n\n*{}*\n\n{}\n\n\
                 Run `/plan open` to edit in $EDITOR.",
                plan_path.display(),
                body
            ),
            _ => "Already in plan mode. No plan written yet.".to_string(),
        };
        emit_slash_text(event_tx, "plan", &text).await;
        return SlashOutcome::Handled;
    }
    match plan_command_query_after_flip(args) {
        Some(desc) => SlashOutcome::RunEngine {
            content: desc.to_string(),
        },
        None => {
            // Unreachable in practice — bare `/plan` and `/plan open`
            // are handled by the earlier branches. Kept defensive so
            // future edits to the cascade can't silently fall through.
            SlashOutcome::Handled
        }
    }
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
    /// Written by whoever fires `.cancel()` so the turn task can emit a
    /// `TurnInterrupted{reason}` with the right discriminant after the
    /// engine returns. `OnceLock` because every cancel callsite is a
    /// first-writer (additional cancels are no-ops at the token level
    /// too). `None` means the turn ended naturally — no terminal event
    /// is synthesised by the runner.
    ///
    /// TS analogue: `abortController.abort(reason)` carries `reason` on
    /// `signal.reason`. The `.finally` block in `REPL.tsx:3001` reads
    /// the reason to decide whether auto-restore applies.
    cancel_reason: Arc<OnceLock<CancelReason>>,
}

enum PendingEditorRequest {
    Memory { path: std::path::PathBuf },
    Plan { path: std::path::PathBuf },
    Prompt { initial_content: String },
}

/// Cancel the in-flight turn (if any) and await its completion.
/// Used by every arm whose semantics conflict with a concurrent
/// turn (Clear / Compact / Rewind / Shutdown / next SubmitInput).
///
/// Always records `SystemPreempt` as the reason — these callers are
/// running cleanup work, not honouring a user "stop this turn"
/// request. `UserCommand::Interrupt` sets `UserCancel` *before*
/// invoking `.cancel()` so the OnceLock has already been written by
/// the time the loop reaches `drain_active_turn` (write-once means
/// the subsequent `SystemPreempt` write here is silently dropped).
async fn drain_active_turn(slot: &Arc<Mutex<Option<ActiveTurn>>>) {
    let state = { slot.lock().await.take() };
    if let Some(s) = state {
        let _ = s.cancel_reason.set(CancelReason::SystemPreempt);
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
    let mut history = coco_messages::MessageHistory::new();
    for arc in runtime.history.lock().await.as_slice().iter().cloned() {
        history.push_arc(arc);
    }
    let event_tx_opt = Some(event_tx.clone());
    engine
        .run_manual_compact(&mut history, &event_tx_opt, custom_instructions)
        .await;
    {
        let mut h = runtime.history.lock().await;
        *h = history;
    }
}

/// Run the clear flow. Drains any active turn first since clear mutates
/// session_id + resets several per-session caches. TS: `clearConversation()`.
///
/// Plan I-1 (Authority): emits a wire-visible event after the clear so
/// the TUI's `TranscriptView` and SDK NDJSON observers stay coherent.
/// Full-scope `/clear` rotates session_id → emit
/// `SessionResetForResume { session_id: new }`; lighter `/clear history`
/// keeps the same session id → emit `MessageTruncated { keep_count: 0 }`.
async fn run_clear_conversation(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    scope: ClearScope,
    active_turn: &Arc<Mutex<Option<ActiveTurn>>>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    drain_active_turn(active_turn).await;
    if let Err(e) = runtime.clear_conversation(scope).await {
        warn!(error = %e, "/clear failed");
        return;
    }
    let notif = match scope {
        ClearScope::History => ServerNotification::MessageTruncated {
            keep_count: 0,
            session_id: String::new(),
            agent_id: None,
        },
        ClearScope::Conversation | ClearScope::All => {
            let new_session_id = runtime.current_session_id().await;
            ServerNotification::SessionResetForResume {
                session_id: new_session_id,
                agent_id: None,
            }
        }
    };
    let _ = event_tx.send(CoreEvent::Protocol(notif)).await;
}

/// Force auto-memory consolidation now (skips the three-gate scheduler).
/// Mirrors the SDK runner's `/dream` short-circuit. Silently no-ops
/// when `Feature::AutoMemory` is off — matches TS.
///
/// Uses [`coco_memory::DreamService::force`] so the time / session /
/// scan-throttle gates are bypassed; the PID + mtime CAS lock is still
/// acquired so this can't race with an in-flight auto-dream.
async fn run_dream_consolidation(runtime: &Arc<crate::session_runtime::SessionRuntime>) {
    let Some(memory_runtime) = runtime.memory_runtime().cloned() else {
        info!("/dream: no MemoryRuntime (Feature::AutoMemory off); skipping");
        return;
    };
    let transcript_dir = memory_runtime
        .transcript_dir()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let now_ms = coco_memory::service::dream::DreamService::now_ms();
    let _ = memory_runtime
        .dream
        .force(&transcript_dir, Vec::new, now_ms)
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
    let history_msgs = runtime.history.lock().await.as_slice().to_vec();
    let tokens = coco_compact::estimate_tokens(&history_msgs);
    // TS parity (`sessionMemory.ts:441-442`): manual /summary still
    // walks history to decide whether to advance the safely-summarized
    // cursor. last_message_id is the latest history uuid; the cursor
    // only advances inside `force` when the previous assistant turn
    // had no tool calls.
    let last_msg_id = history_msgs
        .last()
        .and_then(|m| m.uuid())
        .map(uuid::Uuid::to_string);
    let had_tool_calls = coco_messages::count_tool_calls_in_last_assistant_turn(&history_msgs) > 0;
    let _ = memory_runtime
        .session_memory
        .force(tokens, last_msg_id, had_tool_calls)
        .await;
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
///
/// After the swap we also push the fresh visible-command list to the
/// TUI via [`TuiOnlyEvent::AvailableCommandsRefreshed`] so the `/`
/// autocomplete popup and command palette stop pointing at stale names
/// from removed plugins.
async fn run_reload_plugins(
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let count = runtime.reload_plugins(&cwd).await;
    let body = format!("Reloaded — {count} commands now registered.");
    emit_slash_text(event_tx, "reload-plugins", &body).await;

    let snapshot = runtime.current_command_registry().await.snapshot_for_ui();
    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: snapshot,
        }))
        .await;
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
                "Session permission rules reset. Built-in read-only allow is mode behavior."
                    .to_string(),
            );
            "Session permission rules reset. Custom session allow/deny entries were cleared; \
             built-in read-only tools remain allowed by the active permission mode. File-based rules \
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
    cancel_reason: Arc<OnceLock<CancelReason>>,
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
        // Persist the prompt + system warning via history_push_and_emit
        // so the TUI transcript view picks them up — no LLM call follows
        // this branch, so a silent h.push would leave the user without
        // any visual record of their prompt.
        {
            let mut h = runtime.history.lock().await;
            let event_tx_opt = Some(event_tx.clone());
            coco_query::history_sync::history_push_and_emit(
                &mut h,
                coco_messages::create_user_message(&content),
                &event_tx_opt,
            )
            .await;
            coco_query::history_sync::history_push_and_emit(
                &mut h,
                coco_messages::create_user_message(&stop_msg),
                &event_tx_opt,
            )
            .await;
        }
        return;
    }

    let new_turn_messages = coco_cli::at_mention_turn::build_messages_for_turn(&inputs);

    // Persist user message immediately so engine errors don't lose it.
    // history_push_and_emit fires MessageAppended for each new turn
    // message so the TUI transcript view surfaces them via the standard
    // round-trip (replaces the legacy TUI-local optimistic add_message).
    let messages: Vec<coco_messages::Message> = {
        let mut h = runtime.history.lock().await;
        let event_tx_opt = Some(event_tx.clone());
        for m in new_turn_messages.iter().cloned() {
            coco_query::history_sync::history_push_and_emit(&mut h, m, &event_tx_opt).await;
        }
        h.iter().map(|a| (**a).clone()).collect()
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

    let messages: Vec<std::sync::Arc<coco_messages::Message>> =
        messages.into_iter().map(std::sync::Arc::new).collect();
    match engine.run_with_messages(messages, core_event_tx).await {
        Ok(result) => {
            let mut h = runtime.history.lock().await;
            h.clear();
            for arc in result.final_messages {
                h.push_arc(arc);
            }
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

    // Emit a runner-synthesised `TurnInterrupted{reason}` when the turn
    // was cancelled. The engine's `TurnCompleted` (if it fired mid-turn
    // before the cancel was observed) is still forwarded above — the
    // TUI's `on_turn_completed` no longer mutates state on interrupt,
    // so the only auto-restore code path is the `TurnInterrupted`
    // handler below in protocol.rs. Mirrors TS REPL.tsx's `.finally`
    // block (`signal.reason === 'user-cancel'`).
    if let Some(reason) = cancel_reason.get().copied() {
        let _ = event_tx
            .send(CoreEvent::Protocol(ServerNotification::TurnInterrupted(
                coco_types::TurnInterruptedParams {
                    turn_id: None,
                    reason: Some(reason),
                },
            )))
            .await;
    }

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

/// Synchronous TUI-cancel cleanup.
///
/// Truncates the runtime history at the target user message and emits
/// the authoritative `MessageTruncated` event so SDK + TUI observers
/// converge. Never touches the workspace — file rewind belongs to the
/// explicit [`handle_rewind`] flow. See
/// `engine-tui-unified-transcript-plan.md` §7.4.
async fn handle_auto_truncate(
    message_id: &str,
    event_tx: &mpsc::Sender<CoreEvent>,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
) {
    let mut h = runtime.history.lock().await;
    let Some(idx) = h.as_slice().iter().position(|m| match m.as_ref() {
        coco_messages::Message::User(u) => u.uuid.to_string() == message_id,
        _ => false,
    }) else {
        // Auto-restore is fire-and-forget; if the target uuid is gone
        // (e.g. a compaction wiped it between TUI dispatch and engine
        // handler), we'd rather skip silently than panic. `warn` so
        // ops can correlate "auto-restore quietly did nothing" with
        // an upstream truncation race.
        tracing::warn!(
            target: "coco_cli::auto_truncate",
            message_id,
            history_len = h.len(),
            "AutoTruncate target message not found in history (likely raced with compaction)",
        );
        return;
    };
    let pre_count = h.len() as i32;
    let removed = (pre_count - idx as i32).max(0);
    h.truncate(idx);
    tracing::info!(
        target: "coco_cli::auto_truncate",
        message_id,
        keep_count = idx,
        removed,
        "AutoTruncate applied",
    );
    coco_otel::events::emit_conversation_rewind(
        pre_count as i64,
        h.len() as i64,
        removed as i64,
        idx as i64,
    );
    let _ = event_tx
        .send(CoreEvent::Protocol(ServerNotification::MessageTruncated {
            keep_count: idx as i64,
            session_id: String::new(),
            agent_id: None,
        }))
        .await;
}

/// Explicit `/rewind` command driver — picker-confirmed.
///
/// TS: REPL.tsx `rewindConversationTo()` + `fileHistoryRewind()`.
/// Branches on `restore_type`:
///
/// - `Both` / `CodeOnly` — `file_history.rewind()` restores files.
/// - `Both` / `ConversationOnly` — truncate history and emit
///   `MessageTruncated`.
/// - `SummarizeFrom` / `SummarizeUpTo` — dispatch to
///   `handle_summarize_rewind` (partial compaction).
///
/// Always emits `RewindCompleted` so the TUI dismisses the picker overlay.
#[allow(clippy::too_many_arguments)]
async fn handle_rewind(
    restore_type: &coco_tui::state::RestoreType,
    message_id: &str,
    rewound_turn: i32,
    file_history: &Option<Arc<RwLock<FileHistoryState>>>,
    config_home: &std::path::Path,
    session_id: &str,
    event_tx: &mpsc::Sender<CoreEvent>,
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
) {
    use coco_tui::state::RestoreType;

    let mut files_changed = 0i32;
    let mut messages_removed = 0i32;

    tracing::info!(
        target: "coco_cli::rewind",
        message_id,
        rewound_turn,
        ?restore_type,
        "Explicit rewind: dispatching",
    );

    // Summarize variants: dispatch to partial_compact_conversation
    // and replace the history with the resulting messages. TS:
    // `screens/REPL.tsx:4918-4988` (`onSummarize` branch).
    if matches!(
        restore_type,
        RestoreType::SummarizeFrom { .. } | RestoreType::SummarizeUpTo { .. }
    ) {
        handle_summarize_rewind(restore_type, message_id, runtime, event_tx).await;
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
        let mut h = runtime.history.lock().await;
        match h.as_slice().iter().position(|m| match m.as_ref() {
            coco_messages::Message::User(u) => u.uuid.to_string() == message_id,
            _ => false,
        }) {
            Some(idx) => {
                let pre_count = h.len() as i32;
                messages_removed = (pre_count - idx as i32).max(0);
                h.truncate(idx);
                tracing::info!(
                    target: "coco_cli::rewind",
                    message_id,
                    keep_count = idx,
                    messages_removed,
                    files_changed,
                    "Explicit rewind: truncated history",
                );
                // TS `tengu_conversation_rewind` (`screens/REPL.tsx:3665-3670`).
                coco_otel::events::emit_conversation_rewind(
                    pre_count as i64,
                    h.len() as i64,
                    messages_removed as i64,
                    idx as i64,
                );
                // Explicit-rewind converges on the same `MessageTruncated`
                // event the AutoRestore path emits, so SDK consumers see
                // one authoritative truncation signal regardless of trigger.
                let _ = event_tx
                    .send(CoreEvent::Protocol(ServerNotification::MessageTruncated {
                        keep_count: idx as i64,
                        session_id: String::new(),
                        agent_id: None,
                    }))
                    .await;
            }
            None => {
                tracing::warn!(
                    target: "coco_cli::rewind",
                    message_id,
                    history_len = h.len(),
                    "Explicit rewind: target user message not found in history",
                );
            }
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
    runtime: &Arc<crate::session_runtime::SessionRuntime>,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    use coco_messages::PartialCompactDirection;
    use coco_tui::state::RestoreType;

    let (direction, feedback) = match restore_type {
        RestoreType::SummarizeFrom { feedback } => (PartialCompactDirection::Newest, feedback),
        RestoreType::SummarizeUpTo { feedback } => (PartialCompactDirection::Oldest, feedback),
        _ => return,
    };

    let messages: Vec<std::sync::Arc<coco_messages::Message>> = {
        let h = runtime.history.lock().await;
        h.as_slice().to_vec()
    };

    // Pivot index: position of the picked user message in the
    // history vec.
    let pivot_index = match messages.iter().position(|m| match m.as_ref() {
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

    let engine = runtime.build_engine(CancellationToken::new()).await;
    let mut history = coco_messages::MessageHistory::new();
    for arc in messages {
        history.push_arc(arc);
    }
    let event_tx_opt = Some(event_tx.clone());
    let outcome = engine
        .run_partial_compact(
            &mut history,
            &event_tx_opt,
            pivot_index,
            direction,
            feedback.clone(),
            /*custom_instructions*/ None,
        )
        .await;

    match outcome {
        coco_compact::CompactOutcome::Applied => {
            {
                let mut h = runtime.history.lock().await;
                *h = history;
            }
            // Emit a RewindCompleted with empty target so the TUI
            // dismisses the modal + shows a toast, but does NOT try
            // to truncate by message_id (the message is gone after
            // summarization).
            let _ = event_tx
                .send(CoreEvent::Tui(TuiOnlyEvent::RewindCompleted {
                    target_message_id: String::new(),
                    files_changed: 0,
                }))
                .await;
        }
        coco_compact::CompactOutcome::Skipped | coco_compact::CompactOutcome::Failed => {
            warn!("partial-compact rewind failed");
            let _ = event_tx
                .send(CoreEvent::Protocol(coco_query::ServerNotification::Error(
                    coco_types::ErrorParams {
                        message: "Summarize failed".into(),
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
            stop_sequences: None,
        };

        let raw = match client.query(&params).await {
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
                let stop = result.stop_reason;
                if text.is_empty() || stop.is_some_and(coco_messages::StopReason::is_abnormal) {
                    warn!(
                        stop_reason = ?stop,
                        tokens_out = result.usage.output_tokens,
                        text_chars = text.len(),
                        "title generation unexpected outcome — session keeps default title"
                    );
                }
                text
            }
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

/// Encode TUI paste-pill image bytes as base64 [`QueuedImage`]s for
/// `CommandQueue` storage. `QueuedImage` carries a base64 payload (the
/// shape coco-rs uses for system-reminder image attachments) so we
/// encode once at the bridge and the engine ships it through unchanged.
///
/// MIME defaults to `image/png` when missing — matches TS
/// `attachments.ts:1119-1121` (`media_type ?? 'image/png'`).
fn image_data_to_queued(images: &[coco_tui::paste::ImageData]) -> Vec<QueuedImage> {
    use base64::Engine;
    images
        .iter()
        .map(|img| QueuedImage {
            media_type: if img.mime.is_empty() {
                "image/png".to_string()
            } else {
                img.mime.clone()
            },
            data_base64: base64::engine::general_purpose::STANDARD.encode(&img.bytes),
        })
        .collect()
}

/// Construct the engine `Message::System(...)` payload from a
/// TUI-originated [`coco_tui::SystemPushKind`]. Centralises the
/// kind → sub-variant mapping so every TUI-side push site agrees on
/// shape, and so adding a new kind only touches one match arm.
fn build_system_message_from_push_kind(kind: coco_tui::SystemPushKind) -> coco_messages::Message {
    let sys = match kind {
        coco_tui::SystemPushKind::Informational {
            level,
            title,
            message,
        } => {
            coco_messages::SystemMessage::Informational(coco_messages::SystemInformationalMessage {
                uuid: uuid::Uuid::new_v4(),
                level,
                title,
                message,
            })
        }
        coco_tui::SystemPushKind::LocalCommand { command, output } => {
            coco_messages::SystemMessage::LocalCommand(coco_messages::SystemLocalCommandMessage {
                uuid: uuid::Uuid::new_v4(),
                command,
                output,
            })
        }
    };
    coco_messages::Message::System(sys)
}

/// Run a prompt-mode bash submission (`!ls -la`). Mirrors TS's
/// `LocalShellTask` semantics: the model loop is bypassed entirely;
/// the command runs once in the session cwd via [`coco_shell::ShellExecutor`]
/// and the merged stdout+stderr is folded back into the transcript as a
/// `MessageContent::BashOutput`.
///
/// Output is capped at 200 lines / ~8 KB so a `find /` doesn't fill the
/// chat scrollback. The TUI's renderer already truncates display to 20
/// lines (`render_user.rs::BashOutput`) but we keep the wire payload
/// modest to avoid bloating the JSONL transcript.
async fn run_prompt_mode_bash(
    cwd: &std::path::Path,
    user_message_id: String,
    command: String,
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    event_tx: mpsc::Sender<CoreEvent>,
) {
    const MAX_OUTPUT_BYTES: usize = 8 * 1024;
    const MAX_OUTPUT_LINES: usize = 200;

    let mut executor = coco_shell::ShellExecutor::new(cwd);
    let exec_opts = coco_shell::ExecOptions::default();
    let (output, exit_code) = match executor.execute(&command, &exec_opts).await {
        Ok(result) => {
            let mut merged = String::new();
            if !result.stdout.is_empty() {
                merged.push_str(&result.stdout);
            }
            if !result.stderr.is_empty() {
                if !merged.is_empty() && !merged.ends_with('\n') {
                    merged.push('\n');
                }
                merged.push_str(&result.stderr);
            }
            (
                truncate_output(merged, MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES),
                result.exit_code,
            )
        }
        Err(err) => (format!("error: {err}"), -1),
    };

    // Push a single SystemLocalCommandMessage into engine MessageHistory
    // so the chat transcript (TUI + SDK consumers + JSONL) records the
    // bash invocation via the standard `MessageAppended` event path.
    // Pairs with Commit 2 deleting the TUI-local `add_message`
    // optimistic echoes for both the `!cmd` input row and the matching
    // output row.
    {
        let msg = coco_messages::Message::System(coco_messages::SystemMessage::LocalCommand(
            coco_messages::SystemLocalCommandMessage {
                uuid: uuid::Uuid::new_v4(),
                command: command.clone(),
                output: output.clone(),
            },
        ));
        let mut h = runtime.history.lock().await;
        let event_tx_opt = Some(event_tx.clone());
        coco_query::history_sync::history_push_and_emit(&mut h, msg, &event_tx_opt).await;
    }

    let _ = event_tx
        .send(CoreEvent::Tui(TuiOnlyEvent::BashCommandCompleted {
            user_message_id,
            output,
            exit_code,
        }))
        .await;
}

/// Create a selected `/memory` target if needed and launch the configured
/// editor. Effects live in the CLI bridge so TUI reducers stay pure.
async fn run_open_memory_file(path: std::path::PathBuf, event_tx: mpsc::Sender<CoreEvent>) {
    let path_display = path.display().to_string();
    let result = tokio::task::spawn_blocking(move || open_memory_file_blocking(&path)).await;

    let event = match result {
        Ok(Ok(())) => TuiOnlyEvent::MemoryFileOpened { path: path_display },
        Ok(Err(error)) => TuiOnlyEvent::MemoryFileOpenFailed {
            path: path_display,
            error,
        },
        Err(err) => {
            warn!(error = %err, "memory editor task panicked");
            TuiOnlyEvent::MemoryFileOpenFailed {
                path: path_display,
                error: format!("memory editor task failed: {err}"),
            }
        }
    };

    let _ = event_tx.send(CoreEvent::Tui(event)).await;
}

/// Create this session's plan target if needed and launch the configured
/// editor. Uses the same terminal handoff as prompt and memory editing.
async fn run_open_plan_file(path: std::path::PathBuf, event_tx: mpsc::Sender<CoreEvent>) {
    let path_display = path.display().to_string();
    let result = tokio::task::spawn_blocking(move || open_plan_file_blocking(&path)).await;

    let event = match result {
        Ok(Ok(())) => TuiOnlyEvent::PlanFileOpened { path: path_display },
        Ok(Err(error)) => TuiOnlyEvent::PlanFileOpenFailed {
            path: path_display,
            error,
        },
        Err(err) => {
            warn!(error = %err, "plan editor task panicked");
            TuiOnlyEvent::PlanFileOpenFailed {
                path: path_display,
                error: format!("plan editor task failed: {err}"),
            }
        }
    };

    let _ = event_tx.send(CoreEvent::Tui(event)).await;
}

async fn emit_editor_prepare_failed(
    request: PendingEditorRequest,
    error: String,
    event_tx: mpsc::Sender<CoreEvent>,
) {
    let message = format!("failed to prepare terminal for editor: {error}");
    let event = match request {
        PendingEditorRequest::Memory { path } => TuiOnlyEvent::MemoryFileOpenFailed {
            path: path.display().to_string(),
            error: message,
        },
        PendingEditorRequest::Plan { path } => TuiOnlyEvent::PlanFileOpenFailed {
            path: path.display().to_string(),
            error: message,
        },
        PendingEditorRequest::Prompt { .. } => TuiOnlyEvent::PromptEditorFailed { error: message },
    };
    let _ = event_tx.send(CoreEvent::Tui(event)).await;
}

fn open_memory_file_blocking(path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create parent directory: {err}"))?;
    }

    // `wx` semantics: create exclusively, but an existing memory file is
    // fine. We just need the target present before launching the editor.
    if let Err(err) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        && err.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(format!("failed to create memory file: {err}"));
    }

    run_editor_on_file(path)
}

fn open_plan_file_blocking(path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create plans directory: {err}"))?;
    }

    if let Err(err) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        && err.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(format!("failed to create plan file: {err}"));
    }

    run_editor_on_file(path)
}

async fn run_prompt_editor(initial_content: String, event_tx: mpsc::Sender<CoreEvent>) {
    let result =
        tokio::task::spawn_blocking(move || open_prompt_editor_blocking(&initial_content)).await;

    let event = match result {
        Ok(Ok((content, modified))) => TuiOnlyEvent::PromptEditorCompleted { content, modified },
        Ok(Err(error)) => TuiOnlyEvent::PromptEditorFailed { error },
        Err(err) => {
            warn!(error = %err, "prompt editor task panicked");
            TuiOnlyEvent::PromptEditorFailed {
                error: format!("prompt editor task failed: {err}"),
            }
        }
    };

    let _ = event_tx.send(CoreEvent::Tui(event)).await;
}

fn open_prompt_editor_blocking(initial_content: &str) -> Result<(String, bool), String> {
    let path = std::env::temp_dir().join(format!("coco-prompt-edit-{}.md", uuid::Uuid::new_v4()));
    std::fs::write(&path, initial_content)
        .map_err(|err| format!("failed to write editor temp file: {err}"))?;

    let result = run_editor_on_file(&path).and_then(|()| {
        let content = std::fs::read_to_string(&path)
            .map_err(|err| format!("failed to read editor temp file: {err}"))?;
        let modified = content != initial_content;
        Ok((content, modified))
    });

    if let Err(err) = std::fs::remove_file(&path)
        && result.is_ok()
    {
        return Err(format!("failed to remove editor temp file: {err}"));
    }

    result
}

fn resolve_editor_command() -> Result<(String, Vec<String>), String> {
    let raw = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string());

    parse_editor_command(&raw)
}

fn parse_editor_command(raw: &str) -> Result<(String, Vec<String>), String> {
    let mut parts =
        shlex::split(raw).ok_or_else(|| format!("failed to parse editor command `{raw}`"))?;
    if parts.is_empty() {
        return Err("editor command resolved to an empty argv".to_string());
    }
    let program = parts.remove(0);
    Ok((program, parts))
}

fn run_editor_on_file(path: &std::path::Path) -> Result<(), String> {
    let (program, args) = resolve_editor_command()?;
    let status = std::process::Command::new(&program)
        .args(args)
        .arg(path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|err| format!("failed to launch editor `{program}`: {err}"))?;

    if !status.success() {
        return Err(format!("editor `{program}` exited with status {status}"));
    }

    Ok(())
}

/// Cap `text` at the smaller of `max_bytes` or `max_lines`, appending a
/// short notice when truncation occurs. Splits on char boundaries so
/// UTF-8 stays intact even when the byte limit lands mid-codepoint.
fn truncate_output(text: String, max_bytes: usize, max_lines: usize) -> String {
    let line_count = text.lines().count();
    let byte_over = text.len() > max_bytes;
    if !byte_over && line_count <= max_lines {
        return text;
    }
    let mut truncated: String = text.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    if truncated.len() > max_bytes {
        let cut = truncated
            .char_indices()
            .take_while(|(i, _)| *i <= max_bytes)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        truncated.truncate(cut);
    }
    truncated.push_str("\n… (truncated)");
    truncated
}

/// Build the TUI's session-frozen model catalog from the resolved
/// `ModelRegistry`. Each registered `(provider, model_id)` pair becomes
/// one entry; the same `model_id` shared across providers (e.g.
/// `deepseek-v4` under both `deepseek-openai` and `deepseek-anthropic`)
/// yields one entry per provider. Models not paired with any registered
/// provider are unreachable at runtime and therefore not surfaced.
fn build_model_catalog(
    runtime_config: &coco_config::RuntimeConfig,
) -> Vec<coco_tui::state::ModelCatalogEntry> {
    use coco_tui::state::ModelCatalogEntry;
    let mut entries: Vec<ModelCatalogEntry> = runtime_config
        .model_registry
        .resolved
        .iter()
        .map(|((provider, model_id), resolved)| {
            let info = &resolved.info;
            let supported_efforts: Vec<coco_types::ReasoningEffort> = info
                .supported_thinking_levels
                .as_ref()
                .map(|levels| levels.iter().map(|l| l.effort).collect())
                .unwrap_or_default();
            ModelCatalogEntry {
                provider: provider.clone(),
                provider_display: provider_display_label(provider),
                model_id: model_id.clone(),
                display_name: info
                    .display_name
                    .clone()
                    .unwrap_or_else(|| model_id.clone()),
                context_window: Some(info.context_window.get() as i64),
                supported_efforts,
                default_effort: info.default_thinking_level,
            }
        })
        .collect();

    // Stable sort: provider_display → display_name. Matches the
    // picker's section-by-provider rendering.
    entries.sort_by(|a, b| {
        a.provider_display
            .cmp(&b.provider_display)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    entries
}

fn build_provider_statuses(
    runtime_config: &coco_config::RuntimeConfig,
) -> std::collections::HashMap<String, coco_tui::state::ProviderStatus> {
    use coco_tui::state::ProviderStatus;
    use coco_tui::state::ProviderUnavailableReason;

    runtime_config
        .providers
        .iter()
        .map(|(provider, cfg)| {
            let mut unavailable_reasons = Vec::new();
            if cfg.base_url.trim().is_empty() {
                unavailable_reasons.push(ProviderUnavailableReason::MissingBaseUrl);
            }
            let has_api_key = cfg
                .resolve_api_key()
                .is_some_and(|key| !key.trim().is_empty())
                || cfg.client_options.auth_token.is_some();
            if !has_api_key {
                unavailable_reasons.push(ProviderUnavailableReason::MissingApiKey {
                    env_key: cfg.env_key.clone(),
                });
            }
            (
                provider.clone(),
                ProviderStatus {
                    provider_display: provider_display_label(provider),
                    unavailable_reasons,
                },
            )
        })
        .collect()
}

/// Build the initial `model_by_role` map from
/// `RuntimeConfig.model_roles`. Each role gets a `ModelBinding` with
/// `effort: None` (the engine's resolver picks the model's default
/// thinking level when no explicit effort is set).
fn build_model_by_role(
    runtime_config: &coco_config::RuntimeConfig,
) -> std::collections::HashMap<coco_types::ModelRole, coco_tui::state::ModelBinding> {
    use coco_tui::state::ModelBinding;
    use coco_types::ModelRole;
    const ROLES: [ModelRole; 8] = [
        ModelRole::Main,
        ModelRole::Fast,
        ModelRole::Plan,
        ModelRole::Explore,
        ModelRole::Review,
        ModelRole::HookAgent,
        ModelRole::Memory,
        ModelRole::Subagent,
    ];
    let mut out = std::collections::HashMap::new();
    for role in ROLES {
        if let Some(spec) = runtime_config.model_roles.get(role) {
            out.insert(
                role,
                ModelBinding {
                    model_id: spec.model_id.clone(),
                    provider: spec.provider.clone(),
                    effort: None,
                },
            );
        }
    }
    out
}

/// Provider id → human display label. Falls back to the raw id for
/// providers without an explicit label (e.g. user-named custom
/// providers, or `deepseek-openai` / `deepseek-anthropic` which keep
/// their qualified id so the picker can distinguish them).
fn provider_display_label(provider: &str) -> String {
    match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "google" => "Google",
        "deepseek" => "DeepSeek",
        "bytedance" => "ByteDance",
        other => return other.to_string(),
    }
    .to_string()
}

/// Apply a `(role, provider, model_id, effort)` selection to the live
/// [`SessionRuntime`] in-memory and emit
/// [`ServerNotification::ModelRoleChanged`] so the TUI refreshes its
/// `model_by_role` mirror (and, when `role == Main`, the status-bar
/// fields).
///
/// **No file write.** Users who want the binding to survive across
/// sessions edit `~/.coco.json::model_roles.<role>.primary` themselves.
/// The picker is for fast experimentation, not persistence.
///
/// Non-Main roles take effect on the next turn that drives that role.
/// Main effort takes effect immediately; Main model_id changes only
/// take effect on next session restart — see
/// [`SessionRuntime::client_for_role`] doc-comment.
async fn apply_role_in_memory(
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    role: coco_types::ModelRole,
    provider: String,
    model_id: String,
    effort: Option<coco_types::ReasoningEffort>,
    event_tx: tokio::sync::mpsc::Sender<CoreEvent>,
) {
    // Best-effort display name lookup from the resolved registry.
    // Falls back to the model_id itself so the TUI always has *some*
    // label.
    let display_name = runtime
        .runtime_config
        .model_registry
        .resolve(&provider, &model_id)
        .map(|resolved| {
            resolved
                .info
                .display_name
                .clone()
                .unwrap_or_else(|| model_id.clone())
        })
        .unwrap_or_else(|| model_id.clone());
    let api = runtime
        .runtime_config
        .providers
        .get(&provider)
        .map(|p| p.api)
        .unwrap_or(coco_types::ProviderApi::Anthropic);
    let spec = coco_types::ModelSpec {
        provider: provider.clone(),
        api,
        model_id: model_id.clone(),
        display_name,
    };
    // Main: this rebuilds + hot-swaps the live `ApiClient`. The
    // build can fail (e.g. provider unregistered, model_factory
    // error) — surface that as an `Error` notification so the TUI
    // raises a toast / dialog and the user's status bar reverts
    // along with `ModelRoleChanged` not firing. Non-Main: build is
    // lazy (`client_for_role`), so install always succeeds.
    if let Err(err) = runtime
        .apply_role_override(role, crate::session_runtime::RoleOverride { spec, effort })
        .await
    {
        tracing::warn!(
            role = %role.as_str(),
            %provider,
            %model_id,
            error = %err,
            "apply_role_override failed; reverting picker mirror"
        );
        let _ = event_tx
            .send(CoreEvent::Protocol(ServerNotification::Error(
                coco_types::ErrorParams {
                    message: format!(
                        "failed to apply {role_label} → {provider}/{model_id}: {err}",
                        role_label = role.as_str(),
                    ),
                    category: Some("model_role_apply_failed".to_string()),
                    retryable: true,
                },
            )))
            .await;
        return;
    }
    tracing::info!(
        role = %role.as_str(),
        %provider,
        %model_id,
        effort = ?effort,
        "applied in-memory model-role override (not persisted)"
    );
    let _ = event_tx
        .send(CoreEvent::Protocol(ServerNotification::ModelRoleChanged(
            coco_types::ModelRoleChangedParams {
                role,
                model_id,
                provider,
                effort,
            },
        )))
        .await;
}

/// Apply a thinking-level change to the Main role in-memory (Ctrl+T
/// cycle). Reuses [`apply_role_in_memory`]'s end (event emission) so
/// the TUI mirror updates through the same `ModelRoleChanged` path.
async fn apply_main_effort_in_memory(
    runtime: Arc<crate::session_runtime::SessionRuntime>,
    level: String,
    event_tx: tokio::sync::mpsc::Sender<CoreEvent>,
) {
    let effort = match level.parse::<coco_types::ReasoningEffort>() {
        Ok(e) => Some(e),
        Err(err) => {
            tracing::warn!(level = %level, error = %err, "SetThinkingLevel: bad effort string, ignoring");
            return;
        }
    };
    runtime
        .apply_role_effort(coco_types::ModelRole::Main, effort)
        .await;
    // Re-emit ModelRoleChanged so the TUI's `model_by_role` and
    // status-bar mirrors stay coherent. Pull spec back from the
    // runtime so the event carries the live (model, provider) pair.
    let Some(resolved) = runtime.resolve_role(coco_types::ModelRole::Main).await else {
        return;
    };
    let _ = event_tx
        .send(CoreEvent::Protocol(ServerNotification::ModelRoleChanged(
            coco_types::ModelRoleChangedParams {
                role: coco_types::ModelRole::Main,
                model_id: resolved.spec.model_id,
                provider: resolved.spec.provider,
                effort,
            },
        )))
        .await;
}

#[cfg(test)]
#[path = "tui_runner.test.rs"]
mod tests;
