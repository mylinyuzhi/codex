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
use coco_types::messages::Message;
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
#[derive(Default)]
struct ExtractState {
    /// Last message UUID that's been folded into an extraction. The
    /// extraction analyzes only messages newer than this.
    last_cursor: Option<String>,
    /// Throttle counter — increments per eligible turn, resets on fire.
    turns_since_last: i32,
    /// True while a fork is running. New triggers stash for trailing.
    in_progress: bool,
    /// Latest stashed `TurnInput` queued during an in-flight run.
    /// TS parity (`extractMemories.ts:506-521`): the trailing run
    /// uses **the most recent** stashed context — overwriting prior
    /// stashes — so it picks up messages that arrived during the
    /// primary run rather than re-running on the same stale slice.
    /// Carrying the full `TurnInput` here means the closure
    /// (`fork_messages`) is captured at stash time, so it serializes
    /// the latest history when invoked.
    pending_trailing: Option<TurnInput>,
}

impl std::fmt::Debug for ExtractState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractState")
            .field("last_cursor", &self.last_cursor)
            .field("turns_since_last", &self.turns_since_last)
            .field("in_progress", &self.in_progress)
            .field("pending_trailing", &self.pending_trailing.is_some())
            .finish()
    }
}

/// Shared swappable cell for the agent handle. The runtime owns the
/// master `Arc<RwLock<...>>` and hands clones to every service so a
/// later `MemoryRuntime::install_agent(handle)` propagates atomically
/// — no need to rebuild service instances.
///
/// `std::sync::RwLock` (not `tokio::sync::RwLock`) because reads are
/// "clone the inner `Arc` and drop the guard immediately" — no await
/// across the guard, no need for an async-aware lock, no futex hop
/// onto the runtime. `arc_swap::ArcSwapAny<Arc<dyn AgentHandle>>`
/// would be even cheaper but DSTs (`dyn AgentHandle`) don't satisfy
/// arc-swap's `RefCnt: Sized` bound; RwLock is the next-best primitive.
pub type AgentSlot = Arc<std::sync::RwLock<AgentHandleRef>>;

/// Turn-end extraction service.
pub struct ExtractService {
    memory_dir: PathBuf,
    config: MemoryConfig,
    agent: AgentSlot,
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    /// User-visible save notices land here on a successful
    /// extraction; the engine drains it once per turn and injects a
    /// `SystemMemorySavedMessage` into history. TS parity:
    /// `extractMemories.ts:491-496 appendSystemMessage(createMemorySavedMessage(...))`.
    notices: crate::notice::NoticeInbox,
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
/// (throttled, in-flight, direct-write) avoid per-message allocations
/// entirely.
///
/// Returns `Vec<Arc<Message>>` so the in-process spawn path shares
/// the parent's authoritative history allocations directly — no
/// serialize/deserialize round-trip across the trait boundary.
pub type LazyForkMessages = Box<dyn FnOnce() -> Vec<Arc<Message>> + Send>;

/// Lazy predicate for the "main agent wrote to memdir since the
/// cursor" check. Evaluated at gate time so trailing runs — which
/// fire after a primary run completes and may see new direct-writes
/// that landed during the primary's window — get a fresh answer
/// instead of the stale snapshot from when this `TurnInput` was first
/// constructed. TS parity: `hasMemoryWritesSince` in
/// `extractMemories.ts:121-148` is re-evaluated by every entry into
/// `runExtraction`, not cached.
pub type HasMemoryWritesFn = Box<dyn FnOnce() -> bool + Send>;

/// Per-turn input: a lazy slice builder + turn-level signals.
///
/// `has_memory_writes` is a **closure** rather than a boolean so the
/// trailing run can re-check it against history that grew during the
/// primary's window. Without this, a main-agent direct-write that
/// landed mid-primary would slip past the trailing run's mutual-
/// exclusion gate.
pub struct TurnInput {
    pub fork_messages: LazyForkMessages,
    pub message_count: i32,
    pub last_message_id: Option<String>,
    pub has_memory_writes: HasMemoryWritesFn,
}

impl Default for TurnInput {
    fn default() -> Self {
        Self {
            fork_messages: Box::new(Vec::new),
            message_count: 0,
            last_message_id: None,
            has_memory_writes: Box::new(|| false),
        }
    }
}

impl ExtractService {
    pub fn new(memory_dir: PathBuf, config: MemoryConfig, agent: AgentHandleRef) -> Self {
        Self::with_shared_agent(
            memory_dir,
            config,
            Arc::new(std::sync::RwLock::new(agent)),
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
        Self::with_shared_agent_and_notices(
            memory_dir,
            config,
            agent,
            telemetry,
            crate::notice::NoticeInbox::default(),
        )
    }

    /// Full constructor — `MemoryRuntimeBuilder` uses this so the
    /// inbox is shared with the runtime's drain endpoint.
    pub fn with_shared_agent_and_notices(
        memory_dir: PathBuf,
        config: MemoryConfig,
        agent: AgentSlot,
        telemetry: Arc<dyn MemoryTelemetryEmitter>,
        notices: crate::notice::NoticeInbox,
    ) -> Self {
        Self {
            memory_dir,
            config,
            agent,
            telemetry,
            notices,
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

        // Destructure so we can evaluate `has_memory_writes` OUTSIDE
        // the mutex critical section. The closure could in principle
        // do non-trivial work (scan the message slice for tool calls)
        // and we don't want to hold `state` across it.
        let TurnInput {
            fork_messages,
            message_count,
            last_message_id,
            has_memory_writes,
        } = input;

        {
            let mut state = self.state.lock().await;
            if state.in_progress {
                // TS parity (`extractMemories.ts:557-563`): stash the
                // **latest** context and let the trailing run use it.
                // Overwrites any prior stash so we always replay the
                // freshest message slice, not the one that triggered
                // the in-flight run. Re-pack into a `TurnInput`
                // because the closure is `FnOnce` and we've already
                // moved its field out of `input`.
                state.pending_trailing = Some(TurnInput {
                    fork_messages,
                    message_count,
                    last_message_id,
                    has_memory_writes,
                });
                tracing::debug!(
                    message_count,
                    "auto-memory extract skipped: in progress (queued trailing)"
                );
                self.telemetry.emit(MemoryEvent::ExtractionCoalesced);
                return ExtractOutcome::Skipped(SkipReason::InProgress);
            }
            // Drop the state lock before invoking the user closure
            // (it may walk message history). Re-acquire below.
            drop(state);
        }
        let direct_write = has_memory_writes();
        {
            let mut state = self.state.lock().await;
            if direct_write {
                // TS parity (`extractMemories.ts:347-360`): when the
                // main agent wrote memory directly this turn, skip
                // the fork AND advance the cursor past the range so
                // the next eligible turn doesn't re-consider these
                // already-handled messages.
                if let Some(ref id) = last_message_id {
                    state.last_cursor = Some(id.clone());
                }
                self.telemetry
                    .emit(MemoryEvent::ExtractionSkippedDirectWrite { message_count });
                tracing::debug!(
                    message_count,
                    "auto-memory extract skipped: model wrote memory directly"
                );
                return ExtractOutcome::Skipped(SkipReason::DirectWrite);
            }
            state.turns_since_last += 1;
            if state.turns_since_last < self.config.extraction_throttle {
                tracing::debug!(
                    turns_since_last = state.turns_since_last,
                    throttle = self.config.extraction_throttle,
                    "auto-memory extract skipped: throttled"
                );
                return ExtractOutcome::Skipped(SkipReason::Throttled);
            }
            state.turns_since_last = 0;
            state.in_progress = true;
        }
        tracing::info!(
            message_count,
            "auto-memory extract dispatch (forking agent)"
        );

        let primary_fork_context = fork_messages();
        let outcome = self.run(message_count, primary_fork_context).await;
        tracing::info!(
            outcome = ?std::mem::discriminant(&outcome),
            "auto-memory extract done"
        );

        {
            let mut state = self.state.lock().await;
            state.in_progress = false;
            // CLAUDE.md invariant: cursor only advances on a successful
            // fold. On `Failed` the next eligible turn retries the same
            // range (otherwise a transient subagent crash would silently
            // skip messages from extraction forever).
            if let (ExtractOutcome::Completed { .. }, Some(id)) = (&outcome, last_message_id) {
                state.last_cursor = Some(id);
            }
        }

        // Drain trailing runs in a loop. Each iteration takes the
        // latest stashed `TurnInput` (set during this primary's
        // window) and runs it against its own fresh fork-messages
        // slice. The trailing closure for `has_memory_writes` is
        // re-evaluated against the now-newer history — TS parity for
        // direct-write skip. Cursor advances per trailing input ONLY
        // on success.
        loop {
            let pending = {
                let mut state = self.state.lock().await;
                state.pending_trailing.take()
            };
            let Some(trailing_input) = pending else {
                break;
            };
            let TurnInput {
                fork_messages: trailing_fork_messages,
                message_count: trailing_count,
                last_message_id: trailing_last_id,
                has_memory_writes: trailing_has_writes,
            } = trailing_input;

            // Re-check direct-write against history that grew during
            // the primary's window. If the main agent wrote to memdir
            // during that window, the trailing fork would step on the
            // user's edits — skip + advance cursor.
            if trailing_has_writes() {
                let mut state = self.state.lock().await;
                if let Some(id) = trailing_last_id {
                    state.last_cursor = Some(id);
                }
                self.telemetry
                    .emit(MemoryEvent::ExtractionSkippedDirectWrite {
                        message_count: trailing_count,
                    });
                tracing::debug!(
                    message_count = trailing_count,
                    "auto-memory trailing extract skipped: model wrote memory directly"
                );
                continue;
            }
            {
                let mut state = self.state.lock().await;
                state.in_progress = true;
            }
            let trailing_fork_context = trailing_fork_messages();
            let trailing_outcome = self.run(trailing_count, trailing_fork_context).await;
            let mut state = self.state.lock().await;
            state.in_progress = false;
            if let (ExtractOutcome::Completed { .. }, Some(id)) =
                (&trailing_outcome, trailing_last_id)
            {
                state.last_cursor = Some(id);
            }
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

    async fn run(&self, message_count: i32, fork_context: Vec<Arc<Message>>) -> ExtractOutcome {
        let start = Instant::now();
        let manifest = scan::format_memory_manifest(&scan::scan_memory_files(&self.memory_dir));
        let prompt = build_extract_prompt(
            message_count,
            &manifest,
            self.config.skip_index,
            self.config.team_memory_enabled,
        );
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
            // TS `runForkedAgent({skipTranscript: true})`
            // (`extractMemories.ts:421-423`): the background agent
            // must not record per-message entries to the user's
            // transcript — its tool-uses race the main thread's
            // writer and pollute the JSONL.
            skip_transcript: true,
            // TS `extractMemories.ts:415` `canUseTool: createAutoMemCanUseTool(memoryDir)`.
            // Allows Read/Glob/Grep, read-only Bash, Edit/Write
            // within memory_dir; denies everything else. The
            // canUseTool gate runs at tool-runtime step 3.5,
            // composing with the `allowed_write_roots` fence above
            // (callback = inner ring; field = outer ring).
            can_use_tool: Some(crate::can_use_tool::create_auto_mem_handle(
                self.memory_dir.clone(),
            )),
            require_can_use_tool: false,
            fork_label: Some(coco_types::ForkLabel::ExtractMemories),
            ..Default::default()
        };

        // Clone the inner `Arc<dyn AgentHandle>` while holding the
        // sync read guard, then drop the guard before any `.await`.
        // `PoisonError::into_inner` recovers from a poisoned lock —
        // poisoning only happens if a prior write panicked, and for
        // an install-only handle a stale-but-readable handle beats
        // crashing the extract path.
        let agent = self
            .agent
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        match agent.spawn_agent(request).await {
            Ok(response) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                // Prefer the explicit `paths_written` list when the
                // spawn driver populated it — this lets us mirror TS
                // `extractMemories.ts:465-467` and exclude `MEMORY.md`
                // from the user-facing "Saved" count (the index file
                // is mechanical). Fall back to `tool_use_counts` for
                // legacy / minimal driver impls that haven't wired
                // the path list yet.
                let entrypoint = crate::store::ENTRYPOINT_NAME;
                let topic_paths: Vec<String> = response
                    .paths_written
                    .iter()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n != entrypoint)
                    })
                    .map(|p| p.display().to_string())
                    .collect();
                let files_written: i32 = if !response.paths_written.is_empty() {
                    topic_paths.len() as i32
                } else {
                    [
                        coco_types::ToolName::Write.as_str(),
                        coco_types::ToolName::Edit.as_str(),
                        coco_types::ToolName::NotebookEdit.as_str(),
                    ]
                    .iter()
                    .map(|t| response.tool_use_counts.get(*t).copied().unwrap_or(0))
                    .sum::<i64>() as i32
                };
                tracing::info!(
                    target: "coco_memory::extract",
                    duration_ms,
                    files_written,
                    turn_count = response.total_tool_use_count,
                    input_tokens = response.input_tokens,
                    output_tokens = response.output_tokens,
                    cache_read = response.cache_read_tokens,
                    cache_create = response.cache_creation_tokens,
                    "extraction complete"
                );
                self.telemetry.emit(MemoryEvent::ExtractionCompleted {
                    turn_count: response.total_tool_use_count as i32,
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                    cache_read_tokens: response.cache_read_tokens,
                    cache_creation_tokens: response.cache_creation_tokens,
                    files_written,
                    duration_ms,
                });
                // TS `extractMemories.ts:490-496`: only push a
                // user-visible notice when at least one topic file
                // was written (the index doesn't count).
                if !topic_paths.is_empty() {
                    self.notices.push(crate::notice::MemoryUserNotice {
                        written_paths: topic_paths,
                        verb: crate::notice::NoticeVerb::Saved,
                    });
                }
                ExtractOutcome::Completed { duration_ms }
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                tracing::warn!(
                    target: "coco_memory::extract",
                    error = %e,
                    duration_ms,
                    "extraction subagent failed"
                );
                self.telemetry
                    .emit(MemoryEvent::ExtractionError { duration_ms });
                ExtractOutcome::Failed { reason: e }
            }
        }
    }
}

#[cfg(test)]
#[path = "extract.test.rs"]
mod tests;
