//! Session-loop state, grouped by lifecycle.
//!
//! Mirrors TS `query.ts:204-217` `State` type, adapted to Rust:
//! TS uses a single mutable `state` struct rewritten at every `continue`
//! site. coco-rs splits it into four groups so helpers can take just the
//! slice they need without grabbing the whole state by `&mut`:
//!
//! - [`LoopAccumulator`] — cross-turn accumulators (usage / cost /
//!   permission denials / artifacts). Read at terminal sites to build
//!   [`crate::QueryResult`].
//! - [`LoopTurnState`] — iteration state machine. Reset (or partially
//!   reset) at `continue` sites; carries the `transition: Option<TurnTransition>`
//!   that mirrors TS `state.transition`.
//! - [`LoopServices`] — long-lived owned objects (model runtime, progress
//!   forwarder tx, plan-mode side-effect driver, system-reminder
//!   orchestrator). Constructed once at session entry; not rebuilt at
//!   `continue` sites.
//! - [`LoopConstants`] — entry-derived immutable values (start instant,
//!   user-message uuid, plans directory, reminder window sizing).
//!
//! [`QueryEngine::init_loop_state`] is the single bundled entry that
//! builds all four substructs, spawns the progress-drain task,
//! populates `MessageHistory` with the initial `turn_messages`, and
//! takes the per-prompt file-history snapshot. Callers destructure
//! the returned tuple — see the call site in
//! [`crate::engine::QueryEngine::run_session_loop`].

use std::path::PathBuf;
use std::sync::Arc;

use coco_config::EnvKey;
use coco_config::env;
use coco_messages::CostTracker;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_system_reminder::SystemReminderOrchestrator;
use coco_types::TokenUsage;
use tracing::warn;

use crate::budget::BudgetTracker;
use crate::config::ContinueReason;
use crate::engine::QueryEngine;
use crate::engine::RunArtifacts;
use crate::engine_helpers::ProgressThrottle;
use crate::engine_helpers::drain_one_progress;
use crate::model_runtime::ModelRuntime;
use crate::plan_mode_reminder::PlanModeReminder;

/// Alias mirroring TS `Continue` type-union. The Rust enum carrying these
/// variants is [`crate::config::ContinueReason`] — defined there so the
/// public [`crate::QueryResult::last_continue_reason`] field type stays
/// stable. New code referencing the iteration-transition signal should
/// use this name to make the parity with TS `state.transition` explicit.
pub(crate) type TurnTransition = ContinueReason;

/// Cross-turn accumulators. Read once at every terminal site (clean
/// completion, cancellation, budget exhaustion, error bail) to build
/// the final [`crate::QueryResult`]. Never reset mid-session.
#[derive(Default)]
pub(crate) struct LoopAccumulator {
    /// Cumulative LLM API wall-clock across all turns of this session.
    pub(crate) api_time_ms: i64,
    /// Cumulative token usage (input/output/cache) across all turns.
    pub(crate) total_usage: TokenUsage,
    /// Per-model cost tracking; sums across turns and across fallback
    /// switches (each switch records under its own provider/model id).
    pub(crate) cost_tracker: CostTracker,
    /// Every `PermissionDecision::Deny` outcome accumulates here and
    /// flushes into `SessionResultParams.permission_denials` at session
    /// end. TS parity: `QueryEngine.ts:244-271` permissionDenials wrapper.
    pub(crate) permission_denials: Vec<coco_types::PermissionDenialInfo>,
    /// Side-channel collectors filled at emission sites so finalize
    /// doesn't need to scan `history` (which mid-run compaction can
    /// replace, invalidating any captured index).
    pub(crate) run_artifacts: RunArtifacts,
}

/// Iteration state machine. Mirrors TS `State` (`query.ts:204-217`)
/// modulo Rust-specific fields. Mutated at `continue` sites and at
/// turn-start to record the per-iteration transition reason.
///
/// Construct via [`Self::new`]; the `BudgetTracker` field requires
/// config-derived arguments that no `Default` impl could supply.
pub(crate) struct LoopTurnState {
    /// 1-based turn counter inside this `run_session_loop` invocation.
    /// Distinct from `session_turn` (the workspace-wide human-turn
    /// count over the whole conversation). Resets per user submit.
    pub(crate) turn: i32,
    /// Why the previous iteration `continue`d (or `None` on the first
    /// iteration / first error path). Surfaced in
    /// [`crate::QueryResult::last_continue_reason`] for SDK consumers
    /// and test assertions. TS parity: `state.transition`.
    pub(crate) transition: Option<TurnTransition>,
    /// TS `stop_hook_active`: set to `true` once a Stop hook has
    /// blocked the loop, so subsequent Stop firings can advertise the
    /// re-entry to the hook.
    pub(crate) stop_hook_active: bool,
    /// How many "inject resume nudge" recovery attempts have fired so
    /// far in this session. Capped at
    /// [`crate::config::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`].
    pub(crate) max_tokens_recovery_count: i32,
    /// Consecutive capacity errors (529/503/Overloaded/RateLimited)
    /// observed on the active model slot. Counts from 0; on the third
    /// streak event the runtime advances to the next fallback slot
    /// (TS: `MAX_529_RETRIES = 3`). Reset on any successful stream
    /// open (which covers the probe-recovery success path by virtue
    /// of running through the same `query_stream` `Ok` arm) and on
    /// fallback advance. Probe-failure revert intentionally does NOT
    /// reset — the counter tracks the active slot's streak, and
    /// reverting to fallback preserves the streak that originally
    /// triggered the advance.
    pub(crate) consecutive_capacity_errors: i32,
    /// TS `input`-parameter parity: the UUID of the last user message
    /// already handed to UserPrompt-tier reminders. Prevents duplicate
    /// `at_mentioned_files` / `agent_mentions` / `ultrathink_effort`
    /// emissions when the same human turn re-enters the loop on a
    /// tool-result iteration.
    pub(crate) reminder_last_user_input_uuid: Option<uuid::Uuid>,
    /// Token / turn / continuation budget gate.
    pub(crate) budget: BudgetTracker,
}

impl LoopTurnState {
    /// Construct a fresh turn-state at session entry. Counters start at
    /// zero, no transition has been recorded yet, and the budget is
    /// initialized from the provided session caps.
    pub(crate) fn new(
        total_token_budget: Option<i64>,
        max_turns: i32,
        max_continuations: i32,
    ) -> Self {
        Self {
            turn: 0,
            transition: None,
            stop_hook_active: false,
            max_tokens_recovery_count: 0,
            consecutive_capacity_errors: 0,
            reminder_last_user_input_uuid: None,
            budget: BudgetTracker::new(total_token_budget, max_turns, max_continuations),
        }
    }
}

/// Long-lived owned objects. Constructed once at session entry, never
/// rebuilt at `continue` sites. Field names are short to keep the
/// callsite ergonomic (`services.runtime` vs `services.model_runtime`);
/// the semantically-named original locals carried unnecessary prefix
/// noise when accessed through a struct path.
pub(crate) struct LoopServices {
    /// Multi-slot model runtime. Walks the fallback chain on capacity
    /// streaks; runs the half-open probe back to the primary when a
    /// recovery policy is installed. Multi-provider — slots may carry
    /// different providers, and `advance()` is a provider switch.
    pub(crate) runtime: ModelRuntime,
    /// Sender side of the per-session progress-event channel. Cloned
    /// into every `ToolUseContext` built for this loop; the receiver
    /// is owned by the spawned drain task (whose `JoinHandle` is
    /// dropped — the task terminates when the last `tx` clone drops).
    pub(crate) progress_tx: tokio::sync::mpsc::UnboundedSender<coco_tool_runtime::ToolProgress>,
    /// Per-turn plan-mode side-effect driver (mode reconcile / mailbox
    /// polling / leader-pending-approvals). NOT a reminder emitter —
    /// the orchestrator below owns that.
    pub(crate) plan: PlanModeReminder,
    /// System-reminder orchestrator (plan / auto-mode / todo / task /
    /// critical / compaction / date-change reminders). Holds
    /// per-attachment throttle state across turns.
    pub(crate) reminders: SystemReminderOrchestrator,
}

/// Entry-derived immutable values. Set once at the top of
/// `run_session_loop`, read everywhere. Fields are owned (no `Arc`)
/// because each is cheap to clone or stored by value.
///
/// Construct via [`Self::derive`]; the per-field derivation logic
/// (config reads, message-list scan, derived ratios) is captured there
/// to keep the loop entry concise.
pub(crate) struct LoopConstants {
    /// Wall-clock start of this session loop. Read at terminal sites
    /// to compute `QueryResult.duration_ms`.
    pub(crate) started_at: std::time::Instant,
    /// UUID-string of the last `User` message in the initial
    /// `turn_messages` list (i.e. the prompt that triggered this loop).
    /// Keys the file-history snapshot taken once at session entry.
    pub(crate) user_uuid: String,
    /// Resolved plans directory (from settings + config_home + project
    /// dir). `None` when no `config_home` is wired (test paths).
    pub(crate) plans_dir: Option<PathBuf>,
    /// Todo-list lookup key: `agent_id` when this engine is a
    /// subagent, otherwise `session_id`. TS parity: `agentId ?? sessionId`.
    pub(crate) todo_key: String,
    /// Model context window in tokens (raw `ModelInfo.context_window`).
    pub(crate) context_window: i64,
    /// 90% of `context_window`, matching `coco-compact`'s
    /// effective-window approximation. Used by the compaction reminder
    /// generator.
    pub(crate) effective_window: i64,
}

impl LoopConstants {
    /// Derive the loop's immutable constants from the engine config and
    /// the initial `turn_messages` list.
    pub(crate) fn derive(engine: &QueryEngine, turn_messages: &[Arc<Message>]) -> Self {
        Self {
            started_at: std::time::Instant::now(),
            // The "current turn" user message id is the LAST user message
            // in `turn_messages`. In single-turn mode the list is
            // `[user_msg, attachment, ...]` and the first (and only) user
            // message is also the last. In multi-turn SDK mode the list
            // is `[prior_history..., new_user_msg]`, so the LAST user
            // message is the current turn's prompt — which is what file
            // history snapshots should key on.
            user_uuid: turn_messages
                .iter()
                .rev()
                .find_map(|m| match m.as_ref() {
                    Message::User(u) => Some(u.uuid.to_string()),
                    Message::Assistant(_)
                    | Message::System(_)
                    | Message::Attachment(_)
                    | Message::ToolResult(_)
                    | Message::Progress(_)
                    | Message::Tombstone(_) => None,
                })
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            plans_dir: crate::plan_mode_reminder::PlanModeReminder::resolve_plans_dir(
                engine.config_home.as_deref(),
                engine.config.project_dir.as_deref(),
                engine.config.plans_directory.as_deref(),
            ),
            todo_key: engine
                .config
                .agent_id
                .clone()
                .unwrap_or_else(|| engine.config.session_id.clone()),
            context_window: engine.config.context_window,
            // Effective = 90% of window (reserve 10% for output),
            // matching the same approximation `coco-compact` uses.
            effective_window: (engine.config.context_window * 9) / 10,
        }
    }
}

impl QueryEngine {
    /// Single-call bundled setup for `run_session_loop`. Builds the
    /// four substructs, spawns the per-session progress-drain task,
    /// populates `MessageHistory` with the initial `turn_messages`,
    /// and snapshots file history for the current prompt.
    ///
    /// Returned tuple is ordered `(acc, turn_state, services, consts)`
    /// to match the destructure at the call site. The progress drain
    /// task's `JoinHandle` is dropped — the task is owned by the
    /// tokio runtime and terminates naturally when the last
    /// `progress_tx` clone (held on `LoopServices.progress_tx`) drops
    /// at the end of the session.
    pub(crate) async fn init_loop_state(
        &self,
        turn_messages: Vec<Arc<Message>>,
        event_tx: &Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        history: &mut MessageHistory,
    ) -> (LoopAccumulator, LoopTurnState, LoopServices, LoopConstants) {
        let consts = LoopConstants::derive(self, &turn_messages);

        // Permission denials accumulate into `acc.permission_denials`
        // on each `PermissionDecision::Deny` branch and flush into
        // `SessionResultParams.permission_denials` via `make_query_result`
        // (TS parity: `QueryEngine.permissionDenials`, QueryEngine.ts:244-271).
        // Run-local artifacts captured at emission sites avoid scanning
        // `history` at finalize time — mid-run compaction replaces the
        // history Vec, so any index captured before then becomes stale.
        let acc = LoopAccumulator::default();

        let turn_state = LoopTurnState::new(
            self.config.total_token_budget,
            self.config.max_turns,
            /*max_continuations*/ 3,
        );

        // Build the per-session ModelRuntime. When the caller installed
        // fallback clients via `with_fallback_client(s)`, the runtime
        // holds a multi-slot chain and walks it on capacity-error
        // streaks via `advance()`.
        //
        // Fallback trigger (TS parity, `services/api/withRetry.ts:335`):
        // after `MAX_529_RETRIES` consecutive `Overloaded` (529/503)
        // responses from the active slot, the next turn advances to
        // the next slot. The engine tracks consecutive capacity errors
        // because provider-layer retries are internal to the
        // vercel-ai crates — this counter only ticks when the retry
        // layer gives up and surfaces an error to us.
        let mut mr_init = ModelRuntime::new(self.client.clone(), self.fallback_clients.clone());
        if let Some(policy) = self.recovery_policy {
            mr_init = mr_init.with_recovery_policy(policy);
        }

        // ── Progress-event forwarder ──
        //
        // Spawn one drain task per session. Tools send `ToolProgress`
        // updates through `ctx.progress_tx`; the drain fans them out
        // to:
        //
        //   1. `TuiOnlyEvent::ToolProgress { tool_use_id, data }` —
        //      every event, unthrottled, carries the raw payload for
        //      the TUI to render progress bars or byte counts.
        //
        //   2. `ServerNotification::ToolProgress(ToolProgressParams)` —
        //      TS-parity wire event. Only emitted for
        //      `bash_progress` / `powershell_progress` payload types
        //      and throttled to ≤1 per 30 s per `parent_tool_use_id`
        //      (or `tool_use_id` if the parent is absent), matching
        //      `utils/queryHelpers.ts:99-189`.
        //
        // Lifecycle: the tx is cloned into every `ToolUseContext`
        // built for this session. When the session loop exits, the
        // last tx clone (owned here) drops, the rx closes, and the
        // drain task finishes naturally — no explicit await needed.
        let (progress_tx_init, mut progress_rx_session) =
            tokio::sync::mpsc::unbounded_channel::<coco_tool_runtime::ToolProgress>();
        let progress_event_tx = event_tx.clone();
        let _progress_drain = tokio::spawn(async move {
            let mut throttle = ProgressThrottle::new();
            while let Some(progress) = progress_rx_session.recv().await {
                drain_one_progress(&progress_event_tx, progress, &mut throttle).await;
            }
        });

        // Plan-mode reminder tracker — per-turn side-effect driver
        // (mode reconcile + mailbox polling + leader-pending-approvals).
        // Reminder emission itself moved to the orchestrator below.
        let mut pr_init = PlanModeReminder::new(
            self.config.permission_mode,
            Some(self.config.session_id.clone()),
            self.config.agent_id.clone(),
            consts.plans_dir.clone(),
            self.app_state.clone(),
        );
        // Wire mailbox for swarm polling if identity is set and a
        // mailbox handle is installed. Agent + team names come from
        // env vars (set by the swarm spawner); mirror
        // `swarm_identity::get_agent_name` env fallback. Env namespace
        // is `COCO_*` — see swarm_constants.
        let agent_name_env = env::env_opt(EnvKey::CocoAgentName);
        let team_name_env = env::env_opt(EnvKey::CocoTeamName);
        if let (Some(mbox), Some(agent), Some(team)) =
            (self.mailbox.clone(), agent_name_env, team_name_env)
        {
            pr_init = pr_init.with_mailbox(
                mbox,
                agent,
                team,
                self.config.is_teammate && self.config.plan_mode_required,
            );
        }
        // Install the protocol-event sink so leader-pending-approval
        // polling can surface `PlanApprovalRequested` to the TUI in
        // addition to injecting the LLM-prompt attachment. Absent sink
        // (SDK-only / headless) means the overlay simply never fires.
        if let Some(tx) = event_tx.clone() {
            pr_init = pr_init.with_event_sink(tx);
        }

        // System-reminder orchestrator — owns reminder emission for
        // the whole session (plan/auto/todo/task/critical/compaction/
        // date-change). Send+Sync; accumulates per-attachment throttle
        // state across turns. Config cloned because the orchestrator
        // owns its copy — subsequent settings reloads won't
        // retroactively disable reminders until the next engine build.
        let ro_init = SystemReminderOrchestrator::new(self.config.system_reminder.clone())
            .with_default_generators();

        let services = LoopServices {
            runtime: mr_init,
            progress_tx: progress_tx_init,
            plan: pr_init,
            reminders: ro_init,
        };

        // I-1 (Authority) — D2 fix: callers (`tui_runner`, SDK turn
        // handler, subagent factory) emit `MessageAppended` for any
        // NEW messages they introduce BEFORE invoking the engine. The
        // initial load here just populates the engine's per-turn
        // working `MessageHistory` with prior context — re-emitting
        // would deliver duplicate events to consumers on every turn
        // (TUI dedups by UUID, but SDK NDJSON observers would see N
        // copies after N turns). Subsequent push sites inside the
        // loop (new assistant turns, tool results, system messages)
        // still emit normally.
        for arc in turn_messages {
            history.push_arc(arc);
        }

        // NOTE: `SessionStarted` + `SessionStateChanged(Running)` +
        // the hook → CoreEvent forwarder are set up by the outer
        // `run_internal_with_messages` BEFORE calling this function,
        // so SDK consumers see them even if the session loop errors
        // out before its first turn.

        // Create file history snapshot for this user message.
        // TS: fileHistoryMakeSnapshot() in handlePromptSubmit.ts +
        // QueryEngine.ts
        if let (Some(fh), Some(ch)) = (&self.file_history, &self.config_home) {
            let mut fh = fh.write().await;
            if let Err(e) = fh
                .make_snapshot(&consts.user_uuid, ch, &self.config.session_id)
                .await
            {
                warn!("file history make_snapshot failed: {e}");
            }
        }

        (acc, turn_state, services, consts)
    }
}
