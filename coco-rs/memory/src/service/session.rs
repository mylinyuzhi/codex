//! Session-memory service: 9-section markdown insights per session.
//!
//! TS: `services/SessionMemory/sessionMemory.ts`. Distinct from compact's
//! `SessionMemoryConfig` — that one is a compact-time summary; this is
//! a structured 9-section document the model edits incrementally during
//! the conversation, capped at per-section + total-section token
//! budgets.
//!
//! Trigger gates:
//! - Init: total context tokens ≥ `session_memory_init_tokens` (once).
//! - Update: (token growth ≥ update threshold) AND ((tool calls ≥ 3) OR
//!   (no tool calls last turn — natural break)).
//!
//! Storage: one file per session at
//! `<memory_base>/projects/<slug>/<session_id>/session-memory/summary.md`,
//! mode 0o600, directory mode 0o700. The per-project slug pins the
//! file to the right project so unrelated sessions don't share state.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use arc_swap::ArcSwap;
use coco_paths::ProjectPaths;
use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::AgentSpawnConstraints;
use coco_tool_runtime::AgentSpawnRequest;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::compact_truncate::truncate_session_memory_for_compact;
use crate::config::MemoryConfig;
use crate::prompt::build_session_memory_template;
use crate::prompt::build_session_memory_update_prompt;
use crate::telemetry::MemoryEvent;
use crate::telemetry::MemoryTelemetryEmitter;
use crate::telemetry::NoopEmitter;

/// `wait_for_extraction` default timeout — TS 15s.
pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(15);

/// Stale extraction threshold — TS 60s. Past this we don't wait, the
/// extraction is presumed crashed.
pub const STALE_THRESHOLD: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct SessionState {
    initialized: bool,
    last_extraction_tokens: i64,
    last_extraction_tool_calls: i32,
    /// Last message UUID folded into a successful extraction. Cumulative
    /// tool-call counts are computed since this cursor — TS parity with
    /// `lastMemoryMessageUuid` in `services/SessionMemory/sessionMemory.ts`.
    /// Kept as `String` because the engine threads it through callsites
    /// that already operate on the JSON-string form; converting at the
    /// boundary is cheaper than UUID round-tripping every turn.
    last_extraction_message_uuid: Option<String>,
    /// Last message UUID up to which the SM file is **safely** caught
    /// up — only advances when the previous assistant turn had no
    /// tool calls, matching TS `updateLastSummarizedMessageIdIfSafe`
    /// (`sessionMemory.ts:488-494`). Used by compact/summary readers
    /// that need to know "where SM has covered to" without risking
    /// orphaned tool_results in a downstream summary.
    ///
    /// Stored as `String` because engine callsites already pass
    /// opaque message IDs (`MessageHistory::message_id()` is not
    /// guaranteed to be a UUID — synthetic IDs, test stubs, and
    /// pre-UUID legacy transcripts pass non-UUID strings). The
    /// typed accessor parses on read and silently returns `None`
    /// for non-UUID strings, preserving cursor semantics.
    last_summarized_message_uuid: Option<String>,
    in_progress: bool,
    extraction_started_at: Option<Instant>,
}

/// Per-session memory service.
pub struct SessionMemoryService {
    /// Current session id. Read on every `file_path()` call; mutated
    /// only on `/clear` (`set_session_id`). `ArcSwap` is the right
    /// primitive for this "rare write, frequent sync read" shape:
    /// reads are a single relaxed atomic load, no lock acquisition,
    /// no `.await`. The inner `Arc<String>` lets callers cheaply
    /// snapshot a stable view of the id even if a `/clear` lands
    /// mid-call.
    session_id: ArcSwap<String>,
    /// Per-project filesystem layout — resolves the canonical
    /// `<projectDir>/<sessionId>/session-memory/summary.md` path. TS
    /// parity: `getSessionMemoryPath()` in
    /// `utils/permissions/filesystem.ts:269`.
    project_paths: Arc<ProjectPaths>,
    config: MemoryConfig,
    agent: crate::service::extract::AgentSlot,
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    state: Mutex<SessionState>,
    /// In-memory text cache. Empty string ⇒ no extract yet / file missing.
    /// Populated by [`Self::load_from_disk`] at session start and
    /// refreshed after every successful extract. Compact's SM-first
    /// short-circuit reads this without touching disk.
    text_cache: tokio::sync::RwLock<String>,
    /// Notified each time `in_progress` flips false (extract finishes
    /// or fails). [`Self::wait_for_extraction`] uses this instead of
    /// a 50ms polling loop so callers don't burn ~300 mutex acquisitions
    /// per 15s wait. Tokio's `Notify` is permit-based so a notify
    /// landing before a waiter still wakes the next caller.
    extract_done: Notify,
}

impl std::fmt::Debug for SessionMemoryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionMemoryService")
            .field("session_id", &*self.session_id.load())
            .field("file_path", &self.file_path())
            .field(
                "session_memory_enabled",
                &self.config.session_memory_enabled,
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMemoryOutcome {
    Skipped(SkipReason),
    Completed { duration_ms: i64 },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    InProgress,
    BelowInitThreshold,
    BelowUpdateThreshold,
    NeitherToolCallsNorBreak,
}

impl SessionMemoryService {
    pub fn new(
        project_paths: Arc<ProjectPaths>,
        session_id: String,
        config: MemoryConfig,
        agent: AgentHandleRef,
    ) -> Self {
        Self::with_shared_agent(
            project_paths,
            session_id,
            config,
            Arc::new(std::sync::RwLock::new(agent)),
            Arc::new(NoopEmitter),
        )
    }

    /// Shared-cell constructor — used by [`crate::MemoryRuntimeBuilder`]
    /// so all three services see the same swappable handle.
    pub fn with_shared_agent(
        project_paths: Arc<ProjectPaths>,
        session_id: String,
        config: MemoryConfig,
        agent: crate::service::extract::AgentSlot,
        telemetry: Arc<dyn MemoryTelemetryEmitter>,
    ) -> Self {
        Self {
            session_id: ArcSwap::from_pointee(session_id),
            project_paths,
            config,
            agent,
            telemetry,
            state: Mutex::new(SessionState::default()),
            text_cache: tokio::sync::RwLock::new(String::new()),
            extract_done: Notify::new(),
        }
    }

    /// Resolved on-disk path of the session-memory summary file.
    /// Re-computed each call against the current session id so
    /// `/clear` regen lands subsequent reads / writes in the new
    /// session's directory. TS layout:
    /// `<projectDir>/<sessionId>/session-memory/summary.md`.
    pub fn file_path(&self) -> PathBuf {
        let id = self.read_session_id();
        self.project_paths.session_memory_summary(&id)
    }

    /// Update the session id used for disk paths. Called by
    /// `MemoryRuntime::reset` / `SessionRuntime::clear_conversation`
    /// after `regenerateSessionId`. Also wipes in-memory state and
    /// the text cache — the new session has no extracted content
    /// yet, and a stale cached body must not leak across sessions.
    ///
    /// Best-effort fence against in-flight extractions: we wait up to
    /// 5s for the running fork to settle so its `unmark_in_progress` and
    /// state update lands against the *old* session id, then reset.
    /// If the fork is genuinely stuck (rare) we proceed anyway — the
    /// stale `in_progress` flag would otherwise wedge every future
    /// gate check on the new session id. The fork's late state write
    /// (if it ever lands) overwrites the post-reset default with old-
    /// session state, but the user's next `maybe_extract` re-initializes
    /// from fresh tokens, so the leak window is bounded.
    pub async fn set_session_id(&self, new_id: String) {
        let _ = self.wait_for_extraction(Duration::from_secs(5)).await;
        self.session_id.store(Arc::new(new_id));
        self.text_cache.write().await.clear();
        *self.state.lock().await = SessionState::default();
    }

    /// Best-effort warm of the text cache from disk. Call once at
    /// session start (post-construction) so the SM-first compact
    /// short-circuit can read the cached body without a disk hit.
    /// Missing file ⇒ cache stays empty.
    pub async fn load_from_disk(&self) {
        let path = self.file_path();
        if let Ok(body) = tokio::fs::read_to_string(&path).await {
            *self.text_cache.write().await = body;
        }
    }

    /// Cached session-memory body. Empty string ⇒ no extract yet /
    /// file missing. Compact's SM-first short-circuit reads this
    /// to decide whether to dispatch full LLM summarization or
    /// reuse the pre-extracted SM body.
    pub async fn current_text(&self) -> String {
        self.text_cache.read().await.clone()
    }

    /// Wipe in-memory state and text cache after a compaction
    /// completes — TS `clearAfterCompact` semantics. The on-disk
    /// file is left alone; the next extract overwrites it
    /// section-by-section via the forked-agent Edit pass.
    pub async fn clear_after_compact(&self) {
        self.text_cache.write().await.clear();
        let mut state = self.state.lock().await;
        state.initialized = false;
        state.last_extraction_tokens = 0;
        state.last_extraction_tool_calls = 0;
        state.last_extraction_message_uuid = None;
        state.last_summarized_message_uuid = None;
    }

    /// Override the safely-summarized cursor manually — used by the
    /// SM-first compact path to anchor the kept-tail boundary after
    /// a compact-and-keep-tail write. TS:
    /// `setLastSummarizedMessageId` (`sessionMemoryUtils.ts:44-69`).
    pub async fn set_last_summarized_message_id(&self, uuid: Option<Uuid>) {
        self.state.lock().await.last_summarized_message_uuid = uuid.map(|u| u.to_string());
    }

    /// Decide whether to fire a session-memory update.
    ///
    /// `tool_calls_since_last_extraction` mirrors TS
    /// `countToolCallsSince(messages, lastMemoryMessageUuid)` —
    /// **cumulative** across all turns since the last successful
    /// extraction (or session start), not just the last assistant
    /// turn. The engine computes this by walking from
    /// [`Self::last_extraction_message_id`].
    /// `had_tool_calls_in_last_turn` is the natural-break signal —
    /// TS `hasToolCallsInLastAssistantTurn(messages)`. When the last
    /// assistant turn used no tools, extraction can fire even when
    /// the cumulative tool-call gate hasn't met threshold. `last_message_id`
    /// is advanced into the cursor on a successful gate pass so the
    /// next call's cumulative count starts from the right boundary.
    pub async fn maybe_extract(
        &self,
        current_tokens: i64,
        tool_calls_since_last_extraction: i32,
        had_tool_calls_in_last_turn: bool,
        last_message_id: Option<String>,
    ) -> SessionMemoryOutcome {
        if !self.config.session_memory_enabled {
            return SessionMemoryOutcome::Skipped(SkipReason::Disabled);
        }
        let start = Instant::now();
        {
            let mut state = self.state.lock().await;
            if state.in_progress {
                return SessionMemoryOutcome::Skipped(SkipReason::InProgress);
            }
            if !state.initialized {
                if current_tokens < self.config.session_memory_init_tokens {
                    return SessionMemoryOutcome::Skipped(SkipReason::BelowInitThreshold);
                }
            } else {
                let token_growth = current_tokens - state.last_extraction_tokens;
                if token_growth < self.config.session_memory_update_tokens {
                    return SessionMemoryOutcome::Skipped(SkipReason::BelowUpdateThreshold);
                }
                let tool_call_gate =
                    tool_calls_since_last_extraction >= self.config.session_memory_tool_calls;
                let natural_break = !had_tool_calls_in_last_turn;
                if !tool_call_gate && !natural_break {
                    return SessionMemoryOutcome::Skipped(SkipReason::NeitherToolCallsNorBreak);
                }
            }
            // Gate passed. Claim the `in_progress` flag, stamp the
            // start time, and advance the cursor — all in the same
            // critical section, so a concurrent caller that lands
            // before we dispatch the fork still sees `in_progress =
            // true` and bails with `SkipReason::InProgress`. TS sets
            // `lastMemoryMessageUuid` + `inProgress` together inside
            // `shouldExtractMemory` (`sessionMemory.ts:174-176`) for
            // the same reason.
            state.in_progress = true;
            state.extraction_started_at = Some(start);
            if let Some(id) = &last_message_id {
                state.last_extraction_message_uuid = Some(id.clone());
            }
        }
        tracing::info!(
            current_tokens,
            tool_calls_since_last_extraction,
            had_tool_calls_in_last_turn,
            "session-memory extract dispatch"
        );
        self.run_with_label(
            start,
            current_tokens,
            tool_calls_since_last_extraction,
            last_message_id,
            had_tool_calls_in_last_turn,
            coco_types::ForkLabel::SessionMemoryAuto,
            /* already_marked */ true,
        )
        .await
    }

    /// Force a fresh extraction regardless of gates — `/summary`.
    ///
    /// `last_message_id` and `had_tool_calls_in_last_turn` mirror
    /// TS `manuallyExtractSessionMemory` →
    /// `updateLastSummarizedMessageIdIfSafe(messages)`
    /// (`sessionMemory.ts:441-442` + `488-494`): the summarized
    /// cursor only advances when the last assistant turn has no tool
    /// calls, so a downstream compact summary can't orphan
    /// tool_results. Callers that don't have those signals (legacy
    /// callers / minimal embeddings) should pass `None` + `false` —
    /// the cursor simply won't advance.
    pub async fn force(
        &self,
        current_tokens: i64,
        last_message_id: Option<String>,
        had_tool_calls_in_last_turn: bool,
    ) -> SessionMemoryOutcome {
        // TS parity (`sessionMemory.ts:436`
        // `tengu_session_memory_manual_extraction`) — the manual
        // /summary path emits its own telemetry event so the auto vs
        // manual cadence is measurable independently.
        self.telemetry
            .emit(MemoryEvent::SessionMemoryManualExtraction);
        self.run_with_label(
            Instant::now(),
            current_tokens,
            0,
            last_message_id,
            had_tool_calls_in_last_turn,
            coco_types::ForkLabel::SessionMemoryManual,
            /* already_marked */ false,
        )
        .await
    }

    /// Cursor uuid the engine should walk from when computing
    /// cumulative tool-call counts for the next [`Self::maybe_extract`]
    /// call. `None` until the first successful extraction.
    pub async fn last_extraction_message_id(&self) -> Option<String> {
        self.state.lock().await.last_extraction_message_uuid.clone()
    }

    /// Last "safely summarized" message UUID as a String — TS parity
    /// with `lastSummarizedMessageId` (`sessionMemoryUtils.ts:44-69`).
    /// Only advances after a successful run when the prior assistant
    /// turn had no tool calls, so compact / summary readers can use
    /// it as an orphan-safe cursor. `None` until the first eligible
    /// extraction completes.
    pub async fn last_summarized_message_id(&self) -> Option<String> {
        self.state.lock().await.last_summarized_message_uuid.clone()
    }

    /// Uuid-typed accessor — convenience for compact callers that
    /// hold the cursor as `Option<Uuid>`. Returns `None` for non-UUID
    /// strings (synthetic / legacy message IDs); callers that need
    /// the raw string go through [`Self::last_summarized_message_id`].
    pub async fn last_summarized_message_uuid(&self) -> Option<Uuid> {
        self.state
            .lock()
            .await
            .last_summarized_message_uuid
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
    }

    /// Whether the SM file currently holds nothing but the seed
    /// template — TS `isSessionMemoryEmpty` (`prompts.ts:220-224`).
    /// Compact readers use this to fall back to LLM summarization
    /// when SM hasn't yet been populated with real content.
    /// Returns `true` when the file is missing (nothing to read).
    pub async fn is_empty(&self) -> bool {
        let template = self.load_template().await;
        match tokio::fs::read_to_string(self.file_path()).await {
            Ok(content) => content.trim() == template.trim(),
            Err(_) => true,
        }
    }

    /// Read the optional template override from
    /// `<session-memory-dir>/config/template.md` — coco-rs extension
    /// loosely modeled on TS `loadSessionMemoryTemplate`
    /// (`prompts.ts:86-104`). Falls back to the static 9-section
    /// default on ENOENT or empty file.
    async fn load_template(&self) -> String {
        if let Some(parent) = self.file_path().parent() {
            let path = parent.join("config").join("template.md");
            if let Ok(s) = tokio::fs::read_to_string(&path).await
                && !s.trim().is_empty()
            {
                return s;
            }
        }
        build_session_memory_template().to_string()
    }

    /// Read the optional update-prompt override from
    /// `<session-memory-dir>/config/prompt.md` — coco-rs extension
    /// loosely modeled on TS `loadSessionMemoryPrompt`
    /// (`prompts.ts:111-129`). When present it replaces the default
    /// update prompt body; placeholders are substituted by
    /// [`build_session_memory_update_prompt`]. `None` ⇒ use the
    /// static default.
    async fn load_prompt_override(&self) -> Option<String> {
        let p = self.file_path();
        let parent = p.parent()?;
        let path = parent.join("config").join("prompt.md");
        let s = tokio::fs::read_to_string(&path).await.ok()?;
        if s.trim().is_empty() { None } else { Some(s) }
    }

    /// Wait up to `timeout` (default 15s) for an in-flight extraction
    /// to finish. Returns false on timeout. Past 60s the extraction is
    /// considered stale and we stop waiting.
    ///
    /// Driven by `extract_done.notified()` rather than polling so a
    /// 15s wait costs ~1 mutex acquisition + 1 notify wait instead of
    /// ~300 polls. A periodic re-check still fires every 1s as
    /// belt-and-braces against a notify lost during runtime shutdown.
    pub async fn wait_for_extraction(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let (in_progress, started_at) = {
                let state = self.state.lock().await;
                (state.in_progress, state.extraction_started_at)
            };
            if !in_progress {
                return true;
            }
            if let Some(t) = started_at
                && t.elapsed() > STALE_THRESHOLD
            {
                return false;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let wakeup = remaining.min(Duration::from_secs(1));
            let _ = tokio::time::timeout(wakeup, self.extract_done.notified()).await;
        }
    }

    /// Read the session-memory file from disk, truncating each section
    /// to the per-section token budget. Returns `None` when the file
    /// doesn't exist yet (no extraction has fired).
    pub async fn current_content(&self) -> Option<String> {
        let raw = tokio::fs::read_to_string(self.file_path()).await.ok()?;
        // TS parity (`sessionMemoryUtils.ts:117 logEvent
        // tengu_session_memory_loaded`) — fires whenever a downstream
        // consumer (compact, /summary surface) loads SM content.
        self.telemetry.emit(MemoryEvent::SessionMemoryLoaded {
            content_length: raw.len() as i64,
        });
        Some(truncate_session_memory_for_compact(
            &raw,
            self.config.session_memory_per_section_tokens,
        ))
    }

    /// Stateless truncation pass-through, exposed so compact glue can
    /// use it on already-loaded content.
    pub fn truncate_for_compact(&self, content: &str) -> String {
        truncate_session_memory_for_compact(content, self.config.session_memory_per_section_tokens)
    }

    /// Wipe per-conversation state. Called from `MemoryRuntime::reset`
    /// on `/clear`. The on-disk file is left alone — the next session
    /// extracts fresh data into it via Edit, and the prior content
    /// gets overwritten section-by-section.
    pub async fn reset(&self) {
        *self.state.lock().await = SessionState::default();
        self.text_cache.write().await.clear();
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_with_label(
        &self,
        start: Instant,
        current_tokens: i64,
        tool_calls_since_last_extraction: i32,
        last_message_id: Option<String>,
        had_tool_calls_in_last_turn: bool,
        fork_label: coco_types::ForkLabel,
        already_marked: bool,
    ) -> SessionMemoryOutcome {
        let file_path = self.file_path();
        let session_id_for_logs = self.read_session_id();
        if !already_marked {
            // `force()` path: no gate ran, so we still need to claim
            // the in-progress slot before doing real work.
            let mut state = self.state.lock().await;
            state.in_progress = true;
            state.extraction_started_at = Some(start);
        }

        // Ensure parent dir exists. TS uses 0o700 for the dir, 0o600
        // for the file — session memory contains a structured summary
        // of the conversation (potentially sensitive). On a multi-user
        // box other accounts shouldn't be able to read it. Non-Unix
        // platforms get process-default ACLs.
        //
        // Apply perms before any write so a brief window where the
        // dir is world-rwx can't be exploited to read the seed file.
        if let Some(parent) = file_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                self.unmark_in_progress().await;
                return SessionMemoryOutcome::Failed {
                    reason: format!("create session-memory dir: {e}"),
                };
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) =
                    tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await
                {
                    tracing::warn!(
                        target: "coco_memory::session",
                        error = %e,
                        path = %parent.display(),
                        "failed to chmod 0o700 session-memory dir — surface may be readable to other users"
                    );
                    self.telemetry.emit(MemoryEvent::SessionMemoryPermsFailed {
                        path: parent.display().to_string(),
                    });
                }
            }
        }

        // Seed the file with the 9-section template if missing.
        // Atomic create + perms — TS uses `flag:'wx'` + `mode:0o600`
        // for the same effect (`sessionMemory.ts:196-206`). If a
        // racing call beat us to creation we read its body instead of
        // overwriting.
        let template = self.load_template().await;
        let current = match self.seed_if_missing(&file_path, &template).await {
            Ok(body) => body,
            Err(e) => {
                self.unmark_in_progress().await;
                return SessionMemoryOutcome::Failed { reason: e };
            }
        };

        // Optional user-provided prompt template override.
        let prompt_override = self.load_prompt_override().await;
        let prompt = build_session_memory_update_prompt(
            &current,
            &file_path,
            prompt_override.as_deref(),
            self.config.session_memory_per_section_tokens,
            self.config.session_memory_total_tokens,
        );
        // TS parity (`sessionMemory.ts:228 logEvent
        // tengu_session_memory_file_read`).
        self.telemetry.emit(MemoryEvent::SessionMemoryFileRead {
            content_length: current.len() as i64,
        });
        let request = AgentSpawnRequest {
            prompt,
            description: Some("session memory update".into()),
            subagent_type: Some("general-purpose".into()),
            constraints: Some(AgentSpawnConstraints {
                // Session memory edits one file via a small fixed pass.
                max_turns: Some(3),
                allowed_write_roots: file_path
                    .parent()
                    .map(|p| vec![p.to_path_buf()])
                    .unwrap_or_default(),
            }),
            // TS `runForkedAgent({skipTranscript: false})` is the
            // default in TS sessionMemory — but Rust's choice is to
            // suppress per-message transcript writes for SM too,
            // since the user-facing transcript shouldn't surface the
            // SM file's read/edit machinery. Matches our consistent
            // policy across all three memory subagents.
            skip_transcript: true,
            // TS `sessionMemory.ts:318` `canUseTool: createSessionMemCanUseTool(memoryPath)`.
            // Session-mem policy is tighter than auto-mem: Edit
            // ONLY on the canonical SM file path, Read otherwise.
            // This guarantees the session-memory update can't
            // sprawl into other files even if the model tries.
            can_use_tool: Some(crate::can_use_tool::create_session_mem_handle(
                file_path.clone(),
            )),
            require_can_use_tool: false,
            // TS parity: auto cadence vs `/summary` manual trigger
            // emit distinct labels so analytics can split them.
            // `force()` passes `SessionMemoryManual`; auto path via
            // `maybe_extract` passes `SessionMemoryAuto`.
            fork_label: Some(fork_label),
            ..Default::default()
        };

        tracing::info!(
            target: "coco_memory::session",
            session_id = %session_id_for_logs,
            current_tokens,
            tool_calls_since_last_extraction,
            "spawning session-memory update subagent"
        );

        let agent = self
            .agent
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let outcome = match agent.spawn_agent(request).await {
            Ok(resp) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                tracing::info!(
                    target: "coco_memory::session",
                    session_id = %session_id_for_logs,
                    duration_ms,
                    input_tokens = resp.input_tokens,
                    output_tokens = resp.output_tokens,
                    "session-memory update complete"
                );
                self.telemetry.emit(MemoryEvent::SessionMemoryExtracted {
                    input_tokens: resp.input_tokens,
                    output_tokens: resp.output_tokens,
                    cache_read_tokens: resp.cache_read_tokens,
                    cache_creation_tokens: resp.cache_creation_tokens,
                    duration_ms,
                });
                let mut state = self.state.lock().await;
                state.initialized = true;
                state.last_extraction_tokens = current_tokens;
                state.last_extraction_tool_calls = tool_calls_since_last_extraction;
                // TS parity (`sessionMemory.ts:488-494`
                // updateLastSummarizedMessageIdIfSafe): advance the
                // "safely summarized" cursor only when the prior
                // assistant turn had no tool calls. This prevents a
                // downstream compact summary from orphaning a
                // tool_result whose tool_use isn't in the SM body.
                if !had_tool_calls_in_last_turn && let Some(id) = last_message_id {
                    state.last_summarized_message_uuid = Some(id);
                }
                drop(state);
                // Refresh the in-memory text cache so the next
                // SM-first compact short-circuit reads the freshly
                // written body without a disk hit.
                if let Ok(new_text) = tokio::fs::read_to_string(&file_path).await {
                    *self.text_cache.write().await = new_text;
                }
                SessionMemoryOutcome::Completed { duration_ms }
            }
            Err(e) => {
                tracing::warn!(
                    target: "coco_memory::session",
                    session_id = %session_id_for_logs,
                    error = %e,
                    "session-memory update failed"
                );
                SessionMemoryOutcome::Failed { reason: e }
            }
        };

        self.unmark_in_progress().await;
        outcome
    }

    async fn unmark_in_progress(&self) {
        {
            let mut state = self.state.lock().await;
            state.in_progress = false;
            state.extraction_started_at = None;
        }
        // Wake every wait_for_extraction caller — notify_waiters wakes
        // all currently-parked waiters but doesn't queue a permit for
        // future callers, which matches the "extraction just finished"
        // semantic we want.
        self.extract_done.notify_waiters();
    }

    fn read_session_id(&self) -> String {
        // ArcSwap::load returns a Guard<Arc<String>>; clone the inner
        // String so callers can format/log without holding the guard
        // across awaits.
        (**self.session_id.load()).clone()
    }

    /// Atomically seed the session-memory file with `template` if it
    /// doesn't exist, then read back the current content.
    ///
    /// `OpenOptions::create_new(true)` is the Unix `O_CREAT | O_EXCL`
    /// pair; combined with `mode(0o600)` it gives us the same
    /// "create-with-private-perms or fail" semantics TS gets from
    /// `writeFile(path, body, {flag:'wx', mode:0o600})`. Two
    /// concurrent calls race cleanly: the loser sees `AlreadyExists`
    /// and reads the winner's body instead of overwriting it.
    async fn seed_if_missing(
        &self,
        file_path: &std::path::Path,
        template: &str,
    ) -> std::result::Result<String, String> {
        let path = file_path.to_path_buf();
        let body = template.to_string();
        // Spawn-blocking because std::fs::OpenOptionsExt's
        // `mode()` setter is only on the sync std type; tokio's
        // OpenOptions doesn't expose it directly. The seed file is
        // tiny so blocking is cheap.
        let seed_result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            let mut opts = std::fs::OpenOptions::new();
            opts.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(mut f) => {
                    f.write_all(body.as_bytes())?;
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                Err(e) => Err(e),
            }
        })
        .await;
        match seed_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("write session-memory seed: {e}")),
            Err(e) => return Err(format!("spawn_blocking failed: {e}")),
        }
        tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| format!("read session-memory after seed: {e}"))
    }
}

#[cfg(test)]
#[path = "session.test.rs"]
mod tests;
