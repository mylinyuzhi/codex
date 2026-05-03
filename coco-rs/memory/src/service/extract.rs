//! Turn-end memory extraction service.
//!
//! TS: `services/extractMemories/extractMemories.ts`. After every
//! eligible turn, fork a subagent with a 5-turn cap and a memdir-only
//! write fence. The agent reads existing memories (manifest pre-injected
//! into its prompt), then writes / edits memory files based on the
//! conversation slice since the last cursor.
//!
//! State machine:
//! - throttle gate (every Nth turn)
//! - mutual exclusion (don't run while in-flight; stash + trailing run)
//! - skip-if-main-already-wrote (`has_memory_writes_since`)
//! - cursor advance after success

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::AgentSpawnConstraints;
use coco_tool_runtime::AgentSpawnRequest;
use tokio::sync::Mutex;

use crate::config::MemoryConfig;
use crate::prompt::build_extract_prompt;
use crate::scan;
use crate::telemetry::MemoryEvent;
use crate::telemetry::MemoryTelemetryEmitter;
use crate::telemetry::NoopEmitter;

/// Drain timeout on shutdown — TS `drainPendingExtraction(60_000)`.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

/// Cross-turn extraction state.
#[derive(Debug, Default)]
struct ExtractState {
    /// Last message UUID that's been folded into an extraction. The
    /// extraction analyzes only messages newer than this.
    last_cursor: Option<String>,
    /// Throttle counter — increments per eligible turn, resets on fire.
    turns_since_last: i32,
    /// True while a fork is running. New triggers stash for trailing.
    in_progress: bool,
    /// One trailing run is queued when a turn ends mid-extraction.
    pending_trailing: bool,
}

/// Shared swappable cell for the agent handle. The runtime owns the
/// master `Arc<RwLock<...>>` and hands clones to every service so a
/// later `MemoryRuntime::install_agent(handle)` propagates atomically
/// — no need to rebuild service instances. `tokio::sync::RwLock` is
/// the right primitive here: writes are rare (session bootstrap), reads
/// are async-friendly and concurrent.
pub type AgentSlot = Arc<tokio::sync::RwLock<AgentHandleRef>>;

/// Turn-end extraction service.
pub struct ExtractService {
    memory_dir: PathBuf,
    config: MemoryConfig,
    agent: AgentSlot,
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    state: Mutex<ExtractState>,
}

impl std::fmt::Debug for ExtractService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractService")
            .field("memory_dir", &self.memory_dir)
            .field("extraction_enabled", &self.config.extraction_enabled)
            .finish()
    }
}

/// One per-turn outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractOutcome {
    /// Extraction wasn't fired this turn.
    Skipped(SkipReason),
    /// Extraction fired and completed (synchronous wait).
    Completed { duration_ms: i64 },
    /// Extraction fired and failed.
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    InProgress,
    Throttled,
    DirectWrite,
}

/// Boxed lazy builder for the message slice the extraction agent
/// analyzes. Invoked **only** when all gates pass and the service is
/// actually about to spawn — turns where extraction is skipped
/// (throttled, in-flight, direct-write) avoid the per-message
/// `serde_json::to_value` allocations entirely.
///
/// Carried as `serde_json::Value` so the request crosses the
/// `coco-tool-runtime → coco-query` boundary without pulling message
/// types into `coco-tool-runtime`.
pub type LazyForkMessages = Box<dyn FnOnce() -> Vec<serde_json::Value> + Send>;

/// Per-turn input: a lazy slice builder + turn-level signals.
///
/// `has_memory_writes` is true when the main agent wrote to the
/// memory directory during this turn — extraction stays out of the
/// way to avoid stomping on user-curated edits.
pub struct TurnInput {
    pub fork_messages: LazyForkMessages,
    pub message_count: i32,
    pub last_message_id: Option<String>,
    pub has_memory_writes: bool,
}

impl Default for TurnInput {
    fn default() -> Self {
        Self {
            fork_messages: Box::new(Vec::new),
            message_count: 0,
            last_message_id: None,
            has_memory_writes: false,
        }
    }
}

impl ExtractService {
    pub fn new(memory_dir: PathBuf, config: MemoryConfig, agent: AgentHandleRef) -> Self {
        Self::with_shared_agent(
            memory_dir,
            config,
            Arc::new(tokio::sync::RwLock::new(agent)),
            Arc::new(NoopEmitter),
        )
    }

    /// Shared-cell constructor — used by the builder so all three
    /// services see the same swappable handle.
    pub fn with_shared_agent(
        memory_dir: PathBuf,
        config: MemoryConfig,
        agent: AgentSlot,
        telemetry: Arc<dyn MemoryTelemetryEmitter>,
    ) -> Self {
        Self {
            memory_dir,
            config,
            agent,
            telemetry,
            state: Mutex::new(ExtractState::default()),
        }
    }

    /// Run-or-skip decision keyed off [`TurnInput`]. The caller's
    /// `fork_messages` closure is invoked **only** once all gates pass
    /// — skipped turns avoid the per-message JSON serialization.
    pub async fn maybe_extract(&self, input: TurnInput) -> ExtractOutcome {
        if !self.config.extraction_enabled {
            return ExtractOutcome::Skipped(SkipReason::Disabled);
        }

        {
            let mut state = self.state.lock().await;
            if state.in_progress {
                state.pending_trailing = true;
                return ExtractOutcome::Skipped(SkipReason::InProgress);
            }
            if input.has_memory_writes {
                self.telemetry
                    .emit(MemoryEvent::ExtractionSkippedDirectWrite {
                        message_count: input.message_count,
                    });
                return ExtractOutcome::Skipped(SkipReason::DirectWrite);
            }
            state.turns_since_last += 1;
            if state.turns_since_last < self.config.extraction_throttle {
                return ExtractOutcome::Skipped(SkipReason::Throttled);
            }
            state.turns_since_last = 0;
            state.in_progress = true;
        }

        // Materialize the slice once now that we know we'll fire.
        // Reused for both the primary run and any trailing run so the
        // caller's closure stays `FnOnce`.
        let fork_context = (input.fork_messages)();
        let outcome = self.run(input.message_count, fork_context.clone()).await;

        {
            let mut state = self.state.lock().await;
            state.in_progress = false;
            if let Some(id) = input.last_message_id {
                state.last_cursor = Some(id);
            }
        }

        // Drain trailing runs in a loop — one queued at a time.
        loop {
            let should_trail = {
                let mut state = self.state.lock().await;
                let trail = state.pending_trailing;
                state.pending_trailing = false;
                trail
            };
            if !should_trail {
                break;
            }
            {
                let mut state = self.state.lock().await;
                state.in_progress = true;
            }
            let _trailing = self.run(input.message_count, fork_context.clone()).await;
            let mut state = self.state.lock().await;
            state.in_progress = false;
        }

        outcome
    }

    /// Force a fresh extraction regardless of throttle / in-progress
    /// flags — bound to a `/dream` or `/extract` slash command.
    pub async fn force(&self, input: TurnInput) -> ExtractOutcome {
        {
            let mut state = self.state.lock().await;
            state.in_progress = true;
        }
        let fork_context = (input.fork_messages)();
        let outcome = self.run(input.message_count, fork_context).await;
        let mut state = self.state.lock().await;
        state.in_progress = false;
        state.turns_since_last = 0;
        outcome
    }

    /// Wait up to `timeout` for an in-flight extraction to complete.
    /// Used at session shutdown so partial writes don't get lost.
    pub async fn drain(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let in_progress = self.state.lock().await.in_progress;
            if !in_progress {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Last cursor (message UUID); call sites use this to compute the
    /// "messages since last extraction" slice for the agent prompt.
    pub async fn last_cursor(&self) -> Option<String> {
        self.state.lock().await.last_cursor.clone()
    }

    /// Wipe per-conversation state (cursor + throttle counter +
    /// pending-trailing flag) — called from `MemoryRuntime::reset`
    /// on `/clear` so the next conversation extracts fresh.
    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = ExtractState::default();
    }

    async fn run(
        &self,
        message_count: i32,
        fork_context: Vec<serde_json::Value>,
    ) -> ExtractOutcome {
        let start = Instant::now();
        let manifest = scan::format_memory_manifest(&scan::scan_memory_files(&self.memory_dir));
        let prompt = build_extract_prompt(message_count, &manifest, self.config.skip_index);
        tracing::info!(
            target: "coco_memory::extract",
            message_count,
            fork_context = fork_context.len(),
            max_turns = self.config.extraction_max_turns,
            "spawning extraction subagent"
        );

        // Inherit the parent's resolved model role through the
        // `Memory` slot. TS hardcodes Sonnet here; coco-rs goes
        // through `ModelRoles::Memory` so the operator can swap
        // provider+model without touching this crate.
        let request = AgentSpawnRequest {
            prompt,
            description: Some("memory extraction".into()),
            subagent_type: Some("general-purpose".into()),
            run_in_background: false,
            // Fork mode so the child sees the parent's message slice
            // prepended to its first turn (TS `forkContextMessages`).
            isolation: if fork_context.is_empty() {
                None
            } else {
                Some("fork".into())
            },
            fork_context_messages: fork_context,
            constraints: Some(AgentSpawnConstraints {
                max_turns: Some(self.config.extraction_max_turns),
                allowed_write_roots: vec![self.memory_dir.clone()],
            }),
            ..Default::default()
        };

        let agent = self.agent.read().await.clone();
        match agent.spawn_agent(request).await {
            Ok(response) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                // Sum file-mutation invocations the agent performed
                // via the canonical typed `ToolName::*::as_str()` keys.
                let files_written: i32 = [
                    coco_types::ToolName::Write.as_str(),
                    coco_types::ToolName::Edit.as_str(),
                    coco_types::ToolName::NotebookEdit.as_str(),
                ]
                .iter()
                .map(|t| response.tool_use_counts.get(*t).copied().unwrap_or(0))
                .sum::<i64>() as i32;
                tracing::info!(
                    target: "coco_memory::extract",
                    duration_ms,
                    files_written,
                    turn_count = response.total_tool_use_count,
                    input_tokens = response.input_tokens,
                    output_tokens = response.output_tokens,
                    "extraction complete"
                );
                self.telemetry.emit(MemoryEvent::ExtractionCompleted {
                    turn_count: response.total_tool_use_count as i32,
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                    files_written,
                    duration_ms,
                });
                ExtractOutcome::Completed { duration_ms }
            }
            Err(e) => {
                tracing::warn!(
                    target: "coco_memory::extract",
                    error = %e,
                    "extraction subagent failed"
                );
                ExtractOutcome::Failed { reason: e }
            }
        }
    }
}

#[cfg(test)]
#[path = "extract.test.rs"]
mod tests;
