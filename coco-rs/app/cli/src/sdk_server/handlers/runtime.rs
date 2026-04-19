//! Runtime-state mutations (`setModel` / `setPermissionMode` / `setThinking`
//! / `updateEnv` / `stopTask`) plus observability and lightweight stub
//! handlers (`context/usage`, `plugin/reload`, `config/applyFlags`).

use tracing::info;

use super::DEFAULT_SDK_MODEL;
use super::HandlerContext;
use super::HandlerResult;

/// `control/setModel` — mutate the active session's model.
///
/// The updated model takes effect on the *next* `turn/start`. In-flight
/// turns continue running against the previous model (they'd need
/// restarting to swap models mid-call).
///
/// Passing `None` means "revert to the default model", which we
/// interpret as `claude-opus-4-6` (the bootstrap default from
/// `handle_session_start`).
pub(super) async fn handle_set_model(
    params: coco_types::SetModelParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    let new_model = params
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_SDK_MODEL.into());
    info!(
        session_id = %session.session_id,
        old_model = %session.model,
        new_model = %new_model,
        "SdkServer: control/setModel"
    );
    session.model = new_model;
    HandlerResult::ok_empty()
}

/// `control/setPermissionMode` — mutate the session's permission mode.
///
/// TS parity: `cyclePermissionMode` → `setAppState(prev => ({ ...prev,
/// toolPermissionContext: { ...preparedContext, mode: nextMode } }))`
/// (`PromptInput.tsx:1537-1547`). Writes:
/// 1. [`SessionHandle::permission_mode`] — session-scoped override read
///    by `sdk_runner::run_turn` as a fallback when the turn params
///    don't carry an explicit mode.
/// 2. [`SessionHandle::app_state`] `permission_mode` — the engine's
///    live mode source of truth. Updating it mid-session propagates
///    to any in-flight engine's next `create_tool_context` read,
///    mirroring TS's `getAppState()` live-read semantics. Without
///    this write, mid-session toggles are invisible to the plan-mode
///    reminder + permission evaluator.
/// 3. On Auto↔non-Auto transitions, also manages
///    `app_state.stripped_dangerous_rules` stash (cleared on leaving
///    Auto so the next ctx rebuild doesn't carry a stale stash into
///    a non-Auto mode). TS parity: `permissionSetup.ts:627-637`.
pub(super) async fn handle_set_permission_mode(
    params: coco_types::SetPermissionModeParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    // Mid-session bypass guard — TS parity: cli/print.ts:4588-4600.
    // Reject any attempt to escalate into `BypassPermissions` when the
    // session was not launched with one of the authorization flags.
    // Catches accidental SDK clients and closes the ungated-bypass
    // surface exposed by the TUI plan-exit overlay before its fix.
    if params.mode == coco_types::PermissionMode::BypassPermissions
        && !ctx
            .state
            .bypass_permissions_available
            .load(std::sync::atomic::Ordering::Relaxed)
    {
        return HandlerResult::Err {
            code: coco_types::error_codes::PERMISSION_DENIED,
            message: "Cannot set permission mode to bypassPermissions because \
                      the session was not launched with \
                      --dangerously-skip-permissions (or \
                      --allow-dangerously-skip-permissions)."
                .into(),
            data: None,
        };
    }

    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    info!(
        session_id = %session.session_id,
        mode = ?params.mode,
        "SdkServer: control/setPermissionMode"
    );
    session.permission_mode = Some(params.mode);

    // Propagate to app_state so the engine sees the new mode live.
    let app_state = session.app_state.clone();
    // Drop the session write lock before taking app_state's lock to
    // keep lock order consistent (session → app_state never inverted).
    drop(slot);
    let mut guard = app_state.write().await;
    let prev_mode = guard
        .permission_mode
        .unwrap_or(coco_types::PermissionMode::Default);
    guard.permission_mode = Some(params.mode);
    // Auto-boundary strip-stash management lives in the shared helper
    // so TUI + SDK paths stay in sync.
    coco_permissions::apply_auto_transition_to_app_state(&mut guard, prev_mode, params.mode);
    drop(guard);

    // Broadcast the change to any attached client (TUI / SDK
    // subscribers). TS parity: `notifyPermissionModeChanged` in
    // `state/onChangeAppState.ts`. The `bypass_available` field is a
    // snapshot of the (static) session capability — readers that rely
    // on the gate stay consistent without needing a separate event.
    let bypass_available = ctx
        .state
        .bypass_permissions_available
        .load(std::sync::atomic::Ordering::Relaxed);
    let _ = ctx
        .notif_tx
        .send(coco_query::CoreEvent::Protocol(
            coco_types::ServerNotification::PermissionModeChanged(
                coco_types::PermissionModeChangedParams {
                    mode: params.mode,
                    bypass_available,
                },
            ),
        ))
        .await;

    HandlerResult::ok_empty()
}

/// `control/setThinking` — mutate the session's thinking level.
///
/// `thinking_level = None` clears the override so turns fall back to
/// the engine's default (matches TS `max_thinking_tokens: null`).
pub(super) async fn handle_set_thinking(
    params: coco_types::SetThinkingParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    info!(
        session_id = %session.session_id,
        level = ?params.thinking_level,
        "SdkServer: control/setThinking"
    );
    session.thinking_level = params.thinking_level;
    HandlerResult::ok_empty()
}

/// `control/stopTask` — cooperative cancellation of a specific task.
///
/// Coco-rs's in-process background task registry isn't threaded through
/// the SDK server yet, so this is structurally equivalent to
/// `turn/interrupt`: we cancel any in-flight turn so the runner unwinds.
/// The `task_id` is logged for later correlation once the task manager
/// is wired through `SdkServerState`.
pub(super) async fn handle_stop_task(
    params: coco_types::StopTaskParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let slot = ctx.state.session.read().await;
    let Some(session) = slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    match &session.active_turn_cancel {
        Some(token) => {
            info!(
                session_id = %session.session_id,
                task_id = %params.task_id,
                "SdkServer: control/stopTask (cancels active turn)"
            );
            token.cancel();
            HandlerResult::ok_empty()
        }
        None => HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: format!("no task in flight matching task_id {}", params.task_id),
            data: None,
        },
    }
}

/// `control/updateEnv` — merge environment variable updates into the
/// session's override map.
///
/// Passing an empty string for a value is interpreted as "unset" and
/// removes the key from the override map. The resulting map is passed
/// to tool invocations when the shell executor is wired to read it.
pub(super) async fn handle_update_env(
    params: coco_types::UpdateEnvParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let mut slot = ctx.state.session.write().await;
    let Some(session) = slot.as_mut() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session".into(),
            data: None,
        };
    };
    let mut applied = 0;
    let mut cleared = 0;
    for (k, v) in params.env {
        if v.is_empty() {
            if session.env_overrides.remove(&k).is_some() {
                cleared += 1;
            }
        } else {
            session.env_overrides.insert(k, v);
            applied += 1;
        }
    }
    info!(
        session_id = %session.session_id,
        applied,
        cleared,
        total = session.env_overrides.len(),
        "SdkServer: control/updateEnv"
    );
    HandlerResult::ok_empty()
}

/// `context/usage` — return the active session's token usage
/// breakdown.
///
/// Derived from `SessionHandle.stats` which is folded from per-turn
/// `SessionResult` events. This is a coarse total — the rich per-
/// category breakdown from TS (system prompt, tools, history, etc.) is
/// not yet computed; `categories` is empty. A follow-up could wire this
/// via engine-level accounting.
///
/// Errors:
/// - `INVALID_REQUEST` if no session is active
pub(super) async fn handle_context_usage(ctx: &HandlerContext) -> HandlerResult {
    let slot = ctx.state.session.read().await;
    let Some(session) = slot.as_ref() else {
        return HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "no active session; call session/start first".into(),
            data: None,
        };
    };
    let stats = &session.stats;
    // Default context window used by QueryEngineRunner. A future
    // refactor can make this dynamic per-model.
    let max_tokens: i64 = 200_000;
    let total = stats.usage.input_tokens + stats.usage.output_tokens;
    let percentage = if max_tokens > 0 {
        (total as f64 / max_tokens as f64) * 100.0
    } else {
        0.0
    };
    HandlerResult::ok(coco_types::ContextUsageResult {
        total_tokens: total,
        max_tokens,
        raw_max_tokens: max_tokens,
        percentage,
        model: session.model.clone(),
        categories: Vec::new(),
        is_auto_compact_enabled: true,
        auto_compact_threshold: None,
        message_breakdown: None,
    })
}

/// `plugin/reload` — hot-reload plugins.
///
/// Returns an empty result since the SDK server does not yet expose a
/// plugin manager. Acknowledges the client's request so heartbeat-style
/// usage works.
///
/// TS reference: `SDKControlReloadPluginsResponseSchema`.
pub(super) async fn handle_plugin_reload(_ctx: &HandlerContext) -> HandlerResult {
    info!("SdkServer: plugin/reload (no plugin manager wired, returning empty)");
    HandlerResult::ok(coco_types::PluginReloadResult {
        plugins: Vec::new(),
        commands: Vec::new(),
        agents: Vec::new(),
        error_count: 0,
    })
}

/// `config/applyFlags` — apply runtime feature-flag settings.
///
/// Currently logs the flags and acks. A follow-up could merge them into
/// a runtime overrides map on `SdkServerState` so other handlers see
/// the effective values.
///
/// TS reference: `SDKControlApplyFlagSettingsRequestSchema`.
pub(super) async fn handle_config_apply_flags(
    params: coco_types::ConfigApplyFlagsParams,
    _ctx: &HandlerContext,
) -> HandlerResult {
    info!(
        count = params.settings.len(),
        "SdkServer: config/applyFlags (logged; no runtime override map yet)"
    );
    HandlerResult::ok_empty()
}
