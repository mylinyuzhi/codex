//! Per-turn plan-mode system-reminder injection.
//!
//! TS: `getPlanModeAttachments` in `src/utils/attachments.ts:1186` +
//! `normalizeAttachmentForAPI()` cases in `src/utils/messages.ts`.
//! Driven by the state flags `hasExitedPlanModeInSession()` /
//! `needsPlanModeExitAttachment()`.
//!
//! Cadence matches TS `PLAN_MODE_ATTACHMENT_CONFIG`:
//! - First Plan turn always attaches.
//! - After that, attach only every `TURNS_BETWEEN_ATTACHMENTS` **human
//!   turns** (5). The tool-execution loop calls `turn_start` once per
//!   LLM iteration (each tool-result round is a separate iteration), so
//!   the reminder tracks the last-seen human-turn UUID in app_state and
//!   only bumps the throttle on a NEW human turn — tool rounds within a
//!   single human turn count as one, matching TS
//!   `getPlanModeAttachmentTurnCount` (counts non-meta non-tool-result
//!   user messages).
//! - Among attached turns, the `FULL_REMINDER_EVERY_N_ATTACHMENTS`th (5)
//!   uses the `Full` variant; the rest use `Sparse`. So the pattern is
//!   attachments #1, #6, #11, … are Full.
//! - `Reentry` wins over `Full` on the very first Plan turn after a
//!   previous exit; it is a one-shot that clears `has_exited_plan_mode`.
//!
//! Callers:
//! - [`PlanModeReminder::turn_start`] — call once per outer turn iteration
//!   (before building the LLM prompt). It mutates history and updates
//!   throttle counters on app_state so cadence survives across runs.

use coco_context::Phase4Variant;
use coco_context::PlanModeAttachment;
use coco_context::PlanModeExitAttachment;
use coco_context::PlanWorkflow;
use coco_context::ReminderType;
use coco_messages::MessageHistory;
use coco_messages::wrapping::wrap_in_system_reminder;
use coco_types::AttachmentMessage;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Clamped agent count (1..=10) referenced in the 5-phase plan-mode
/// prompt. The clamp invariant lives in the type so downstream
/// rendering code can trust the value without re-validating.
///
/// TS parity: `getPlanModeV2ExploreAgentCount` /
/// `getPlanModeV2AgentCount` both clamp to 1..=10 at read time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentCount(i32);

impl AgentCount {
    /// Construct from a raw `i32`, clamping to `[MIN_AGENTS, MAX_AGENTS]`.
    const fn new(raw: i32) -> Self {
        let clamped = if raw < MIN_AGENTS {
            MIN_AGENTS
        } else if raw > MAX_AGENTS {
            MAX_AGENTS
        } else {
            raw
        };
        Self(clamped)
    }

    const fn get(self) -> i32 {
        self.0
    }
}

/// Minimum + maximum agent count the prompt template allows.
const MIN_AGENTS: i32 = 1;
const MAX_AGENTS: i32 = 10;

/// TS: `PLAN_MODE_ATTACHMENT_CONFIG.TURNS_BETWEEN_ATTACHMENTS` = 5.
///
/// First Plan turn always attaches. Beyond that, a reminder is emitted
/// only every N turns so we don't burn tokens reminding the model on
/// every tick.
const TURNS_BETWEEN_ATTACHMENTS: i64 = 5;

/// TS: `PLAN_MODE_ATTACHMENT_CONFIG.FULL_REMINDER_EVERY_N_ATTACHMENTS` = 5.
///
/// Among emitted attachments, every Nth (1st, 6th, 11th, …) uses the
/// Full variant; the rest use Sparse.
const FULL_REMINDER_EVERY_N_ATTACHMENTS: i64 = 5;

/// Stateful tracker + injector for the plan-mode reminder stream.
///
/// Keyed to a single `run_session_loop` invocation, but throttle state
/// is persisted on app_state so cadence survives across engine runs
/// (each user turn spawns a new run).
pub struct PlanModeReminder {
    /// Fallback permission mode used only when `app_state` is `None`.
    /// In production the engine always attaches an `app_state` and
    /// the reminder reads `app_state.permission_mode` live each turn
    /// (TS parity: `appState.toolPermissionContext.mode`). This
    /// fallback exists so reminder unit tests that don't construct an
    /// `app_state` can still drive the reminder's dispatch branches.
    fallback_permission_mode: PermissionMode,
    /// Whether the first Plan reminder has already been injected this
    /// run. Within a single run we still emit a fresh Full on re-entry
    /// post-compaction even if app_state says the counter is higher.
    full_sent: bool,
    /// Session identifier for resolving the per-session plan file path.
    session_id: Option<String>,
    /// Optional active agent ID (subagents get a separate plan file).
    agent_id: Option<String>,
    /// Plans directory (resolved from config_home + project settings).
    plans_dir: Option<PathBuf>,
    /// Shared typed app_state — read/cleared for the
    /// `needs_plan_mode_exit_attachment` flag set by
    /// `ExitPlanModeTool::execute`, plus the cross-run throttle
    /// counters (`plan_mode_attachment_count`,
    /// `plan_mode_turns_since_last_attachment`).
    app_state: Option<Arc<RwLock<ToolAppState>>>,
    /// Mailbox handle — enables teammate approval-response polling and
    /// leader pending-approvals attachment. `None` in non-swarm sessions
    /// or when the handle isn't installed; both features become no-ops.
    mailbox: Option<coco_tool::MailboxHandleRef>,
    /// Agent identity for mailbox scoping. Required for polling; if
    /// `None`, approval poll is skipped.
    agent_name: Option<String>,
    /// Team name for mailbox scoping. Required for polling.
    team_name: Option<String>,
    /// Set when this engine runs AS a teammate whose role requires
    /// leader approval. Enables approval-response polling.
    is_teammate_awaiting: bool,
    /// Optional protocol-event sink for surfacing plan-approval requests
    /// to the leader's TUI as `ServerNotification::PlanApprovalRequested`.
    /// `None` in SDK-only sessions or tests; the LLM-prompt attachment
    /// path continues regardless.
    event_tx: Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,
    /// Which Full-variant workflow to render. Configured via
    /// `settings.plan_mode.workflow`. Defaults to `FivePhase`.
    workflow: PlanWorkflow,
    /// Phase-4 "Final Plan" strictness. Only affects 5-phase Full.
    phase4_variant: Phase4Variant,
    /// Explore agent count referenced in 5-phase Full. Default 3.
    explore_agent_count: AgentCount,
    /// Plan agent count referenced in 5-phase Full. Default 1.
    plan_agent_count: AgentCount,
}

impl PlanModeReminder {
    pub fn new(
        permission_mode: PermissionMode,
        session_id: Option<String>,
        agent_id: Option<String>,
        plans_dir: Option<PathBuf>,
        app_state: Option<Arc<RwLock<ToolAppState>>>,
    ) -> Self {
        Self {
            fallback_permission_mode: permission_mode,
            full_sent: false,
            session_id,
            agent_id,
            plans_dir,
            app_state,
            mailbox: None,
            agent_name: None,
            team_name: None,
            is_teammate_awaiting: false,
            event_tx: None,
            workflow: PlanWorkflow::default(),
            phase4_variant: Phase4Variant::default(),
            explore_agent_count: AgentCount::new(3),
            plan_agent_count: AgentCount::new(1),
        }
    }

    /// Resolve the current live permission mode. Reads from
    /// `app_state.permission_mode` — TS parity:
    /// `appState.toolPermissionContext.mode`. Falls back to the
    /// constructor-time value when `app_state` is `None` (unit tests
    /// without a shared state) or when `app_state.permission_mode` is
    /// `None` (app_state hasn't been seeded — the caller skipped
    /// `with_app_state`'s bootstrap write, typically an isolated
    /// reminder unit test).
    async fn current_permission_mode(&self) -> PermissionMode {
        match self.app_state.as_ref() {
            Some(state) => state
                .read()
                .await
                .permission_mode
                .unwrap_or(self.fallback_permission_mode),
            None => self.fallback_permission_mode,
        }
    }

    /// Builder: install mailbox handle + identity for teammate
    /// approval-response polling + leader pending-approvals attachment.
    pub fn with_mailbox(
        mut self,
        mailbox: coco_tool::MailboxHandleRef,
        agent_name: String,
        team_name: String,
        is_teammate_awaiting: bool,
    ) -> Self {
        self.mailbox = Some(mailbox);
        self.agent_name = Some(agent_name);
        self.team_name = Some(team_name);
        self.is_teammate_awaiting = is_teammate_awaiting;
        self
    }

    /// Builder: install a protocol-event sink so leader-pending-approval
    /// polling can surface each request to the TUI as a
    /// `ServerNotification::PlanApprovalRequested`. Leaves the
    /// LLM-prompt attachment path unchanged.
    pub fn with_event_sink(
        mut self,
        event_tx: tokio::sync::mpsc::Sender<coco_types::CoreEvent>,
    ) -> Self {
        self.event_tx = Some(event_tx);
        self
    }

    /// Builder: configure the Full-variant workflow (5-phase vs interview).
    pub fn with_workflow(mut self, workflow: PlanWorkflow) -> Self {
        self.workflow = workflow;
        self
    }

    /// Builder: configure the 5-phase Phase-4 prompt variant.
    pub fn with_phase4_variant(mut self, variant: Phase4Variant) -> Self {
        self.phase4_variant = variant;
        self
    }

    /// Builder: configure the explore + plan agent counts referenced in
    /// the 5-phase Full prompt. Raw `i32`s are clamped into the
    /// `AgentCount` newtype, which carries the `[1, 10]` invariant
    /// through to the renderer without requiring a second clamp at
    /// the call site.
    pub fn with_agent_counts(mut self, explore: i32, plan: i32) -> Self {
        self.explore_agent_count = AgentCount::new(explore);
        self.plan_agent_count = AgentCount::new(plan);
        self
    }

    /// Inject any turn-start reminders into `history`.
    ///
    /// Order:
    /// 1. Detect an "unannounced" Plan-mode transition — if app_state's
    ///    `last_permission_mode` differs from the current one, apply the
    ///    same side-effects that `ExitPlanModeTool` would set (exit
    ///    attachment + has-exited flag). TS parity:
    ///    `transitionPermissionMode` in `permissionSetup.ts:597-646`.
    /// 2. Plan-mode-exit reminder (one-shot, clears flag) — always first
    ///    in the output ordering so the model reads the exit banner
    ///    before any steady-state reminder.
    /// 3. Plan-mode reminder (only if currently in Plan). Variant is:
    ///    - `Reentry` on the first Plan turn after a previous exit
    ///      (`has_exited_plan_mode` flag set). The flag is CLEARED after
    ///      emitting Reentry — TS parity: `setHasExitedPlanMode(false)`
    ///      in `attachments.ts:1218`.
    ///    - `Full` on the very first Plan turn of this run;
    ///    - `Sparse` on every subsequent Plan turn.
    pub async fn turn_start(&mut self, history: &mut MessageHistory) {
        // Resolve the live mode ONCE at the top of the turn so every
        // downstream decision (reconcile, exit-banner suppression,
        // plan-reminder gate) agrees on the same snapshot. Reading
        // from `app_state` per call matches TS `getAppState()` fresh
        // semantics in attachments.ts.
        let current_mode = self.current_permission_mode().await;

        // Each emit step is an independent one-shot — ordered so the
        // reader sees narrower-scope events first (plan mode, plan
        // file) and broader ones after (auto mode). Swarm polling
        // runs before banners so any consumed approval response
        // reconciles the mode before the exit-banner check.
        self.reconcile_mode_transition(current_mode).await;
        self.poll_teammate_approval(history).await;
        self.inject_leader_pending_approvals(history).await;
        self.emit_exit_banner_if_flagged(history).await;
        self.emit_auto_mode_exit_banner_if_flagged(history, current_mode)
            .await;
        if current_mode == PermissionMode::Plan {
            self.emit_plan_reminder_if_due(history).await;
        }
    }

    /// One-shot exit banner (`## Exited Plan Mode`). Set by
    /// `ExitPlanModeTool` and cleared here after emission. Resets the
    /// attachment-cadence counters so the next Plan entry starts
    /// fresh at Full — TS `countPlanModeAttachmentsSinceLastExit`
    /// stops counting at exits.
    async fn emit_exit_banner_if_flagged(&self, history: &mut MessageHistory) {
        if !self.take_exit_flag().await {
            return;
        }
        let attachment = self.build_exit_attachment();
        history.push(Self::exit_message(&attachment));
        self.reset_throttle_counters().await;
    }

    /// One-shot auto-mode-exit banner (`## Exited Auto Mode`).
    /// Appended AFTER the plan-mode-exit banner so the model reads
    /// narrower scope first. TS parity: `getAutoModeExitAttachment`
    /// in `attachments.ts:1380`.
    async fn emit_auto_mode_exit_banner_if_flagged(
        &self,
        history: &mut MessageHistory,
        current_mode: PermissionMode,
    ) {
        if !self.take_auto_mode_exit_flag(current_mode).await {
            return;
        }
        history.push(Self::raw_reminder_message(
            &coco_context::render_auto_mode_exit_reminder(),
        ));
    }

    /// Steady-state plan-mode reminder: Full/Sparse cadence plus the
    /// co-emitted Reentry on the first Plan turn after a prior exit.
    ///
    /// Throttle: bump turns-since-last counter only on a NEW human
    /// turn (matching TS `getPlanModeAttachmentTurnCount`), then fire
    /// if enough human turns have passed. Tool-result rounds within
    /// the same human turn share a user-message UUID, so they don't
    /// re-bump the counter.
    ///
    /// The latest human-turn UUID is resolved up-front so both the
    /// first-fire path (counter == 0) and the throttled path can
    /// stamp it onto `app_state` after emitting. Without the up-front
    /// read, the next tool-round `turn_start` would see
    /// `last_human_turn_uuid_seen == None` and mistake it for a
    /// fresh human turn — bumping the counter spuriously. Fix B
    /// regression guard: `tool_rounds_do_not_advance_cadence_…`.
    async fn emit_plan_reminder_if_due(&mut self, history: &mut MessageHistory) {
        let latest_human_uuid = Self::latest_non_meta_user_uuid(history);
        let total_attachments = self.attachment_count().await;
        let should_fire = if total_attachments == 0 {
            // Very first Plan-mode attachment in this (possibly
            // resumed) session — always emit.
            true
        } else {
            let turns_since = self.observe_turn_and_count(latest_human_uuid).await;
            turns_since >= TURNS_BETWEEN_ATTACHMENTS
        };
        if !should_fire {
            return;
        }

        let next_count = total_attachments + 1;
        let (path, exists) = self.resolve_plan_file();

        // TS (attachments.ts:1213-1239) emits Reentry as an **additional**
        // attachment alongside the normal Full/Sparse — not a replacement.
        // Gate on `plan_exists` to match the TS `existingPlan !== null`
        // check at attachments.ts:1216. Sub-agents never hit Reentry
        // (they run in fresh sub-sessions without the exit flag set).
        let is_reentry =
            self.agent_id.is_none() && exists && self.session_has_exited_plan_mode().await;
        if is_reentry {
            let reentry = self.build_attachment_at(ReminderType::Reentry, &path, exists);
            history.push(Self::reminder_message(&reentry));
            self.clear_has_exited_plan_mode().await;
        }

        // Normal cadence: Full every Nth attachment, Sparse otherwise.
        let normal_type = if next_count % FULL_REMINDER_EVERY_N_ATTACHMENTS == 1 {
            ReminderType::Full
        } else {
            ReminderType::Sparse
        };
        let normal = self.build_attachment_at(normal_type, &path, exists);
        history.push(Self::reminder_message(&normal));

        self.full_sent = true;
        self.set_attachment_count(next_count).await;
        self.reset_turns_since_last_attachment_and_stamp(latest_human_uuid)
            .await;
    }

    /// Scan `history` backwards for the most recent non-meta user
    /// message UUID. This is the "HUMAN turn" marker in TS parlance
    /// (`type === 'user' && !isMeta && !hasToolResultContent`); in
    /// Rust `Message::User` with `is_meta == false` fills the same
    /// role (tool results are their own `Message::ToolResult` variant).
    fn latest_non_meta_user_uuid(history: &MessageHistory) -> Option<uuid::Uuid> {
        history.messages.iter().rev().find_map(|m| match m {
            Message::User(u) if !u.is_meta => Some(u.uuid),
            _ => None,
        })
    }

    // ── Throttle counters on app_state ──
    //
    // These use blocking `.read().await` / `.write().await` because the
    // counters are load-bearing for cadence correctness: silently dropping
    // a write under contention would let `attachment_count` read 0 and
    // mis-classify the next turn as Full. Contention is rare in practice
    // (turn-boundary code) and the lock is held for only a scalar assign,
    // so waiting is the right tradeoff.

    async fn attachment_count(&self) -> i64 {
        let Some(state) = self.app_state.as_ref() else {
            return if self.full_sent { 1 } else { 0 };
        };
        state.read().await.plan_mode_attachment_count
    }

    async fn set_attachment_count(&self, n: i64) {
        let Some(state) = self.app_state.as_ref() else {
            return;
        };
        state.write().await.plan_mode_attachment_count = n;
    }

    /// Reset the turns-since-last-attachment counter AND stamp the
    /// human-turn UUID we just attached against. Called immediately
    /// after emitting a reminder: the counter resets to 0 and the
    /// stash is updated so the next `observe_turn_and_count` call
    /// correctly identifies subsequent human turns as "new" vs.
    /// "continuing tool rounds of the same turn".
    async fn reset_turns_since_last_attachment_and_stamp(&self, latest: Option<uuid::Uuid>) {
        let Some(state) = self.app_state.as_ref() else {
            return;
        };
        let mut guard = state.write().await;
        guard.plan_mode_turns_since_last_attachment = 0;
        // Only update the stash if we actually found a user message —
        // otherwise leave whatever the previous run stored. An empty
        // history is either a synthetic test (`None` is fine) or a
        // truncated/compacted history (prior stash still meaningful).
        if latest.is_some() {
            guard.last_human_turn_uuid_seen = latest;
        }
    }

    /// Diff `latest_uuid` (precomputed by the caller) against the
    /// stashed `last_human_turn_uuid_seen`. If it's a new human turn,
    /// bump `plan_mode_turns_since_last_attachment` and stash the new
    /// UUID. Otherwise the counter stays put. Returns the counter
    /// value after the (possibly skipped) bump — the caller compares
    /// it against `TURNS_BETWEEN_ATTACHMENTS`.
    ///
    /// Matches TS `getPlanModeAttachmentTurnCount` semantics: count
    /// only non-meta, non-tool-result user messages. Tool-result
    /// rounds are a separate `Message::ToolResult` variant in Rust,
    /// so we only need to skip `is_meta` user messages.
    async fn observe_turn_and_count(&self, latest_uuid: Option<uuid::Uuid>) -> i64 {
        let Some(state) = self.app_state.as_ref() else {
            // No app_state: fall back to scalar bump so tests without a
            // real state still observe cadence.
            return if self.full_sent { 1 } else { 0 };
        };
        let mut guard = state.write().await;
        let is_new_human_turn = match (latest_uuid, guard.last_human_turn_uuid_seen) {
            (Some(new), Some(old)) => new != old,
            (Some(_), None) => true,
            _ => false,
        };
        if is_new_human_turn {
            guard.plan_mode_turns_since_last_attachment += 1;
            guard.last_human_turn_uuid_seen = latest_uuid;
        }
        guard.plan_mode_turns_since_last_attachment
    }

    /// Reset cadence counters after an exit banner fires. Matches TS
    /// `countPlanModeAttachmentsSinceLastExit`: exit resets the cycle
    /// so the next plan entry starts from Full again. Also clears
    /// `last_human_turn_uuid_seen` so the very first turn post-exit
    /// counts as the "new human turn" baseline.
    async fn reset_throttle_counters(&self) {
        let Some(state) = self.app_state.as_ref() else {
            return;
        };
        let mut guard = state.write().await;
        guard.plan_mode_attachment_count = 0;
        guard.plan_mode_turns_since_last_attachment = 0;
        guard.last_human_turn_uuid_seen = None;
    }

    /// Detect and record cross-run permission-mode transitions.
    ///
    /// The engine config is immutable within a run, but the user can
    /// flip modes between runs (Shift+Tab, SDK `setPermissionMode`, etc).
    /// If the last-seen mode on app_state differs from the current one,
    /// fire the same "just exited plan" side-effects the tool would set:
    /// `has_exited_plan_mode=true` and `needs_plan_mode_exit_attachment=true`.
    /// Entry into plan mode clears the exit-attachment pending flag so a
    /// rapid out-and-back toggle doesn't emit a stale exit banner.
    async fn reconcile_mode_transition(&self, current_mode: PermissionMode) {
        let Some(app_state) = self.app_state.as_ref() else {
            return;
        };
        let mut guard = app_state.write().await;
        let last_mode = guard.last_permission_mode;
        let current = current_mode;

        if let Some(prev) = last_mode {
            if prev == PermissionMode::Plan && current != PermissionMode::Plan {
                // Cycled OUT of Plan without going through the tool.
                guard.has_exited_plan_mode = true;
                guard.needs_plan_mode_exit_attachment = true;
            } else if current == PermissionMode::Plan && prev != PermissionMode::Plan {
                // Cycled INTO Plan — avoid double-attaching a stale
                // exit banner on a rapid Plan→Default→Plan toggle.
                guard.needs_plan_mode_exit_attachment = false;
            }
            // Auto→non-Auto: the user (or the Plan exit path) is
            // leaving the classifier. TS parity: `setNeedsAutoModeExit
            // Attachment(true)` at `permissionSetup.ts:635, 1530`. We
            // observe it here rather than from the key handler so SDK
            // + bridge callers get the banner too.
            if prev == PermissionMode::Auto && current != PermissionMode::Auto {
                guard.needs_auto_mode_exit_attachment = true;
            } else if current == PermissionMode::Auto {
                // Re-entered Auto before the banner fired — clear the
                // stale pending flag (TS parity: `permissionSetup.ts:1526`).
                guard.needs_auto_mode_exit_attachment = false;
            }
        }
        guard.last_permission_mode = Some(current);
    }

    async fn session_has_exited_plan_mode(&self) -> bool {
        let Some(app_state) = self.app_state.as_ref() else {
            return false;
        };
        app_state.read().await.has_exited_plan_mode
    }

    /// Clear the `has_exited_plan_mode` app_state flag after emitting a
    /// Reentry reminder. Mirrors TS `setHasExitedPlanMode(false)` at
    /// `attachments.ts:1218` — the flag is one-shot guidance.
    async fn clear_has_exited_plan_mode(&self) {
        let Some(app_state) = self.app_state.as_ref() else {
            return;
        };
        app_state.write().await.has_exited_plan_mode = false;
    }

    /// Assemble a `PlanModeAttachment` with the given reminder type and
    /// pre-resolved plan-file path + existence. Reused for the Reentry
    /// banner + the normal Full/Sparse reminder emitted on the same turn.
    fn build_attachment_at(
        &self,
        reminder_type: ReminderType,
        plan_file_path: &str,
        plan_exists: bool,
    ) -> PlanModeAttachment {
        PlanModeAttachment {
            reminder_type,
            workflow: self.workflow,
            phase4_variant: self.phase4_variant,
            explore_agent_count: self.explore_agent_count.get(),
            plan_agent_count: self.plan_agent_count.get(),
            is_sub_agent: self.agent_id.is_some(),
            plan_file_path: plan_file_path.to_string(),
            plan_exists,
        }
    }

    fn build_exit_attachment(&self) -> PlanModeExitAttachment {
        let (path, exists) = self.resolve_plan_file();
        PlanModeExitAttachment {
            plan_file_path: path,
            plan_exists: exists,
        }
    }

    fn resolve_plan_file(&self) -> (String, bool) {
        let Some(sid) = self.session_id.as_deref() else {
            return (String::new(), false);
        };
        let Some(plans_dir) = self.plans_dir.as_deref() else {
            return (String::new(), false);
        };
        let path = coco_context::get_plan_file_path(sid, plans_dir, self.agent_id.as_deref());
        let exists = path.exists();
        (path.to_string_lossy().into_owned(), exists)
    }

    /// Read and clear the `needs_plan_mode_exit_attachment` app_state flag.
    ///
    /// Returns true if the exit reminder should be emitted this turn.
    async fn take_exit_flag(&self) -> bool {
        let Some(app_state) = self.app_state.as_ref() else {
            return false;
        };
        let mut guard = app_state.write().await;
        let flag = guard.needs_plan_mode_exit_attachment;
        if flag {
            guard.needs_plan_mode_exit_attachment = false;
        }
        flag
    }

    /// Read and clear the `needs_auto_mode_exit_attachment` flag.
    /// Returns true if the `## Exited Auto Mode` one-shot should fire.
    ///
    /// Suppressed (and the flag cleared) when the engine is currently
    /// in Auto — the classifier is still driving, so the reminder
    /// would be a lie. TS parity: `getAutoModeExitAttachment`
    /// (`attachments.ts:1388-1396`).
    async fn take_auto_mode_exit_flag(&self, current_mode: PermissionMode) -> bool {
        let Some(app_state) = self.app_state.as_ref() else {
            return false;
        };
        let mut guard = app_state.write().await;
        if !guard.needs_auto_mode_exit_attachment {
            return false;
        }
        guard.needs_auto_mode_exit_attachment = false;
        // Suppress when auto is still the active mode (flag stale).
        current_mode != PermissionMode::Auto
    }

    fn reminder_message(attachment: &PlanModeAttachment) -> Message {
        let text = coco_context::render_plan_mode_reminder(attachment);
        Message::Attachment(AttachmentMessage {
            uuid: uuid::Uuid::new_v4(),
            message: LlmMessage::user_text(wrap_in_system_reminder(&text)),
            is_meta: true,
        })
    }

    fn exit_message(attachment: &PlanModeExitAttachment) -> Message {
        let text = coco_context::render_plan_mode_exit_reminder(attachment);
        Message::Attachment(AttachmentMessage {
            uuid: uuid::Uuid::new_v4(),
            message: LlmMessage::user_text(wrap_in_system_reminder(&text)),
            is_meta: true,
        })
    }

    /// Resolve the plans directory from a config_home path and optional
    /// project override. Helper so the engine can call this once when
    /// constructing the tracker.
    pub fn resolve_plans_dir(
        config_home: Option<&Path>,
        project_dir: Option<&Path>,
        plans_directory_setting: Option<&str>,
    ) -> Option<PathBuf> {
        config_home.map(|ch| {
            coco_context::resolve_plans_directory(ch, project_dir, plans_directory_setting)
        })
    }

    /// Scan the teammate's own inbox for a `plan_approval_response`
    /// matching the request this teammate sent. If found:
    ///   - Inject an approval or rejection reminder.
    ///   - Clear `awaiting_plan_approval` flags on app_state.
    ///   - If approved and response carries `permissionMode`, write it
    ///     to `app_state.last_permission_mode` so the engine picks up
    ///     the switch on the next mode-transition reconcile.
    ///   - Mark the message as read so we don't re-consume it.
    ///
    /// No-op when this engine isn't an awaiting teammate.
    async fn poll_teammate_approval(&self, history: &mut MessageHistory) {
        if !self.is_teammate_awaiting {
            return;
        }
        let (Some(mailbox), Some(agent), Some(team)) =
            (&self.mailbox, &self.agent_name, &self.team_name)
        else {
            return;
        };
        let Some(app_state) = &self.app_state else {
            return;
        };
        // Retrieve the request_id we're waiting for.
        let expected_id = app_state
            .read()
            .await
            .awaiting_plan_approval_request_id
            .clone();
        let Some(expected_id) = expected_id else {
            return;
        };

        let Ok(unread) = mailbox.read_unread(agent, team).await else {
            return;
        };
        for msg in &unread {
            // Deserialize into the typed protocol. Non-approval messages
            // round-trip through serde's untagged error and we skip.
            let Ok(coco_tool::PlanApprovalMessage::PlanApprovalResponse(resp)) =
                serde_json::from_str::<coco_tool::PlanApprovalMessage>(&msg.text)
            else {
                continue;
            };
            if resp.request_id != expected_id {
                continue;
            }

            // Build + inject the approval/rejection reminder.
            let text = if resp.approved {
                let tail = match resp.permission_mode {
                    Some(m) => {
                        let serialized = serde_json::to_string(&m).unwrap_or_default();
                        // Strip the outer quotes from the JSON-encoded
                        // enum variant so we surface `accept_edits`
                        // rather than `"accept_edits"` in the reminder.
                        let label = serialized.trim_matches('"');
                        format!(
                            " The team lead set your mode to `{label}`; proceed with implementation."
                        )
                    }
                    None => " Proceed with implementation.".to_string(),
                };
                format!("## Plan Approved\n\nThe team lead approved your plan.{tail}")
            } else {
                let feedback_line = resp
                    .feedback
                    .as_deref()
                    .map(|f| format!("\n\n**Feedback:** {f}"))
                    .unwrap_or_default();
                format!(
                    "## Plan Rejected\n\nThe team lead rejected your plan. Stay in plan \
                     mode and refine based on the feedback.{feedback_line}"
                )
            };
            history.push(Self::raw_reminder_message(&text));

            // Clear awaiting flags + record target mode.
            let mut guard = app_state.write().await;
            guard.awaiting_plan_approval = false;
            guard.awaiting_plan_approval_request_id = None;
            if let Some(mode) = resp.permission_mode {
                guard.last_permission_mode = Some(mode);
            }
            drop(guard);

            // Mark read + stop — one response per poll.
            let _ = mailbox.mark_read(agent, team, msg.index).await;
            return;
        }
    }

    /// Scan the leader's own inbox for unread `plan_approval_request`
    /// messages and inject an attachment summarizing what's pending.
    /// The leader model can then call `SendMessage` to respond.
    ///
    /// No-op when the reminder isn't configured with a mailbox handle
    /// or when the agent name doesn't resolve to the leader role (we
    /// use `TEAM_LEAD_NAME` convention = "team-lead").
    async fn inject_leader_pending_approvals(&self, history: &mut MessageHistory) {
        // Only fire if we're the team-lead identity. Teammates skip.
        let Some(mailbox) = &self.mailbox else {
            return;
        };
        let Some(agent) = &self.agent_name else {
            return;
        };
        let Some(team) = &self.team_name else {
            return;
        };
        // Canonical leader name. TS: `TEAM_LEAD_NAME = 'team-lead'`.
        if agent != "team-lead" {
            return;
        }

        let Ok(unread) = mailbox.read_unread(agent, team).await else {
            return;
        };
        let pending: Vec<(usize, coco_tool::PlanApprovalRequest)> = unread
            .iter()
            .filter_map(|m| {
                match serde_json::from_str::<coco_tool::PlanApprovalMessage>(&m.text).ok()? {
                    coco_tool::PlanApprovalMessage::PlanApprovalRequest(req) => {
                        Some((m.index, req))
                    }
                    _ => None,
                }
            })
            .collect();

        if pending.is_empty() {
            return;
        }

        let mut body = String::from(
            "## Pending Plan Approvals\n\n\
             One or more teammates have submitted plans and are waiting for your \
             review. Use the `SendMessage` tool to respond with a structured \
             `plan_approval_response` message.\n",
        );
        for (_idx, req) in &pending {
            body.push_str(&format!(
                "\n---\n**From:** `{from}`  **Request ID:** `{request_id}`  \
                 **Plan file:** `{plan_file}`\n\n{plan}\n",
                from = req.from,
                request_id = req.request_id,
                plan_file = req.plan_file_path,
                plan = req.plan_content,
            ));
        }
        body.push_str(
            "\n---\nTo approve: `SendMessage(to: \"<teammate>\", message: {{\
             type: \"plan_approval_response\", request_id: \"<id>\", approve: true}})`.\n\
             To reject with feedback: `SendMessage(to: \"<teammate>\", message: {{\
             type: \"plan_approval_response\", request_id: \"<id>\", approve: false, \
             feedback: \"<why>\"}})`.",
        );
        history.push(Self::raw_reminder_message(&body));

        // Surface each pending request to the TUI as a modal approval
        // overlay. The event sink is optional: SDK-only sessions skip
        // this while still receiving the LLM-prompt attachment above.
        // One notification per request — the overlay priority-queues
        // multiple arrivals.
        if let Some(tx) = self.event_tx.as_ref() {
            for (_idx, req) in &pending {
                let params = coco_types::PlanApprovalRequestedParams {
                    request_id: req.request_id.clone(),
                    from: req.from.clone(),
                    plan_file_path: Some(req.plan_file_path.clone()),
                    plan_content: req.plan_content.clone(),
                };
                let _ = tx
                    .send(coco_types::CoreEvent::Protocol(
                        coco_types::ServerNotification::PlanApprovalRequested(params),
                    ))
                    .await;
            }
        }

        // Mark all seen requests as read so we don't re-inject next turn.
        // TS keeps them unread until the leader responds, but we dedup
        // via attachment-already-injected semantics — reintroducing
        // every turn would be wasteful. If the leader ignores them, the
        // teammate's tool_result still instructs them to wait.
        for (idx, _) in &pending {
            let _ = mailbox.mark_read(agent, team, *idx).await;
        }
    }

    /// Build a bare system-reminder message from raw text. Used by the
    /// swarm pollers; the plan-mode-specific attachments use
    /// [`Self::reminder_message`] instead.
    fn raw_reminder_message(text: &str) -> Message {
        use coco_messages::wrapping::wrap_in_system_reminder;
        Message::Attachment(AttachmentMessage {
            uuid: uuid::Uuid::new_v4(),
            message: LlmMessage::user_text(wrap_in_system_reminder(text)),
            is_meta: true,
        })
    }
}

#[cfg(test)]
#[path = "plan_mode_reminder.test.rs"]
mod tests;
