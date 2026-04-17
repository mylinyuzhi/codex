//! `initialize` + full session lifecycle (`session/*`) + per-turn event
//! forwarding and session-stat aggregation.

use std::sync::Arc;

use coco_types::CoreEvent;
use coco_types::InitializeResult;
use coco_types::SdkAccountInfo;
use coco_types::SdkModelInfo;
use coco_types::SessionStartResult;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::DEFAULT_SDK_FAST_MODEL;
use super::DEFAULT_SDK_MODEL;
use super::HandlerContext;
use super::HandlerResult;
use super::PROTOCOL_VERSION;
use super::SdkServerState;
use super::SessionHandle;

/// `initialize` — capability negotiation. Returns a TS-conformant
/// `InitializeResult`.
///
/// Data sourcing:
/// - `models`: static list of the two Anthropic models coco-rs ships with
///   (promoted from a fixed table; model discovery is a separate follow-up).
/// - `commands`, `agents`, `account`, `output_style`,
///   `available_output_styles`, `fast_mode_state`: populated from an
///   optional [`super::InitializeBootstrap`] trait object installed via
///   `SdkServer::with_initialize_bootstrap()`. When no bootstrap is
///   wired the handler returns TS-valid defaults (empty lists / default
///   account / `"default"` output style).
/// - Internal `_cocoRs*` extension fields carry the coco-rs binary and
///   protocol version for debugging.
///
/// TS reference: `SDKControlInitializeRequestSchema` +
/// `SDKControlInitializeResponseSchema` in `controlSchemas.ts:57-95`.
pub(super) async fn handle_initialize(
    _params: coco_types::InitializeParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    info!("SdkServer: initialize");

    // Pull the bootstrap provider out of state, drop the read guard, then
    // call its async accessors. Holding the guard across awaits would
    // block any concurrent mutation (e.g. a hot-swap via builder).
    let bootstrap = {
        let slot = ctx.state.initialize_bootstrap.read().await;
        slot.as_ref().map(Arc::clone)
    };

    let (commands, agents, account, output_style, available_output_styles, fast_mode_state) =
        if let Some(b) = bootstrap {
            (
                b.commands().await,
                b.agents().await,
                b.account().await,
                b.output_style().await,
                b.available_output_styles().await,
                b.fast_mode_state().await,
            )
        } else {
            (
                Vec::new(),
                Vec::new(),
                SdkAccountInfo::default(),
                "default".into(),
                vec!["default".into()],
                None,
            )
        };

    let result = InitializeResult {
        commands,
        agents,
        output_style,
        available_output_styles,
        models: vec![
            SdkModelInfo {
                value: DEFAULT_SDK_MODEL.into(),
                display_name: "Claude Opus 4.6".into(),
                description: "Anthropic's most capable model for deep reasoning tasks.".into(),
                supports_effort: Some(true),
                supported_effort_levels: Vec::new(),
                supports_adaptive_thinking: Some(true),
                supports_fast_mode: Some(true),
                supports_auto_mode: Some(true),
            },
            SdkModelInfo {
                value: DEFAULT_SDK_FAST_MODEL.into(),
                display_name: "Claude Sonnet 4.6".into(),
                description: "Fast, cost-efficient model for everyday coding tasks.".into(),
                supports_effort: Some(true),
                supported_effort_levels: Vec::new(),
                supports_adaptive_thinking: Some(true),
                supports_fast_mode: Some(true),
                supports_auto_mode: Some(true),
            },
        ],
        account,
        pid: Some(std::process::id()),
        fast_mode_state,
        protocol_version: PROTOCOL_VERSION.into(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    HandlerResult::ok(result)
}

/// `session/start` — create a new SDK session.
///
/// Records the session in `SdkServerState.session` and returns a generated
/// `session_id`. The QueryEngine is not spawned here — `turn/start` does
/// that per-turn.
///
/// TS reference: `print.ts runHeadless()` creates a session at the top of
/// headless mode; coco-rs lets the SDK client explicitly trigger this via
/// `session/start` instead.
pub(super) async fn handle_session_start(
    params: coco_types::SessionStartParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Short critical section: check the slot is empty, then drop the lock.
    // Holding the session write lock across the subsequent disk persistence
    // would stall every other session-touching handler (turn/start,
    // turn/interrupt, session/archive, and the event forwarder's
    // stat-accumulation path) for the duration of the fs write.
    {
        let session_slot = ctx.state.session.read().await;
        if session_slot.is_some() {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: "a session is already active; archive it first or use session/resume"
                    .into(),
                data: None,
            };
        }
    }

    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    let cwd = params.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let model = params
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_SDK_MODEL.into());

    info!(session_id = %session_id, cwd = %cwd, model = %model, "SdkServer: session/start");

    // Persist to disk if a SessionManager is wired. This makes the
    // session visible to `session/list` and resumable via
    // `session/resume`. Failure to persist is non-fatal — the session
    // still runs in-memory; we log a warning and continue.
    //
    // `SessionManager::save` is sync (`std::fs::write`); run it on the
    // blocking pool so the tokio worker isn't stalled by disk I/O.
    // The manager Arc is cloned out of the read guard which is then
    // dropped BEFORE the spawn_blocking await — holding the guard
    // across a blocking call would serialize every session_manager
    // reader behind this request.
    let manager_arc = {
        let manager_slot = ctx.state.session_manager.read().await;
        manager_slot.as_ref().map(Arc::clone)
    };
    if let Some(manager) = manager_arc {
        let record = coco_session::Session {
            id: session_id.clone(),
            created_at: coco_session::timestamp_now(),
            updated_at: None,
            model: model.clone(),
            working_dir: std::path::PathBuf::from(&cwd),
            title: None,
            message_count: 0,
            total_tokens: 0,
        };
        let save_result = tokio::task::spawn_blocking(move || manager.save(&record)).await;
        match save_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warn!(
                session_id = %session_id,
                error = %e,
                "session/start: failed to persist session to disk"
            ),
            Err(join_err) => warn!(
                session_id = %session_id,
                error = %join_err,
                "session/start: persistence task panicked"
            ),
        }
    }

    // Re-acquire the write lock and install the session. A concurrent
    // session/start would have hit the `is_some()` guard above and
    // errored out before persistence, so this path only races with an
    // external archive — in which case we claim the slot first and
    // the archiver moves on.
    let mut session_slot = ctx.state.session.write().await;
    if session_slot.is_some() {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "a session was started concurrently; retry with session/archive first".into(),
            data: None,
        };
    }
    *session_slot = Some(SessionHandle::new(session_id.clone(), cwd, model));

    HandlerResult::ok(SessionStartResult { session_id })
}

/// Drain per-turn CoreEvents and forward to the outbound notification
/// channel, intercepting session envelope events.
///
/// Specifically:
/// - `SessionResult` events are **not** forwarded. Instead, their stats
///   are folded into `SessionHandle.stats` (only if the current session
///   still matches `owner_session_id`). The aggregated `SessionResult`
///   is emitted once when `session/archive` runs.
/// - `SessionStarted` events are also swallowed (defensive — the current
///   runner doesn't emit them, but if a future runner enables the
///   bootstrap path, we still want exactly one per session from the SDK
///   server side, not one per turn).
/// - All other events pass through unchanged.
///
/// `owner_session_id` is the session this forwarder was created for.
/// If `session/archive` + `session/start` has replaced the active
/// session while this forwarder is still draining, we refuse to fold
/// stats into the unrelated new session.
pub(super) async fn forward_turn_events(
    mut rx: mpsc::Receiver<CoreEvent>,
    tx: mpsc::Sender<CoreEvent>,
    state: Arc<SdkServerState>,
    owner_session_id: String,
) {
    use coco_types::ServerNotification;
    while let Some(event) = rx.recv().await {
        match event {
            CoreEvent::Protocol(ServerNotification::SessionResult(params)) => {
                accumulate_session_result(&state, &owner_session_id, &params).await;
                // Swallow — aggregated result is emitted by session/archive.
            }
            CoreEvent::Protocol(ServerNotification::SessionStarted(_)) => {
                // Swallow: SessionStarted is owned by the SDK server, not the engine.
            }
            other => {
                if tx.send(other).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Fold a per-turn `SessionResult` into the active session's aggregated
/// stats. No-op if:
/// - the session has already been archived, OR
/// - the current session's id doesn't match `owner_session_id` (the
///   turn belongs to an already-archived session whose cleanup is
///   still winding down; we must not contaminate the successor session)
async fn accumulate_session_result(
    state: &SdkServerState,
    owner_session_id: &str,
    params: &coco_types::SessionResultParams,
) {
    let mut slot = state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return;
    };
    if session.session_id != owner_session_id {
        // Cross-session guard: the slot now holds a different session.
        // This turn's stats belong to a dead session — drop them.
        debug!(
            owner = owner_session_id,
            current = %session.session_id,
            "accumulate_session_result: session mismatch, dropping stats"
        );
        return;
    }
    let s = &mut session.stats;
    s.total_turns = s.total_turns.saturating_add(1);
    s.total_duration_api_ms = s
        .total_duration_api_ms
        .saturating_add(params.duration_api_ms);
    s.total_cost_usd += params.total_cost_usd;
    s.usage = s.usage + params.usage;
    for (model, mu) in &params.model_usage {
        let entry = s.model_usage.entry(model.clone()).or_default();
        entry.input_tokens += mu.input_tokens;
        entry.output_tokens += mu.output_tokens;
        entry.cache_read_input_tokens += mu.cache_read_input_tokens;
        entry.cache_creation_input_tokens += mu.cache_creation_input_tokens;
        entry.web_search_requests += mu.web_search_requests;
        entry.cost_usd += mu.cost_usd;
    }
    s.permission_denials
        .extend(params.permission_denials.iter().cloned());
    if params.result.is_some() {
        s.last_result_text = params.result.clone();
    }
    s.last_stop_reason = Some(params.stop_reason.clone());
    if params.is_error {
        s.had_error = true;
        s.errors.extend(params.errors.iter().cloned());
    }
    if let Some(n) = params.num_api_calls {
        s.num_api_calls = s.num_api_calls.saturating_add(n);
    }
}

/// `session/archive` — drop the active session slot.
///
/// Emits the aggregated `SessionResult` (built from the session's
/// accumulated stats) as a final notification before clearing the slot.
/// This gives SDK clients exactly one `SessionResult` per session,
/// regardless of how many `turn/start` calls happened inside it.
///
/// **Ordering note**: The `SessionResult` notification goes through
/// `ctx.notif_tx` (drained by the dispatcher's notification forwarder
/// task) while the archive response goes directly through the
/// transport from the handler. These two paths are not synchronized,
/// so the response may reach the wire *before* the notification. SDK
/// clients that need strict ordering should drain inbound messages
/// until both arrive, matching on type.
///
/// **Archive-during-running-turn**: If a turn is in flight when
/// `session/archive` is called, the aggregate is built from whatever
/// stats have been accumulated so far (the in-flight turn's stats are
/// NOT included — it's cancelled after the aggregate is built). This
/// matches TS headless semantics where archive discards in-progress
/// work.
///
/// Errors:
/// - `INVALID_REQUEST` if no session is active
/// - `INVALID_REQUEST` if the `session_id` param doesn't match the
///   currently-active session (prevents clients from archiving someone
///   else's session by mistake)
pub(super) async fn handle_session_archive(
    params: coco_types::SessionArchiveParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Hold the write lock for the entire archive operation:
    // (1) validate session_id, (2) build the aggregate from a
    // consistent snapshot, (3) clear the slot. This closes the
    // TOCTOU window that an earlier read/write-lock split opened —
    // a concurrent `forward_turn_events` forwarder cannot slip a
    // `SessionResult` into stats between the aggregate build and
    // the clear, because it contends for the same write lock and we
    // hold it end-to-end here.
    //
    // Cancellation of an in-flight turn and emission of the
    // aggregated notification both happen AFTER the lock is released:
    // cancellation is idempotent and the notification send doesn't
    // need the session lock.
    let (result_params, token_to_cancel, turn_handle, forwarder_handle) = {
        let mut slot = ctx.state.session.write().await;
        let Some(session) = slot.as_mut() else {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: "no active session to archive".into(),
                data: None,
            };
        };
        if session.session_id != params.session_id {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: format!(
                    "session_id mismatch: active is {}, archive requested for {}",
                    session.session_id, params.session_id
                ),
                data: None,
            };
        }
        info!(session_id = %params.session_id, "SdkServer: session/archive");
        let result = build_aggregated_session_result(session);
        // Take ownership of the cancel token and both JoinHandles before
        // clearing the slot. The turn task's normal-completion cleanup
        // path checks `session.session_id` against its `owner_session_id`
        // and no-ops if the slot is cleared — so taking these out here
        // is safe w.r.t. the concurrent task cleanup.
        let token = session.active_turn_cancel.take();
        let turn_handle = session.active_turn_task.take();
        let forwarder_handle = session.active_turn_forwarder.take();
        // Clear the slot under the write lock so any racing forwarder
        // that later acquires the lock sees `None` and no-ops.
        *slot = None;
        (result, token, turn_handle, forwarder_handle)
    };

    // Cancel any running turn. Outside the lock because:
    //   (a) `CancellationToken::cancel` is cheap and non-blocking
    //   (b) the turn task's subsequent cleanup (writing to session
    //       slot to clear `active_turn_cancel`) also takes the write
    //       lock — holding it here would deadlock
    if let Some(token) = token_to_cancel {
        token.cancel();
    }

    // Drain the in-flight turn before emitting the aggregated result.
    //
    // Ordering contract: the client must see every per-turn event
    // BEFORE the aggregated `SessionResult` for the session, otherwise
    // a late `AgentMessageDelta` / `TurnFailed` slipping out after the
    // archive notification confuses the wire stream.
    //
    // Sequence is:
    //   1. Wait for the runner task to exit (it drops its `inner_tx`).
    //   2. Wait for the forwarder task to exit (it sees channel closed
    //      once `inner_tx` is dropped, drains any buffered events, and
    //      returns from its loop).
    //
    // Both awaits are bounded by a 5s timeout so a pathological runner
    // ignoring the cancel token can't hang archive indefinitely.
    if let Some(handle) = turn_handle {
        match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
            Ok(Ok(())) => {}
            Ok(Err(join_err)) => warn!(
                session_id = %params.session_id,
                error = %join_err,
                "session/archive: turn task join failed"
            ),
            Err(_) => warn!(
                session_id = %params.session_id,
                "session/archive: turn task did not exit within 5s of cancel; \
                 emitting aggregate anyway (late events may still follow)"
            ),
        }
    }
    if let Some(handle) = forwarder_handle {
        match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
            Ok(Ok(())) => {}
            Ok(Err(join_err)) => warn!(
                session_id = %params.session_id,
                error = %join_err,
                "session/archive: forwarder task join failed"
            ),
            Err(_) => warn!(
                session_id = %params.session_id,
                "session/archive: forwarder task did not drain within 5s"
            ),
        }
    }

    // Delete the persisted session record if a SessionManager is wired.
    // Non-fatal — log and continue if disk delete fails. Runs on the
    // blocking pool to avoid stalling the tokio worker on `remove_file`.
    // Clone the Arc out and drop the read guard before the blocking
    // call so other readers aren't serialized behind disk I/O.
    let manager_arc = {
        let manager_slot = ctx.state.session_manager.read().await;
        manager_slot.as_ref().map(Arc::clone)
    };
    if let Some(manager) = manager_arc {
        let target_id = params.session_id.clone();
        let delete_result = tokio::task::spawn_blocking(move || manager.delete(&target_id)).await;
        match delete_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warn!(
                session_id = %params.session_id,
                error = %e,
                "session/archive: failed to delete persisted session record"
            ),
            Err(join_err) => warn!(
                session_id = %params.session_id,
                error = %join_err,
                "session/archive: delete task panicked"
            ),
        }
    }

    // Emit the aggregated SessionResult on the outbound notification
    // channel. Ignore a send error (transport may have shut down)
    // since the state is already cleared.
    let result_event = CoreEvent::Protocol(coco_types::ServerNotification::SessionResult(
        Box::new(result_params),
    ));
    let _ = ctx.notif_tx.send(result_event).await;

    HandlerResult::ok_empty()
}

/// Build a final `SessionResultParams` from an active session's
/// accumulated stats. Used by `session/archive` to synthesize the
/// once-per-session aggregate the SDK client expects.
fn build_aggregated_session_result(session: &SessionHandle) -> coco_types::SessionResultParams {
    let stats = &session.stats;
    coco_types::SessionResultParams {
        session_id: session.session_id.clone(),
        total_turns: stats.total_turns,
        duration_ms: session.started_at.elapsed().as_millis() as i64,
        duration_api_ms: stats.total_duration_api_ms,
        is_error: stats.had_error,
        stop_reason: stats
            .last_stop_reason
            .clone()
            .unwrap_or_else(|| "archived".into()),
        total_cost_usd: stats.total_cost_usd,
        usage: stats.usage,
        model_usage: stats.model_usage.clone(),
        permission_denials: stats.permission_denials.clone(),
        result: stats.last_result_text.clone(),
        errors: stats.errors.clone(),
        structured_output: None,
        fast_mode_state: None,
        num_api_calls: if stats.num_api_calls > 0 {
            Some(stats.num_api_calls)
        } else {
            None
        },
    }
}

/// Convert a `coco_session::Session` record to the wire-format
/// summary used by list/read/resume results.
fn session_to_summary(s: &coco_session::Session) -> coco_types::SdkSessionSummary {
    coco_types::SdkSessionSummary {
        session_id: s.id.clone(),
        model: s.model.clone(),
        cwd: s.working_dir.to_string_lossy().into_owned(),
        created_at: s.created_at.clone(),
        updated_at: s.updated_at.clone(),
        title: s.title.clone(),
        message_count: s.message_count,
        total_tokens: s.total_tokens,
    }
}

/// `session/list` — enumerate persisted sessions, newest first.
///
/// Delegates to `SessionManager::list()`. Returns an empty list if no
/// manager is wired (session persistence disabled).
///
/// Errors:
/// - `INTERNAL_ERROR` if `SessionManager::list()` fails (e.g. filesystem error)
pub(super) async fn handle_session_list(ctx: &HandlerContext) -> HandlerResult {
    // `list()` walks the session directory with `read_dir` and reads every
    // JSON blob synchronously — offload to the blocking pool so a session-
    // browser client polling this endpoint can't stall the tokio worker.
    // Clone the Arc out and drop the read guard before the blocking call.
    let manager = {
        let slot = ctx.state.session_manager.read().await;
        match slot.as_ref() {
            Some(m) => Arc::clone(m),
            None => {
                info!("SdkServer: session/list (no session manager installed, returning empty)");
                return HandlerResult::ok(coco_types::SessionListResult::default());
            }
        }
    };
    let list_result = tokio::task::spawn_blocking(move || manager.list()).await;
    match list_result {
        Ok(Ok(sessions)) => {
            let summaries = sessions.iter().map(session_to_summary).collect::<Vec<_>>();
            info!(count = summaries.len(), "SdkServer: session/list");
            HandlerResult::ok(coco_types::SessionListResult {
                sessions: summaries,
            })
        }
        Ok(Err(e)) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("session/list failed: {e}"),
            data: None,
        },
        Err(join_err) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("session/list task panicked: {join_err}"),
            data: None,
        },
    }
}

/// `session/read` — load a single persisted session's metadata.
///
/// Returns the summary only; message history retrieval via the JSONL
/// transcript is a follow-up.
///
/// Errors:
/// - `INVALID_REQUEST` if no session manager is wired
/// - `INVALID_REQUEST` if the session_id is not found on disk
pub(super) async fn handle_session_read(
    params: coco_types::SessionReadParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Clone the Arc out and drop the read guard before the blocking call.
    let manager = {
        let slot = ctx.state.session_manager.read().await;
        match slot.as_ref() {
            Some(m) => Arc::clone(m),
            None => {
                return HandlerResult::Err {
                    code: coco_types::error_codes::INVALID_REQUEST,
                    message: "session persistence is not enabled on this server".into(),
                    data: None,
                };
            }
        }
    };
    let session_id = params.session_id.clone();
    let load_result = tokio::task::spawn_blocking(move || manager.load(&session_id)).await;
    match load_result {
        Ok(Ok(session)) => {
            info!(session_id = %params.session_id, "SdkServer: session/read");
            HandlerResult::ok(coco_types::SessionReadResult {
                session: session_to_summary(&session),
                messages: Vec::new(),
                next_cursor: None,
                has_more: false,
            })
        }
        Ok(Err(e)) => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("session/read: {e}"),
            data: None,
        },
        Err(join_err) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("session/read task panicked: {join_err}"),
            data: None,
        },
    }
}

/// `session/resume` — load a persisted session from disk and install
/// it as the active session.
///
/// Replaces the current session slot (if any) with a fresh
/// `SessionHandle` built from the persisted metadata. Any in-flight
/// turn on the previous session is cancelled first to prevent
/// orphaned state.
///
/// Note: this restores session metadata (id, model, cwd) but does NOT
/// reload the message history from the JSONL transcript — the resumed
/// session starts with an empty history. A follow-up will thread the
/// transcript reader in.
///
/// Errors:
/// - `INVALID_REQUEST` if no session manager is wired
/// - `INVALID_REQUEST` if the session_id is not found on disk
/// - `INTERNAL_ERROR` if the session manager's resume operation fails
pub(super) async fn handle_session_resume(
    params: coco_types::SessionResumeParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_slot = ctx.state.session_manager.read().await;
    let Some(manager) = manager_slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "session persistence is not enabled on this server".into(),
            data: None,
        };
    };
    let manager_arc = Arc::clone(manager);
    // Release the manager read lock before acquiring the session write lock
    // to avoid potential lock-ordering complications in future refactors.
    drop(manager_slot);
    let target_id = params.session_id.clone();
    let resume_result = tokio::task::spawn_blocking(move || manager_arc.resume(&target_id)).await;
    let session = match resume_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INVALID_REQUEST,
                message: format!("session/resume: {e}"),
                data: None,
            };
        }
        Err(join_err) => {
            return HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("session/resume task panicked: {join_err}"),
                data: None,
            };
        }
    };

    // Install as the active session. If a session is already active,
    // cancel any in-flight turn and replace it — `session/resume`
    // implicitly archives the prior session.
    let mut slot = ctx.state.session.write().await;
    if let Some(prior) = slot.as_ref()
        && let Some(token) = &prior.active_turn_cancel
    {
        warn!(
            prior_session = %prior.session_id,
            new_session = %session.id,
            "SdkServer: session/resume replaces active session; cancelling in-flight turn"
        );
        token.cancel();
    }
    *slot = Some(SessionHandle::new(
        session.id.clone(),
        session.working_dir.to_string_lossy().into_owned(),
        session.model.clone(),
    ));

    info!(session_id = %session.id, "SdkServer: session/resume");
    HandlerResult::ok(coco_types::SessionResumeResult {
        session: session_to_summary(&session),
    })
}
