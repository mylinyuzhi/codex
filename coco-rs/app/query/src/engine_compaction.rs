//! Full LLM compaction + session-memory short-circuit + manual entry-point.
//!
//! Owns the three "summarize and rewrite history" flows:
//! - [`QueryEngine::run_manual_compact`] — `/compact [instructions]` user entry,
//! - [`QueryEngine::try_full_compact`] — full LLM summarization (+ pre/post-compact hooks
//!   + post-compact attachment re-injection),
//! - [`QueryEngine::try_session_memory_compact`] — pre-extracted memory short-circuit
//!   that rewrites history without an LLM call when it would still fit under the
//!   auto-compact threshold.
//!
//! The reactive (PTL recovery) path and the per-turn auto-compact ladder live in
//! `crate::engine_finalize_turn` because they share the `finalize_turn_post_tools`
//! sequence and emit a different set of `CompactionPhase` events.

use tracing::info;
use tracing::warn;

use coco_inference::QueryParams;
use coco_messages::AssistantContent;
use coco_messages::LlmMessage;
use coco_messages::MessageHistory;

use crate::CoreEvent;
use crate::ServerNotification;
use crate::emit::emit_protocol;
use crate::engine::QueryEngine;

impl QueryEngine {
    /// Public manual entry-point for `/compact [instructions]`.
    ///
    /// Equivalent to the auto path but with `CompactTrigger::Manual` and
    /// the user-supplied instructions threaded into the summary prompt.
    /// Callers (TUI / SDK) can drive compaction directly without going
    /// through the auto-trigger threshold.
    pub async fn run_manual_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        custom_instructions: Option<String>,
    ) {
        // TS commands/compact/compact.ts:98 calls `microcompactMessages`
        // before `compactConversation`, but that function is a no-op in
        // external builds (only the `feature('CACHED_MICROCOMPACT')` /
        // time-based paths mutate, and neither fires synchronously here).
        // Opt-in via `compact.micro.count_based_enabled` (default off);
        // when off, we behave like TS external — straight to SM/LLM.
        let micro_keep = self.config.compact.micro.keep_recent.max(0) as usize;
        let will_try_sm =
            custom_instructions.is_none() && self.config.compact.session_memory.enabled;
        if !will_try_sm
            && self.config.compact.micro.enabled
            && self.config.compact.micro.count_based_enabled
        {
            coco_compact::micro_compact(&mut history.messages, micro_keep);
        }

        // SM-first short-circuit + LLM fallback are both centralized in
        // `try_full_compact` — manual path just passes the trigger and
        // any custom instructions through. TS parity
        // (commands/compact/compact.ts:55-62): when custom instructions
        // are present we want the LLM path; `try_full_compact` already
        // skips SM in that case (see its branch).
        self.try_full_compact(
            history,
            event_tx,
            coco_types::CompactTrigger::Manual,
            custom_instructions,
        )
        .await;
    }

    /// Try the session-memory-first compact path. Returns `true` when SM
    /// produced a result and history was rewritten; `false` when the
    /// caller should fall through to LLM summarization.
    ///
    /// TS: services/compact/sessionMemoryCompact.ts:514 `trySessionMemoryCompaction`.
    /// The SM path bypasses PreCompact / PostCompact hooks (sessionMemoryCompact.ts:584
    /// only fires sessionStart hooks) — context recovery is already in
    /// the memory text itself.
    pub(crate) async fn try_session_memory_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        outer_trigger: coco_types::CompactTrigger,
    ) -> bool {
        // Wait for any in-flight extraction so we don't snapshot an
        // about-to-be-overwritten memory file. TS: waitForSessionMemoryExtraction
        // (sessionMemoryCompact.ts:527).
        if let Some(svc) = &self.session_memory_service {
            svc.wait_for_extraction().await;
        }
        // Same guard for the new auto-memory 9-section session
        // memory: if a SessionMemoryService extraction is in flight,
        // wait up to 15 s so the file on disk is settled before
        // compact reads it. Past 60 s (`STALE_THRESHOLD`) the call
        // returns false and we proceed — extraction is presumed
        // crashed.
        if let Some(runtime) = &self.memory_runtime {
            let _ = runtime
                .session_memory
                .wait_for_extraction(coco_memory::service::session::DEFAULT_WAIT_TIMEOUT)
                .await;
        }

        let memory_text = {
            let guard = self.session_memory_text.read().await;
            guard.clone()
        };
        if memory_text.trim().is_empty() {
            return false;
        }

        // Phase: pre-compact hooks → SM only fires SessionStart hooks
        // (TS sessionMemoryCompact.ts:583-585) since context recovery is
        // already in the memory text. Show a "Summarizing via session
        // memory" spinner so the user can tell which path is running.
        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::HooksStart,
                hook_type: Some(coco_types::CompactionHookType::SessionStart),
            }),
        )
        .await;

        // Build the SM compact config from the resolved settings, threading
        // the auto-compact threshold so compaction declines when the result
        // wouldn't actually shrink below the line.
        let sm_cfg = &self.config.compact.session_memory;
        let auto_threshold = coco_compact::auto_compact_threshold(
            self.config.context_window,
            self.config.max_output_tokens,
            &self.config.compact.auto,
        );
        let path_str = self
            .config_home
            .as_ref()
            .map(|p| format!("{}/session-memory/summary.md", p.display()));
        let sm_compact_cfg = coco_compact::SessionMemoryCompactConfig {
            min_tokens: sm_cfg.min_tokens,
            min_text_block_messages: sm_cfg.min_text_block_messages,
            max_tokens: sm_cfg.max_tokens,
            auto_compact_threshold: Some(auto_threshold),
            max_summary_chars: Some(sm_cfg.max_summary_chars as usize),
            session_memory_path: path_str,
        };

        // Read the boundary anchor (TS getLastSummarizedMessageId).
        // Prefer the service's value when installed — the extractor
        // writes it there on each successful extract. Fall back to the
        // engine-local Mutex for tests / SDK paths that bypass the
        // service. Sync the local cache so subsequent reads agree.
        let last_summarized = if let Some(svc) = &self.session_memory_service {
            let from_svc = svc.last_summarized_message_id().await;
            if let Some(uuid) = from_svc
                && let Ok(mut guard) = self.last_summarized_message_id.lock()
            {
                *guard = Some(uuid);
            }
            from_svc.or_else(|| self.last_summarized_message_id.lock().ok().and_then(|g| *g))
        } else {
            self.last_summarized_message_id.lock().ok().and_then(|g| *g)
        };

        let result = match coco_compact::compact_session_memory(
            &history.messages,
            &memory_text,
            last_summarized,
            &sm_compact_cfg,
        ) {
            Ok(Some(r)) => r,
            Ok(None) => return false, // SM declined; fall through to LLM.
            Err(e) => {
                warn!("session-memory compaction errored: {e}");
                return false;
            }
        };

        // Run SessionStart hooks BEFORE rewriting history so a hook that
        // emits `additional_context` user messages lands in the right
        // place. Failure is logged + tolerated — SM-compact still proceeds.
        // TS sessionMemoryCompact.ts:583 calls processSessionStartHooks('compact').
        let mut session_start_messages: Vec<coco_messages::Message> = Vec::new();
        if let Some(registry) = self.hooks.as_ref() {
            let ctx = self.orchestration_ctx();
            let model_id = self.config.model_id.as_str();
            let model_arg = if model_id.is_empty() {
                None
            } else {
                Some(model_id)
            };
            match coco_hooks::orchestration::execute_session_start(
                registry, &ctx, "compact", /*agent_type*/ None, model_arg,
            )
            .await
            {
                Ok(res) => {
                    for ctx_text in res.additional_contexts {
                        if !ctx_text.trim().is_empty() {
                            session_start_messages
                                .push(coco_messages::create_user_message(&ctx_text));
                        }
                    }
                }
                Err(e) => warn!("SessionStart hook execution failed: {e}"),
            }
        }

        info!(
            pre = result.pre_compact_tokens,
            post = result.post_compact_tokens,
            outer_trigger = ?outer_trigger,
            "session-memory compaction applied",
        );

        // FileReadState clear (same as the LLM path).
        if let Some(frs) = &self.file_read_state {
            let mut frs = frs.write().await;
            frs.clear();
        }

        // Update lastSummarizedMessageId to the *new* boundary anchor —
        // the last kept assistant message's uuid (or None when no
        // assistants survived). TS autoCompact.ts:296. Mirror to the
        // service so the next extraction sees the same anchor.
        let new_anchor = result
            .messages_to_keep
            .iter()
            .rev()
            .find(|m| matches!(m, coco_messages::Message::Assistant(_)))
            .and_then(|m| m.uuid())
            .copied();
        if let Ok(mut guard) = self.last_summarized_message_id.lock() {
            *guard = new_anchor;
        }
        if let Some(svc) = &self.session_memory_service {
            svc.set_last_summarized_message_id(new_anchor).await;
        }

        let summary_tokens = result.post_compact_tokens as i32;
        let pre_tokens = result.pre_compact_tokens;
        let post_tokens = result.post_compact_tokens;
        let mut new_messages = coco_compact::build_post_compact_messages(&result);
        // Append SessionStart-injected messages after the boundary so
        // they appear as fresh user context post-compaction.
        new_messages.extend(session_start_messages);
        history.messages = new_messages.clone();

        let _ = emit_protocol(
            event_tx,
            ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                removed_messages: 0,
                summary_tokens,
                trigger: coco_types::CompactTrigger::SessionMemory,
                pre_tokens: Some(pre_tokens),
                post_tokens: Some(post_tokens),
            }),
        )
        .await;

        // Notify post-compact observers (file caches, permissions, …).
        // `is_main_agent = config.agent_id.is_none()`: subagents must not
        // wipe main-thread DenialTracker / ToolAppState — those are
        // owned by the parent.
        let is_main_agent = self.config.agent_id.is_none();
        self.compaction_observers
            .notify_all(&result, is_main_agent)
            .await;
        self.compaction_observers
            .notify_post_compact(&new_messages)
            .await;
        // TS sessionMemoryCompact.ts:65 `notifyCompaction()`: reset the
        // cache-break baseline so the post-compact drop in cache_read
        // tokens doesn't false-positive as a break.
        let qs = self.query_source_label();
        self.client
            .notify_compaction(qs, self.config.agent_id.as_deref())
            .await;
        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::Done,
                hook_type: None,
            }),
        )
        .await;
        true
    }

    /// Attempt full LLM-summarized compaction.
    ///
    /// TS: `compactConversation()` — snapshot readFileState, clear it, call LLM
    /// to summarize old rounds, then re-inject recently read files.
    ///
    /// Sequence:
    /// 1. SM-first short-circuit (Auto path only — manual handled in
    ///    `run_manual_compact`). Returns immediately if SM produced a result.
    /// 2. PreCompact hooks (TS `executePreCompactHooks`) — collect any custom
    ///    instructions and merge into the summary prompt.
    /// 3. Snapshot + clear FileReadState; keep backup for failure restore.
    /// 4. Call `compact_conversation` with the LLM summarizer.
    /// 5. Notify CompactionObservers (TS `runPostCompactCleanup`).
    /// 6. PostCompact hooks (TS `executePostCompactHooks`).
    pub(crate) async fn try_full_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        trigger: coco_types::CompactTrigger,
        custom_instructions: Option<String>,
    ) {
        let trigger_label = match trigger {
            coco_types::CompactTrigger::Manual => "manual",
            coco_types::CompactTrigger::Auto => "auto",
            coco_types::CompactTrigger::Reactive => "reactive",
            coco_types::CompactTrigger::TimeBased => "time_based",
            coco_types::CompactTrigger::SessionMemory => "session_memory",
            coco_types::CompactTrigger::ContextCollapse => "context_collapse",
        };

        // 1. SM-first short-circuit. Auto always tries SM (autoCompact.ts:288);
        //    Manual tries SM only when the user gave no custom instructions
        //    (commands/compact/compact.ts:55-62) — with instructions the
        //    user wants the LLM to honor them, and SM can't.
        let can_try_sm = match trigger {
            coco_types::CompactTrigger::Auto => true,
            coco_types::CompactTrigger::Manual => custom_instructions.is_none(),
            _ => false,
        };
        if can_try_sm
            && self.config.compact.session_memory.enabled
            && self
                .try_session_memory_compact(history, event_tx, trigger)
                .await
        {
            return;
        }

        // Emit phase: HooksStart{PreCompact}. TS REPL.tsx:2502 maps this
        // to the "Running PreCompact hooks…" spinner.
        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::HooksStart,
                hook_type: Some(coco_types::CompactionHookType::PreCompact),
            }),
        )
        .await;

        // 1. PreCompact hooks. They may produce additional custom_instructions
        //    that get merged into the summary prompt, plus a userDisplayMessage
        //    for the TUI.
        let mut effective_instructions = custom_instructions.clone();
        let mut pre_display: Option<String> = None;
        if let Some(registry) = self.hooks.as_ref() {
            let ctx = self.orchestration_ctx();
            match coco_hooks::orchestration::execute_pre_compact(
                registry,
                &ctx,
                trigger_label,
                custom_instructions.as_deref(),
            )
            .await
            {
                Ok(res) => {
                    effective_instructions = coco_compact::merge_hook_instructions(
                        effective_instructions.as_deref(),
                        res.new_custom_instructions.as_deref(),
                    );
                    pre_display = res.user_display_message;
                }
                Err(e) => warn!("PreCompact hook execution failed: {e}"),
            }
        }

        // 2. Snapshot + clear FileReadState (TS: cacheToObject + readFileState.clear())
        let snapshot = if let Some(frs) = &self.file_read_state {
            let mut frs = frs.write().await;
            let snap = frs.snapshot_by_recency();
            frs.clear();
            snap
        } else {
            Vec::new()
        };
        // Keep a copy for restoration on failure.
        let snapshot_backup = snapshot.clone();

        // 2. Build the attachment callback that captures the snapshot.
        // TS: createPostCompactFileAttachments + createPlanAttachmentIfNeeded
        // + createPlanModeAttachmentIfNeeded + createAsyncAgentAttachmentsIfNeeded
        // + getInvokedSkillsForAgent (in-band skill re-injection).
        let cwd = std::env::current_dir().unwrap_or_default();
        let session_id = self.config.session_id.clone();
        let config_home = self.config_home.clone();
        let project_dir = self.config.project_dir.clone();
        let plans_directory_setting = self.config.plans_directory.clone();
        let captured_skills: Vec<coco_compact::PostCompactSkill> = self
            .post_compact_skills
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        // Plan-mode snapshot for `createPlanModeAttachmentIfNeeded`.
        // Read live permission mode from `ToolAppState` (Plan = in plan mode);
        // workflow / phase4_variant / agent counts come from QueryEngineConfig.
        let agent_id_for_attachments = self.config.agent_id.clone();
        let captured_plan_mode_snapshot: Option<coco_compact::PlanModeAttachment> = {
            let in_plan_mode = if let Some(state) = &self.app_state {
                let g = state.read().await;
                g.permission_mode == Some(coco_types::PermissionMode::Plan)
            } else {
                self.config.permission_mode == coco_types::PermissionMode::Plan
            };
            if !in_plan_mode {
                None
            } else {
                let pm = &self.config.plan_mode_settings;
                let workflow = match pm.workflow {
                    coco_config::PlanModeWorkflow::FivePhase => {
                        coco_context::PlanWorkflow::FivePhase
                    }
                    coco_config::PlanModeWorkflow::Interview => {
                        coco_context::PlanWorkflow::Interview
                    }
                };
                let phase4 = match pm.phase4_variant {
                    coco_config::PlanPhase4Variant::Standard => {
                        coco_context::Phase4Variant::Standard
                    }
                    coco_config::PlanPhase4Variant::Trim => coco_context::Phase4Variant::Trim,
                    coco_config::PlanPhase4Variant::Cut => coco_context::Phase4Variant::Cut,
                    coco_config::PlanPhase4Variant::Cap => coco_context::Phase4Variant::Cap,
                };
                let (plan_path, plan_exists_flag) =
                    match (config_home.as_deref(), session_id.as_str()) {
                        (Some(ch), sid) if !sid.is_empty() => {
                            let plans_dir = coco_context::resolve_plans_directory(
                                ch,
                                project_dir.as_deref(),
                                plans_directory_setting.as_deref(),
                            );
                            let path = coco_context::get_plan_file_path(
                                sid,
                                &plans_dir,
                                agent_id_for_attachments.as_deref(),
                            );
                            let exists = coco_context::plan_exists(
                                sid,
                                &plans_dir,
                                agent_id_for_attachments.as_deref(),
                            );
                            (path.display().to_string(), exists)
                        }
                        _ => (String::new(), false),
                    };
                Some(coco_compact::PlanModeAttachment {
                    reminder_type: coco_context::ReminderType::Full,
                    workflow,
                    phase4_variant: phase4,
                    explore_agent_count: pm.explore_agent_count,
                    plan_agent_count: pm.plan_agent_count,
                    is_sub_agent: agent_id_for_attachments.is_some(),
                    plan_file_path: plan_path,
                    plan_exists: plan_exists_flag,
                })
            }
        };
        // Async-agent snapshot for `createAsyncAgentAttachmentsIfNeeded`.
        // Read running TaskManager state and filter for unretrieved
        // local_agent tasks owned by another agent. TS: compact.ts:1568.
        let captured_async_agents = self.snapshot_async_agents_for_post_compact().await;
        // Snapshot recently @mentioned paths for priority restoration.
        // The closure runs synchronously inside `compact_conversation`, so
        // we read the lock now and move the resolved set in. Self-designed
        // augmentation; TS has no mention-aware re-injection.
        let prioritized_paths = self.recently_mentioned_paths_snapshot().await;
        let attachment_fn: coco_compact::compact::PostCompactAttachmentFn =
            Box::new(move |result: &coco_compact::CompactResult| {
                // Resolve plan file path for exclusion from file restore.
                let plan_file = config_home.as_ref().map(|ch| {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        project_dir.as_deref(),
                        plans_directory_setting.as_deref(),
                    );
                    coco_context::get_plan_file_path(
                        &session_id,
                        &plans_dir,
                        /*agent_id*/ None,
                    )
                });

                let mut atts = coco_compact::create_post_compact_file_attachments_with_priority(
                    &snapshot,
                    &result.messages_to_keep,
                    &cwd,
                    plan_file.as_deref(),
                    &prioritized_paths,
                );

                // TS: `createPlanAttachmentIfNeeded()` (`compact.ts:1470`)
                // — re-inject the plan file's content so it survives the
                // compaction boundary. Body uses the verbatim
                // `plan_file_reference` text template from
                // `messages.ts:3636-3642`.
                if let Some(ref ch) = config_home {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        project_dir.as_deref(),
                        plans_directory_setting.as_deref(),
                    );
                    let plan_path = coco_context::get_plan_file_path(
                        &session_id,
                        &plans_dir,
                        /*agent_id*/ None,
                    );
                    let plan_content =
                        coco_context::get_plan(&session_id, &plans_dir, /*agent_id*/ None);
                    if let Some(att) = coco_compact::create_plan_attachment_if_needed(
                        &plan_path,
                        plan_content.as_deref(),
                    ) {
                        atts.push(att);
                    }
                }

                // In-band skill re-injection. TS compact.ts calls
                // `getInvokedSkillsForAgent()` here so each invoked skill
                // surfaces both as a post-compact attachment AND in the
                // next-turn `<system-reminder>` (double-write for budget
                // resilience).
                atts.extend(coco_compact::create_post_compact_skill_attachments(
                    &captured_skills,
                ));

                // TS `createPlanModeAttachmentIfNeeded` (compact.ts:1542):
                // when the session is in plan mode at compact time, re-emit
                // `plan_mode` reminderType='full' so plan instructions land
                // on the FIRST post-compact turn rather than waiting for
                // the system-reminder cadence to next fire.
                if let Some(pm_attachment) = captured_plan_mode_snapshot.clone()
                    && let Some(att) = coco_compact::create_plan_mode_attachment_if_needed(
                        /*is_plan_mode*/ true,
                        pm_attachment,
                    )
                {
                    atts.push(att);
                }

                // TS `createAsyncAgentAttachmentsIfNeeded` (compact.ts:1568):
                // emit one `task_status` attachment per running
                // background agent so the model doesn't spawn duplicates
                // after compaction wipes the visible conversation.
                atts.extend(coco_compact::create_async_agent_attachments(
                    &captured_async_agents,
                ));

                atts
            });

        // Emit phase: Summarizing — TUI flips spinner to "Compacting conversation".
        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::Summarizing,
                hook_type: None,
            }),
        )
        .await;

        // 3. Build compact run-options. `custom_prompt` carries any
        //    instructions returned by PreCompact hooks merged with the
        //    user's `/compact <instructions>` argument.
        // Derive RecompactionInfo from the last-compact tracker. TS:
        // `compact.ts:317-323`. Auto-compact threshold mirrors the gate
        // we already evaluated above, so we recompute it here for the
        // analytics-aligned struct.
        let auto_threshold = coco_compact::auto_compact_threshold(
            self.config.context_window,
            self.config.max_output_tokens,
            &self.config.compact.auto,
        );
        let current_turn = self.turn_counter.load(std::sync::atomic::Ordering::Relaxed);
        let recompaction_info = self
            .last_compact_state
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .map(|prev| coco_compact::types::RecompactionInfo {
                is_recompaction: true,
                turns_since_previous: (current_turn - prev.turn_id).max(0) as i32,
                auto_compact_threshold: auto_threshold,
            });
        let compact_run_options = coco_compact::CompactRunOptions {
            context_window: self.config.context_window,
            trigger,
            custom_prompt: effective_instructions.clone(),
            recompaction_info,
            ..Default::default()
        };

        // 4. Call compact_conversation with LLM summarize callback
        let client = self.client.clone();
        let summarize_fn = |prompt: String| {
            let client = client.clone();
            async move {
                let params = QueryParams {
                    prompt: vec![LlmMessage::user_text(&prompt)],
                    max_tokens: Some(coco_compact::types::MAX_OUTPUT_TOKENS_FOR_SUMMARY),
                    thinking_level: None,
                    fast_mode: false,
                    tools: None,
                    // Compaction summarizer never carries context_edits —
                    // it's a forked agent, not the main thread.
                    context_management: None,
                    // Bypass cache-break detector: the summarizer prompt
                    // (compact instructions + truncated history) shares
                    // *no* cache prefix with main-thread turns. If we
                    // routed it through `compact` → `repl_main_thread`
                    // tracking, the summarizer would overwrite main's
                    // snapshot and the next main turn would falsely
                    // attribute "system prompt changed". Detection for
                    // the post-compact main turn is handled by the
                    // explicit `client.notify_compaction(...)` call site
                    // below.
                    query_source: None,
                    agent_id: None,
                    time_since_last_assistant_ms: None,
                    // Helper call (compaction summarizer) — skip the
                    // `claude-code-20250219` agentic baseline beta and
                    // cache strategy. Aligns with TS parity:
                    // summarizer prompts are forked, not part of the
                    // main agent loop.
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

        match coco_compact::compact_conversation(
            &history.messages,
            &compact_run_options,
            summarize_fn,
            Some(attachment_fn),
        )
        .await
        {
            Ok(mut result) => {
                info!(
                    pre = result.pre_compact_tokens,
                    post = result.post_compact_tokens,
                    "full compaction completed (trigger={trigger_label})"
                );

                // Carry any PreCompact userDisplayMessage forward so the
                // TUI can show it next to the boundary marker.
                if let Some(msg) = pre_display.as_ref() {
                    result.user_display_message = Some(match result.user_display_message {
                        Some(prev) => format!("{prev}\n{msg}"),
                        None => msg.clone(),
                    });
                }

                // PostCompact hooks. The hook payload's `compactSummary`
                // field receives the joined text of the summary messages
                // (TS uses `summary` from the LLM call directly).
                let summary_text = result
                    .summary_messages
                    .iter()
                    .filter_map(coco_compact::tokens::extract_message_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = emit_protocol(
                    event_tx,
                    ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                        phase: coco_types::CompactionPhase::HooksStart,
                        hook_type: Some(coco_types::CompactionHookType::PostCompact),
                    }),
                )
                .await;
                if let Some(registry) = self.hooks.as_ref() {
                    let ctx = self.orchestration_ctx();
                    match coco_hooks::orchestration::execute_post_compact(
                        registry,
                        &ctx,
                        trigger_label,
                        &summary_text,
                    )
                    .await
                    {
                        Ok(res) => {
                            if let Some(msg) = res.user_display_message {
                                result.user_display_message =
                                    Some(match result.user_display_message {
                                        Some(prev) => format!("{prev}\n{msg}"),
                                        None => msg,
                                    });
                            }
                        }
                        Err(e) => warn!("PostCompact hook execution failed: {e}"),
                    }
                }

                // TS `compact.ts:592` calls `processSessionStartHooks('compact')`
                // after the LLM-summarized path so user-defined SessionStart
                // hooks (settings.json) fire after compact. Hooks emit
                // `additional_contexts` strings; we render each non-empty
                // entry as a synthetic user message and fold into
                // `result.hook_results` so `build_post_compact_messages`
                // includes them in the new history.
                if let Some(registry) = self.hooks.as_ref() {
                    let ctx = self.orchestration_ctx();
                    let model_id = self.config.model_id.as_str();
                    let model_arg = if model_id.is_empty() {
                        None
                    } else {
                        Some(model_id)
                    };
                    match coco_hooks::orchestration::execute_session_start(
                        registry, &ctx, "compact", /*agent_type*/ None, model_arg,
                    )
                    .await
                    {
                        Ok(agg) => {
                            for ctx_text in agg.additional_contexts {
                                if !ctx_text.trim().is_empty() {
                                    result
                                        .hook_results
                                        .push(coco_messages::create_user_message(&ctx_text));
                                }
                            }
                        }
                        Err(e) => warn!("SessionStart hook execution failed (compact): {e}"),
                    }
                }

                // TS-aligned order: boundary, summaryMessages, messagesToKeep,
                // attachments, hookResults. Use the canonical helper.
                let summary_tokens = result.post_compact_tokens as i32;
                let new_messages = coco_compact::build_post_compact_messages(&result);
                history.messages = new_messages.clone();

                // Record the successful compaction for the next turn's
                // `RecompactionInfo`. TS: `compact.ts:317` chain
                // tracking. Run id = boundary uuid for transcript-aligned
                // observability.
                let run_id = result.boundary_marker.uuid().copied().unwrap_or_default();
                if let Ok(mut guard) = self.last_compact_state.lock() {
                    *guard = Some(crate::engine::LastCompactState {
                        turn_id: self.turn_counter.load(std::sync::atomic::Ordering::Relaxed),
                        run_id: run_id.to_string(),
                    });
                }

                // TS `runPostCompactCleanup`: notify each registered observer
                // so per-crate caches (file/memory/skill state) drop their
                // pre-compact entries. `is_main_agent = agent_id.is_none()`:
                // subagent compactions must not wipe main-thread state.
                let is_main_agent = self.config.agent_id.is_none();
                self.compaction_observers
                    .notify_all(&result, is_main_agent)
                    .await;
                self.compaction_observers
                    .notify_post_compact(&new_messages)
                    .await;
                // TS compact.ts:699 / commands/compact/compact.ts:68
                // `notifyCompaction(query_source, agent_id)`. After full
                // LLM compaction the message list is rewritten; the new
                // baseline must not be compared against pre-compact
                // cache_read tokens.
                let qs = self.query_source_label();
                self.client
                    .notify_compaction(qs, self.config.agent_id.as_deref())
                    .await;

                let _delivered = emit_protocol(
                    event_tx,
                    ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                        removed_messages: 0,
                        summary_tokens,
                        trigger,
                        pre_tokens: Some(result.pre_compact_tokens),
                        post_tokens: Some(result.post_compact_tokens),
                    }),
                )
                .await;
                let _ = emit_protocol(
                    event_tx,
                    ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                        phase: coco_types::CompactionPhase::Done,
                        hook_type: None,
                    }),
                )
                .await;
            }
            Err(e) => {
                warn!("full compaction failed: {e}");
                let _ = emit_protocol(
                    event_tx,
                    ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                        phase: coco_types::CompactionPhase::Done,
                        hook_type: None,
                    }),
                )
                .await;
                // Restore FileReadState from backup so dedup/changed-file
                // detection continues to work after a failed compact attempt.
                if let Some(frs) = &self.file_read_state {
                    let mut frs = frs.write().await;
                    for (path, entry) in snapshot_backup {
                        frs.set(path, entry);
                    }
                }
            }
        }
    }
}
