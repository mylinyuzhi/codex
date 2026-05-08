//! Per-turn tail of [`QueryEngine::run_session_loop`] + reactive recovery.
//!
//! Owns:
//! - [`QueryEngine::finalize_turn_post_tools`] — the tail-of-turn ladder that
//!   drains the command queue + inbox, runs the auto-compact ladder
//!   (time-based microcompact → file-stub cleanup → SM extraction →
//!   threshold microcompact → SM-first / full LLM), and emits
//!   `TurnCompleted`.
//! - [`QueryEngine::do_reactive_compact`] — `prompt_too_long` recovery.
//!   Capability-split between Anthropic's server-side `context_management`
//!   (cache-preserving) and the client-side `api_microcompact` +
//!   `peel_head_for_ptl_retry` fallback.
//!
//! Extracted from `engine.rs` to keep the multi-turn loop file focused on
//! orchestration. The full LLM / SM / manual compact paths live in
//! `crate::engine_compaction`.

use tracing::info;
use tracing::warn;

use coco_messages::MessageHistory;
use coco_types::TokenUsage;

use crate::CoreEvent;
use crate::ServerNotification;
use crate::command_queue::QueuePriority;
use crate::emit::emit_protocol;
use crate::engine::QueryEngine;
use crate::engine_helpers::render_transcript_for_extractor;
use crate::helpers::drain_command_queue_into_history;

impl QueryEngine {
    /// Shrink `history` with a reactive microcompact and emit the paired
    /// `CompactionStarted` → `ContextCompacted` notifications. Shared by both
    /// `prompt_too_long` recovery sites (stream-open failure and mid-stream
    /// failure) — keeps the two paths bit-identical.
    #[tracing::instrument(
        skip_all,
        name = "compaction",
        fields(
            trigger = "reactive",
            session_id = %self.config.session_id,
            history_len = history.messages.len(),
        ),
    )]
    pub(crate) async fn do_reactive_compact(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) {
        // Circuit-breaker check (TS reactiveCompact.ts).
        // If we've already failed 3× in a row, don't keep wasting API calls.
        {
            let state = self.reactive_state.lock().await;
            if !state.should_attempt_reactive_compact() {
                warn!(
                    failures = state.failure_count(),
                    "reactive compact circuit-breaker tripped; skipping"
                );
                return;
            }
        }

        let pre_tokens = coco_compact::estimate_tokens(&history.messages);
        let pre_count = history.messages.len() as i32;
        let drop_target = coco_compact::reactive::calculate_drop_target(
            pre_tokens,
            &coco_compact::ReactiveCompactConfig {
                context_window: self.config.context_window,
                max_output_tokens: self.config.max_output_tokens,
                ..Default::default()
            },
            &self.config.compact.auto,
        );
        let _ = emit_protocol(event_tx, ServerNotification::CompactionStarted).await;

        // Step 0: if staged-collapse is active, try draining staged
        // ranges into commits before falling back to head-truncation.
        // TS: query.ts:1094 `recoverFromOverflow()` precedes
        // `truncateHeadForPTLRetry`. Drained commits don't strip
        // messages here — they only mark them as committed; the next
        // `apply_collapses_if_needed` (run before each prompt build)
        // performs the actual splice. Until that pass is wired, this
        // path emits a phase event so TUI can show the recovery and
        // proceeds to the standard reactive microcompact below.
        let drained: Vec<coco_compact::StagedCommitEntry> =
            if let Some(ledger) = &self.staged_ledger {
                let mut g = ledger.lock().await;
                g.drain_overflow(self.staged_session_id, |_| uuid::Uuid::new_v4())
            } else {
                Vec::new()
            };
        if !drained.is_empty() {
            info!(
                drained = drained.len(),
                "PTL recovery: drained staged collapses into commits"
            );
            // Persist each drained commit so resume can replay them.
            // TS: utils/sessionStorage.ts:1541 recordContextCollapseCommit.
            if let (Some(store), Some(sid)) = (&self.transcript_store, &self.transcript_session_id)
            {
                for entry in &drained {
                    if let Ok(payload) = serde_json::to_value(entry)
                        && let Err(e) = store.append_marble_origami_commit(sid, payload)
                    {
                        warn!("failed to persist marble-origami-commit: {e}");
                    }
                }
                // Persist the (now-empty) snapshot so resume sees the
                // armed=false state. Last-wins semantics make this safe.
                if let Some(ledger) = &self.staged_ledger {
                    let g = ledger.lock().await;
                    if let Some(snap) = g.snapshot.as_ref()
                        && let Ok(payload) = serde_json::to_value(snap)
                        && let Err(e) = store.append_marble_origami_snapshot(sid, payload)
                    {
                        warn!("failed to persist marble-origami-snapshot: {e}");
                    }
                }
            }
        }

        // Provider capability split. On Anthropic (server-side edits)
        // we attach a one-shot `context_management` payload to the next
        // QueryParams build instead of mutating messages locally — the
        // API clears tool results in place and the prompt cache stays
        // intact. On other providers, fall back to the original
        // client-side mutation path (cache-invalidating but universal).
        if self.client.supports_server_side_context_edits() {
            // Build aggressive ApiContextOptions from current state.
            // `trigger_threshold = pre_tokens` ensures the server applies
            // clearing for the current oversized prompt; `keep_target`
            // aims for `pre_tokens - drop_target` so the server frees at
            // least `drop_target` worth.
            let opts = coco_compact::ApiContextOptions {
                has_thinking: self.config.thinking_level.is_some(),
                is_redact_thinking_active: false,
                clear_all_thinking: true,
                clear_tool_results: true,
                clear_tool_uses: true,
                trigger_threshold: pre_tokens.max(1),
                keep_target: (pre_tokens - drop_target).max(1),
            };
            let strategies = coco_compact::get_api_context_management(&opts);
            if let Some(payload) = coco_compact::encode_anthropic_context_management(&strategies) {
                let mut pending = self.pending_reactive_context_management.lock().await;
                *pending = Some(payload);
                info!(
                    drop_target,
                    "queued reactive context_management for next API call"
                );
            }
            // Server clears in place — no local mutation. The next API
            // call sends the original (oversized) prompt + the payload;
            // Anthropic strips and bills accordingly.
        } else {
            coco_compact::reactive::api_microcompact(&mut history.messages, drop_target);
            let post_micro_tokens = coco_compact::estimate_tokens(&history.messages);
            let freed = (pre_tokens - post_micro_tokens).max(0);

            // Escalate when api_microcompact couldn't free enough — most
            // likely all old tool results are already cleared. Peel oldest
            // API-round groups until we've freed `drop_target` tokens.
            // TS reactiveCompact.ts: head-truncation falls back here when
            // the in-place tool-result clear can't recover budget.
            if freed < drop_target
                && let Some(survivors) =
                    coco_compact::peel_head_for_ptl_retry(&history.messages, drop_target - freed)
            {
                history.messages = survivors;
            }
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let post_tokens = coco_compact::estimate_tokens(&history.messages);
        let actually_freed = (pre_tokens - post_tokens).max(0);
        {
            let mut state = self.reactive_state.lock().await;
            // Treat any meaningful reduction as a success; if we
            // couldn't drop anything, mark a failure so the breaker
            // trips eventually.
            if actually_freed > 0 {
                state.record_success(now_ms);
            } else {
                state.record_failure(now_ms);
            }
        }

        let removed = (pre_count - history.messages.len() as i32).max(0);
        let _ = emit_protocol(
            event_tx,
            ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                removed_messages: removed,
                summary_tokens: 0,
                trigger: coco_types::CompactTrigger::Reactive,
                pre_tokens: Some(pre_tokens),
                post_tokens: Some(post_tokens),
            }),
        )
        .await;

        // Reactive recovery shares the post-compact-cleanup path with
        // full / SM compaction (TS commands/compact/compact.ts:201
        // calls `runPostCompactCleanup()` after `tryReactiveCompact`).
        // We build a synthetic CompactResult — observers in
        // `app/query/src/observers.rs` only inspect `trigger` /
        // `is_main_agent`, not summary content, so empty fields are fine.
        let is_main_agent = self.config.agent_id.is_none();
        let synth = coco_compact::CompactResult {
            boundary_marker: coco_messages::create_compact_boundary_message(
                pre_tokens,
                post_tokens,
            ),
            summary_messages: Vec::new(),
            attachments: Vec::new(),
            messages_to_keep: history.messages.clone(),
            hook_results: Vec::new(),
            user_display_message: None,
            pre_compact_tokens: pre_tokens,
            post_compact_tokens: post_tokens,
            true_post_compact_tokens: post_tokens,
            is_recompaction: false,
            trigger: coco_types::CompactTrigger::Reactive,
        };
        self.compaction_observers
            .notify_all(&synth, is_main_agent)
            .await;
        self.compaction_observers
            .notify_post_compact(&history.messages)
            .await;

        // Reset the cache-break baseline — TS notifyCompaction(query_source, agent_id).
        // Reactive shares the `repl_main_thread` tracking key with main loop, so
        // we use the same source attribution as the API call site. After this,
        // the next response's lower cache_read tokens won't false-positive
        // as a break.
        let qs = self.query_source_label();
        self.client
            .notify_compaction(qs, self.config.agent_id.as_deref())
            .await;

        // TS `getUnifiedTaskAttachments(ctx)` only fires post-compaction; the
        // next reminder build consumes (and clears) this flag.
        self.pending_just_compacted
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Finalize a turn after tools have executed: drain queued commands + inbox,
    /// auto-compact if over threshold, then emit `TurnCompleted`.
    ///
    /// Extracted from `run_session_loop` to keep that function focused on the
    /// decision/transition logic. Mirrors the TS tail-of-turn sequence in
    /// `query.ts` where messageQueueManager flush + compactConversation +
    /// turn-complete emission all happen together.
    pub(crate) async fn finalize_turn_post_tools(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        turn_id: String,
        usage: TokenUsage,
    ) {
        // Bump the per-engine turn counter so RecompactionInfo can derive
        // `turns_since_previous` accurately. TS: `compact.ts:317-323`.
        self.turn_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Drain command queue: all priorities land before the next API call.
        // Slash commands excluded (processed post-turn). Agent-filtered.
        // TS: `messageQueueManager.ts` flushes pending messages between tool
        // execution and the next API call.
        drain_command_queue_into_history(
            &self.command_queue,
            history,
            event_tx,
            QueuePriority::Later,
            None,
        )
        .await;

        // Drain inbox messages from teammates. When a teammate sends a
        // `<task-notification>` XML envelope (TS `coordinatorMode.ts:130-152`,
        // emitted by the coordinator runner on worker terminate), surface
        // the structured fields explicitly on the wrapper so the leader
        // model can reason about task-id / status / summary / result /
        // usage without re-parsing the inner XML.
        let inbox_msgs = self.inbox.drain_unconsumed().await;
        for msg in inbox_msgs {
            let text = render_teammate_message_wrapper(&msg.from_agent, &msg.content);
            history.push(coco_messages::create_user_message(&text));
        }

        // Tool-result budget (Level 2) — TS `query.ts:379
        // applyToolResultBudget` runs BEFORE microcompact so the
        // budget cap acts on a freshly-eligible message set. Level 2
        // is enabled iff `compact.tool_result_budget.enabled`; the
        // pure-logic call lives in `coco_tool_runtime::tool_result_storage`.
        // No-op when disabled. We materialise candidates from the
        // most recent tool_result run (TS scopes per-message; coco-rs
        // scopes per-history-tail because messages here are flat).
        if self.config.compact.tool_result_budget.enabled {
            apply_tool_result_budget_to_history(
                history,
                &self.tool_result_replacement_state,
                self.config.compact.tool_result_budget.per_message_chars,
            )
            .await;
        }

        // Auto-compaction ladder (mirrors TS query.ts tail-of-turn):
        //  0. Time-based microcompact — fire on long inactivity gap so the
        //     next API call doesn't carry stale tool result content.
        //  1. Threshold micro_compact — keep last N compactable tool uses.
        //  2. Session-memory-first — replace LLM summary with pre-extracted
        //     memory when the post-SM count would still fit.
        //  3. Full LLM compact — fallback when SM declined or wasn't enabled.
        //
        // `should_auto_compact_guarded` reads the resolved
        // `AutoCompactConfig` (user toggle + env kill switches +
        // overrides folded in by `coco_config::CompactConfig::resolve`)
        // and adds the recursion guard. `Other` source = main thread /
        // SDK; subagent paths set their own source when wired through.
        let auto_cfg = &self.config.compact.auto;
        let micro_keep = self.config.compact.micro.keep_recent.max(0) as usize;

        // Step 0: time-based microcompact (gap > threshold && main thread).
        // Independent of token threshold — fires whenever the cache TTL has
        // likely expired, preventing stale tool results from poisoning the
        // next prompt cache.
        let tb_cfg = &self.config.compact.micro.time_based;
        if self.config.compact.micro.enabled && tb_cfg.enabled {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let last_ms = self
                .last_assistant_ms
                .load(std::sync::atomic::Ordering::Acquire);
            let last_opt = if last_ms > 0 { Some(last_ms) } else { None };
            if let Some(trigger) = coco_compact::evaluate_time_based_trigger(
                tb_cfg, now_ms, last_opt, /*is_main_thread*/ true,
            ) {
                let pre_tb_tokens = coco_compact::estimate_tokens(&history.messages);
                if let Some(res) =
                    coco_compact::time_based_microcompact(&mut history.messages, &trigger)
                {
                    info!(
                        cleared = res.messages_cleared,
                        gap_min = trigger.gap_minutes,
                        "time-based micro-compaction triggered",
                    );
                    let post_tb_tokens = coco_compact::estimate_tokens(&history.messages);
                    // TS does not emit a CompactBoundary for time-based MC —
                    // it logs an analytics event (`tengu_time_based_microcompact`)
                    // and leaves the trigger label to the surrounding flow.
                    // Reuse `Auto` so the boundary trigger taxonomy matches TS;
                    // the `TimeBased` variant remains for callers that still
                    // want the distinction in custom UIs.
                    let _ = emit_protocol(
                        event_tx,
                        ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                            removed_messages: res.messages_cleared,
                            summary_tokens: 0,
                            trigger: coco_types::CompactTrigger::Auto,
                            pre_tokens: Some(pre_tb_tokens),
                            post_tokens: Some(post_tb_tokens),
                        }),
                    )
                    .await;
                    // TS: microCompact.ts:525 `notifyCacheDeletion` — the
                    // next response's cache_read drop is from us, not a real
                    // break. Use the same query_source attribution as the
                    // main API call so they share the tracking key.
                    let qs = self.query_source_label();
                    self.client
                        .notify_cache_deletion(qs, self.config.agent_id.as_deref())
                        .await;
                }
            }
        }

        // Step 0.5: file-unchanged stub cleanup. After many turns of
        // re-reading the same file, accumulated `[file unchanged]`
        // tool_result placeholders eat tokens for no benefit. Replace
        // with a smaller marker so the next turn's prompt cache stays
        // healthy. No TS equivalent in external builds — opt-in via
        // `compact.micro.clear_file_unchanged_stubs_enabled` (default off,
        // matches TS-feature-stripped behavior).
        if self.config.compact.micro.enabled
            && self.config.compact.micro.clear_file_unchanged_stubs_enabled
        {
            let _ = coco_compact::clear_file_unchanged_stubs(&mut history.messages);
        }

        // Compute message-level stats once and share across the
        // session-memory hook (Step 0.7), the auto-memory fan-out, and
        // the auto-compact threshold check below — all of them read the
        // same post-Step-0.5 history.
        let estimated_tokens = coco_compact::estimate_tokens(&history.messages);
        let tool_calls_last_turn =
            coco_session_memory::count_tool_calls_in_last_assistant_turn(&history.messages);
        let last_assistant_uuid = history
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m, coco_messages::Message::Assistant(_)))
            .and_then(|m| m.uuid())
            .copied();

        // Step 0.7: session-memory extraction. Forked-agent path runs
        // `maybe_extract` after each assistant turn so the next compaction
        // can short-circuit via SM-first. TS:
        // services/SessionMemory/sessionMemory.ts:374 registerPostSamplingHook.
        if let Some(svc) = self.session_memory_service.clone()
            && self.config.compact.session_memory.enabled
            && self.config.agent_id.is_none()
        {
            let transcript = render_transcript_for_extractor(&history.messages);
            let outcome = svc
                .maybe_extract(
                    estimated_tokens,
                    tool_calls_last_turn,
                    last_assistant_uuid,
                    &transcript,
                )
                .await;
            // Refresh in-memory SM text after a successful extract so the
            // *next* SM-first compact reads it.
            if matches!(
                outcome,
                coco_session_memory::ExtractionOutcome::Extracted { .. }
            ) {
                let new_text = svc.current_text().await;
                self.set_session_memory_text(new_text).await;
            }
        }

        // Auto-memory turn-end fan-out (TS feature `AutoMemory`): fire
        // the 9-section session memory and forked extraction services
        // concurrently. Both gate internally; the lazy `fork_messages`
        // closure defers per-message serialization until extraction
        // actually fires.
        if let Some(runtime) = self.memory_runtime.clone()
            && self.config.agent_id.is_none()
        {
            let last_cursor = runtime.extract.last_cursor().await;
            let has_memory_writes =
                main_agent_wrote_memory(&history.messages, runtime.personal_dir());
            let messages_for_fork = history.messages.clone();
            let extract_input = coco_memory::service::extract::TurnInput {
                fork_messages: Box::new(move || {
                    serialize_messages_since(&messages_for_fork, last_cursor.as_deref())
                }),
                message_count: history.messages.len() as i32,
                last_message_id: last_assistant_uuid.map(|u| u.to_string()),
                has_memory_writes,
            };
            let session_memory = runtime.session_memory.clone();
            let extract = runtime.extract.clone();
            let (_sm, _ex) = tokio::join!(
                session_memory.maybe_extract(
                    estimated_tokens,
                    tool_calls_last_turn,
                    tool_calls_last_turn > 0
                ),
                extract.maybe_extract(extract_input),
            );
        }
        // Collapse-aware guard: when staged_compact is active it owns
        // the threshold ladder, so proactive autocompact suppresses.
        let collapse_active = self.is_collapse_active();
        if coco_compact::should_auto_compact_guarded_with_collapse(
            estimated_tokens,
            self.config.context_window,
            self.config.max_output_tokens,
            auto_cfg,
            coco_compact::CompactQuerySource::Other,
            collapse_active,
        ) {
            // Step 1: threshold micro_compact (count-based). TS external
            // doesn't run this — `microcompactMessages` is a no-op outside
            // `feature('CACHED_MICROCOMPACT')`. Opt-in via
            // `compact.micro.count_based_enabled` (default off). When off,
            // we go straight to SM/LLM compaction below.
            let pre_count = history.messages.len() as i32;
            let pre_micro_tokens = estimated_tokens;
            if self.config.compact.micro.enabled && self.config.compact.micro.count_based_enabled {
                coco_compact::micro_compact(&mut history.messages, micro_keep);
                info!("auto micro-compaction triggered (keep_recent={micro_keep})");
            }
            let removed = (pre_count - history.messages.len() as i32).max(0);
            let post_micro_tokens = coco_compact::estimate_tokens(&history.messages);
            let _ = emit_protocol(
                event_tx,
                ServerNotification::ContextCompacted(coco_types::ContextCompactedParams {
                    removed_messages: removed,
                    summary_tokens: 0,
                    trigger: coco_types::CompactTrigger::Auto,
                    pre_tokens: Some(pre_micro_tokens),
                    post_tokens: Some(post_micro_tokens),
                }),
            )
            .await;

            if removed > 0 {
                // Auto micro_compact mutated tool result content — TS
                // notifyCacheDeletion semantics. Suppresses the false-
                // positive cache-break warning on the next API call.
                let qs = self.query_source_label();
                self.client
                    .notify_cache_deletion(qs, self.config.agent_id.as_deref())
                    .await;
            }

            if coco_compact::should_auto_compact_guarded_with_collapse(
                post_micro_tokens,
                self.config.context_window,
                self.config.max_output_tokens,
                auto_cfg,
                coco_compact::CompactQuerySource::Other,
                collapse_active,
            ) {
                // Step 2 → 3: SM-first → full LLM. `try_full_compact` owns the
                // branch internally so manual `/compact` benefits too.
                self.try_full_compact(
                    history,
                    event_tx,
                    coco_types::CompactTrigger::Auto,
                    /*custom_instructions*/ None,
                )
                .await;
            }
        }

        // D8: snapshot post-turn cache-safe params for future
        // post-turn fork features (`/btw`, `promptSuggestion`,
        // `postTurnSummary`). TS parity: `handleStopHooks` calls
        // `saveCacheSafeParams` here. Helper handles the empty-history
        // skip + serialisation; called from text-only exits in
        // `engine.rs::run_session_loop` too so every successful turn
        // updates the slot.
        self.save_post_turn_cache_params(history).await;

        // P5 / A3: kick off the post-turn promptSuggestion fork when
        // the gate allows. The helper checks env, plan-mode,
        // non-interactive, and pending-plan-approval; all four must
        // be open for a suggestion to fire. The actual fork runs in
        // a detached task so the turn can finalise immediately —
        // the suggestion lands on `ToolAppState.prompt_suggestion`
        // when the model responds (the TUI consumer reads from there).
        // TS parity: `query/stopHooks.ts:139` `executePromptSuggestion`.
        if let Some(app_state) = self.app_state.as_ref() {
            let env_disable =
                coco_config::env::is_env_truthy(coco_config::EnvKey::CocoPromptSuggestionDisable);
            let is_non_interactive = self.config.is_non_interactive;
            let snapshot = app_state.read().await;
            let should = crate::prompt_suggestion::should_suggest(
                &snapshot,
                is_non_interactive,
                env_disable,
            );
            drop(snapshot);
            if should {
                self.spawn_prompt_suggestion_task(app_state.clone()).await;
            }
        }

        // Per-turn JSONL transcript append. Walks `history` and writes
        // any user/assistant/system/attachment message whose uuid isn't
        // already in the cross-engine dedup set. Skips silently when
        // the store / session id / dedup set aren't all wired (e.g.
        // tests, headless runs without persistence). TS parity:
        // `Project.recordTranscript` flushes the per-message queue at
        // turn end.
        self.record_transcript_tail(history).await;

        info!(
            turn_id = %turn_id,
            tokens_in = usage.input_tokens,
            tokens_out = usage.output_tokens,
            history_len = history.messages.len(),
            "turn completed"
        );
        let _ = emit_protocol(
            event_tx,
            ServerNotification::TurnCompleted(coco_types::TurnCompletedParams {
                turn_id: Some(turn_id),
                usage,
            }),
        )
        .await;
    }

    /// Append every history message whose uuid isn't already in the
    /// dedup set to the JSONL transcript, with parent_uuid linking to
    /// the previous message in the chain. No-op when transcript
    /// persistence isn't wired (`with_transcript_store` +
    /// `with_transcript_dedup` not both called).
    pub(crate) async fn record_transcript_tail(&self, history: &MessageHistory) {
        let (Some(store), Some(sid), Some(seen)) = (
            self.transcript_store.as_ref(),
            self.transcript_session_id.as_deref(),
            self.transcript_dedup.as_ref(),
        ) else {
            return;
        };

        let mut seen_guard = seen.lock().await;
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let now = chrono::Utc::now().to_rfc3339();
        let mut prev_uuid: Option<String> = None;

        for msg in &history.messages {
            let Some(uuid) = msg.uuid().copied() else {
                continue;
            };
            // Track chain order even for already-written entries so the
            // next *new* entry's parent_uuid points to the most recent
            // message rather than the last new one.
            if !seen_guard.insert(uuid) {
                prev_uuid = Some(uuid.to_string());
                continue;
            }
            let entry =
                match build_transcript_entry(msg, &uuid, prev_uuid.as_deref(), sid, &cwd, &now) {
                    Some(e) => e,
                    None => {
                        prev_uuid = Some(uuid.to_string());
                        continue;
                    }
                };
            if let Err(e) = store.append_message(sid, &entry) {
                warn!(error = %e, "failed to append transcript entry");
                // Don't mark uuid as written when the append fails —
                // a retry on the next turn can recover, mirroring TS
                // best-effort persistence semantics.
                seen_guard.remove(&uuid);
            }
            prev_uuid = Some(uuid.to_string());
        }
    }

    /// Spawn the post-turn promptSuggestion fork in a detached task
    /// (D2 — production wiring).
    ///
    /// Drives a one-shot fork via [`crate::forked_agent::ForkDispatcher`]
    /// (installed by the CLI bootstrap) using the parent's cached
    /// system prompt + history. The dispatcher builds a *fresh*
    /// engine, so the parent loop is never mutated.
    ///
    /// The suggestion is best-effort: any of these silently skip
    /// recording (the TUI then falls back to the default placeholder):
    /// - no cache slot (first turn hasn't completed)
    /// - no fork dispatcher installed (test / minimal embedding)
    /// - dispatch error (transport crash etc.)
    /// - empty / placeholder-only response from the model
    ///
    /// TS parity: `services/PromptSuggestion/promptSuggestion.ts`
    /// calls `runForkedAgent` with the bespoke suggestion system
    /// prompt and `effort: undefined` (cache parity preserved).
    async fn spawn_prompt_suggestion_task(
        &self,
        app_state: std::sync::Arc<tokio::sync::RwLock<coco_types::ToolAppState>>,
    ) {
        let cache = match self.last_cache_safe_params().await {
            Some(c) => c,
            None => return,
        };
        let dispatcher = match self.fork_dispatcher.clone() {
            Some(d) => d,
            None => return,
        };
        // Detach: the suggestion is fire-and-forget. The parent turn
        // has already emitted `TurnCompleted`; we don't want a slow
        // suggestion fork blocking the next user prompt.
        tokio::spawn(async move {
            let options = crate::forked_agent::one_shot_options("promptSuggestion");
            let system = crate::prompt_suggestion::build_suggestion_system_prompt().to_string();
            // The fork sees a special-purpose system prompt + the
            // parent's history; the user message is intentionally
            // empty (TS does the same — the model is told via the
            // system prompt to produce a suggestion based on what
            // came before). Some providers reject a literally empty
            // user message, so we send a single space.
            let result = dispatcher
                .dispatch(&cache, &options, " ", Some(system))
                .await;
            match result {
                Ok(r) => {
                    let text = r.text.trim();
                    // Filter empty / NONE / overly-long responses
                    // (matches TS's filter logic which guards against
                    // model NACKs and rejects out-of-band noise).
                    if text.is_empty() || text.eq_ignore_ascii_case("NONE") || text.len() > 200 {
                        return;
                    }
                    let prompt_id = uuid::Uuid::new_v4().to_string();
                    let now = chrono::Utc::now().to_rfc3339();
                    let mut state = app_state.write().await;
                    crate::prompt_suggestion::record_suggestion(
                        &mut state,
                        text.to_string(),
                        prompt_id,
                        now,
                        None,
                    );
                }
                Err(e) => {
                    tracing::debug!(error = %e, "promptSuggestion fork dispatch failed");
                }
            }
        });
    }
}

/// Slice the message history to "everything newer than `last_cursor`"
/// and serialize as JSON for `AgentSpawnRequest::fork_context_messages`.
/// When `last_cursor` is `None` (first extraction), return the full
/// history.
///
/// TS parity: `messagesSinceCursor` in `services/extractMemories/`.
/// We keep the slice as `serde_json::Value` so the boundary doesn't
/// pull `coco_messages::Message` types into `coco-tool-runtime`.
fn serialize_messages_since(
    messages: &[coco_messages::Message],
    last_cursor: Option<&str>,
) -> Vec<serde_json::Value> {
    let cursor_idx = last_cursor.and_then(|c| {
        messages
            .iter()
            .position(|m| m.uuid().map(|u| u.to_string() == c).unwrap_or(false))
    });
    let slice = match cursor_idx {
        Some(i) => &messages[i + 1..],
        None => messages,
    };
    slice
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect()
}

/// Detect whether the main agent's last assistant turn wrote into the
/// memory directory via Write / Edit / NotebookEdit. Used by
/// `ExtractService::maybe_extract` to skip extraction when the user
/// just curated memory directly — TS `hasMemoryWritesSince`.
fn main_agent_wrote_memory(
    messages: &[coco_messages::Message],
    memory_dir: &std::path::Path,
) -> bool {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use coco_messages::Message;
    for msg in messages.iter().rev() {
        let Message::Assistant(assistant) = msg else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &assistant.message else {
            return false;
        };
        for block in content {
            let AssistantContent::ToolCall(call) = block else {
                continue;
            };
            // Compare against the canonical typed names instead of
            // raw string literals.
            let name = call.tool_name.as_str();
            let is_write_tool = name == coco_types::ToolName::Write.as_str()
                || name == coco_types::ToolName::Edit.as_str()
                || name == coco_types::ToolName::NotebookEdit.as_str();
            if !is_write_tool {
                continue;
            }
            let Some(file_path) = call
                .input
                .get("file_path")
                .or_else(|| call.input.get("notebook_path"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let path = std::path::Path::new(file_path);
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            };
            if absolute.starts_with(memory_dir) {
                return true;
            }
        }
        // Only consider the single most-recent assistant turn — older
        // tool uses aren't relevant to "did the model just write?"
        return false;
    }
    false
}

/// Build a `coco_session::TranscriptEntry` from a [`coco_messages::Message`].
///
/// Returns `None` for messages we don't persist (`Progress`,
/// `Tombstone`, `ToolUseSummary` — none round-trip through resume).
/// TS parity: `Project.recordTranscript` in `utils/sessionStorage.ts`
/// writes user/assistant/system/attachment/tool_result entries, with
/// `isSidechain` set for subagent transcripts.
fn build_transcript_entry(
    msg: &coco_messages::Message,
    uuid: &uuid::Uuid,
    parent_uuid: Option<&str>,
    session_id: &str,
    cwd: &str,
    timestamp: &str,
) -> Option<coco_session::TranscriptEntry> {
    use coco_messages::Message;
    use coco_session::storage::entry_kind;
    let (entry_type, message_value, model, usage, cost_usd) = match msg {
        Message::User(u) => (
            entry_kind::USER,
            serde_json::to_value(&u.message).ok(),
            None,
            None,
            None,
        ),
        Message::Assistant(a) => {
            let usage = a.usage.as_ref().map(|u| coco_session::TranscriptUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: Some(u.input_token_details.cache_read_tokens),
                cache_creation_tokens: Some(u.input_token_details.cache_write_tokens),
            });
            (
                entry_kind::ASSISTANT,
                serde_json::to_value(&a.message).ok(),
                Some(a.model.clone()).filter(|m| !m.is_empty()),
                usage,
                a.cost_usd,
            )
        }
        Message::System(s) => (
            entry_kind::SYSTEM,
            serde_json::to_value(s).ok(),
            None,
            None,
            None,
        ),
        Message::Attachment(att) => (
            entry_kind::ATTACHMENT,
            serde_json::to_value(&att.body).ok(),
            None,
            None,
            None,
        ),
        Message::ToolResult(t) => (
            entry_kind::TOOL_RESULT,
            serde_json::to_value(t).ok(),
            None,
            None,
            None,
        ),
        Message::Progress(_) | Message::Tombstone(_) | Message::ToolUseSummary(_) => return None,
    };
    Some(coco_session::TranscriptEntry {
        entry_type: entry_type.to_string(),
        uuid: uuid.to_string(),
        parent_uuid: parent_uuid.map(str::to_string),
        session_id: session_id.to_string(),
        cwd: cwd.to_string(),
        timestamp: timestamp.to_string(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        git_branch: None,
        is_sidechain: false,
        message: message_value,
        usage,
        model,
        cost_usd,
        extra: serde_json::Map::new(),
    })
}

/// Wrap a teammate's inbox message in the `<teammate-message>` envelope
/// the leader's model sees. When `content` is a `<task-notification>`
/// XML envelope (TS `coordinatorMode.ts:130-152`) we surface the
/// structured fields (task-id / status / summary) as wrapper attributes
/// so the leader model can reason about them without re-parsing the
/// inner XML — falling back to a plain wrapper when the parse fails or
/// the message isn't a task notification.
///
/// Pure logic — extracted out of `finalize_turn_post_tools` so the
/// receive-side wrapping is unit-testable without an engine fixture.
pub(crate) fn render_teammate_message_wrapper(from: &str, content: &str) -> String {
    if coco_subagent::looks_like_task_notification(content)
        && let Some(parsed) = coco_subagent::parse_task_notification(content)
    {
        return format!(
            "<teammate-message from=\"{from}\" task-id=\"{task_id}\" \
             status=\"{status}\" summary=\"{summary}\">{content}</teammate-message>",
            from = from,
            task_id = parsed.task_id,
            status = parsed.status.as_str(),
            summary = parsed.summary,
            content = content,
        );
    }
    format!("<teammate-message from=\"{from}\">{content}</teammate-message>")
}

/// Project the recent tool_result tail of `history` into
/// `ToolResultCandidate` shape, run [`coco_tool_runtime::tool_result_storage::apply_tool_result_budget`],
/// and rewrite each newly-replaced ToolResult message's body to the
/// canonical `[Old tool result content cleared]` placeholder. TS
/// parity: `query.ts:379` + `enforceToolResultBudget` from
/// `utils/toolResultStorage.ts`.
///
/// `per_message_chars` comes from `compact.tool_result_budget` and
/// `i64::MAX` opts the call out (the helper still consumes the
/// state's seen_ids to keep the freeze-once contract). Tools'
/// `max_result_size_chars` would normally drive `persistence_opted_out`
/// per candidate, but we don't carry the registry through here —
/// every tool result is treated as evictable. (Per-tool opt-out is a
/// future follow-up that needs the tool registry plumbed into the
/// finalize-turn surface.)
async fn apply_tool_result_budget_to_history(
    history: &mut MessageHistory,
    state: &coco_tool_runtime::tool_result_storage::ContentReplacementStateRef,
    per_message_chars: i64,
) {
    use coco_messages::Message;
    use coco_tool_runtime::tool_result_storage as trb;

    // Collect candidates from the history tail. We scope to recent
    // entries (last 32) — an entire-history walk would dominate the
    // hot path on long sessions and the budget cap acts on aggregate
    // content size which only the recent tail can blow.
    const SCAN_TAIL: usize = 32;
    let start = history.messages.len().saturating_sub(SCAN_TAIL);
    let mut candidates: Vec<trb::ToolResultCandidate> = Vec::new();
    for msg in history.messages.iter().skip(start) {
        if let Message::ToolResult(tr) = msg {
            let text = coco_messages::wrapping::extract_text_from_llm_message(&tr.message);
            candidates.push(trb::ToolResultCandidate {
                tool_use_id: tr.tool_use_id.clone(),
                content_chars: text.len() as i64,
                tool_name: Some(tr.tool_id.to_string()),
                persistence_opted_out: false,
            });
        }
    }
    if candidates.is_empty() {
        return;
    }

    // Override the state's per_message_chars on each call so the
    // budget reflects the live config (config can be hot-reloaded
    // through `RuntimeConfig`). Cheap — single field write.
    {
        let mut s = state.write().await;
        s.per_message_chars = per_message_chars;
    }

    let outcome = trb::apply_tool_result_budget(&candidates, state).await;
    if outcome.newly_replaced.is_empty() {
        return;
    }

    // Apply replacements: rewrite each matching ToolResult message's
    // body to the canonical placeholder. Lookup map keyed by
    // tool_use_id since the same id never appears twice in a turn.
    let replaced: std::collections::HashSet<String> = outcome.newly_replaced.into_iter().collect();
    for msg in history.messages.iter_mut() {
        if let Message::ToolResult(tr) = msg
            && replaced.contains(&tr.tool_use_id)
        {
            tr.message =
                coco_inference::LanguageModelMessage::user_text(trb::TOOL_RESULT_CLEARED_MESSAGE);
        }
    }
}

#[cfg(test)]
#[path = "engine_finalize_turn.test.rs"]
mod tests;
