//! Typed cross-turn shared state carried on `ToolUseContext.app_state`.
//!
//! This struct replaces a previously untyped `serde_json::Value` map. It
//! is shared between `coco-tools` (writers: EnterPlanMode/ExitPlanMode),
//! `coco-query` (reader+writer: PlanModeReminder), and the `coco-cli`
//! driver (writer: ClearConversation + reader: auto-title gate).
//!
//! TS parity: `appState.toolPermissionContext` in `state/AppStateStore.ts`.
//! TS keeps the live permission-mode + plan-mode latches on a single
//! shared-mutable store; readers call `getAppState()` fresh and writers
//! use `setAppState(prev => ...)` to mutate. Rust mirrors this via
//! `Arc<RwLock<ToolAppState>>` on the engine + every tool context.
//!
//! All fields are plain value types so `Default` produces the initial
//! empty state; adding a field is a one-line edit here, not a string key
//! coordination across three crates.

use crate::PermissionMode;
use crate::PermissionRulesBySource;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Cross-turn shared state carried on `ToolUseContext.app_state`.
///
/// Grouped by lifecycle:
/// - **Live permission mode** (`permission_mode`, `pre_plan_mode`,
///   `stripped_dangerous_rules`) — source of truth for mode-dependent
///   decisions. TS parity: `appState.toolPermissionContext.{mode,
///   prePlanMode, strippedDangerousRules}`. Rebuilt into
///   `ToolUseContext.permission_context` on every batch boundary so
///   tools always see the latest value.
/// - Plan-mode latches (`has_exited_plan_mode`, `needs_plan_mode_exit_attachment`).
/// - Plan-mode reminder throttle (`plan_mode_attachment_count`,
///   `plan_mode_turns_since_last_attachment`).
/// - Permission-mode echo (`last_permission_mode`) for Reentry detection.
/// - Plan-mode entry timestamp (`plan_mode_entry_ms`) for verify-execution.
/// - Teammate approval handshake (`awaiting_plan_approval*`).
///
/// `PartialEq/Eq` is **not** derived: `PermissionRulesBySource` (used by
/// `stripped_dangerous_rules`) contains `PermissionRule` values which
/// aren't comparable. Tests compare fields individually.
#[derive(Debug, Clone, Default)]
pub struct ToolAppState {
    // ── Live permission-mode state (TS appState.toolPermissionContext) ──
    /// The current live permission mode. `None` means "not yet
    /// initialized from any source"; callers (engine `with_app_state`,
    /// tests) seed this to `Some(config.permission_mode)` at session
    /// bootstrap. After bootstrap, every write (EnterPlanMode exec,
    /// ExitPlanMode exec, Shift+Tab handler) stores `Some(X)` — the
    /// Option sentinel only distinguishes uninitialized state from a
    /// deliberately-Default setting. Readers:
    /// `unwrap_or(config.permission_mode)` or similar fallback.
    ///
    /// TS parity: `appState.toolPermissionContext.mode` — TS initializes
    /// it at store-create time, we match with explicit seeding.
    pub permission_mode: Option<PermissionMode>,

    /// Mode active before entering plan mode. Set by
    /// `EnterPlanModeTool::execute` when the engine transitions
    /// into Plan; consumed by `ExitPlanModeTool::execute` to restore
    /// the prior mode.
    ///
    /// TS parity: `appState.toolPermissionContext.prePlanMode`.
    pub pre_plan_mode: Option<PermissionMode>,

    /// Dangerous permission rules stashed when the classifier (Auto
    /// mode) is active. Set by `transition_context_with_auto` /
    /// `strip_dangerous_rules`, restored on auto-mode exit. Carried
    /// on shared state (not the per-batch ctx) so an Auto→Plan→Default
    /// transition can find the stash when Plan exits back to Default.
    ///
    /// TS parity: `appState.toolPermissionContext.strippedDangerousRules`.
    pub stripped_dangerous_rules: Option<PermissionRulesBySource>,

    // ── Plan-mode latches (one-shot signaling) ──
    /// Set by `ExitPlanModeTool` on success; read + cleared by the
    /// plan-mode reminder on the first following turn to emit the
    /// `Reentry` variant.
    pub has_exited_plan_mode: bool,

    /// One-shot: set by `ExitPlanModeTool` and by the reminder when it
    /// detects an unannounced mode transition. Cleared by the reminder
    /// after the exit-attachment is appended to history.
    pub needs_plan_mode_exit_attachment: bool,

    /// One-shot: set when leaving Auto mode (ExitPlanMode from a
    /// plan entered via Auto, or an unannounced Auto→non-Auto cycle
    /// detected by the reminder). Cleared by the reminder after the
    /// `## Exited Auto Mode` attachment is appended. TS parity:
    /// `needsAutoModeExitAttachment` in `bootstrap/state.ts`.
    pub needs_auto_mode_exit_attachment: bool,

    /// Total reminder attachments emitted this session. Drives the
    /// "every 5th attachment is Full" cadence.
    pub plan_mode_attachment_count: i64,

    /// Human turns elapsed since the last reminder attachment. Drives
    /// the 5-turn Sparse throttle. TS parity: counts only non-meta,
    /// non-tool-result user messages, matching
    /// `getPlanModeAttachmentTurnCount` in `utils/attachments.ts`. The
    /// `PlanModeReminder` only bumps this when it observes a NEW human
    /// turn UUID in history (see `last_human_turn_uuid_seen`), so
    /// multi-tool-round human turns count as one turn, not many.
    pub plan_mode_turns_since_last_attachment: i64,

    /// UUID of the most recent non-meta user message the
    /// `PlanModeReminder` has already accounted for in its turn
    /// throttle. On each `turn_start` the reminder scans `history` for
    /// the newest non-meta user UUID; if it differs from this value
    /// the turn counter bumps and this is updated. Prevents
    /// multi-tool-round human turns from being counted multiple times.
    /// Only meaningful when the engine is in plan mode; cleared
    /// opportunistically on exit/reset.
    pub last_human_turn_uuid_seen: Option<Uuid>,

    /// `PermissionMode` from the prior turn. Reminder uses this to
    /// detect Plan ↔ non-Plan transitions; the driver uses it after a
    /// teammate plan approval to restore the leader's override.
    pub last_permission_mode: Option<PermissionMode>,

    /// UNIX-ms timestamp written by `EnterPlanModeTool`. `ExitPlanModeTool`
    /// compares the plan file's mtime against this to gate the
    /// `verify_plan_execution` warning.
    pub plan_mode_entry_ms: Option<i64>,

    /// `true` while a leader is awaiting an approval reply from a teammate.
    /// Cleared by the reminder when the matching approval message arrives.
    pub awaiting_plan_approval: bool,

    /// Outstanding `plan_approval-<teammate>-<team>-<nonce>` correlation id
    /// for the current pending approval.
    pub awaiting_plan_approval_request_id: Option<String>,
}

// ────────────────────────────────────────────────────────────────
// Read-only handle + queued-patch types (tool-facing API surface)
// ────────────────────────────────────────────────────────────────
//
// `ToolUseContext.app_state` holds an `AppStateReadHandle` — a wrapper
// around `Arc<RwLock<ToolAppState>>` that exposes **only** `read()`.
// Tools thus cannot call `.write()` on app_state from inside
// `execute()`; the type system prevents it.
//
// Mutations flow through `ToolResult::app_state_patch`: a boxed
// `FnOnce(&mut ToolAppState)` that the executor applies post-execute
// (serial) or post-batch (concurrent) under a single write lock.
// This matches TS `orchestration.ts:queuedContextModifiers` exactly:
// tools return a `(ctx) => newCtx` modifier; the orchestrator queues
// them per tool_use_id and applies after the concurrent batch. No
// tool can observe another tool's mutation mid-batch, and no tool
// can directly mutate the shared store.

/// Read-only handle to the shared [`ToolAppState`]. Tools receive
/// this on [`crate::ToolUseContext::app_state`] and can query live
/// state via [`AppStateReadHandle::read`]. Mutations are **not**
/// exposed — tools return an [`AppStatePatch`] through
/// [`crate::ToolResult::app_state_patch`] instead.
///
/// TS parity: `appState.toolPermissionContext` is visible via
/// `context.getAppState()`, but writes go through
/// `context.setAppState(...)` which the orchestrator funnels into
/// `queuedContextModifiers` for post-batch apply. Rust's type
/// system enforces the same discipline: the handle has no write
/// surface at all, so a tool that tries to mutate simply won't
/// compile. Elegant over documented.
///
/// Non-tool callers (engine, reminder, TUI / SDK mode handlers)
/// that architecturally *are* authorized to mutate hold the
/// underlying `Arc<RwLock<ToolAppState>>` directly; they never
/// route through this handle.
#[derive(Debug, Clone)]
pub struct AppStateReadHandle {
    inner: Arc<RwLock<ToolAppState>>,
}

impl AppStateReadHandle {
    /// Wrap an existing shared state Arc.
    pub fn new(inner: Arc<RwLock<ToolAppState>>) -> Self {
        Self { inner }
    }

    /// Acquire a read lock. Tools use this to inspect live state
    /// (e.g. `ctx.app_state.as_ref()?.read().await.permission_mode`).
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, ToolAppState> {
        self.inner.read().await
    }
}

impl From<Arc<RwLock<ToolAppState>>> for AppStateReadHandle {
    fn from(arc: Arc<RwLock<ToolAppState>>) -> Self {
        Self::new(arc)
    }
}

/// A mutation of the shared [`ToolAppState`], queued by a tool via
/// [`crate::ToolResult::app_state_patch`] and applied by the
/// executor after `execute` returns.
///
/// TS parity: `update.newContext: (ctx) => ctx` in
/// `orchestration.ts`. Per-tool, ordered by submission (= TS
/// `Object.entries(queuedContextModifiers)` iteration order), applied
/// under a single write lock so intermediate states are never
/// observable.
pub type AppStatePatch = Box<dyn FnOnce(&mut ToolAppState) + Send + Sync + 'static>;

#[cfg(test)]
#[path = "app_state.test.rs"]
mod tests;
