//! Per-turn system-reminder pipeline extracted from `run_session_loop`.
//!
//! The reminder pipeline is the longest single block of work the agent loop
//! performs each turn (5 phases, ~500 lines):
//! 1. Plan-mode side effects that *mutate* `app_state`
//!    (`turn_start_side_effects_only`).
//! 2. Build a [`TurnReminderInput`] from engine state + a fresh
//!    `app_state` snapshot + per-source [`ReminderSources::materialize`]
//!    fan-out.
//! 3. Run the orchestrator with that input → reminders.
//! 4. Post-emit bookkeeping: clear stale exit flags, bump cadence counters,
//!    refresh "last announced" sets for delta-style reminders.
//! 5. Drain the silent attachment inbox + inject reminders into history.
//!
//! Owns one method on [`QueryEngine`], [`QueryEngine::run_turn_reminder_pipeline`],
//! that performs all five phases. Returns the `app_state` snapshot the
//! caller passes to [`QueryEngine::build_tool_definitions`].

use std::collections::HashSet;

use coco_messages::CostTracker;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_system_reminder::AttachmentType as ReminderAttachmentType;
use coco_system_reminder::SystemReminderOrchestrator;
use coco_system_reminder::TurnReminderInput;
use coco_system_reminder::count_human_turns;
use coco_system_reminder::inject_reminders;
use coco_system_reminder::run_turn_reminders;
use coco_types::Feature;
use coco_types::PermissionMode;
use coco_types::TokenUsage;
use coco_types::ToolAppState;

use crate::engine::QueryEngine;
use crate::engine_helpers::compute_agents_delta;
use crate::engine_helpers::compute_mcp_instructions_delta;
use crate::engine_helpers::compute_tools_delta;
use crate::engine_helpers::latest_user_input_text;
use crate::plan_mode_reminder::PlanModeReminder;

/// Per-turn inputs to [`QueryEngine::run_turn_reminder_pipeline`].
///
/// Bundles the local-to-`run_session_loop` state the pipeline needs to
/// thread through. Three flavors:
/// - `&mut`: state the pipeline mutates (`history`, `plan_reminder`,
///   `last_user_input_uuid`).
/// - `&`: read-only per-turn signals (`orchestrator`, `total_usage`,
///   `cost_tracker`).
/// - by-value: small scalars (`todo_key`, `context_window`,
///   `effective_window`).
pub(crate) struct TurnReminderContext<'a> {
    pub history: &'a mut MessageHistory,
    pub plan_reminder: &'a mut PlanModeReminder,
    pub orchestrator: &'a SystemReminderOrchestrator,
    pub last_user_input_uuid: &'a mut Option<uuid::Uuid>,
    pub total_usage: &'a TokenUsage,
    pub cost_tracker: &'a CostTracker,
    pub todo_key: &'a str,
    pub context_window: i64,
    pub effective_window: i64,
    /// Wire channel for the reminder injection step so each
    /// model-visible reminder appended to `MessageHistory` emits a
    /// `ServerNotification::MessageAppended` and observers stay
    /// coherent (I-1 authority). `None` for paths that don't have a
    /// wire (forked agents, tests).
    pub event_tx: &'a Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,
}

impl QueryEngine {
    /// Run the per-turn system-reminder pipeline (TS QueryEngine.ts Phase D.3).
    ///
    /// 1. `plan_reminder.turn_start_side_effects_only` (mode reconcile +
    ///    mailbox + leader-pending). Mutates `app_state` BEFORE the
    ///    orchestrator reads it below.
    /// 2. Build [`TurnReminderInput`] from engine state + a fresh
    ///    `app_state` snapshot + a parallel [`ReminderSources::materialize`].
    /// 3. Run the orchestrator → reminders.
    /// 4. Post-emit bookkeeping: clear stale exit flags, bump plan-mode
    ///    cadence counters, refresh "last announced" sets for delta
    ///    reminders.
    /// 5. Drain silent attachments, inject reminders into history.
    ///
    /// Returns the `app_state` snapshot the caller passes to
    /// [`QueryEngine::build_tool_definitions`] for the same turn — same
    /// permission mode / pre-plan-mode / stripped-rules view the reminders
    /// just observed.
    pub(crate) async fn run_turn_reminder_pipeline(
        &self,
        ctx: TurnReminderContext<'_>,
    ) -> ToolAppState {
        let TurnReminderContext {
            history,
            plan_reminder,
            orchestrator: reminder_orchestrator,
            last_user_input_uuid,
            total_usage,
            cost_tracker,
            todo_key: reminder_todo_key,
            context_window: reminder_context_window,
            effective_window: reminder_effective_window,
            event_tx,
        } = ctx;
        // Phase 1. Run non-reminder side effects (mode reconciliation +
        // mailbox polling + leader pending-approvals) — these MUTATE
        // app_state (setting `needs_plan_mode_exit_attachment` /
        // `has_exited_plan_mode` when detecting unannounced mode
        // transitions). Must run BEFORE the orchestrator reads
        // app_state below.
        plan_reminder.turn_start_side_effects_only(history).await;

        // Phase 2. Build orchestrator input from engine state + current
        // app_state snapshot.
        //
        // `turn_number` uses **human turns** (non-meta user messages)
        // so plan-mode / auto-mode throttle cadence matches TS
        // (counts human turns, not LLM iterations). Tool-result
        // rounds within one human turn share the same counter value
        // so reminders don't spam mid-turn.
        // Take the app_state snapshot *before* building the stub so
        // the reminder context observes the same discovered-tool set
        // that `engine_prompt::build_tool_definitions` will use this
        // turn — keeps the `deferred_tools_delta` reminder aligned
        // with what the LLM actually receives as tools.
        let pre_snapshot_discovered: std::sync::Arc<std::collections::HashSet<String>> = match self
            .app_state
            .as_ref()
        {
            Some(state) => std::sync::Arc::new(state.read().await.discovered_tool_names.clone()),
            None => std::sync::Arc::new(std::collections::HashSet::new()),
        };
        // Match `engine_prompt::build_tool_definitions` capability
        // resolution exactly so the registry partitions seen here
        // line up byte-for-byte with the LLM-visible tools set. When
        // either capability is absent, `tool_search_active()` returns
        // false in the registry and `deferred_tools` is empty — the
        // `deferred_tools_delta` reminder collapses to "nothing
        // searchable", which is the correct truth.
        let supports_tool_reference = self.client.model_info().is_some_and(|info| {
            info.has_capability(coco_types::Capability::ServerSideToolReference)
        });
        let supports_client_side_tool_search = self
            .client
            .model_info()
            .is_some_and(|info| info.has_capability(coco_types::Capability::ClientSideToolSearch));
        // Both partitions share the same filter context so they
        // cover disjoint halves of the registry — `loaded` includes
        // discovered tools, `deferred` excludes them.
        let (reminder_loaded_tools, reminder_deferred_tools): (Vec<String>, Vec<String>) = {
            let stub_ctx = coco_tool_runtime::ToolUseContext::stub_for_filtering(
                self.config.features.clone(),
                self.config.tool_overrides.clone(),
                self.config.tool_filter.clone(),
                self.config.permission_mode,
            )
            .with_discovered_tool_names(pre_snapshot_discovered)
            .with_model_capabilities(supports_tool_reference, supports_client_side_tool_search);
            let loaded: Vec<String> = self
                .tools
                .loaded_tools(&stub_ctx)
                .iter()
                .map(|t| t.name().to_string())
                .collect();
            let deferred: Vec<String> = self
                .tools
                .deferred_tools(&stub_ctx)
                .iter()
                .map(|t| t.name().to_string())
                .collect();
            (loaded, deferred)
        };
        // `reminder_tools` retains its TS name as the model-visible
        // (loaded) tool list — used by `TurnReminderInput::tools` and
        // unchanged consumers below.
        let reminder_tools = reminder_loaded_tools.clone();
        let pm_settings = &self.config.plan_mode_settings;
        let workflow_rm = match pm_settings.workflow {
            coco_config::PlanModeWorkflow::FivePhase => coco_context::PlanWorkflow::FivePhase,
            coco_config::PlanModeWorkflow::Interview => coco_context::PlanWorkflow::Interview,
        };
        let phase4_rm = match pm_settings.phase4_variant {
            coco_config::PlanPhase4Variant::Standard => coco_context::Phase4Variant::Standard,
            coco_config::PlanPhase4Variant::Trim => coco_context::Phase4Variant::Trim,
            coco_config::PlanPhase4Variant::Cut => coco_context::Phase4Variant::Cut,
            coco_config::PlanPhase4Variant::Cap => coco_context::Phase4Variant::Cap,
        };
        // Plan file path / existence — same resolver the deprecated
        // emission path uses, so both paths agree on the filesystem state.
        let (reminder_plan_path, reminder_plan_exists) =
            match (self.config_home.as_deref(), &self.config.session_id) {
                (Some(ch), sid) if !sid.is_empty() => {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        self.config.project_dir.as_deref(),
                        self.config.plans_directory.as_deref(),
                    );
                    let path = coco_context::get_plan_file_path(
                        sid,
                        &plans_dir,
                        self.config.agent_id.as_deref(),
                    );
                    let exists = path.exists();
                    (Some(path), exists)
                }
                _ => (None, false),
            };

        let reminder_human_turn_number = count_human_turns(history.as_slice());

        // Take an app_state snapshot so the input struct holds an
        // immutable borrow; any post-emit clearing happens after the
        // orchestrator returns.
        let app_state_snapshot = match self.app_state.as_ref() {
            Some(state) => state.read().await.clone(),
            None => ToolAppState::default(),
        };

        // Seed the orchestrator's throttle state from `app_state` so
        // reminder cadence survives across `run_session_loop`
        // invocations. Each `run_plan_mode_turn` / `run_internal`
        // call constructs a fresh orchestrator but `app_state`
        // persists — without seeding, turn 2 of a multi-turn test
        // would see an empty throttle and fire a second reminder.
        //
        // Implied `last_generated_turn`: the current human-turn
        // counter minus the stored gap. Tool-result rounds within
        // the same human turn keep the same value, so the throttle
        // correctly blocks within-turn re-firing.
        if app_state_snapshot.plan_mode_attachment_count > 0 {
            let gap = i32::try_from(app_state_snapshot.plan_mode_turns_since_last_attachment)
                .unwrap_or(i32::MAX);
            let last_gen_turn = reminder_human_turn_number.saturating_sub(gap);
            reminder_orchestrator.throttle().seed_state(
                ReminderAttachmentType::PlanMode,
                coco_system_reminder::ThrottleState {
                    last_generated_turn: Some(last_gen_turn),
                    session_count: i32::try_from(app_state_snapshot.plan_mode_attachment_count)
                        .unwrap_or(i32::MAX),
                    trigger_turn: None,
                },
            );
        }

        // TS `autoModeStateModule?.isAutoModeActive()`. `None` means the
        // engine was built without a permissions auto-mode state — auto
        // mode is therefore inactive, matching TS's `?? false` fallback.
        let reminder_auto_classifier_active = self
            .auto_mode_state
            .as_ref()
            .map(|s| s.is_active())
            .unwrap_or(false);
        let reminder_permission_mode = app_state_snapshot
            .permission_mode
            .unwrap_or(self.config.permission_mode);
        let reminder_is_plan_mode = reminder_permission_mode == PermissionMode::Plan;
        let reminder_is_auto_mode = reminder_permission_mode == PermissionMode::Auto
            || (reminder_permission_mode == PermissionMode::Plan
                && reminder_auto_classifier_active);
        // TS `isTodoV2Enabled()` — coco-rs reads the explicit
        // `Feature::TaskV2` gate (default-on) which drives the V1/V2
        // mutual exclusion at tool level (`is_enabled` on TodoWrite vs
        // TaskCreate/Get/List/Update). The flag is the source of truth;
        // `TASK_MANAGEMENT_TOOLS` is still used by `turn_runner` for
        // counting turns since the last task mutation, but no longer
        // for V2-mode detection.
        let reminder_task_v2_enabled = self.config.features.enabled(Feature::TaskV2);
        // TS `isAutoCompactEnabled()` — a user-facing toggle. coco-rs
        // resolves it through `QueryEngineConfig.compact.auto.is_active()`
        // (user toggle AND env kill switches) so the SDK / CLI / TUI
        // can control it per session without re-reading settings from
        // disk.
        let reminder_auto_compact_enabled = self.config.is_auto_compact_active();
        // TS `getDeferredToolsDelta` — diff the current **deferred**
        // tool set against the last announced set on app_state. The
        // loaded set is supplied so a tool that moves deferred →
        // loaded (model discovered via `ToolSearch`) stays silently
        // in the announced pool. Non-empty added or removed triggers
        // the `deferred_tools_delta` reminder.
        let reminder_deferred_tools_delta = compute_tools_delta(
            &reminder_deferred_tools,
            &reminder_loaded_tools,
            &app_state_snapshot.last_announced_tools,
        );
        // Clone the deferred list for post-emit bookkeeping — TS
        // `getDeferredToolsDelta` replaces `announced` with the
        // current deferred set after emission.
        let reminder_deferred_tools_clone = reminder_deferred_tools.clone();
        // TS `getAgentListingDeltaAttachment` — diff the current
        // agent-type set (from `SessionBootstrap`) against the
        // last-announced set on app_state.
        let reminder_current_agents: Vec<String> = self
            .session_bootstrap
            .as_ref()
            .map(|b| b.agents.clone())
            .unwrap_or_default();
        let reminder_agent_listing_delta = compute_agents_delta(
            &reminder_current_agents,
            &app_state_snapshot.last_announced_agents,
        );
        // TS date-change latch: current local ISO date vs. the one
        // stored on `ToolAppState.last_emitted_date`. When they
        // differ, emit once + update the latch. Runs at turn start
        // so the reminder sees today's date even for long-running
        // sessions that cross midnight.
        let reminder_new_date = self.observe_date_change().await;

        // TS `getAttachments(input, ...)` — the user's raw prompt
        // text for this turn. Extract from the most-recent non-meta
        // user message's text content; used by both the
        // ultrathink-keyword gate and mention-based reminders.
        //
        // TS parity: `input` is non-null only on the first tool-loop
        // iteration of a human turn, not on subsequent tool-result
        // rounds (query.ts nulls it out). coco-rs tracks the last
        // user-message UUID that has already been reminder-scanned
        // and skips re-parsing it so the user-input tier fires once
        // per human turn, not once per tool-result iteration.
        let reminder_current_user_uuid = history.iter().rev().find_map(|m| match m.as_ref() {
            Message::User(u) => Some(u.uuid),
            _ => None,
        });
        let reminder_is_new_human_turn = reminder_current_user_uuid != *last_user_input_uuid;
        let reminder_user_input: Option<String> = if reminder_is_new_human_turn {
            *last_user_input_uuid = reminder_current_user_uuid;
            latest_user_input_text(history)
        } else {
            None
        };
        let reminder_mentions: Vec<coco_context::user_input::Mention> = reminder_user_input
            .as_deref()
            .map(|raw| coco_context::user_input::process_user_input(raw).mentions)
            .unwrap_or_default();
        let reminder_at_mentioned_files: Vec<coco_system_reminder::MentionedFileEntry> =
            reminder_mentions
                .iter()
                .filter(|m| {
                    matches!(
                        m.mention_type,
                        coco_context::user_input::MentionType::FilePath
                    )
                })
                .map(|m| coco_system_reminder::MentionedFileEntry {
                    filename: m.text.clone(),
                    display_path: m.text.clone(),
                })
                .collect();
        let reminder_agent_mentions: Vec<coco_system_reminder::AgentMentionEntry> =
            reminder_mentions
                .iter()
                .filter(|m| matches!(m.mention_type, coco_context::user_input::MentionType::Agent))
                .map(|m| coco_system_reminder::AgentMentionEntry {
                    agent_type: m.text.clone(),
                })
                .collect();

        // TS `toolUseContext.options.*` bag analog — fan-out to every
        // per-subsystem source (hooks / LSP / tasks / skills / MCP /
        // swarm / IDE / memory) in parallel, with per-source timeout
        // + error-to-default. Empty `ReminderSources` → all defaults.
        //
        // Resolve relative paths against cwd so they match the absolute
        // keys used by `FileReadState` (populated by `mention_resolver`
        // and the Read tool). Without this, the AlreadyReadFile silent
        // reminder and nested-memory lookups never hit on @-mentions.
        let reminder_cwd = std::env::current_dir().unwrap_or_default();
        let reminder_mentioned_paths: Vec<std::path::PathBuf> = reminder_mentions
            .iter()
            .filter(|m| {
                matches!(
                    m.mention_type,
                    coco_context::user_input::MentionType::FilePath
                )
            })
            .map(|m| {
                let p = std::path::PathBuf::from(&m.text);
                if p.is_absolute() {
                    p
                } else {
                    reminder_cwd.join(p)
                }
            })
            .collect();

        let reminder_source_timeout =
            std::time::Duration::from_millis(if reminder_orchestrator.config().timeout_ms > 0 {
                reminder_orchestrator.config().timeout_ms as u64
            } else {
                coco_system_reminder::DEFAULT_TIMEOUT_MS as u64
            });
        // One-shot flag: every successful compaction (full / SM / reactive)
        // sets it; the next reminder build consumes (swap-to-false) so
        // `task_status` only fires on the immediately-following turn —
        // matching TS `getUnifiedTaskAttachments` post-compact emission
        // surface (`attachments.ts:962`).
        let just_compacted = self
            .pending_just_compacted
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        // Tools the model successfully invoked since the previous human
        // turn. TS `collectRecentSuccessfulTools` (`utils/attachments.ts`)
        // feeds this into the `findRelevantMemories` ranker so it can
        // deprioritize reference docs for tools the model is actively
        // exercising. Empty when no human-turn boundary is established
        // yet (start of conversation, or no successful tool runs).
        let reminder_recent_tools = collect_recent_successful_tools(history.as_slice());
        let materialized = self
            .reminder_sources
            .materialize(coco_system_reminder::MaterializeContext {
                config: reminder_orchestrator.config(),
                agent_id: self.config.agent_id.as_deref(),
                user_input: reminder_user_input.as_deref(),
                mentioned_paths: &reminder_mentioned_paths,
                recent_tools: &reminder_recent_tools,
                just_compacted,
                per_source_timeout: reminder_source_timeout,
            })
            .await;

        // Part 1 silent reminder: intersect every path this turn
        // might try to load (@-mentions + nested memory + relevant
        // memory prefetch) with the session file-read cache. Paths
        // whose mtime still matches disk are "already loaded into
        // context" — we emit a silent dedup marker so downstream
        // tooling (transcript, telemetry) knows the model has current
        // content for those paths. Mirrors TS `already_read_file`
        // emission surface area (`utils/attachments.ts:3100`).
        let reminder_already_read_file_paths: Vec<std::path::PathBuf> =
            if let Some(frs) = &self.file_read_state {
                let mut candidates: Vec<std::path::PathBuf> = reminder_mentioned_paths.clone();
                candidates.extend(
                    materialized
                        .nested_memories
                        .iter()
                        .map(|m| std::path::PathBuf::from(&m.path)),
                );
                candidates.extend(
                    materialized
                        .relevant_memories
                        .iter()
                        .map(|m| std::path::PathBuf::from(&m.path)),
                );
                if candidates.is_empty() {
                    Vec::new()
                } else {
                    // Dedup while preserving first-seen order so the
                    // resulting list is deterministic across turns.
                    let mut seen = HashSet::new();
                    candidates.retain(|p| seen.insert(p.clone()));
                    let guard = frs.read().await;
                    guard.unchanged_paths(&candidates).await
                }
            } else {
                Vec::new()
            };

        // Cross-turn typed silent attachments (command_permissions,
        // structured_output, hook events, …) come in via the session
        // `AttachmentEmitter` and are drained from the inbox at the
        // head of `inject_reminders_into_history` below — there is no
        // separate mailbox state to thread into `TurnReminderInput`.

        let reminder_input = TurnReminderInput {
            config: reminder_orchestrator.config(),
            turn_number: reminder_human_turn_number,
            agent_id: self.config.agent_id.clone(),
            user_input: reminder_user_input.clone(),
            last_human_turn_uuid: history.iter().rev().find_map(|m| match m.as_ref() {
                Message::User(u) => Some(u.uuid),
                _ => None,
            }),
            plan_file_path: reminder_plan_path,
            plan_exists: reminder_plan_exists,
            plan_workflow: workflow_rm,
            phase4_variant: phase4_rm,
            explore_agent_count: pm_settings.explore_agent_count,
            plan_agent_count: pm_settings.plan_agent_count,
            is_plan_interview_phase: false,
            app_state: &app_state_snapshot,
            fallback_permission_mode: self.config.permission_mode,
            is_auto_classifier_active: reminder_auto_classifier_active,
            tools: reminder_tools,
            is_task_v2_enabled: reminder_task_v2_enabled,
            history,
            todo_key: reminder_todo_key.to_string(),
            is_auto_compact_enabled: reminder_auto_compact_enabled,
            context_window: reminder_context_window,
            effective_context_window: reminder_effective_window,
            used_tokens: total_usage.input_tokens,
            new_date: reminder_new_date,
            has_pending_plan_verification: app_state_snapshot.pending_plan_verification,
            // Phase 1 engine-local inputs.
            total_cost_usd: cost_tracker.total_cost_usd(),
            max_budget_usd: self.config.max_budget_usd,
            // Injected at turn start — TS `getTurnOutputTokens()` is zero
            // at this point; cumulative session count comes from usage.
            output_tokens_turn: 0,
            output_tokens_session: total_usage.output_tokens,
            // Not yet wired (requires feature('TOKEN_BUDGET')-equivalent).
            output_token_budget: None,
            // Companion subsystem lives in a future Buddy crate; for now
            // suppress the reminder by leaving these unset.
            companion_name: None,
            companion_species: None,
            has_prior_companion_intro: false,
            deferred_tools_delta: reminder_deferred_tools_delta.clone(),
            agent_listing_delta: reminder_agent_listing_delta.clone(),
            // McpSource.instructions() returns the current per-server
            // map; engine diffs against `last_announced_mcp_instructions`
            // to produce the delta (same pattern as deferred_tools_delta).
            mcp_instructions_delta: compute_mcp_instructions_delta(
                &materialized.mcp_instructions_current,
                &app_state_snapshot.last_announced_mcp_instructions,
            ),
            // Phase 3: cross-crate state flows via `ReminderSources`.
            // Sources that aren't wired → default output → generator skips.
            hook_events: materialized.hook_events,
            diagnostics: materialized.diagnostics,
            // TS `getOutputStyleAttachment` — reads style name from
            // `SessionBootstrap` (CLI-resolved from `settings.output_style`).
            // This is a simple read, not cross-crate state, so no Source
            // trait is needed.
            output_style: self
                .session_bootstrap
                .as_ref()
                .and_then(|b| b.output_style.as_ref())
                .filter(|s| !s.is_empty())
                .map(|name| coco_system_reminder::OutputStyleSnapshot { name: name.clone() }),
            queued_commands: self
                .command_queue
                .snapshot_for_reminder(self.config.agent_id.as_deref())
                .await,
            task_statuses: materialized.task_statuses,
            // SkillsSource wins when present; else fall back to
            // SessionBootstrap names-only listing.
            skill_listing: materialized.skill_listing.or_else(|| {
                self.session_bootstrap
                    .as_ref()
                    .filter(|b| !b.skills.is_empty())
                    .map(|b| {
                        b.skills
                            .iter()
                            .map(|s| format!("- {s}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
            }),
            invoked_skills: materialized.invoked_skills,
            teammate_mailbox: materialized.teammate_mailbox,
            team_context: materialized.team_context,
            agent_pending_messages: materialized.agent_pending_messages,
            // Phase 4: mention-based reminders are populated from
            // `process_user_input`. MCP resources come from the MCP
            // source; IDE state is a main-thread reminder source.
            at_mentioned_files: reminder_at_mentioned_files,
            mcp_resources: materialized.mcp_resources,
            agent_mentions: reminder_agent_mentions,
            ide_selection: materialized.ide_selection,
            ide_opened_file: materialized.ide_opened_file,
            // Nested memories: engine-driven via the per-batch
            // `drain_nested_memory_triggers` pipeline (Read tool
            // populates `ctx.nested_memory_attachment_triggers` →
            // engine drains end-of-batch → traverses CWD→file →
            // appends here). `MemoryAdapter::nested_memories`
            // intentionally returns empty so the engine path is the
            // single source. We `extend` (rather than replace) in case
            // a future MemorySource impl wants to contribute as well —
            // currently the materialized side is always empty.
            nested_memories: {
                let mut v = self.take_pending_nested_memory().await;
                if !materialized.nested_memories.is_empty() {
                    v.extend(materialized.nested_memories);
                }
                v
            },
            relevant_memories: materialized.relevant_memories,
            // Silent reminder-native attachments (Part 1).
            // `already_read_file_paths`: intersection of this turn's
            // @-mentioned paths with the `FileReadState` cache where
            // mtime still matches disk — computed above via
            // `FileReadState::unchanged_paths`.
            // `edited_image_file_paths`: reserved for a future image-
            // mtime tracker. Text `FileReadState` is text-only; image
            // drift detection would need a parallel cache.
            already_read_file_paths: reminder_already_read_file_paths,
            edited_image_file_paths: Vec::new(),
            skill_discovery: materialized.skill_discovery,
        };
        let reminders = run_turn_reminders(reminder_orchestrator, reminder_input).await;

        // Phase 4. Post-emit bookkeeping on app_state. Writing AFTER the
        // orchestrator read ensures we don't clear a flag whose
        // reminder got throttled (so it can fire next turn).
        //
        // Covers three concerns:
        // - One-shot flags consumed by the generators that fired
        //   (PlanModeExit / AutoModeExit / PlanModeReentry).
        // - Cadence counters the TUI / tests observe via app_state
        //   (`plan_mode_attachment_count` +
        //   `plan_mode_turns_since_last_attachment`). These mirror
        //   the ThrottleManager state but are exposed on app_state
        //   for TS parity with `getAppState().planModeAttachmentCount`.
        let stale_plan_exit_flag =
            app_state_snapshot.needs_plan_mode_exit_attachment && reminder_is_plan_mode;
        let stale_auto_exit_flag =
            app_state_snapshot.needs_auto_mode_exit_attachment && reminder_is_auto_mode;
        let needs_reminder_bookkeeping =
            !reminders.is_empty() || stale_plan_exit_flag || stale_auto_exit_flag;
        if needs_reminder_bookkeeping && self.app_state.is_some() {
            let fired_types: HashSet<ReminderAttachmentType> =
                reminders.iter().map(|r| r.attachment_type).collect();
            if let Some(state) = self.app_state.as_ref() {
                let mut guard = state.write().await;
                // TS clears stale one-shot exit flags when the engine is
                // still in the matching mode instead of preserving them
                // for a later, unrelated turn.
                if stale_plan_exit_flag {
                    guard.needs_plan_mode_exit_attachment = false;
                }
                if stale_auto_exit_flag {
                    guard.needs_auto_mode_exit_attachment = false;
                }
                if fired_types.contains(&ReminderAttachmentType::PlanModeExit) {
                    guard.needs_plan_mode_exit_attachment = false;
                    // TS: exit resets the plan-mode cadence cycle.
                    guard.plan_mode_attachment_count = 0;
                    guard.plan_mode_turns_since_last_attachment = 0;
                    guard.last_human_turn_uuid_seen = None;
                }
                if fired_types.contains(&ReminderAttachmentType::AutoModeExit) {
                    guard.needs_auto_mode_exit_attachment = false;
                }
                if fired_types.contains(&ReminderAttachmentType::PlanModeReentry) {
                    guard.has_exited_plan_mode = false;
                }
                if fired_types.contains(&ReminderAttachmentType::PlanMode) {
                    // Bump the TS-parity cadence counter + reset the
                    // "turns since last attachment" counter so the TUI
                    // and integration tests observe the same cadence
                    // state as the pre-Phase-D PlanModeReminder flow.
                    guard.plan_mode_attachment_count =
                        guard.plan_mode_attachment_count.saturating_add(1);
                    guard.plan_mode_turns_since_last_attachment = 0;
                    // Stamp the current human-turn UUID so subsequent
                    // tool-result rounds sharing the same UUID don't
                    // advance the counter (mirror of the old
                    // `observe_turn_and_count` behavior).
                    if let Some(uuid) = history.iter().rev().find_map(|m| match m.as_ref() {
                        Message::User(u) => Some(u.uuid),
                        _ => None,
                    }) {
                        guard.last_human_turn_uuid_seen = Some(uuid);
                    }
                }
                // TS `getDeferredToolsDelta` replaces the announced
                // set with the current **deferred** tool list after
                // successful emission. Subsequent turns then diff
                // against the fresh baseline.
                if fired_types.contains(&ReminderAttachmentType::DeferredToolsDelta) {
                    guard.last_announced_tools =
                        reminder_deferred_tools_clone.iter().cloned().collect();
                }
                // Same pattern for the agent-listing delta.
                if fired_types.contains(&ReminderAttachmentType::AgentListingDelta) {
                    guard.last_announced_agents = reminder_current_agents.iter().cloned().collect();
                }
                // Same pattern for the MCP-instructions delta.
                if fired_types.contains(&ReminderAttachmentType::McpInstructionsDelta) {
                    guard.last_announced_mcp_instructions =
                        materialized.mcp_instructions_current.clone();
                }
            }
        }

        // Phase 5. Inject typed attachment events and reminder messages into history. Model-visible
        // reminders append to `history`; silent reminders
        // (`Coverage::SilentReminder` + `ReminderOutput::Silent*`)
        // come back as `display_only` so they never leak into the
        // API call but stay observable for UI / telemetry.
        // Drain any silent attachments queued by owner crates
        // (hooks / permissions / tools / etc.) since the prior turn.
        // Must happen BEFORE inject_reminders so the reminder pipeline
        // sees any cross-crate-produced attachments in history.
        let drained = self.drain_attachment_inbox(history, event_tx).await;
        if drained > 0 {
            tracing::debug!(
                target: "coco::attachment_inbox",
                drained,
                "drained silent attachments into history"
            );
        }

        // I-1 (Authority): every reminder appended to history must emit
        // a `MessageAppended` so TUI's TranscriptView + SDK NDJSON
        // observers track it. Pull the materialized reminder messages
        // and push each through the event-emitting helper.
        let batch = inject_reminders(reminders);
        if !batch.is_empty() {
            tracing::debug!(
                target: "coco::system_reminder::inject",
                model_visible = batch.model_visible.len(),
                display_only = batch.display_only.len(),
                "injecting reminder batch",
            );
        }
        for msg in batch.model_visible {
            crate::history_sync::history_push_and_emit(history, msg, event_tx).await;
        }
        for msg in &batch.display_only {
            tracing::debug!(
                target: "coco::system_reminder::display_only",
                injected = ?msg,
                "silent reminder routed to display-only sink"
            );
        }

        app_state_snapshot
    }
}

/// Tool names the assistant successfully invoked since the previous
/// human turn. TS parity with `collectRecentSuccessfulTools`
/// (`utils/attachments.ts`):
///
/// - Slice messages from the most recent user message to the end —
///   that's "this human turn".
/// - In that slice, harvest `tool_call_id → tool_name` from every
///   assistant `ToolCall` block, and `tool_use_id → !is_error` from
///   every `ToolResultMessage`.
/// - Return the unique set of tool names whose result was non-error.
///
/// Empty when no human turn has happened yet (no Message::User in
/// history) or no tool produced a non-error result. Order is
/// unspecified — the recall ranker uses set membership only.
fn collect_recent_successful_tools<M: std::borrow::Borrow<Message>>(messages: &[M]) -> Vec<String> {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use std::collections::HashMap;

    let Some(last_user_idx) = messages
        .iter()
        .rposition(|m| matches!(m.borrow(), Message::User(_)))
    else {
        return Vec::new();
    };

    let mut use_id_to_name: HashMap<String, String> = HashMap::new();
    let mut success_by_use_id: HashMap<String, bool> = HashMap::new();
    for m in &messages[last_user_idx..] {
        match m.borrow() {
            Message::Assistant(a) => {
                if let LlmMessage::Assistant { content, .. } = &a.message {
                    for block in content {
                        if let AssistantContent::ToolCall(call) = block {
                            use_id_to_name
                                .insert(call.tool_call_id.clone(), call.tool_name.clone());
                        }
                    }
                }
            }
            Message::ToolResult(tr) => {
                success_by_use_id.insert(tr.tool_use_id.clone(), !tr.is_error);
            }
            _ => {}
        }
    }

    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (use_id, name) in &use_id_to_name {
        if success_by_use_id.get(use_id).copied().unwrap_or(false) && seen.insert(name.clone()) {
            out.push(name.clone());
        }
    }
    out
}
