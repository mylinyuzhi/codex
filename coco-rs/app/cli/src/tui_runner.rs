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

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
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
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_query::ServerNotification;
use coco_tool::ToolRegistry;
use coco_tui::App;
use coco_tui::UserCommand;
use coco_tui::app::create_channels;
use coco_types::ToolAppState;
use coco_types::TuiOnlyEvent;
use tokio_util::sync::CancellationToken;

use crate::Cli;

/// Run the interactive TUI mode.
///
/// TS: launchRepl() → <REPL /> (React/Ink component).
/// Rust: spawns agent_driver as background task, runs TUI in foreground.
pub async fn run_tui(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let runtime_config = crate::build_runtime_config_for_cli(cli, &cwd)?;
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

    // Model + client
    let (model, mode) = crate::create_model(cli.model.as_deref(), &runtime_config);
    let model_id = model.model_id().to_string();
    let client = Arc::new(ApiClient::new(
        model,
        runtime_config.api.retry.clone().into(),
    ));

    // Tools
    let mut registry = ToolRegistry::new();
    coco_tools::register_all_tools(&mut registry);
    let tools = Arc::new(registry);

    // System prompt
    let system_prompt = crate::build_system_prompt(&cwd, &model_id);

    // Config home for file history — delegated to global_config so
    // `COCO_CONFIG_DIR` moves everything in lockstep.
    let config_home = coco_config::global_config::config_home();

    // Session ID
    let session_id = uuid::Uuid::new_v4().to_string();

    // File read state — session-level cache for @mention dedup and change detection.
    // TS: readFileState (FileStateCache) — shared across tools and mentions.
    let file_read_state = Arc::new(RwLock::new(coco_context::FileReadState::new()));

    // File history (if enabled)
    // TS: fileHistoryEnabled() in fileHistory.ts — enabled by default
    let file_history = if settings.merged.file_checkpointing_enabled {
        Some(Arc::new(RwLock::new(FileHistoryState::new())))
    } else {
        None
    };

    // Create channels
    let (command_tx, command_rx, notification_tx, notification_rx) = create_channels();

    // Create TUI app
    let mut app = App::new(command_tx, notification_rx)
        .map_err(|e| anyhow::anyhow!("Failed to create TUI: {e}"))?;

    // Wire file_history_enabled into TUI session state so the rewind
    // overlay knows whether to show code restore options.
    app.state_mut().session.file_history_enabled = file_history.is_some();

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

    // Build engine config — threads LoopConfig from the runtime so
    // max_turns / max_tokens / streaming flags flow from settings.json.
    let engine_config = QueryEngineConfig {
        model_name: model_id.clone(),
        permission_mode,
        bypass_permissions_available,
        context_window: 200_000,
        max_output_tokens: 16_384,
        max_turns: runtime_config.loop_config.max_turns.unwrap_or(30),
        max_tokens: cli
            .max_tokens
            .or_else(|| runtime_config.loop_config.max_tokens.map(i64::from)),
        system_prompt: Some(system_prompt),
        streaming_tool_execution: runtime_config.loop_config.enable_streaming_tools,
        session_id: session_id.clone(),
        project_dir: runtime_config
            .paths
            .project_dir
            .clone()
            .or_else(|| Some(cwd.clone())),
        plan_mode_settings: settings.merged.plan_mode.clone(),
        system_reminder: settings.merged.system_reminder.clone(),
        tool_config: runtime_config.tool.clone(),
        sandbox_config: runtime_config.sandbox.clone(),
        memory_config: runtime_config.memory.clone(),
        shell_config: runtime_config.shell.clone(),
        web_fetch_config: runtime_config.web_fetch.clone(),
        web_search_config: runtime_config.web_search.clone(),
        ..Default::default()
    };

    // Shared app_state — persists across turns so PlanModeReminder's
    // throttle counters + has_exited_plan_mode flag survive the
    // per-turn engine construction. Lives in the driver; engines are
    // built with `.with_app_state(app_state.clone())` per turn.
    let app_state: Arc<RwLock<ToolAppState>> = Arc::new(RwLock::new(ToolAppState::default()));

    // Session manager for auto-title persistence (F5). Shares the same
    // `~/.coco/sessions` path as non-TUI entry points (`main::sessions_dir`).
    let sessions_dir = coco_config::global_config::config_home().join("sessions");
    let session_manager = Arc::new(coco_session::SessionManager::new(sessions_dir));
    // Create or load the session record so `auto_title` has a canonical
    // row to write into. Silently swallow failures — title gen is a
    // best-effort advisory feature.
    let _ = session_manager.create(&model_id, &cwd);

    // Background housekeeping: prune session files older than the
    // default retention period. Mirrors TS `utils/cleanup.ts`
    // `DEFAULT_CLEANUP_PERIOD_DAYS = 30`. Fire-and-forget: cleanup
    // failures never block startup.
    {
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
    // users who only configured an API key. Credential check goes
    // through `ProviderConfig::resolve_api_key` so both env and
    // `settings.providers.anthropic.api_key` are honored.
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
    let auto_title_enabled = settings.merged.session.auto_title;

    // Spawn agent driver
    let driver_handle = tokio::spawn(run_agent_driver(
        command_rx,
        notification_tx,
        client,
        tools,
        engine_config,
        file_read_state,
        file_history.clone(),
        config_home,
        session_id,
        app_state,
        session_manager,
        fast_model_spec,
        auto_title_enabled,
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
#[allow(clippy::too_many_arguments)]
async fn run_agent_driver(
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<CoreEvent>,
    client: Arc<ApiClient>,
    tools: Arc<ToolRegistry>,
    mut engine_config: QueryEngineConfig,
    file_read_state: Arc<RwLock<coco_context::FileReadState>>,
    file_history: Option<Arc<RwLock<FileHistoryState>>>,
    config_home: PathBuf,
    session_id: String,
    app_state: Arc<RwLock<ToolAppState>>,
    session_manager: Arc<coco_session::SessionManager>,
    fast_model_spec: Option<coco_types::ModelSpec>,
    auto_title_enabled: bool,
) {
    // One-shot gate: title gen runs at most once per driver instance.
    // If the first attempt fails (no plan, LLM error), we don't retry —
    // the user can always /rename manually.
    let mut title_gen_attempted = false;
    info!("Agent driver started");

    let cancel = CancellationToken::new();

    while let Some(command) = command_rx.recv().await {
        match command {
            UserCommand::SubmitInput {
                content, images, ..
            } => {
                if content.is_empty() {
                    continue;
                }

                // QueryEngine emits its own TurnStarted; no need to emit here.

                // Resolve @mentions into attachments.
                let processed = coco_context::process_user_input(&content);
                let cwd = std::env::current_dir().unwrap_or_default();

                let mut frs = file_read_state.write().await;
                let file_attachments = coco_context::resolve_mentions(
                    &processed.mentions,
                    &mut frs,
                    &coco_context::MentionResolveOptions {
                        cwd: &cwd,
                        max_dir_entries: 1000,
                    },
                )
                .await;

                // Detect files changed on disk since last read.
                let changed_file_attachments = coco_context::detect_changed_files(&mut frs).await;
                drop(frs);

                // Build user message (text + pasted images) and separate
                // attachment messages (file contents, changes).
                let messages = build_turn_messages(
                    &content,
                    &images,
                    &file_attachments,
                    &changed_file_attachments,
                );

                // Build engine with file history + file read state for this turn
                let mut engine = QueryEngine::new(
                    engine_config.clone(),
                    client.clone(),
                    tools.clone(),
                    cancel.clone(),
                    /*hooks*/ None,
                );
                engine = engine.with_file_read_state(file_read_state.clone());
                engine = engine.with_app_state(app_state.clone());
                // Swarm mailbox handle enables ExitPlanMode teammate
                // branch + approval-response polling + leader pending
                // attachment. Harmless no-op in single-agent sessions.
                engine =
                    engine.with_mailbox(Arc::new(coco_state::swarm_mailbox::SwarmMailboxHandle));
                if let Some(ref fh) = file_history {
                    engine = engine.with_file_history(fh.clone(), config_home.clone());
                }

                // Forward CoreEvent directly from QueryEngine to TUI.
                // No mapping layer — TUI consumes CoreEvent natively via handle_core_event().
                let (core_event_tx, mut core_event_rx) = mpsc::channel::<CoreEvent>(256);

                let event_tx_clone = event_tx.clone();
                let forward_handle = tokio::spawn(async move {
                    while let Some(ev) = core_event_rx.recv().await {
                        let _ = event_tx_clone.send(ev).await;
                    }
                });

                match engine.run_with_messages(messages, core_event_tx).await {
                    Ok(_result) => {
                        // QueryEngine emitted TurnCompleted via Protocol layer.
                    }
                    Err(e) => {
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

                // ── F5: auto-title generation post-turn ──
                //
                // Check all five gate conditions; fire-and-forget a
                // title-gen task on the first turn that matches.
                // Subsequent turns are no-ops (`title_gen_attempted`
                // latches). The task is fully decoupled from the turn
                // loop — next turn is unblocked regardless of outcome.
                let plan_exited = app_state.read().await.has_exited_plan_mode;
                let plans_dir = coco_context::resolve_plans_directory(
                    &config_home,
                    /*project_dir*/ None,
                    /*setting*/ None,
                );
                let plan_text =
                    coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
                let plan_non_empty = plan_text
                    .as_deref()
                    .map(|t| !t.trim().is_empty())
                    .unwrap_or(false);
                if should_trigger_title_gen(
                    auto_title_enabled,
                    title_gen_attempted,
                    fast_model_spec.is_some(),
                    plan_exited,
                    plan_non_empty,
                ) && let (Some(spec), Some(text)) = (fast_model_spec.clone(), plan_text)
                {
                    title_gen_attempted = true;
                    spawn_auto_title_task(spec, text, session_manager.clone(), session_id.clone());
                }
            }

            UserCommand::Rewind {
                message_id,
                restore_type,
            } => {
                handle_rewind(
                    &restore_type,
                    &message_id,
                    &file_history,
                    &config_home,
                    &session_id,
                    &event_tx,
                )
                .await;
            }

            UserCommand::RequestDiffStats { message_id } => {
                // Async diff stats computation.
                // TS: fileHistoryGetDiffStats() in MessageSelector useEffect.
                // Emitted as CoreEvent::Tui since this is a UI-only event.
                if let Some(ref fh) = file_history {
                    let fh = fh.read().await;
                    let (files, ins, del) = match fh
                        .get_diff_stats(&message_id, &config_home, &session_id)
                        .await
                    {
                        Ok(stats) => (
                            stats.files_changed.len() as i32,
                            stats.insertions,
                            stats.deletions,
                        ),
                        Err(_) => (0, 0, 0),
                    };
                    let _ = event_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::DiffStatsReady {
                            message_id,
                            files_changed: files,
                            insertions: ins,
                            deletions: del,
                        }))
                        .await;
                }
            }

            UserCommand::Interrupt => {
                cancel.cancel();
            }

            UserCommand::SetPermissionMode { mode } => {
                // TS parity: user toggles mode via Shift+Tab →
                // `setAppState(prev => ({ ...prev, toolPermissionContext:
                // { ...prepared, mode } }))` (PromptInput.tsx:1537-1547).
                // Rust's equivalent single-source-of-truth is
                // `Arc<RwLock<ToolAppState>>` — update it live so any
                // in-flight engine's next `create_tool_context` sees
                // the new mode. Also update `engine_config` so fresh
                // engines built for subsequent turns start in the
                // right mode. Auto-boundary side-effects (strip stash
                // management) go through the shared
                // `apply_auto_transition_to_app_state` helper.
                //
                // Defense-in-depth: the overlay + Shift+Tab cycle
                // already gate BypassPermissions on the capability,
                // but a UI bug that bypasses the gate could escalate
                // here silently. Re-validate against the startup
                // capability and the runtime killswitch; drop the
                // mutation if the transition is illegitimate.
                if mode == coco_types::PermissionMode::BypassPermissions
                    && !engine_config.bypass_permissions_available
                {
                    warn!(
                        session_id = %session_id,
                        requested = ?mode,
                        "TUI SetPermissionMode denied: bypass capability gate is off"
                    );
                    continue;
                }
                let prev_mode = engine_config.permission_mode;
                engine_config.permission_mode = mode;
                {
                    let mut guard = app_state.write().await;
                    guard.permission_mode = Some(mode);
                    coco_permissions::apply_auto_transition_to_app_state(
                        &mut guard, prev_mode, mode,
                    );
                }
                info!(
                    session_id = %session_id,
                    from = ?prev_mode,
                    to = ?mode,
                    "TUI SetPermissionMode propagated to engine_config + app_state",
                );
            }

            UserCommand::ClearConversation { scope } => {
                // TS parity: `clearConversation` resets the engine's
                // in-process state on top of whatever the TUI already
                // cleared locally. The shared `app_state` is the
                // canonical home for plan-mode cross-turn flags, so the
                // reset is mostly a JSON clear.
                // Always clear plan-mode reminder state so the next
                // Plan-mode turn starts with Full (not Reentry) and a
                // fresh attachment counter. `Default` resets every
                // field in one shot — no risk of forgetting one.
                *app_state.write().await = ToolAppState::default();

                // Aggressive scope: slug cache + file history
                // snapshots. Plan files on disk are already removed by
                // the TUI side before sending this command.
                if matches!(scope, coco_tui::command::ClearScope::All) {
                    coco_context::clear_plan_slug(&session_id);
                    {
                        let mut frs = file_read_state.write().await;
                        frs.clear();
                    }
                    if let Some(ref fh) = file_history {
                        let mut fh = fh.write().await;
                        // Clear file-history snapshots so subsequent
                        // /rewind can't reach back across the clear.
                        *fh = coco_context::FileHistoryState::default();
                    }
                }
                // Conversation / History scope: keep file_history so
                // `/resume` can still restore pre-clear file snapshots.
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
                let mailbox: coco_tool::MailboxHandleRef =
                    Arc::new(coco_state::swarm_mailbox::SwarmMailboxHandle);

                let response = coco_tool::PlanApprovalMessage::PlanApprovalResponse(
                    coco_tool::PlanApprovalResponse {
                        request_id: request_id.clone(),
                        approved,
                        feedback: feedback.clone(),
                        permission_mode: None,
                    },
                );
                let envelope = coco_tool::MailboxEnvelope {
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
                    let mut guard = app_state.write().await;
                    if guard.awaiting_plan_approval_request_id.as_deref()
                        == Some(request_id.as_str())
                    {
                        guard.awaiting_plan_approval = false;
                        guard.awaiting_plan_approval_request_id = None;
                    }
                }
            }

            UserCommand::Shutdown => {
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

    info!("Agent driver stopped");
}

/// Handle a rewind command.
///
/// TS: REPL.tsx rewindConversationTo() + fileHistoryRewind()
/// - Code rewind: calls file_history.rewind() to restore files
/// - Conversation rewind: emits RewindCompleted so TUI truncates messages
/// - Both: does both
async fn handle_rewind(
    restore_type: &coco_tui::state::RestoreType,
    message_id: &str,
    file_history: &Option<Arc<RwLock<FileHistoryState>>>,
    config_home: &PathBuf,
    session_id: &str,
    event_tx: &mpsc::Sender<CoreEvent>,
) {
    use coco_tui::state::RestoreType;

    let mut files_changed = 0i32;

    // Code rewind (file restore)
    // TS: fileHistoryRewind() in REPL.tsx onRestoreCode prop
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

    // Conversation rewind: emit TuiOnlyEvent::RewindCompleted so TUI truncates
    // messages, restores permission mode, and repopulates input.
    // TS: rewindConversationTo() + restoreMessageSync() in REPL.tsx
    let should_truncate = matches!(
        restore_type,
        RestoreType::Both | RestoreType::ConversationOnly
    );

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
}

/// Build the list of messages for a turn: user message + attachment messages.
///
/// TS architecture: user message first, then separate attachment messages
/// wrapped in `<system-reminder>` tags with `is_meta: true`.
///
/// - User message: text + pasted clipboard images (inline content parts)
/// - Attachment messages: file contents, directories, changed files (separate messages)
fn build_turn_messages(
    text: &str,
    images: &[coco_tui::ImageData],
    file_attachments: &[Attachment],
    changed_file_attachments: &[Attachment],
) -> Vec<coco_types::Message> {
    use vercel_ai_provider::UserContentPart;

    let mut messages = Vec::new();

    // 1. User message: text + clipboard images
    if images.is_empty() {
        messages.push(coco_messages::create_user_message(text));
    } else {
        let mut parts: Vec<UserContentPart> = vec![UserContentPart::text(text)];
        for img in images {
            parts.push(UserContentPart::image(img.bytes.clone(), &img.mime));
        }
        messages.push(coco_messages::create_user_message_with_parts(parts));
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
fn attachment_to_message(att: &Attachment) -> Option<coco_types::Message> {
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
                use vercel_ai_provider::FilePart;
                use vercel_ai_provider::UserContentPart;
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
) {
    use coco_inference::ApiClient;
    use coco_inference::QueryParams;
    use coco_inference::RetryConfig;
    use coco_types::AssistantContent;
    use coco_types::LlmMessage;

    tokio::spawn(async move {
        let Ok(model) = crate::build_language_model_from_spec(&spec) else {
            // Provider dispatch failed (e.g. missing API key) — silently
            // abandon; `auto_title` is an advisory feature.
            return;
        };
        let client = ApiClient::new(model, RetryConfig::default());

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
fn changed_file_to_message(att: &Attachment) -> Option<coco_types::Message> {
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
