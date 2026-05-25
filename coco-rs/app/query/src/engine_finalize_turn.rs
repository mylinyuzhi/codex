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

use std::sync::Arc;

use tracing::info;
use tracing::warn;

use coco_messages::MessageHistory;
use coco_types::TokenUsage;

use crate::ContinueReason;
use crate::CoreEvent;
use crate::ServerNotification;
use crate::budget::BudgetTracker;
use crate::command_queue::QueuePriority;
use crate::emit::emit_protocol;
use crate::engine::QueryEngine;
use crate::helpers::drain_command_queue_into_history;

impl QueryEngine {
    /// Unified handler for "input + output won't fit in the model's context
    /// window." Three distinct signals route here:
    ///
    /// 1. HTTP 400 [`coco_inference::InferenceError::ContextWindowExceeded`] —
    ///    provider rejected the request outright (OpenAI / Google / ByteDance
    ///    `context_length_exceeded`, defensive `prompt_too_long` body match).
    /// 2. Mid-stream error string `prompt_too_long` / `context_length` — same
    ///    signal arriving after `message_start` but before the response
    ///    completes.
    /// 3. Anthropic [`coco_messages::StopReason::ContextWindowExceeded`]
    ///    finish reason (extended-context beta only) — request streamed
    ///    cleanly to a finish event whose stop_reason reports window
    ///    exhaustion.
    ///
    /// Always attempts reactive compaction; never escalates
    /// `max_output_tokens`. Raising the output budget cannot help when the
    /// *input* already exceeds the window — it only delays the next failure
    /// by another round-trip and (on the Anthropic finish-reason path) makes
    /// the next request trip the HTTP-400 sibling. Compaction shrinks the
    /// actual culprit. `do_reactive_compact` carries its own 3-failure
    /// circuit breaker, so repeated calls cannot spin.
    ///
    /// `site` is purely a tracing field for distinguishing the three call
    /// sites in logs.
    pub(crate) async fn handle_context_overflow(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        budget: &mut BudgetTracker,
        site: &'static str,
    ) -> ContinueReason {
        warn!(
            site,
            "context window exceeded, attempting reactive compaction"
        );
        self.do_reactive_compact(history, event_tx).await;
        budget.reset_continuations();
        ContinueReason::ReactiveCompactRetry
    }

    /// Shrink `history` with a reactive microcompact and emit the paired
    /// `CompactionStarted` → `ContextCompacted` notifications. Shared by every
    /// context-window-exceeded recovery site (stream-open 400, mid-stream
    /// error, and Anthropic `model_context_window_exceeded` finish reason) —
    /// keeps the three paths bit-identical.
    #[tracing::instrument(
        skip_all,
        name = "compaction",
        fields(
            trigger = "reactive",
            session_id = %self.config.session_id,
            history_len = history.len(),
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

        let pre_tokens = coco_compact::estimate_tokens(history.as_slice());
        let pre_count = history.len() as i32;
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
            history.with_owned_messages(|msgs| {
                coco_compact::reactive::api_microcompact(msgs, drop_target);
            });
            let post_micro_tokens = coco_compact::estimate_tokens(history.as_slice());
            let freed = (pre_tokens - post_micro_tokens).max(0);

            // Escalate when api_microcompact couldn't free enough — most
            // likely all old tool results are already cleared. Peel oldest
            // API-round groups until we've freed `drop_target` tokens.
            // TS reactiveCompact.ts: head-truncation falls back here when
            // the in-place tool-result clear can't recover budget.
            if freed < drop_target
                && let Some(survivors) =
                    coco_compact::peel_head_for_ptl_retry(history.as_slice(), drop_target - freed)
            {
                // I-1 (Authority): reactive head-trim drops oldest
                // messages from history. Pair the swap with truncate
                // + appended-burst so TUI/SDK observers see the new
                // state.
                crate::history_sync::history_replace_and_emit(history, survivors, event_tx).await;
            }
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let post_tokens = coco_compact::estimate_tokens(history.as_slice());
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

        let removed = (pre_count - history.len() as i32).max(0);
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
        // `is_main_agent`, not summary content, so empty fields are fine —
        // `messages_to_keep: Vec::new()` saves an N-message deep clone that
        // would have been thrown away after the observer dispatch.
        let is_main_agent = self.config.agent_id.is_none();
        let synth = coco_compact::CompactResult {
            boundary_marker: coco_messages::create_compact_boundary_message(
                pre_tokens,
                post_tokens,
            ),
            raw_summary: None,
            summary_messages: Vec::new(),
            attachments: Vec::new(),
            messages_to_keep: Vec::new(),
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
            .notify_post_compact(history.as_slice())
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

        // Reactive compact also rewrites history (peels oldest API-round
        // groups, drops attachments) — so it must reset the memory
        // recall state AND clear the SM cache, same as the full / SM-first
        // / partial compact paths. Without this, a long session that
        // survives a PTL retry inherits a saturated `total_bytes` and
        // stale `already_surfaced` set, silently killing recall for the
        // rest of the session.
        if let Some(rt) = &self.memory_runtime {
            rt.reset_recall_state();
            rt.session_memory.clear_after_compact().await;
        }

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
        // Periodic terminal-task eviction. Fires every turn,
        // regardless of success / failure / cancellation outcome —
        // matches TS `applyTaskOffsetsAndEvictions`
        // (`utils/task/framework.ts:213-249`) cadence inside
        // `getAttachments`, which TS calls on every turn boundary
        // regardless of how the turn ended. Without a periodic sweep,
        // `TaskManager`'s in-memory map grows monotonically over a
        // long session. The panel-grace gate is enforced inside
        // (`remove_completed` keeps `retain == true` or
        // `evict_after > now` tasks).
        if let Some(running) = self.running_tasks.as_ref() {
            let removed = running.remove_completed().await;
            if removed > 0 {
                tracing::trace!(
                    target: "coco_query::task_runtime",
                    removed,
                    "per-turn evicted terminal tasks past panel-grace"
                );
            }
        }

        // Tool-use-summary side-fork — TS `query.ts:1411-1482` spawns
        // **immediately** after `query_tool_execution_end`, BEFORE any
        // post-tool processing (queue drain, microcompact, auto-compact,
        // memory fan-out). The spawn captures the just-executed batch
        // (last assistant + matching tool results) from `history`; any
        // later compaction would summarize history and lose the batch
        // we want to label.
        //
        // Gated on:
        //   * `Feature::ToolUseSummary` enabled (default off — UX polish
        //     that silently degrades on reasoning Fast models)
        //   * `role_client_cache` wired (Fast role configured)
        //   * `agent_id.is_none()` (subagent skip — TS query.ts:1419)
        //   * tool batch non-empty (handled inside the spawn helper)
        // Never blocks; failure modes degrade to `None`.
        self.spawn_tool_use_summary(history).await;

        // Bump the per-engine turn counter so RecompactionInfo can derive
        // `turns_since_previous` accurately. TS: `compact.ts:317-323`.
        self.turn_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Drain command queue: all priorities land before the next API
        // call. Slash commands excluded (processed post-turn).
        // Agent-filtered.
        //
        // The queue carries every steering producer through one pipe:
        // human keyboard input (`QueueOrigin::Human`), coordinator
        // teammate messages (`QueueOrigin::Coordinator`), background
        // task completions (`QueueOrigin::TaskNotification`), and MCP
        // channel messages (`QueueOrigin::Channel`). Each item drains
        // into history as a `Message::Attachment` of kind
        // `QueuedCommand` with origin-specific framing prepended via
        // `wrap_command_text` — TS parity with
        // `messageQueueManager.ts` (human prompts) +
        // `getAgentPendingMessageAttachments`
        // (`attachments.ts:1085-1100`, coordinator messages, all of
        // which TS surfaces as `attachment.type === 'queued_command'`).
        drain_command_queue_into_history(
            &self.command_queue,
            history,
            event_tx,
            QueuePriority::Later,
            None,
        )
        .await;

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
                let pre_tb_tokens = coco_compact::estimate_tokens(history.as_slice());
                if let Some(res) = history.with_owned_messages(|msgs| {
                    coco_compact::time_based_microcompact(msgs, &trigger)
                }) {
                    info!(
                        cleared = res.messages_cleared,
                        gap_min = trigger.gap_minutes,
                        "time-based micro-compaction triggered",
                    );
                    let post_tb_tokens = coco_compact::estimate_tokens(history.as_slice());
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
            let _ =
                history.with_owned_messages(|msgs| coco_compact::clear_file_unchanged_stubs(msgs));
        }

        // Compute message-level stats once and share across the
        // auto-memory fan-out and the auto-compact threshold check
        // below — both read the same post-Step-0.5 history.
        let estimated_tokens = coco_compact::estimate_tokens(history.as_slice());
        let tool_calls_last_turn =
            coco_messages::count_tool_calls_in_last_assistant_turn(history.as_slice());

        // Stop-hooks gate (TS `query/stopHooks.ts:136-157`): bare
        // mode skips the entire post-turn fan-out (promptSuggestion +
        // extractMemories + sessionMemory + autoDream). Used by
        // `--bare` SDK / scripted `-p` invocations that don't want
        // background work after each turn.
        let bare_mode_active = coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBareMode);

        // Auto-memory turn-end fan-out — black-boxed through
        // `MemoryRuntime::finalize_turn`. The engine pre-computes
        // everything that needs `MessageHistory` (cursors, counts,
        // fork closures) and hands them through the context; the
        // runtime does the SM + extract + dream fan-out and returns
        // a typed report. Engine then projects notices into history
        // and acts on the KAIROS rollover signal.
        if let Some(runtime) = self.memory_runtime.clone() {
            let report = self
                .build_memory_finalize_ctx_and_run(
                    history,
                    estimated_tokens,
                    tool_calls_last_turn,
                    bare_mode_active,
                    &runtime,
                )
                .await;
            for notice in report.notices {
                let msg =
                    coco_messages::Message::System(coco_messages::SystemMessage::MemorySaved(
                        coco_messages::SystemMemorySavedMessage {
                            uuid: uuid::Uuid::new_v4(),
                            written_paths: notice.written_paths,
                            verb: notice.verb.as_str().to_string(),
                        },
                    ));
                crate::history_sync::history_push_and_emit(history, msg, event_tx).await;
            }
            // KAIROS midnight-rollover signal. The memory crate has
            // already advanced its latch and emitted
            // `MemoryEvent::KairosRollover` telemetry; the engine logs
            // the event under a dedicated target so resume / replay
            // can correlate the day flip with downstream actions.
            // The generic `date_change` system-reminder is independent
            // (it fires for every session via `DateChangeGenerator`),
            // so we don't need to inject a reminder here.
            if let Some(yesterday) = report.kairos_rollover {
                tracing::info!(
                    target: "coco_query::kairos_rollover",
                    yesterday = %yesterday.format("%Y-%m-%d"),
                    session_id = %self.config.session_id,
                    "KAIROS daily-log rollover detected",
                );
            }
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
            let pre_count = history.len() as i32;
            let pre_micro_tokens = estimated_tokens;
            if self.config.compact.micro.enabled && self.config.compact.micro.count_based_enabled {
                history.with_owned_messages(|msgs| {
                    coco_compact::micro_compact(msgs, micro_keep);
                });
                info!("auto micro-compaction triggered (keep_recent={micro_keep})");
            }
            let removed = (pre_count - history.len() as i32).max(0);
            let post_micro_tokens = coco_compact::estimate_tokens(history.as_slice());
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
                let should_attempt_auto = {
                    let state = self.auto_compact_state.lock().await;
                    state.should_attempt_reactive_compact()
                };
                if should_attempt_auto {
                    // Step 2 → 3: SM-first → full LLM. `try_full_compact` owns the
                    // branch internally so manual `/compact` benefits too.
                    let outcome = self
                        .try_full_compact(
                            history,
                            event_tx,
                            coco_types::CompactTrigger::Auto,
                            /*custom_instructions*/ None,
                        )
                        .await;
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let mut state = self.auto_compact_state.lock().await;
                    match outcome {
                        coco_compact::CompactOutcome::Applied => state.record_success(now_ms),
                        coco_compact::CompactOutcome::Failed => state.record_failure(now_ms),
                        coco_compact::CompactOutcome::Skipped => {}
                    }
                } else {
                    warn!(
                        threshold = coco_compact::types::MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES,
                        "auto compaction skipped after repeated failures"
                    );
                }
            }
        }

        self.finalize_successful_turn_tail(history, event_tx, turn_id, usage)
            .await;
    }

    /// Shared successful model-turn tail. Branch-specific work (tool
    /// execution, queue drain, auto-compaction, stop hooks) happens before
    /// this point; every successful turn still needs the same cache snapshot,
    /// transcript flush, and protocol completion event.
    pub(crate) async fn finalize_successful_turn_tail(
        &self,
        history: &mut MessageHistory,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        turn_id: String,
        usage: TokenUsage,
    ) {
        self.flush_successful_turn_state(history).await;
        self.emit_successful_turn_completed(event_tx, history, turn_id, usage)
            .await;
    }

    /// Emit the protocol completion events for a successful model turn.
    ///
    /// Keep this separate from [`Self::flush_successful_turn_state`] because
    /// some no-tool terminal paths intentionally flush before prompt
    /// suggestion and Stop hooks. The completion invariant still belongs in
    /// one place: reasoning metadata, when reported by the provider, must be
    /// anchored by message UUID before `TurnCompleted` lets the TUI render the
    /// completed turn.
    pub(crate) async fn emit_successful_turn_completed(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        history: &MessageHistory,
        turn_id: String,
        usage: TokenUsage,
    ) {
        self.emit_reasoning_metadata_for_last_assistant(event_tx, history, &usage, None)
            .await;
        self.emit_turn_completed(event_tx, turn_id, usage, history.len())
            .await;
    }

    /// Persist successful-turn state that must be current before any
    /// post-turn forks read the parent cache slot. Kept separate from
    /// `TurnCompleted` emission so text-only exits can run promptSuggestion
    /// after cache save but before closing the protocol turn.
    pub(crate) async fn flush_successful_turn_state(&self, history: &mut MessageHistory) {
        // D8: snapshot post-turn cache-safe params for future
        // post-turn fork features (`/btw`, `promptSuggestion`,
        // `postTurnSummary`). TS parity: `handleStopHooks` calls
        // `saveCacheSafeParams` here. Helper handles the empty-history
        // skip + serialisation.
        self.save_post_turn_cache_params(history).await;

        // Per-turn JSONL transcript append. Walks `history` and writes
        // any user/assistant/system/attachment message whose uuid isn't
        // already in the cross-engine dedup set. Skips silently when
        // the store / session id / dedup set aren't all wired (e.g.
        // tests, headless runs without persistence). TS parity:
        // `Project.recordTranscript` flushes the message list through a
        // single deduping writer instead of splitting by tool/text turns.
        self.record_transcript_tail(history).await;
    }

    pub(crate) async fn emit_turn_completed(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        turn_id: String,
        usage: TokenUsage,
        history_len: usize,
    ) {
        info!(
            turn_id = %turn_id,
            tokens_in = usage.input_tokens.total,
            tokens_out = usage.output_tokens.total,
            history_len,
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

    /// Emit `ReasoningMetadataAttached` so the TUI side-cache can anchor
    /// reasoning aggregates by the assistant message UUID rather than
    /// re-walking transcript cells. F3 of the unified-transcript plan
    /// — eliminates the prior "find latest AssistantThinking cell"
    /// scan in the TUI handler.
    pub(crate) async fn emit_reasoning_metadata_for_last_assistant(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
        history: &MessageHistory,
        usage: &TokenUsage,
        duration_ms: Option<i64>,
    ) {
        if usage.output_tokens.reasoning <= 0 {
            if let Some(assistant) = history.iter().rev().find_map(|m| match m.as_ref() {
                coco_messages::Message::Assistant(a) => Some(a),
                coco_messages::Message::User(_)
                | coco_messages::Message::System(_)
                | coco_messages::Message::Attachment(_)
                | coco_messages::Message::ToolResult(_)
                | coco_messages::Message::Progress(_)
                | coco_messages::Message::Tombstone(_) => None,
            }) && let coco_messages::LlmMessage::Assistant { content, .. } = &assistant.message
            {
                let mut reasoning_chars = 0;
                let mut text_chars = 0;
                let mut tool_call_count = 0;
                for part in content {
                    match part {
                        coco_llm_types::AssistantContentPart::Reasoning(r) => {
                            reasoning_chars += r.text.len();
                        }
                        coco_llm_types::AssistantContentPart::Text(t) => {
                            text_chars += t.text.len();
                        }
                        coco_llm_types::AssistantContentPart::ToolCall(_) => {
                            tool_call_count += 1;
                        }
                        coco_llm_types::AssistantContentPart::File(_)
                        | coco_llm_types::AssistantContentPart::ReasoningFile(_)
                        | coco_llm_types::AssistantContentPart::Custom(_)
                        | coco_llm_types::AssistantContentPart::ToolResult(_)
                        | coco_llm_types::AssistantContentPart::Source(_)
                        | coco_llm_types::AssistantContentPart::ToolApprovalRequest(_) => {}
                    }
                }
                if reasoning_chars > 0 {
                    tracing::debug!(
                        message_uuid = %assistant.uuid,
                        model = %assistant.model,
                        stop_reason = ?assistant.stop_reason,
                        tokens_out = usage.output_tokens.total,
                        text_tokens = usage.output_tokens.text,
                        reasoning_tokens = usage.output_tokens.reasoning,
                        reasoning_chars,
                        text_chars,
                        tool_call_count,
                        "assistant reasoning text present without reasoning token usage"
                    );
                }
            }
            return;
        }
        let Some(last_assistant_uuid) = history.iter().rev().find_map(|m| match m.as_ref() {
            coco_messages::Message::Assistant(a) => Some(a.uuid),
            _ => None,
        }) else {
            return;
        };
        let _ = emit_protocol(
            event_tx,
            ServerNotification::ReasoningMetadataAttached(
                coco_types::ReasoningMetadataAttachedParams {
                    message_uuid: last_assistant_uuid.to_string(),
                    duration_ms,
                    reasoning_tokens: usage.output_tokens.reasoning,
                },
            ),
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
        let cwd_path = std::env::current_dir().unwrap_or_default();
        let cwd = cwd_path.display().to_string();
        // TS-parity: `sessionStorage.ts:1013-1019` captures `getBranch()`
        // once per chain and stamps it on every line. Treat a git
        // failure (not in a repo, command missing) as `None` so the
        // field is omitted rather than producing an empty string.
        let git_branch = coco_git::get_current_branch(&cwd_path)
            .ok()
            .flatten()
            .filter(|s| !s.is_empty());
        let now = chrono::Utc::now().to_rfc3339();
        let options = coco_session::storage::ChainWriteOptions {
            cwd,
            timestamp: now,
            is_sidechain: self.config.agent_id.is_some(),
            agent_id: self.config.agent_id.clone(),
            starting_parent_uuid: None,
            git_branch,
        };
        if let Err(e) = store.append_message_chain(
            sid,
            history.iter().map(AsRef::as_ref),
            &mut seen_guard,
            options,
        ) {
            warn!(error = %e, "failed to append transcript chain");
        }
    }

    /// Spawn a `ModelRole::Fast` side-fork to summarize the tool batch
    /// that just completed. Stores the [`tokio::task::JoinHandle`] on
    /// [`QueryEngine::pending_tool_use_summary`] so the await site at
    /// the top of the next `run_session_loop` iteration can drain it.
    ///
    /// TS parity: `query.ts:1411-1482` spawns
    /// `generateToolUseSummary({ tools, signal, lastAssistantText, … })`
    /// and stashes the Promise on `nextPendingToolUseSummary`.
    ///
    /// Silently no-ops when:
    ///   * `Feature::ToolUseSummary` is disabled (default — see
    ///     `coco_types::features` for the rationale)
    ///   * `role_client_cache` is `None` (no Fast role wired)
    ///   * `agent_id` is `Some` (subagent skip — mirrors TS
    ///     `!toolUseContext.agentId` at query.ts:1419)
    ///   * `history` has no tool calls in the last assistant turn
    ///     (nothing to summarize)
    ///
    /// Replacing any prior pending handle aborts it first — defense
    /// against orphan tasks if `run_session_loop` skipped its await
    /// (e.g. early cancel between turns).
    pub(crate) async fn spawn_tool_use_summary(&self, history: &MessageHistory) {
        if !self
            .config
            .features
            .enabled(coco_types::Feature::ToolUseSummary)
        {
            return;
        }
        if self.config.agent_id.is_some() {
            return;
        }
        let Some(role_cache) = self.role_client_cache.clone() else {
            return;
        };
        let Some(input) = crate::tool_use_summary::build_input_from_history(history.as_slice())
        else {
            return;
        };
        if !input.has_tools() {
            return;
        }

        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            // Tie the fork to the parent's cancellation. When the user
            // hits Esc, the side-fork doesn't keep running after the
            // turn loop exits.
            tokio::select! {
                _ = cancel.cancelled() => None,
                result = crate::tool_use_summary::generate_tool_use_summary(input, role_cache) => result,
            }
        });

        let mut slot = self.pending_tool_use_summary.lock().await;
        if let Some(prev) = slot.replace(handle) {
            prev.abort();
        }
    }

    /// Drain the pending tool-use-summary fork at the top of a new
    /// iteration. On success, emits `ServerNotification::ToolUseSummary`
    /// for SDK consumers; the TUI side-caches the payload without
    /// writing it to `MessageHistory` (per I-3: tool-use summaries are
    /// UI-only polish and must not pollute the authoritative
    /// transcript). On `None` / join-error, silent skip — TS parity
    /// `.catch(() => null)` at query.ts:1481.
    ///
    /// **No drain-side timeout, no drain-side cancel guard**:
    ///
    /// - The inner [`crate::tool_use_summary::generate_tool_use_summary`]
    ///   caps work via `tokio::time::timeout(10s, …)` which DROPS the
    ///   future on expiry, so the JoinHandle always resolves within
    ///   ~10 s + tiny overhead. Adding a separate (shorter) drain
    ///   timeout would discard summaries that completed at 2–10 s,
    ///   wasting the tokens we already spent.
    /// - Parent cancellation is honored by the spawn's own
    ///   `tokio::select!` on `cancel.cancelled()` — on session cancel
    ///   the inner future is dropped and the handle resolves to
    ///   `Ok(None)` near-instantly. The drain just awaits.
    ///
    /// TS parity: `await pendingToolUseSummary` at query.ts:1056 has
    /// no timeout — the expected case (per TS line 1054 comment) is
    /// "haiku (~1s) resolved during model streaming (5-30s)" so the
    /// await is a no-op in practice.
    pub(crate) async fn drain_pending_tool_use_summary(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) {
        let handle = {
            let mut slot = self.pending_tool_use_summary.lock().await;
            slot.take()
        };
        let Some(handle) = handle else {
            return;
        };
        let params = match handle.await {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(join_err) => {
                tracing::debug!(error = %join_err, "tool_use_summary task join error");
                return;
            }
        };

        // Wire-level SDK emission: `tool/useSummary` notification. No
        // transcript entry — UI consumers (TUI) cache the summary by
        // `preceding_tool_use_ids` and render it as overlay polish.
        let _ = emit_protocol(event_tx, ServerNotification::ToolUseSummary(params)).await;
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
    /// TS parity: `query.ts` calls `handleStopHooks()` only when the
    /// assistant did not request follow-up tool execution; `stopHooks.ts`
    /// then starts `executePromptSuggestion()` under the non-bare gate.
    pub(crate) async fn maybe_spawn_prompt_suggestion_after_stop(
        &self,
        event_tx: &Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) {
        if coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBareMode) {
            return;
        }
        let Some(app_state) = self.app_state.as_ref() else {
            return;
        };
        prune_stale_rate_limits(app_state).await;
        self.spawn_prompt_suggestion_task(app_state.clone(), event_tx.clone())
            .await;
    }

    /// TS parity: `services/PromptSuggestion/promptSuggestion.ts`
    /// calls `runForkedAgent` with the bespoke suggestion prompt as a
    /// user message and `effort: undefined` (cache parity preserved).
    async fn spawn_prompt_suggestion_task(
        &self,
        app_state: std::sync::Arc<tokio::sync::RwLock<coco_types::ToolAppState>>,
        event_tx: Option<tokio::sync::mpsc::Sender<CoreEvent>>,
    ) {
        let cache = match self.last_cache_safe_params().await {
            Some(c) => c,
            None => return,
        };
        let dispatcher = match self.fork_dispatcher.clone() {
            Some(d) => d,
            None => return,
        };

        // Build the 9-step `SuggestionContext` from the parent's
        // cache + app_state snapshot BEFORE spawning. The pre-fork
        // guards (TooFewTurns / ApiError / CacheCold / suppress
        // reasons) save the API round-trip when they fire — TS
        // parity: `services/PromptSuggestion/promptSuggestion.ts:136-163`
        // runs these checks before `generateSuggestion`.
        let ctx = build_suggestion_context(
            &cache,
            &app_state,
            self.config.is_non_interactive,
            self.config.is_teammate,
        )
        .await;
        if let Some(outcome) = crate::prompt_suggestion::pre_fork_guards(&ctx, false) {
            tracing::debug!(
                outcome = ?outcome,
                "promptSuggestion suppressed by pre-fork guard"
            );
            return;
        }

        // TS `currentAbortController` singleton: cancel any prior
        // in-flight suggestion fork before starting a new one. This
        // means rapid `/clear` cycles don't accumulate fork tasks
        // burning tokens. Allocate a fresh token, store it under the
        // session-scoped slot, hand a clone to the spawn so the next
        // spawn can cancel cleanly.
        let abort_token = tokio_util::sync::CancellationToken::new();
        if let Some(slot) = self.current_suggestion_abort.as_ref() {
            let mut guard = slot.lock().await;
            if let Some(prev) = guard.replace(abort_token.clone()) {
                prev.cancel();
            }
        }

        // Detach: the suggestion is fire-and-forget. The parent turn is
        // finalizing; we don't want a slow suggestion fork blocking the
        // next user prompt.
        let abort_for_task = abort_token.clone();
        tokio::spawn(async move {
            // Bail if a newer spawn already cancelled this fork before
            // we got scheduled.
            if abort_for_task.is_cancelled() {
                return;
            }
            // Install deny-all canUseTool so the fork can't actually
            // invoke tools (TS: `runForkedAgent({canUseTool: deny-all})`
            // at promptSuggestion.ts:302-306).
            let mut options = crate::forked_agent::ForkedAgentOptions::for_label(
                coco_types::ForkLabel::PromptSuggestion,
            );
            options.can_use_tool = Some(crate::forked_agent::deny_all_handle(
                "prompt suggestion: tools disabled",
            ));
            options.overrides.abort = Some(abort_for_task.clone());
            let prompt = crate::prompt_suggestion::build_suggestion_system_prompt().to_string();
            // The fork sees the parent's system prompt/cache-key
            // params unchanged; the suggestion instruction is appended
            // as the fork's user message, matching TS runForkedAgent.
            let result = dispatcher.dispatch(&cache, &options, &prompt, None).await;
            match result {
                Ok(r) => {
                    // Multi-message text walk (TS:332-349 — "model
                    // may loop (try tool → denied → text in next
                    // message)"). Walks every assistant message and
                    // finds the first non-empty text block.
                    let generation =
                        crate::prompt_suggestion::extract_suggestion_generation(&r.messages);
                    // Post-fork validation (steps 7-9): aborted /
                    // empty / NONE / 12-rule filter. TS:
                    // promptSuggestion.ts:171-181.
                    let aborted_after = abort_for_task.is_cancelled();
                    if let Some(outcome) = crate::prompt_suggestion::post_fork_validation(
                        &generation.text,
                        aborted_after,
                    ) {
                        tracing::debug!(
                            outcome = ?outcome,
                            text_len = generation.text.len(),
                            "promptSuggestion dropped by post-fork validation"
                        );
                        return;
                    }
                    let prompt_id = uuid::Uuid::new_v4().to_string();
                    let now = chrono::Utc::now().to_rfc3339();
                    let suggestion = generation.text.trim().to_string();
                    let mut state = app_state.write().await;
                    crate::prompt_suggestion::record_suggestion(
                        &mut state,
                        suggestion.clone(),
                        prompt_id,
                        now,
                        generation.request_id,
                    );
                    drop(state);
                    let _delivered = emit_protocol(
                        &event_tx,
                        ServerNotification::PromptSuggestion {
                            suggestions: vec![suggestion],
                        },
                    )
                    .await;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "promptSuggestion fork dispatch failed");
                }
            }
        });
    }

    /// Build the `FinalizeTurnContext` from engine-side state and
    /// dispatch into `MemoryRuntime::finalize_turn`. The runtime
    /// black-boxes the SM + extract + dream + KAIROS-rollover +
    /// post-write-classify fan-out and returns notices for the engine
    /// to project into history. Subagent gating (`agent_id.is_some()`)
    /// is folded into `is_subagent` rather than a guard at this layer
    /// so the runtime owns the rule.
    pub(crate) async fn build_memory_finalize_ctx_and_run(
        &self,
        history: &MessageHistory,
        estimated_tokens: i64,
        tool_calls_last_turn: i32,
        bare_mode: bool,
        runtime: &Arc<coco_memory::MemoryRuntime>,
    ) -> coco_memory::runtime::FinalizeTurnReport {
        // Pre-compute everything that needs `MessageHistory`. The
        // runtime never re-walks history.
        let last_cursor: Option<String> = runtime.extract.last_cursor().await;
        let sm_cursor: Option<String> = runtime.session_memory.last_extraction_message_id().await;
        let tool_calls_since_sm = count_tool_calls_since(history.as_slice(), sm_cursor.as_deref());
        let last_msg_id = history
            .last()
            .and_then(|m| m.uuid())
            .map(uuid::Uuid::to_string);
        let extract_message_count =
            count_model_visible_since(history.as_slice(), last_cursor.as_deref());

        // Two fresh `messages` clones for the FnOnce closures inside
        // TurnInput. fork_messages and has_memory_writes are evaluated
        // lazily by ExtractService and may fire on the primary OR a
        // trailing stash — both branches need an independent snapshot.
        let messages_for_fork = history.to_vec();
        let messages_for_writes_check = history.to_vec();
        let memory_dir = runtime.personal_dir().to_path_buf();
        let last_cursor_for_writes_check = last_cursor.clone();
        let last_cursor_for_fork = last_cursor.clone();

        let extract_input = coco_memory::service::extract::TurnInput {
            fork_messages: Box::new(move || {
                arc_messages_since(&messages_for_fork, last_cursor_for_fork.as_deref())
            }),
            message_count: extract_message_count,
            last_message_id: last_msg_id.clone(),
            has_memory_writes: Box::new(move || {
                main_agent_wrote_memory(
                    &messages_for_writes_check,
                    &memory_dir,
                    last_cursor_for_writes_check.as_deref(),
                )
            }),
        };

        // Gap 4 — direct-edit toast. Walk the just-finished assistant
        // turn for Write/Edit/NotebookEdit calls and pair each with its
        // matching ToolResult so memory's `classify_written_path` pass
        // can decide whether to emit a `ManualEdit` notice. TS parity:
        // `services/useMemoryUpdateNotification` (UI post-write hook).
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let recent_tool_writes = extract_recent_tool_writes(history.as_slice(), &cwd);

        let ctx = coco_memory::runtime::FinalizeTurnContext {
            estimated_tokens,
            tool_calls_since_sm_cursor: tool_calls_since_sm,
            tool_calls_last_turn,
            last_message_id: last_msg_id,
            auto_compact_enabled: self.config.is_auto_compact_active(),
            bare_mode,
            is_subagent: self.config.agent_id.is_some(),
            now_ms: coco_memory::service::dream::DreamService::now_ms(),
            extract_input,
            recent_tool_writes,
        };

        runtime.finalize_turn(ctx).await
    }
}

/// Walk the last assistant turn for Write / Edit / NotebookEdit tool
/// calls and pair each with its matching `ToolResult` so memory's
/// post-write classification (Gap 4) can decide whether the call
/// produced a `ManualEdit` notice.
///
/// Why only the last assistant turn: notices fire once per turn, so
/// older history was already classified on its own finalize. The
/// matching cost would be `O(history.len())` if we walked the full
/// transcript without buying any extra notices.
///
/// Success is read off `ToolResultMessage.is_error` — the only signal
/// the engine reliably has post-execution. Skipping failed writes
/// matches TS, which only fires the post-write hook on successful
/// file mutations.
///
/// Relative paths are anchored to `cwd` so the downstream
/// `is_within_memory_dir` check (which canonicalises) sees an absolute
/// path. Mirrors the resolution rule already used by `main_agent_wrote_memory`.
fn extract_recent_tool_writes<M: std::borrow::Borrow<coco_messages::Message>>(
    messages: &[M],
    cwd: &std::path::Path,
) -> Vec<coco_memory::runtime::ToolWriteRecord> {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use coco_messages::Message;
    use std::collections::HashMap;

    let Some(last_assistant_idx) = messages
        .iter()
        .rposition(|m| matches!(m.borrow(), Message::Assistant(_)))
    else {
        return Vec::new();
    };
    let Message::Assistant(last_assistant) = messages[last_assistant_idx].borrow() else {
        return Vec::new();
    };
    let LlmMessage::Assistant { content, .. } = &last_assistant.message else {
        return Vec::new();
    };

    // First pass: collect (tool_call_id, tool_name, file_path) from
    // ToolCall parts that name a write tool with a parseable path.
    // Compare against the typed `ToolName` constants — no raw literals.
    let mut pending: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    for part in content {
        let AssistantContent::ToolCall(tc) = part else {
            continue;
        };
        let name = tc.tool_name.as_str();
        let is_write_tool = name == coco_types::ToolName::Write.as_str()
            || name == coco_types::ToolName::Edit.as_str()
            || name == coco_types::ToolName::NotebookEdit.as_str();
        if !is_write_tool {
            continue;
        }
        let Some(file_path_str) = tc
            .input
            .get("file_path")
            .or_else(|| tc.input.get("notebook_path"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let path = std::path::Path::new(file_path_str);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        pending.push((tc.tool_call_id.clone(), name.to_string(), absolute));
    }
    if pending.is_empty() {
        return Vec::new();
    }

    // Index ToolResultMessages after the assistant turn by tool_use_id.
    // Tool results may arrive in any order; build a map then look up.
    let mut results: HashMap<&str, bool> = HashMap::new();
    for msg in &messages[last_assistant_idx + 1..] {
        if let Message::ToolResult(tr) = msg.borrow() {
            results.insert(tr.tool_use_id.as_str(), !tr.is_error);
        }
    }

    pending
        .into_iter()
        .map(
            |(id, tool_name, file_path)| coco_memory::runtime::ToolWriteRecord {
                tool_name,
                file_path,
                // No result yet ⇒ treat as failed; we only emit toasts
                // for confirmed successful writes.
                succeeded: results.get(id.as_str()).copied().unwrap_or(false),
            },
        )
        .collect()
}

/// Build a [`crate::prompt_suggestion::SuggestionContext`] from the
/// parent engine's cache slot + app_state snapshot. Used by the
/// pre-fork guards to short-circuit before the API round-trip.
///
/// `assistant_turn_count` and `last_response_was_api_error` come from
/// deserializing the cache slot's `fork_context_messages`;
/// `parent_uncached_tokens` is the last assistant's
/// `input - cache_read_input + output` tokens (TS
/// `getParentCacheSuppressReason`). Other fields come from
/// `ToolAppState`.
async fn build_suggestion_context(
    cache: &coco_types::CacheSafeParams,
    app_state: &std::sync::Arc<tokio::sync::RwLock<coco_types::ToolAppState>>,
    is_non_interactive: bool,
    is_teammate: bool,
) -> crate::prompt_suggestion::SuggestionContext {
    let mut assistant_turn_count: u32 = 0;
    let mut last_assistant_msg: Option<&coco_messages::AssistantMessage> = None;
    for arc in &cache.fork_context_messages {
        if let coco_messages::Message::Assistant(a) = arc.as_ref() {
            assistant_turn_count = assistant_turn_count.saturating_add(1);
            last_assistant_msg = Some(a);
        }
    }

    let (last_response_was_api_error, parent_uncached_tokens) = match last_assistant_msg {
        Some(a) => {
            let api_error = a.api_error.is_some();
            let usage = a.usage.unwrap_or_default();
            let tokens = crate::prompt_suggestion::parent_uncached_tokens(&usage);
            (api_error, tokens)
        }
        None => (false, 0),
    };

    let snap = app_state.read().await;
    let plan_mode = matches!(snap.permission_mode, Some(coco_types::PermissionMode::Plan));
    let awaiting_plan_approval = snap.awaiting_plan_approval;
    // Phase 7 wire-up: read live counters from `ToolAppState`. Both
    // counters are `Arc<AtomicU32>`, mutated lock-free by RAII guards
    // held by the TUI permission bridge (`pending_permission_count`)
    // and the MCP elicitation service (`elicitation_pending_count`).
    let pending_permission = snap
        .pending_permission_count
        .load(std::sync::atomic::Ordering::Relaxed)
        > 0;
    let elicitation_active = snap
        .elicitation_pending_count
        .load(std::sync::atomic::Ordering::Relaxed)
        > 0;
    // Phase 7c: selective rate-limit suppression — `rate_limits` is
    // keyed by provider instance name; we look up the cache's
    // recorded provider so fast-mode swaps are honoured (the parent
    // turn captured the literally-active provider).
    let now_ms = chrono::Utc::now().timestamp_millis();
    let rate_limit = if cache.provider.is_empty() {
        // Pre-Phase-7 transcripts may carry empty `provider` (serde
        // default). Without a key we can't match selectively; fail
        // open (no suppression) to avoid silencing all suggestions.
        false
    } else {
        snap.rate_limits
            .get(&cache.provider)
            .map(|e| {
                matches!(e.status, coco_types::RateLimitStatus::Rejected)
                    && e.reset_at_ms.is_none_or(|r| now_ms < r)
            })
            .unwrap_or(false)
    };
    let env_disable =
        coco_config::env::is_env_truthy(coco_config::EnvKey::CocoPromptSuggestionDisable);
    let bare_mode = coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBareMode);
    drop(snap);

    crate::prompt_suggestion::SuggestionContext {
        assistant_turn_count,
        last_response_was_api_error,
        parent_uncached_tokens,
        disabled: env_disable,
        pending_permission,
        is_teammate,
        awaiting_plan_approval,
        elicitation_active,
        plan_mode,
        rate_limit,
        bare_mode,
        non_interactive: is_non_interactive,
    }
}

/// Slice the message history to "everything newer than `last_cursor`"
/// for `AgentSpawnRequest::fork_context_messages`. When `last_cursor`
/// is `None` (first extraction), return the full history.
///
/// TS parity: `messagesSinceCursor` in `services/extractMemories/`.
/// Takes the engine's already-shared `Arc<Message>` slice and
/// `Arc::clone`s each entry — no deep `Message` body clones at
/// this seam.
fn arc_messages_since(
    messages: &[std::sync::Arc<coco_messages::Message>],
    last_cursor: Option<&str>,
) -> Vec<std::sync::Arc<coco_messages::Message>> {
    let cursor_idx = last_cursor.and_then(|c| {
        messages
            .iter()
            .position(|m| m.uuid().map(|u| u.to_string() == c).unwrap_or(false))
    });
    let slice = match cursor_idx {
        Some(i) => &messages[i + 1..],
        None => messages,
    };
    slice.to_vec()
}

/// Count user + assistant messages strictly after `since_uuid` —
/// TS `countModelVisibleMessagesSince` (`extractMemories.ts:82-110`).
/// "Model-visible" = anything sent in API calls; excludes progress,
/// system, attachment, tombstone, tool_use_summary. Threaded into
/// the extraction agent's prompt so the "~N messages" guidance is
/// accurate (using `history.len()` would over-count).
///
/// Fall-through: when `since_uuid` is `None` or doesn't match any
/// message in `messages` (e.g. compaction trimmed the cursor), count
/// the whole history — matches TS so a stale cursor doesn't permanently
/// zero the count.
fn count_model_visible_since<M: std::borrow::Borrow<coco_messages::Message>>(
    messages: &[M],
    since_uuid: Option<&str>,
) -> i32 {
    use coco_messages::Message;
    let is_visible = |m: &Message| matches!(m, Message::User(_) | Message::Assistant(_));
    let cursor_idx = since_uuid.and_then(|c| {
        messages.iter().position(|m| {
            m.borrow()
                .uuid()
                .map(|u| u.to_string() == c)
                .unwrap_or(false)
        })
    });
    let start = match cursor_idx {
        Some(i) => i + 1,
        None => 0,
    };
    messages[start..]
        .iter()
        .filter(|m| is_visible(m.borrow()))
        .count() as i32
}

/// Count cumulative `tool_use` blocks across all assistant messages
/// strictly after `since_uuid` (or all messages when the cursor is
/// `None` / not found). TS parity with `countToolCallsSince`
/// (`services/SessionMemory/sessionMemory.ts:108-132`) — the gate
/// signal SessionMemoryService uses to decide if enough work has
/// accumulated since the last extraction.
fn count_tool_calls_since<M: std::borrow::Borrow<coco_messages::Message>>(
    messages: &[M],
    since_uuid: Option<&str>,
) -> i32 {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use coco_messages::Message;
    let cursor_idx = since_uuid.and_then(|c| {
        messages.iter().position(|m| {
            m.borrow()
                .uuid()
                .map(|u| u.to_string() == c)
                .unwrap_or(false)
        })
    });
    let start = match cursor_idx {
        Some(i) => i + 1,
        None => 0,
    };
    let mut count: i32 = 0;
    for msg in &messages[start..] {
        if let Message::Assistant(assistant) = msg.borrow()
            && let LlmMessage::Assistant { content, .. } = &assistant.message
        {
            for block in content {
                if matches!(block, AssistantContent::ToolCall(_)) {
                    count = count.saturating_add(1);
                }
            }
        }
    }
    count
}

/// Detect whether any assistant turn since `since_uuid` wrote into the
/// memory directory via Write / Edit / NotebookEdit. Used by
/// `ExtractService::maybe_extract` to skip extraction when the user
/// just curated memory directly — TS `hasMemoryWritesSince`
/// (`extractMemories.ts:121-148`). When `since_uuid` is `None` (or
/// the cursor uuid isn't found, e.g. compaction trimmed it), walk the
/// entire history — matches TS's fall-through that scans all
/// messages so a stale cursor doesn't permanently mask writes.
fn main_agent_wrote_memory<M: std::borrow::Borrow<coco_messages::Message>>(
    messages: &[M],
    memory_dir: &std::path::Path,
    since_uuid: Option<&str>,
) -> bool {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use coco_messages::Message;
    let cursor_idx = since_uuid.and_then(|c| {
        messages.iter().position(|m| {
            m.borrow()
                .uuid()
                .map(|u| u.to_string() == c)
                .unwrap_or(false)
        })
    });
    let start = match cursor_idx {
        Some(i) => i + 1,
        None => 0,
    };
    for msg in &messages[start..] {
        let Message::Assistant(assistant) = msg.borrow() else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &assistant.message else {
            continue;
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
    }
    false
}

/// Phase 7c: prune `rate_limits` entries whose `reset_at_ms` has
/// passed. Called from `finalize_turn_post_tools` immediately before
/// `spawn_prompt_suggestion_task` reads the map. Bounded keyspace
/// (≤ #configured providers) means pruning is O(few entries) per
/// finalize and there's no hot-path concern.
///
/// Entries with `reset_at_ms = None` (no reset header surfaced) are
/// retained — they get overwritten on the next successful or failing
/// call from the same provider. Bounded by the keyspace anyway.
async fn prune_stale_rate_limits(
    app_state: &std::sync::Arc<tokio::sync::RwLock<coco_types::ToolAppState>>,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut snap = app_state.write().await;
    snap.rate_limits
        .retain(|_, e| e.reset_at_ms.is_none_or(|r| r > now_ms));
}

#[cfg(test)]
#[path = "engine_finalize_turn.test.rs"]
mod tests;
