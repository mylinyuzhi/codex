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

use std::collections::HashMap;
use std::collections::HashSet;

use tracing::info;
use tracing::warn;

use coco_inference::QueryParams;
use coco_messages::AssistantContent;
use coco_messages::LlmMessage;
use coco_messages::Message;
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
            history.with_owned_messages(|msgs| {
                coco_compact::micro_compact(msgs, micro_keep);
            });
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

    pub async fn run_partial_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        pivot_index: usize,
        direction: coco_messages::PartialCompactDirection,
        user_feedback: Option<String>,
        custom_instructions: Option<String>,
    ) -> coco_compact::CompactOutcome {
        let snapshot = if let Some(frs) = &self.file_read_state {
            let frs = frs.read().await;
            frs.snapshot_by_recency()
        } else {
            Vec::new()
        };
        let captured_skills = self
            .post_compact_skills
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let captured_async_agents = self.snapshot_async_agents_for_post_compact().await;
        let captured_plan_mode_snapshot = self.snapshot_plan_mode_attachment().await;
        let prioritized_paths = self.recently_mentioned_paths_snapshot().await;

        let hook_trigger = coco_hooks::orchestration::CompactTrigger::Manual;
        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::HooksStart,
                hook_type: Some(coco_types::CompactionHookType::PreCompact),
            }),
        )
        .await;

        let mut effective_instructions = custom_instructions.clone();
        let mut pre_display: Option<String> = None;
        if let Some(registry) = self.hooks.as_ref() {
            let ctx = self.orchestration_ctx();
            match coco_hooks::orchestration::execute_pre_compact(
                registry,
                &ctx,
                hook_trigger,
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
                Err(e) => warn!("PreCompact hook execution failed (partial compact): {e}"),
            }
        }

        let _ = emit_protocol(
            event_tx,
            ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                phase: coco_types::CompactionPhase::Summarizing,
                hook_type: None,
            }),
        )
        .await;

        let summarize_fn = |attempt: coco_compact::CompactSummaryAttempt| async move {
            self.run_compact_summary_attempt(attempt).await
        };
        let result = coco_compact::partial_compact_conversation(
            history.as_slice(),
            pivot_index,
            direction,
            user_feedback.as_deref(),
            effective_instructions.as_deref(),
            summarize_fn,
            None,
        )
        .await;

        match result {
            Ok(mut result) => {
                if let Some(msg) = pre_display.as_ref() {
                    result.user_display_message = Some(match result.user_display_message {
                        Some(prev) => format!("{prev}\n{msg}"),
                        None => msg.clone(),
                    });
                }

                let fallback_summary = result
                    .summary_messages
                    .iter()
                    .filter_map(coco_compact::tokens::extract_message_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let summary_text = result.raw_summary.as_deref().unwrap_or(&fallback_summary);
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
                        hook_trigger,
                        summary_text,
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
                        Err(e) => warn!("PostCompact hook execution failed (partial compact): {e}"),
                    }
                }

                let cwd = std::env::current_dir().unwrap_or_default();
                let plan_file = self.config_home.as_ref().map(|ch| {
                    let plans_dir = coco_context::resolve_plans_directory(
                        ch,
                        self.config.project_dir.as_deref(),
                        self.config.plans_directory.as_deref(),
                    );
                    coco_context::get_plan_file_path(
                        &self.config.session_id,
                        &plans_dir,
                        /*agent_id*/ None,
                    )
                });
                result.attachments.extend(
                    coco_compact::create_post_compact_file_attachments_with_priority(
                        &snapshot,
                        &result.messages_to_keep,
                        &cwd,
                        plan_file.as_deref(),
                        &prioritized_paths,
                    ),
                );
                if let Some(att) = self.create_current_plan_attachment() {
                    result.attachments.push(att);
                }
                result
                    .attachments
                    .extend(coco_compact::create_post_compact_skill_attachments(
                        &captured_skills,
                    ));
                if let Some(pm) = captured_plan_mode_snapshot
                    && let Some(att) = coco_compact::create_plan_mode_attachment_if_needed(true, pm)
                {
                    result.attachments.push(att);
                }
                result
                    .attachments
                    .extend(coco_compact::create_async_agent_attachments(
                        &captured_async_agents,
                    ));

                if let Some(registry) = self.hooks.as_ref() {
                    let _ = emit_protocol(
                        event_tx,
                        ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                            phase: coco_types::CompactionPhase::HooksStart,
                            hook_type: Some(coco_types::CompactionHookType::SessionStart),
                        }),
                    )
                    .await;
                    result.hook_results.extend(
                        self.compact_session_start_hook_messages(registry, "partial compact")
                            .await,
                    );
                }

                let (delta_attachments, delta_state) = self
                    .create_post_compact_delta_attachments(&result.messages_to_keep)
                    .await;
                result.attachments.extend(delta_attachments);
                let new_messages =
                    coco_compact::build_partial_post_compact_messages(&result, direction);
                let pre_len = history.len() as i32;
                let post_len = new_messages.len() as i32;
                let removed_messages = (pre_len - post_len).max(0);
                // I-1 (Authority): partial compaction rewrites the
                // engine-authoritative history. Pair the swap with a
                // `MessageTruncated { 0 }` + per-message
                // `MessageAppended` burst so the TUI's TranscriptView
                // and SDK observers see the new state.
                crate::history_sync::history_replace_and_emit(
                    history,
                    new_messages.clone(),
                    event_tx,
                )
                .await;
                self.update_post_compact_delta_state(delta_state).await;
                if let Some(frs) = &self.file_read_state {
                    let mut frs = frs.write().await;
                    frs.clear();
                }
                let is_main_agent = self.config.agent_id.is_none();
                self.compaction_observers
                    .notify_all(&result, is_main_agent)
                    .await;
                self.compaction_observers
                    .notify_post_compact(&new_messages)
                    .await;
                let qs = self.query_source_label();
                self.client
                    .notify_compaction(qs, self.config.agent_id.as_deref())
                    .await;
                let _ = emit_protocol(
                    event_tx,
                    ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                        removed_messages,
                        summary_tokens: result.post_compact_tokens as i32,
                        trigger: coco_types::CompactTrigger::Manual,
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
                self.pending_just_compacted
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                coco_compact::CompactOutcome::Applied
            }
            Err(e) => {
                warn!("partial compaction failed: {e}");
                let _ = emit_protocol(
                    event_tx,
                    ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                        phase: coco_types::CompactionPhase::Done,
                        hook_type: None,
                    }),
                )
                .await;
                coco_compact::CompactOutcome::Failed
            }
        }
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
        // Wait for any in-flight forked-agent extraction so we don't
        // snapshot an about-to-be-overwritten memory file. TS:
        // waitForSessionMemoryExtraction (sessionMemoryCompact.ts:527).
        // Past `STALE_THRESHOLD` (60s) the call returns false and we
        // proceed — extraction is presumed crashed.
        if let Some(svc) = &self.session_memory_service {
            let _ = svc
                .wait_for_extraction(coco_memory::service::session::DEFAULT_WAIT_TIMEOUT)
                .await;
        }

        // Prefer the service's cached body — refreshed inside
        // `run_with_label` after each successful extract. Falls
        // back to the engine-local text (legacy / test path) when
        // the service isn't wired.
        let memory_text = if let Some(svc) = &self.session_memory_service {
            svc.current_text().await
        } else {
            self.session_memory_text.read().await.clone()
        };
        if memory_text.trim().is_empty() {
            return false;
        }

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
            let from_svc = svc.last_summarized_message_uuid().await;
            if let Some(uuid) = from_svc
                && let Ok(mut guard) = self.last_summarized_message_id.lock()
            {
                *guard = Some(uuid);
            }
            from_svc.or_else(|| self.last_summarized_message_id.lock().ok().and_then(|g| *g))
        } else {
            self.last_summarized_message_id.lock().ok().and_then(|g| *g)
        };

        let mut result = match coco_compact::compact_session_memory(
            history.as_slice(),
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

        // Run SessionStart hooks and insert their rendered attachment
        // messages into the rewritten history. We collect events directly
        // instead of also pushing them to the next-turn sync buffer, so
        // compact output is not delivered twice.
        if let Some(registry) = self.hooks.as_ref() {
            let _ = emit_protocol(
                event_tx,
                ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                    phase: coco_types::CompactionPhase::HooksStart,
                    hook_type: Some(coco_types::CompactionHookType::SessionStart),
                }),
            )
            .await;
            result.hook_results.extend(
                self.compact_session_start_hook_messages(registry, "session-memory compact")
                    .await,
            );
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
            .find(|m| matches!(m.as_ref(), coco_messages::Message::Assistant(_)))
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
        if let Some(att) = self.create_current_plan_attachment() {
            result.attachments.push(att);
        }
        let (delta_attachments, delta_state) = self
            .create_post_compact_delta_attachments(&result.messages_to_keep)
            .await;
        result.attachments.extend(delta_attachments);
        let new_messages = coco_compact::build_post_compact_messages(&result);
        let pre_len = history.len() as i32;
        let post_len = new_messages.len() as i32;
        let removed_messages = (pre_len - post_len).max(0);
        // I-1 (Authority): session-memory compaction rewrites history.
        // Emit truncate + appended-burst so the TUI/SDK derived views
        // converge on the new state.
        crate::history_sync::history_replace_and_emit(history, new_messages.clone(), event_tx)
            .await;
        self.update_post_compact_delta_state(delta_state).await;

        let _ = emit_protocol(
            event_tx,
            ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                removed_messages,
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
        // Surface task_status reminders on the next turn (TS post-compact
        // emission gate at `attachments.ts:962`).
        self.pending_just_compacted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        true
    }

    async fn run_compact_summary_attempt(
        &self,
        attempt: coco_compact::CompactSummaryAttempt,
    ) -> Result<coco_compact::CompactSummaryResponse, String> {
        if self.cancel.is_cancelled() {
            return Err("compact_summary_aborted: cancelled".to_string());
        }

        if let Some(dispatcher) = self.fork_dispatcher.clone() {
            let mut cache = self.last_cache_safe_params().await.unwrap_or_else(|| {
                coco_types::CacheSafeParams {
                    rendered_system_prompt: self.config.system_prompt.clone().unwrap_or_default(),
                    model_id: self.config.model_id.clone(),
                    provider: self.client.provider().to_string(),
                    prompt_cache: self.config.prompt_cache.clone(),
                    fork_context_messages: Vec::new(),
                }
            });
            // `CompactSummaryAttempt.context_messages` is already
            // `Vec<Arc<Message>>` — Arc-share into the fork context.
            cache.fork_context_messages = attempt.context_messages.clone();

            let mut options =
                crate::forked_agent::ForkedAgentOptions::for_label(coco_types::ForkLabel::Compact);
            options.can_use_tool = Some(crate::forked_agent::deny_all_handle(
                "compact summary: tools disabled",
            ));
            options.require_can_use_tool = true;
            options.overrides.abort = Some(self.cancel.clone());

            match dispatcher
                .dispatch(&cache, &options, &attempt.summary_request, None)
                .await
            {
                Ok(result) => {
                    match extract_compact_summary_from_messages(&result.messages, &self.cancel) {
                        Ok(summary) => {
                            return Ok(coco_compact::CompactSummaryResponse { summary });
                        }
                        Err(e) => {
                            warn!("compact fork returned unusable summary: {e}");
                        }
                    }
                }
                Err(e) => {
                    warn!("compact fork failed, falling back to direct no-tools call: {e}");
                }
            }
        }

        self.run_direct_compact_summary_attempt(attempt).await
    }

    async fn compact_session_start_hook_messages(
        &self,
        registry: &coco_hooks::HookRegistry,
        context_label: &str,
    ) -> Vec<Message> {
        let ctx = self.orchestration_ctx();
        let model_id = self.config.model_id.as_str();
        let model_arg = if model_id.is_empty() {
            None
        } else {
            Some(model_id)
        };
        match coco_hooks::orchestration::execute_session_start_collect_events(
            registry,
            &ctx,
            coco_hooks::orchestration::SessionStartSource::Compact,
            /*agent_type*/ None,
            model_arg,
        )
        .await
        {
            Ok(result) => {
                let effects = crate::session_start_hooks::SessionStartHookSideEffects::from(
                    &result.aggregate,
                );
                if let Some(sink) = &self.session_start_hook_side_effect_sink {
                    sink.handle_session_start_hook_side_effects(effects.clone())
                        .await;
                }

                let mut messages = self.render_session_start_hook_events(result.events).await;
                if let Some(initial) = effects.initial_user_message {
                    messages.push(coco_messages::create_user_message(&initial));
                }
                messages
            }
            Err(e) => {
                warn!("SessionStart hook execution failed ({context_label}): {e}");
                Vec::new()
            }
        }
    }

    async fn render_session_start_hook_events(
        &self,
        events: Vec<coco_system_reminder::HookEvent>,
    ) -> Vec<Message> {
        if events.is_empty() {
            return Vec::new();
        }

        let ctx = coco_system_reminder::GeneratorContextBuilder::new(&self.config.system_reminder)
            .hook_events(events)
            .build();
        let mut reminders = Vec::new();
        for generated in [
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::HookSuccessGenerator,
                &ctx,
            )
            .await,
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::HookBlockingErrorGenerator,
                &ctx,
            )
            .await,
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::HookAdditionalContextGenerator,
                &ctx,
            )
            .await,
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::HookStoppedContinuationGenerator,
                &ctx,
            )
            .await,
        ] {
            match generated {
                Ok(Some(reminder)) => reminders.push(reminder),
                Ok(None) => {}
                Err(e) => warn!("compact session-start hook reminder generation failed: {e}"),
            }
        }

        // Compact-side reminders go into a scratch vector (no
        // MessageHistory yet); event emission is not relevant here
        // because the engine hasn't started the next turn. Just
        // collect the materialized model-visible messages.
        coco_system_reminder::inject_reminders(reminders).model_visible
    }

    async fn run_direct_compact_summary_attempt(
        &self,
        attempt: coco_compact::CompactSummaryAttempt,
    ) -> Result<coco_compact::CompactSummaryResponse, String> {
        if self.cancel.is_cancelled() {
            return Err("compact_summary_aborted: cancelled".to_string());
        }

        let mut prompt = coco_messages::normalize_messages_for_api(&attempt.context_messages);
        prompt.push(LlmMessage::user_text(&attempt.summary_request));
        let params = QueryParams {
            prompt,
            max_tokens: Some(attempt.max_summary_tokens),
            thinking_level: None,
            fast_mode: false,
            tools: None,
            context_management: None,
            query_source: None,
            agent_id: None,
            time_since_last_assistant_ms: None,
            agentic: false,
            cache: None,
            stop_sequences: None,
        };

        match self.client.query(&params).await {
            Ok(result) => {
                let stop = result.stop_reason;
                let stop_abnormal = stop.is_some_and(coco_messages::StopReason::is_abnormal);
                // TS parity (`services/compact/compact.ts:493-515`): a
                // truncated / content-filtered / refused summary is
                // unusable — it would silently contaminate every
                // subsequent turn with partial XML. Match TS's
                // `throw new Error('Failed to generate conversation
                // summary…')` by returning an `Err` whose message
                // carries the `compact_summary_aborted:` prefix; the
                // upper layer at `coco_compact::compact.rs:898-902`
                // routes this prefix into `CompactError::LlmCallFailed`,
                // which the user sees as "Error compacting conversation".
                // Multi-provider note: the Anthropic stream layer in TS
                // (`services/api/claude.ts:2266`) already converts
                // `max_tokens` into a synthetic API-error message —
                // coco-rs runs across providers that don't do that
                // transform, so the side-fork caller has to defend
                // itself by inspecting `stop_reason` directly here.
                if stop_abnormal {
                    warn!(
                        stop_reason = ?stop,
                        tokens_out = result.usage.output_tokens,
                        "compaction aborted: non-normal stop_reason — \
                         dropping truncated summary to avoid contaminating future turns"
                    );
                    return Err(format!(
                        "compact_summary_aborted: model stopped with stop_reason={} \
                         (truncated or filtered summary discarded)",
                        stop.map(coco_messages::StopReason::as_wire_str)
                            .unwrap_or("unknown")
                    ));
                }
                let summary_res = extract_compact_summary_from_content(&result.content);
                if summary_res.is_err() {
                    warn!(
                        stop_reason = ?stop,
                        tokens_out = result.usage.output_tokens,
                        "compaction summary parse failed — XML extractor rejected response"
                    );
                }
                let summary = summary_res?;
                Ok(coco_compact::CompactSummaryResponse { summary })
            }
            Err(e) => Err(e.to_string()),
        }
    }

    async fn create_post_compact_delta_attachments<M: std::borrow::Borrow<Message>>(
        &self,
        preserved_history: &[M],
    ) -> (Vec<coco_messages::AttachmentMessage>, PostCompactDeltaState) {
        let app_state_snapshot = match &self.app_state {
            Some(state) => state.read().await.clone(),
            None => coco_types::ToolAppState::default(),
        };

        let (current_loaded_tools, current_deferred_tools) =
            self.current_tool_search_partitions(&app_state_snapshot);
        let current_agents = self
            .session_bootstrap
            .as_ref()
            .map(|b| b.agents.clone())
            .unwrap_or_default();
        let source_timeout =
            std::time::Duration::from_millis(if self.config.system_reminder.timeout_ms > 0 {
                self.config.system_reminder.timeout_ms as u64
            } else {
                coco_system_reminder::DEFAULT_TIMEOUT_MS as u64
            });
        let materialized = self
            .reminder_sources
            .materialize(coco_system_reminder::MaterializeContext {
                config: &self.config.system_reminder,
                agent_id: self.config.agent_id.as_deref(),
                user_input: None,
                mentioned_paths: &[],
                recent_tools: &[],
                just_compacted: true,
                per_source_timeout: source_timeout,
            })
            .await;
        let current_mcp_instructions = materialized.mcp_instructions_current;

        let baseline_tools = if preserved_contains_attachment_kind(
            preserved_history,
            coco_types::AttachmentKind::DeferredToolsDelta,
        ) {
            app_state_snapshot.last_announced_tools.clone()
        } else {
            HashSet::new()
        };
        let baseline_agents = if preserved_contains_attachment_kind(
            preserved_history,
            coco_types::AttachmentKind::AgentListingDelta,
        ) {
            app_state_snapshot.last_announced_agents.clone()
        } else {
            HashSet::new()
        };
        let baseline_mcp = if preserved_contains_attachment_kind(
            preserved_history,
            coco_types::AttachmentKind::McpInstructionsDelta,
        ) {
            app_state_snapshot.last_announced_mcp_instructions.clone()
        } else {
            HashMap::new()
        };

        let deferred_delta = crate::engine_helpers::compute_tools_delta(
            &current_deferred_tools,
            &current_loaded_tools,
            &baseline_tools,
        );
        let agent_delta =
            crate::engine_helpers::compute_agents_delta(&current_agents, &baseline_agents);
        let mcp_delta = crate::engine_helpers::compute_mcp_instructions_delta(
            &current_mcp_instructions,
            &baseline_mcp,
        );

        let ctx = coco_system_reminder::GeneratorContextBuilder::new(&self.config.system_reminder)
            .deferred_tools_delta(deferred_delta)
            .agent_listing_delta(agent_delta)
            .mcp_instructions_delta(mcp_delta)
            .build();
        let mut reminders = Vec::new();
        for generated in [
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::DeferredToolsDeltaGenerator,
                &ctx,
            )
            .await,
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::AgentListingDeltaGenerator,
                &ctx,
            )
            .await,
            coco_system_reminder::AttachmentGenerator::generate(
                &coco_system_reminder::McpInstructionsDeltaGenerator,
                &ctx,
            )
            .await,
        ] {
            match generated {
                Ok(Some(reminder)) => reminders.push(reminder),
                Ok(None) => {}
                Err(e) => warn!("post-compact delta reminder generation failed: {e}"),
            }
        }

        let batch = coco_system_reminder::inject_reminders(reminders);
        let mut attachments = Vec::new();
        for message in batch.model_visible {
            if let Message::Attachment(att) = message {
                attachments.push(att);
            }
        }

        let state = PostCompactDeltaState {
            current_deferred_tools,
            current_agents,
            current_mcp_instructions,
        };
        (attachments, state)
    }

    fn current_tool_search_partitions(
        &self,
        app_state: &coco_types::ToolAppState,
    ) -> (Vec<String>, Vec<String>) {
        let discovered = std::sync::Arc::new(app_state.discovered_tool_names.clone());
        let supports_tool_reference = self.client.model_info().is_some_and(|info| {
            info.has_capability(coco_types::Capability::ServerSideToolReference)
        });
        let supports_client_side_tool_search = self
            .client
            .model_info()
            .is_some_and(|info| info.has_capability(coco_types::Capability::ClientSideToolSearch));
        let stub_ctx = coco_tool_runtime::ToolUseContext::stub_for_filtering(
            self.config.features.clone(),
            self.config.tool_overrides.clone(),
            self.config.tool_filter.clone(),
            self.config.permission_mode,
        )
        .with_discovered_tool_names(discovered)
        .with_model_capabilities(supports_tool_reference, supports_client_side_tool_search);
        let loaded = self
            .tools
            .loaded_tools(&stub_ctx)
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        let deferred = self
            .tools
            .deferred_tools(&stub_ctx)
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        (loaded, deferred)
    }

    async fn update_post_compact_delta_state(&self, delta_state: PostCompactDeltaState) {
        let Some(app_state) = &self.app_state else {
            return;
        };
        let mut guard = app_state.write().await;
        guard.last_announced_tools = delta_state.current_deferred_tools.into_iter().collect();
        guard.last_announced_agents = delta_state.current_agents.into_iter().collect();
        guard.last_announced_mcp_instructions = delta_state.current_mcp_instructions;
    }

    fn create_current_plan_attachment(&self) -> Option<coco_messages::AttachmentMessage> {
        let ch = self.config_home.as_ref()?;
        let plans_dir = coco_context::resolve_plans_directory(
            ch,
            self.config.project_dir.as_deref(),
            self.config.plans_directory.as_deref(),
        );
        let plan_path = coco_context::get_plan_file_path(
            &self.config.session_id,
            &plans_dir,
            /*agent_id*/ None,
        );
        let plan_content =
            coco_context::get_plan(&self.config.session_id, &plans_dir, /*agent_id*/ None);
        coco_compact::create_plan_attachment_if_needed(&plan_path, plan_content.as_deref())
    }

    async fn snapshot_plan_mode_attachment(&self) -> Option<coco_compact::PlanModeAttachment> {
        let in_plan_mode = if let Some(state) = &self.app_state {
            let g = state.read().await;
            g.permission_mode == Some(coco_types::PermissionMode::Plan)
        } else {
            self.config.permission_mode == coco_types::PermissionMode::Plan
        };
        if !in_plan_mode {
            return None;
        }

        let workflow = match self.config.plan_mode_settings.workflow {
            coco_config::PlanModeWorkflow::FivePhase => coco_context::PlanWorkflow::FivePhase,
            coco_config::PlanModeWorkflow::Interview => coco_context::PlanWorkflow::Interview,
        };
        let phase4 = match self.config.plan_mode_settings.phase4_variant {
            coco_config::PlanPhase4Variant::Standard => coco_context::Phase4Variant::Standard,
            coco_config::PlanPhase4Variant::Trim => coco_context::Phase4Variant::Trim,
            coco_config::PlanPhase4Variant::Cut => coco_context::Phase4Variant::Cut,
            coco_config::PlanPhase4Variant::Cap => coco_context::Phase4Variant::Cap,
        };
        let (plan_file_path, plan_exists) =
            match (self.config_home.as_deref(), self.config.session_id.as_str()) {
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
                    let exists =
                        coco_context::plan_exists(sid, &plans_dir, self.config.agent_id.as_deref());
                    (path.display().to_string(), exists)
                }
                _ => (String::new(), false),
            };

        Some(coco_compact::PlanModeAttachment {
            reminder_type: coco_context::ReminderType::Full,
            workflow,
            phase4_variant: phase4,
            explore_agent_count: self.config.plan_mode_settings.explore_agent_count,
            plan_agent_count: self.config.plan_mode_settings.plan_agent_count,
            is_sub_agent: self.config.agent_id.is_some(),
            plan_file_path,
            plan_exists,
        })
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
    /// 3. Snapshot FileReadState; clear it only after summary success.
    /// 4. Call `compact_conversation` with the LLM summarizer.
    /// 5. Notify CompactionObservers (TS `runPostCompactCleanup`).
    /// 6. PostCompact hooks (TS `executePostCompactHooks`).
    #[tracing::instrument(
        skip_all,
        name = "compaction",
        fields(
            trigger = ?trigger,
            session_id = %self.config.session_id,
            history_len = history.len(),
            has_custom_instructions = custom_instructions.is_some(),
        ),
    )]
    pub(crate) async fn try_full_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        trigger: coco_types::CompactTrigger,
        custom_instructions: Option<String>,
    ) -> coco_compact::CompactOutcome {
        let trigger_label = match trigger {
            coco_types::CompactTrigger::Manual => "manual",
            coco_types::CompactTrigger::Auto => "auto",
            coco_types::CompactTrigger::Reactive => "reactive",
            coco_types::CompactTrigger::TimeBased => "time_based",
            coco_types::CompactTrigger::SessionMemory => "session_memory",
            coco_types::CompactTrigger::ContextCollapse => "context_collapse",
        };
        info!(trigger = trigger_label, "try_full_compact entered");
        // TS hook schema's `trigger` is `enum('manual','auto')` — only
        // those two values are valid on the wire. Coco-rs-only triggers
        // (Reactive / TimeBased / SessionMemory / ContextCollapse) all
        // map to `Auto` for the hook payload (they are autonomous
        // compaction events from the agent's perspective).
        let hook_trigger = match trigger {
            coco_types::CompactTrigger::Manual => coco_hooks::orchestration::CompactTrigger::Manual,
            _ => coco_hooks::orchestration::CompactTrigger::Auto,
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
            return coco_compact::CompactOutcome::Applied;
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
                hook_trigger,
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

        // 2. Snapshot FileReadState. Clear it only after summary success
        // so a failed compact attempt leaves read-file dedup state intact.
        let snapshot = if let Some(frs) = &self.file_read_state {
            let frs = frs.read().await;
            frs.snapshot_by_recency()
        } else {
            Vec::new()
        };

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

        // 4. Call compact_conversation with the query-level summary executor.
        // It prefers a cache-sharing compact fork and falls back to a
        // no-tools structured direct call when no dispatcher is installed.
        let summarize_fn = |attempt: coco_compact::CompactSummaryAttempt| async move {
            self.run_compact_summary_attempt(attempt).await
        };

        match coco_compact::compact_conversation(
            history.as_slice(),
            &compact_run_options,
            summarize_fn,
            Some(attachment_fn),
        )
        .await
        {
            Ok(mut result) => {
                if result.summary_messages.is_empty() {
                    let _ = emit_protocol(
                        event_tx,
                        ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                            phase: coco_types::CompactionPhase::Done,
                            hook_type: None,
                        }),
                    )
                    .await;
                    return coco_compact::CompactOutcome::Skipped;
                }

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

                // PostCompact hooks. TS passes the raw LLM summary
                // before it is wrapped in continuation boilerplate.
                let fallback_summary = result
                    .summary_messages
                    .iter()
                    .filter_map(coco_compact::tokens::extract_message_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let summary_text = result.raw_summary.as_deref().unwrap_or(&fallback_summary);
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
                        hook_trigger,
                        summary_text,
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
                // after the LLM-summarized path. We render those hook
                // events into the rewritten history directly so they are
                // not also emitted by the next-turn sync reminder buffer.
                if let Some(registry) = self.hooks.as_ref() {
                    let _ = emit_protocol(
                        event_tx,
                        ServerNotification::CompactionPhase(coco_types::CompactionPhaseParams {
                            phase: coco_types::CompactionPhase::HooksStart,
                            hook_type: Some(coco_types::CompactionHookType::SessionStart),
                        }),
                    )
                    .await;
                    result.hook_results.extend(
                        self.compact_session_start_hook_messages(registry, "compact")
                            .await,
                    );
                }

                let (delta_attachments, delta_state) = self
                    .create_post_compact_delta_attachments::<std::sync::Arc<Message>>(&[])
                    .await;
                result.attachments.extend(delta_attachments);

                // TS-aligned order: boundary, summaryMessages, messagesToKeep,
                // attachments, hookResults. Use the canonical helper.
                let summary_tokens = result.post_compact_tokens as i32;
                let new_messages = coco_compact::build_post_compact_messages(&result);
                let pre_len = history.len() as i32;
                let post_len = new_messages.len() as i32;
                let removed_messages = (pre_len - post_len).max(0);
                // I-1 (Authority): full LLM compaction rewrites the
                // engine-authoritative history. Pair the swap with a
                // `MessageTruncated { 0 }` + per-message
                // `MessageAppended` burst so the TUI/SDK derived views
                // track the new state.
                crate::history_sync::history_replace_and_emit(
                    history,
                    new_messages.clone(),
                    event_tx,
                )
                .await;
                self.update_post_compact_delta_state(delta_state).await;

                if let Some(frs) = &self.file_read_state {
                    let mut frs = frs.write().await;
                    frs.clear();
                }

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
                        removed_messages,
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
                // Surface task_status reminders on the next turn (TS
                // post-compact emission gate at `attachments.ts:962`).
                self.pending_just_compacted
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                coco_compact::CompactOutcome::Applied
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
                coco_compact::CompactOutcome::Failed
            }
        }
    }
}

struct PostCompactDeltaState {
    current_deferred_tools: Vec<String>,
    current_agents: Vec<String>,
    current_mcp_instructions: HashMap<String, String>,
}

fn preserved_contains_attachment_kind<M: std::borrow::Borrow<Message>>(
    messages: &[M],
    kind: coco_types::AttachmentKind,
) -> bool {
    messages
        .iter()
        .any(|m| matches!(m.borrow(), Message::Attachment(att) if att.kind == kind))
}

fn extract_compact_summary_from_messages(
    messages: &[std::sync::Arc<Message>],
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, String> {
    if cancel.is_cancelled() {
        return Err("compact_summary_aborted: cancelled".to_string());
    }

    let mut chunks = Vec::new();
    for message in messages {
        let Message::Assistant(assistant) = message.as_ref() else {
            continue;
        };
        if let Some(api_error) = &assistant.api_error {
            return Err(format!(
                "compact_summary_invalid: assistant API error: {}",
                api_error.message
            ));
        }
        let LlmMessage::Assistant { content, .. } = &assistant.message else {
            continue;
        };
        chunks.push(extract_compact_summary_from_content(content)?);
    }

    Ok(chunks.join("\n"))
}

fn extract_compact_summary_from_content(content: &[AssistantContent]) -> Result<String, String> {
    let mut chunks = Vec::new();
    for c in content {
        match c {
            AssistantContent::Text(t) if !t.text.is_empty() => chunks.push(t.text.clone()),
            AssistantContent::ToolCall(tc) => {
                return Err(format!(
                    "compact_summary_invalid: summary attempted tool call {}",
                    tc.tool_name
                ));
            }
            _ => {}
        }
    }
    Ok(chunks.join("\n"))
}

#[cfg(test)]
#[path = "engine_compaction.test.rs"]
mod tests;
