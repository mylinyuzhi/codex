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
    pub async fn maybe_extract(
        &self,
        current_tokens: i64,
        tool_calls_in_last_turn: i32,
        had_tool_calls_last_turn: bool,
    ) -> SessionMemoryOutcome {
        if !self.config.session_memory_enabled {
            return SessionMemoryOutcome::Skipped(SkipReason::Disabled);
        }
        {
            let state = self.state.lock().await;
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
                    tool_calls_in_last_turn >= self.config.session_memory_tool_calls;
                let natural_break = !had_tool_calls_last_turn;
                if !tool_call_gate && !natural_break {
                    return SessionMemoryOutcome::Skipped(SkipReason::NeitherToolCallsNorBreak);
                }
            }
        }
        tracing::info!(
            current_tokens,
            tool_calls_in_last_turn,
            had_tool_calls_last_turn,
            "session-memory extract dispatch"
        );
        self.run(current_tokens, tool_calls_in_last_turn).await
    }

    /// Force a fresh extraction regardless of gates — `/summary`.
    pub async fn force(&self, current_tokens: i64) -> SessionMemoryOutcome {
        self.run(current_tokens, 0).await
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

    async fn run(&self, current_tokens: i64, tool_calls_in_last_turn: i32) -> SessionMemoryOutcome {
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
        let current = match tokio::fs::read_to_string(&self.file_path).await {
            Ok(s) => s,
            Err(_) => {
                let template = build_session_memory_template();
                if let Err(e) = tokio::fs::write(&self.file_path, template).await {
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
                template.to_string()
            }
        };

        let prompt = build_session_memory_update_prompt(&current, &self.file_path);
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
            ..Default::default()
        };

        tracing::info!(
            target: "coco_memory::session",
            session_id = %self.session_id,
            current_tokens,
            tool_calls_in_last_turn,
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
                    duration_ms,
                });
                let mut state = self.state.lock().await;
                state.initialized = true;
                state.last_extraction_tokens = current_tokens;
                state.last_extraction_tool_calls = tool_calls_in_last_turn;
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
