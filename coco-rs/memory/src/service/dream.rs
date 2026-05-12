//! Auto-dream consolidation service.
//!
//! TS: `services/autoDream/autoDream.ts`. Three-gate scheduling:
//!
//! 1. **Time** — at least `dream_min_hours` since last consolidation.
//! 2. **Sessions** — at least `dream_min_sessions` distinct sessions
//!    have produced transcripts since the last consolidation.
//! 3. **Lock** — exactly one consolidation in flight, enforced by
//!    [`crate::lock`] PID + mtime CAS.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::AgentSpawnConstraints;
use coco_tool_runtime::AgentSpawnRequest;

use crate::config::MemoryConfig;
use crate::lock;
use crate::lock::LockOutcome;
use crate::prompt::build_dream_prompt;
use crate::telemetry::MemoryEvent;
use crate::telemetry::MemoryTelemetryEmitter;
use crate::telemetry::NoopEmitter;

/// Scan throttle — bail if we already attempted a consolidation
/// within this window. TS `SESSION_SCAN_INTERVAL_MS = 600_000`.
pub const SCAN_THROTTLE: Duration = Duration::from_secs(10 * 60);

/// One per-call outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamOutcome {
    Skipped(SkipReason),
    Completed { duration_ms: i64 },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    KairosMode,
    TimeGate { hours_since: i64 },
    SessionGate { sessions_seen: i32 },
    LockHeld,
    ScanThrottled,
}

/// Auto-dream service.
pub struct DreamService {
    memory_dir: PathBuf,
    config: MemoryConfig,
    agent: crate::service::extract::AgentSlot,
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    /// User-visible notice channel — TS parity with
    /// `autoDream.ts:240-247 appendSystemMessage(... verb: 'Improved')`.
    /// Engine drains the inbox once per turn and injects a
    /// `SystemMemorySavedMessage` with `verb: "Improved"`.
    notices: crate::notice::NoticeInbox,
    last_scan_at: tokio::sync::Mutex<Option<Instant>>,
}

impl std::fmt::Debug for DreamService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DreamService")
            .field("memory_dir", &self.memory_dir)
            .field("dream_enabled", &self.config.dream_enabled)
            .finish()
    }
}

impl DreamService {
    pub fn new(memory_dir: PathBuf, config: MemoryConfig, agent: AgentHandleRef) -> Self {
        Self::with_shared_agent(
            memory_dir,
            config,
            Arc::new(tokio::sync::RwLock::new(agent)),
            Arc::new(NoopEmitter),
        )
    }

    /// Shared-cell constructor — used by [`crate::MemoryRuntimeBuilder`]
    /// so all three services see the same swappable handle.
    pub fn with_shared_agent(
        memory_dir: PathBuf,
        config: MemoryConfig,
        agent: crate::service::extract::AgentSlot,
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
        agent: crate::service::extract::AgentSlot,
        telemetry: Arc<dyn MemoryTelemetryEmitter>,
        notices: crate::notice::NoticeInbox,
    ) -> Self {
        Self {
            memory_dir,
            config,
            agent,
            telemetry,
            notices,
            last_scan_at: tokio::sync::Mutex::new(None),
        }
    }

    /// `transcript_dir` is the project's session-transcript root used by
    /// the agent for narrow grep. `enumerate_sessions` lazily produces
    /// the session-ID slice — invoked **only** after the time + scan
    /// gates pass so callers don't pay the directory walk on every
    /// turn. TS parity (`autoDream.ts:155`): TS only runs
    /// `listSessionsTouchedSince` after the time gate. `now_ms` is the
    /// current wall clock — accept it as a parameter so tests stay
    /// deterministic.
    pub async fn maybe_consolidate<F>(
        &self,
        transcript_dir: &std::path::Path,
        enumerate_sessions: F,
        now_ms: i64,
    ) -> DreamOutcome
    where
        F: FnOnce() -> Vec<String> + Send,
    {
        self.consolidate_with_gates(transcript_dir, enumerate_sessions, now_ms, false)
            .await
    }

    /// Force a consolidation regardless of the time / session / scan
    /// throttle gates — bound to the `/dream` slash command. Still
    /// honors the `dream_enabled` and `kairos_mode` settings (TS parity:
    /// manual `/dream` runs as the disk-skill in the main loop, but
    /// auto-dream is never invoked when these are off). The PID + mtime
    /// CAS lock is still acquired so a manual run cannot race with an
    /// auto-dream in flight. The `enumerate_sessions` closure is
    /// invoked unconditionally under force so the prompt's
    /// session-hint block reflects whatever the caller knows about.
    pub async fn force<F>(
        &self,
        transcript_dir: &std::path::Path,
        enumerate_sessions: F,
        now_ms: i64,
    ) -> DreamOutcome
    where
        F: FnOnce() -> Vec<String> + Send,
    {
        self.consolidate_with_gates(transcript_dir, enumerate_sessions, now_ms, true)
            .await
    }

    async fn consolidate_with_gates<F>(
        &self,
        transcript_dir: &std::path::Path,
        enumerate_sessions: F,
        now_ms: i64,
        force: bool,
    ) -> DreamOutcome
    where
        F: FnOnce() -> Vec<String> + Send,
    {
        if !self.config.dream_enabled {
            return DreamOutcome::Skipped(SkipReason::Disabled);
        }
        if self.config.kairos_mode {
            return DreamOutcome::Skipped(SkipReason::KairosMode);
        }

        // Time gate first — TS parity (`autoDream.ts:140-141`):
        // `lastConsolidatedAt` stat happens before any session scan, so
        // we mirror that order here. Eager `lock::last_consolidated_at`
        // is one stat; cheap regardless of the scan throttle.
        let prior_last_ms = lock::last_consolidated_at(&self.memory_dir);
        let hours_since_initial = prior_last_ms
            .map(|m| (now_ms.saturating_sub(m)) / (60 * 60 * 1000))
            .unwrap_or(i64::MAX);
        if !force && hours_since_initial < self.config.dream_min_hours as i64 {
            return DreamOutcome::Skipped(SkipReason::TimeGate {
                hours_since: hours_since_initial,
            });
        }

        if !force {
            // Scan throttle — TS `SESSION_SCAN_INTERVAL_MS = 600_000`,
            // checked after the time gate so we don't bump
            // `lastSessionScanAt` for turns the time gate already
            // rejected.
            let mut last = self.last_scan_at.lock().await;
            if let Some(t) = *last
                && t.elapsed() < SCAN_THROTTLE
            {
                return DreamOutcome::Skipped(SkipReason::ScanThrottled);
            }
            *last = Some(Instant::now());
        }

        // Session enumeration — lazy, invoked here so callers don't
        // pay the directory walk on time-gated / scan-throttled turns.
        let sessions_since_last = enumerate_sessions();

        if !force {
            let sessions_seen = sessions_since_last.len() as i32;
            if sessions_seen < self.config.dream_min_sessions {
                return DreamOutcome::Skipped(SkipReason::SessionGate { sessions_seen });
            }
        }

        // Lock — kept under both paths so manual /dream and auto-dream
        // never race over MEMORY.md edits.
        let prior_mtime_ms = match lock::try_acquire(&self.memory_dir) {
            LockOutcome::Acquired { prior_mtime_ms } => prior_mtime_ms,
            LockOutcome::Held => {
                return DreamOutcome::Skipped(SkipReason::LockHeld);
            }
            LockOutcome::Error(e) => {
                return DreamOutcome::Failed { reason: e };
            }
        };

        let sessions_seen = sessions_since_last.len() as i32;
        let hours_since_last = if hours_since_initial == i64::MAX {
            0
        } else {
            hours_since_initial
        };
        self.telemetry.emit(MemoryEvent::AutoDreamFired {
            hours_since_last,
            sessions_since_last: sessions_seen,
        });

        let start = Instant::now();
        let prompt = build_dream_prompt(&self.memory_dir, transcript_dir, &sessions_since_last);
        let request = AgentSpawnRequest {
            prompt,
            description: Some("auto-dream consolidation".into()),
            subagent_type: Some("general-purpose".into()),
            constraints: Some(AgentSpawnConstraints {
                max_turns: Some(20),
                allowed_write_roots: vec![self.memory_dir.clone()],
            }),
            // TS `runForkedAgent({skipTranscript: true})`
            // (`autoDream.ts:230`) — same reason as extract: keep the
            // background subagent's tool-uses out of the user's main
            // JSONL transcript.
            skip_transcript: true,
            // TS `autoDream.ts:224` `canUseTool: createAutoMemCanUseTool(memoryRoot)`.
            // AutoDream uses the same auto-mem policy as extract,
            // but pinned to `memory_dir` (which acts as the
            // memoryRoot for the consolidation pass).
            can_use_tool: Some(crate::can_use_tool::create_auto_mem_handle(
                self.memory_dir.clone(),
            )),
            require_can_use_tool: false,
            fork_label: Some(coco_types::ForkLabel::AutoDream),
            ..Default::default()
        };

        tracing::info!(
            target: "coco_memory::dream",
            sessions_seen,
            hours_since = hours_since_last,
            "spawning auto-dream consolidation subagent"
        );

        let agent = self.agent.read().await.clone();
        match agent.spawn_agent(request).await {
            Ok(resp) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                tracing::info!(
                    target: "coco_memory::dream",
                    duration_ms,
                    files_changed = resp.total_tool_use_count,
                    sessions_reviewed = sessions_seen,
                    cache_read = resp.cache_read_tokens,
                    cache_create = resp.cache_creation_tokens,
                    "auto-dream consolidation complete"
                );
                // Same MEMORY.md filter as extract — the index file
                // is mechanical; only topic-file improvements count.
                let entrypoint = crate::store::ENTRYPOINT_NAME;
                let topic_paths: Vec<String> = resp
                    .paths_written
                    .iter()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n != entrypoint)
                    })
                    .map(|p| p.display().to_string())
                    .collect();
                self.telemetry.emit(MemoryEvent::AutoDreamCompleted {
                    sessions_reviewed: sessions_seen,
                    files_changed: resp.total_tool_use_count as i32,
                    cache_read_tokens: resp.cache_read_tokens,
                    cache_creation_tokens: resp.cache_creation_tokens,
                    output_tokens: resp.output_tokens,
                    duration_ms,
                });
                // TS `autoDream.ts:240-247`: only push when at least
                // one topic file was touched. `verb: "Improved"`
                // distinguishes this from extract's "Saved".
                if !topic_paths.is_empty() {
                    self.notices.push(crate::notice::MemoryUserNotice {
                        written_paths: topic_paths,
                        verb: crate::notice::NoticeVerb::Improved,
                    });
                }
                // Re-stamp the lock so its mtime reflects the actual
                // completion time. `try_acquire` already wrote it, but
                // long-running consolidations should record fresh mtime.
                let _ = lock::record_consolidation(&self.memory_dir);
                DreamOutcome::Completed { duration_ms }
            }
            Err(e) => {
                tracing::warn!(
                    target: "coco_memory::dream",
                    error = %e,
                    "auto-dream subagent failed; rolling back lock"
                );
                lock::rollback(&self.memory_dir, prior_mtime_ms);
                // TS parity (`autoDream.ts:267`): emit
                // `tengu_auto_dream_failed` so failures show up in
                // dashboards independently of the lock-rollback log.
                self.telemetry.emit(MemoryEvent::AutoDreamFailed);
                DreamOutcome::Failed { reason: e }
            }
        }
    }

    /// Wall-clock helper so callers don't have to import `SystemTime`.
    pub fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[path = "dream.test.rs"]
mod tests;
