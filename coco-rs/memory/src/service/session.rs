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
//! `<config_home>/session-memory/<session_id>.md`, mode 0o600,
//! directory mode 0o700.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::AgentSpawnConstraints;
use coco_tool_runtime::AgentSpawnRequest;
use tokio::sync::Mutex;

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
    last_extraction_message_uuid: Option<String>,
    /// Last message UUID up to which the SM file is **safely** caught
    /// up — only advances when the previous assistant turn had no
    /// tool calls, matching TS `updateLastSummarizedMessageIdIfSafe`
    /// (`sessionMemory.ts:488-494`). Used by compact/summary readers
    /// that need to know "where SM has covered to" without risking
    /// orphaned tool_results in a downstream summary.
    last_summarized_message_uuid: Option<String>,
    in_progress: bool,
    extraction_started_at: Option<Instant>,
}

/// Per-session memory service.
pub struct SessionMemoryService {
    session_id: String,
    file_path: PathBuf,
    config: MemoryConfig,
    agent: crate::service::extract::AgentSlot,
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    state: Mutex<SessionState>,
}

impl std::fmt::Debug for SessionMemoryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionMemoryService")
            .field("session_id", &self.session_id)
            .field("file_path", &self.file_path)
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
        session_id: String,
        session_memory_dir: PathBuf,
        config: MemoryConfig,
        agent: AgentHandleRef,
    ) -> Self {
        Self::with_shared_agent(
            session_id,
            session_memory_dir,
            config,
            Arc::new(tokio::sync::RwLock::new(agent)),
            Arc::new(NoopEmitter),
        )
    }

    /// Shared-cell constructor — used by [`crate::MemoryRuntimeBuilder`]
    /// so all three services see the same swappable handle.
    pub fn with_shared_agent(
        session_id: String,
        session_memory_dir: PathBuf,
        config: MemoryConfig,
        agent: crate::service::extract::AgentSlot,
        telemetry: Arc<dyn MemoryTelemetryEmitter>,
    ) -> Self {
        let file_path = session_memory_dir.join(format!("{session_id}.md"));
        Self {
            session_id,
            file_path,
            config,
            agent,
            telemetry,
            state: Mutex::new(SessionState::default()),
        }
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
            // Gate passed — advance the cursor BEFORE dispatching so a
            // concurrent call (caught by `in_progress` above) sees the
            // updated boundary if it lands during the run. TS sets
            // `lastMemoryMessageUuid` inside `shouldExtractMemory`
            // (`sessionMemory.ts:174-176`) right when the boolean
            // returns true, with the same forward-looking semantics.
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
            current_tokens,
            tool_calls_since_last_extraction,
            last_message_id,
            had_tool_calls_in_last_turn,
            coco_types::ForkLabel::SessionMemoryAuto,
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
            current_tokens,
            0,
            last_message_id,
            had_tool_calls_in_last_turn,
            coco_types::ForkLabel::SessionMemoryManual,
        )
        .await
    }

    /// Cursor uuid the engine should walk from when computing
    /// cumulative tool-call counts for the next [`Self::maybe_extract`]
    /// call. `None` until the first successful extraction.
    pub async fn last_extraction_message_id(&self) -> Option<String> {
        self.state.lock().await.last_extraction_message_uuid.clone()
    }

    /// Last "safely summarized" message UUID — TS parity with
    /// `lastSummarizedMessageId` (`sessionMemoryUtils.ts:44-69`). Only
    /// advances after a successful run when the prior assistant turn
    /// had no tool calls, so compact / summary readers can use it as
    /// an orphan-safe cursor. `None` until the first eligible
    /// extraction completes.
    pub async fn last_summarized_message_id(&self) -> Option<String> {
        self.state.lock().await.last_summarized_message_uuid.clone()
    }

    /// Whether the SM file currently holds nothing but the seed
    /// template — TS `isSessionMemoryEmpty` (`prompts.ts:220-224`).
    /// Compact readers use this to fall back to LLM summarization
    /// when SM hasn't yet been populated with real content.
    /// Returns `true` when the file is missing (nothing to read).
    pub async fn is_empty(&self) -> bool {
        let template = self.load_template().await;
        match tokio::fs::read_to_string(&self.file_path).await {
            Ok(content) => content.trim() == template.trim(),
            Err(_) => true,
        }
    }

    /// Read the optional template override from
    /// `<session_memory_dir>/config/template.md` — TS
    /// `loadSessionMemoryTemplate` (`prompts.ts:86-104`). Falls back
    /// to the static 9-section default on ENOENT or read error.
    async fn load_template(&self) -> String {
        if let Some(parent) = self.file_path.parent() {
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
    /// `<session_memory_dir>/config/prompt.md` — TS
    /// `loadSessionMemoryPrompt` (`prompts.ts:111-129`). When present
    /// it replaces the default update prompt body; the
    /// `{{currentNotes}}` and `{{notesPath}}` placeholders are
    /// substituted by [`build_session_memory_update_prompt`].
    /// `None` ⇒ use the static default.
    async fn load_prompt_override(&self) -> Option<String> {
        let parent = self.file_path.parent()?;
        let path = parent.join("config").join("prompt.md");
        let s = tokio::fs::read_to_string(&path).await.ok()?;
        if s.trim().is_empty() { None } else { Some(s) }
    }

    /// Wait up to `timeout` (default 15s) for an in-flight extraction
    /// to finish. Returns false on timeout. Past 60s the extraction is
    /// considered stale and we stop waiting.
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
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Read the session-memory file from disk, truncating each section
    /// to the per-section token budget. Returns `None` when the file
    /// doesn't exist yet (no extraction has fired).
    pub async fn current_content(&self) -> Option<String> {
        let raw = tokio::fs::read_to_string(&self.file_path).await.ok()?;
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

    pub fn file_path(&self) -> &std::path::Path {
        &self.file_path
    }

    /// Wipe per-conversation state (init flag + last-extraction
    /// counters + in-flight markers). Called from
    /// `MemoryRuntime::reset` on `/clear`. The on-disk file is left
    /// alone — the next session extracts fresh data into it via
    /// Edit, and the prior content gets overwritten section-by-section.
    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = SessionState::default();
    }

    async fn run_with_label(
        &self,
        current_tokens: i64,
        tool_calls_since_last_extraction: i32,
        last_message_id: Option<String>,
        had_tool_calls_in_last_turn: bool,
        fork_label: coco_types::ForkLabel,
    ) -> SessionMemoryOutcome {
        let start = Instant::now();
        {
            let mut state = self.state.lock().await;
            state.in_progress = true;
            state.extraction_started_at = Some(start);
        }

        // Ensure parent dir exists with restrictive perms (best-effort).
        // TS uses 0o700 for the dir, 0o600 for the file — session
        // memory contains a structured summary of the conversation
        // (potentially sensitive). On a multi-user box other accounts
        // shouldn't be able to read it. Non-Unix platforms get
        // process-default ACLs.
        if let Some(parent) = self.file_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                self.unmark_in_progress().await;
                return SessionMemoryOutcome::Failed {
                    reason: format!("create session-memory dir: {e}"),
                };
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .await;
            }
        }

        // Seed the file with the 9-section template if missing.
        // The template can be overridden via
        // `<session_memory_dir>/config/template.md` (TS parity:
        // `loadSessionMemoryTemplate`).
        let template = self.load_template().await;
        let current = match tokio::fs::read_to_string(&self.file_path).await {
            Ok(s) => s,
            Err(_) => {
                if let Err(e) = tokio::fs::write(&self.file_path, &template).await {
                    self.unmark_in_progress().await;
                    return SessionMemoryOutcome::Failed {
                        reason: format!("write session-memory seed: {e}"),
                    };
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(
                        &self.file_path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .await;
                }
                template.clone()
            }
        };

        // Optional user-provided prompt template override.
        let prompt_override = self.load_prompt_override().await;
        let prompt = build_session_memory_update_prompt(
            &current,
            &self.file_path,
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
                allowed_write_roots: self
                    .file_path
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
                self.file_path.clone(),
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
            session_id = %self.session_id,
            current_tokens,
            tool_calls_since_last_extraction,
            "spawning session-memory update subagent"
        );

        let agent = self.agent.read().await.clone();
        let outcome = match agent.spawn_agent(request).await {
            Ok(resp) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                tracing::info!(
                    target: "coco_memory::session",
                    session_id = %self.session_id,
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
                SessionMemoryOutcome::Completed { duration_ms }
            }
            Err(e) => {
                tracing::warn!(
                    target: "coco_memory::session",
                    session_id = %self.session_id,
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
        let mut state = self.state.lock().await;
        state.in_progress = false;
        state.extraction_started_at = None;
    }
}

#[cfg(test)]
#[path = "session.test.rs"]
mod tests;
