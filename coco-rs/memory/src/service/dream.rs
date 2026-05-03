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
        Self {
            memory_dir,
            config,
            agent,
            telemetry,
            last_scan_at: tokio::sync::Mutex::new(None),
        }
    }

    /// `transcript_dir` is the project's session-transcript root used by
    /// the agent for narrow grep. `sessions_since_last` is the slice of
    /// session IDs the caller already enumerated (via the project's
    /// session store). `now_ms` is the current wall clock — accept it
    /// as a parameter so tests stay deterministic.
    pub async fn maybe_consolidate(
        &self,
        transcript_dir: &std::path::Path,
        sessions_since_last: &[String],
        now_ms: i64,
    ) -> DreamOutcome {
        if !self.config.dream_enabled {
            return DreamOutcome::Skipped(SkipReason::Disabled);
        }
        if self.config.kairos_mode {
            return DreamOutcome::Skipped(SkipReason::KairosMode);
        }

        // Scan throttle.
        {
            let mut last = self.last_scan_at.lock().await;
            if let Some(t) = *last
                && t.elapsed() < SCAN_THROTTLE
            {
                return DreamOutcome::Skipped(SkipReason::ScanThrottled);
            }
            *last = Some(Instant::now());
        }

        // Time gate.
        if let Some(last_ms) = lock::last_consolidated_at(&self.memory_dir) {
            let hours_since = (now_ms.saturating_sub(last_ms)) / (60 * 60 * 1000);
            if hours_since < self.config.dream_min_hours as i64 {
                return DreamOutcome::Skipped(SkipReason::TimeGate { hours_since });
            }
        }

        // Session gate.
        let sessions_seen = sessions_since_last.len() as i32;
        if sessions_seen < self.config.dream_min_sessions {
            return DreamOutcome::Skipped(SkipReason::SessionGate { sessions_seen });
        }

        // Lock.
        let prior_mtime_ms = match lock::try_acquire(&self.memory_dir) {
            LockOutcome::Acquired { prior_mtime_ms } => prior_mtime_ms,
            LockOutcome::Held => {
                return DreamOutcome::Skipped(SkipReason::LockHeld);
            }
            LockOutcome::Error(e) => {
                return DreamOutcome::Failed { reason: e };
            }
        };

        let hours_since_last = lock::last_consolidated_at(&self.memory_dir)
            .map(|m| (now_ms.saturating_sub(m)) / (60 * 60 * 1000))
            .unwrap_or(0);
        self.telemetry.emit(MemoryEvent::AutoDreamFired {
            hours_since_last,
            sessions_since_last: sessions_seen,
        });

        let start = Instant::now();
        let prompt = build_dream_prompt(&self.memory_dir, transcript_dir, sessions_since_last);
        let request = AgentSpawnRequest {
            prompt,
            description: Some("auto-dream consolidation".into()),
            subagent_type: Some("general-purpose".into()),
            constraints: Some(AgentSpawnConstraints {
                max_turns: Some(20),
                allowed_write_roots: vec![self.memory_dir.clone()],
            }),
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
                    "auto-dream consolidation complete"
                );
                self.telemetry.emit(MemoryEvent::AutoDreamCompleted {
                    sessions_reviewed: sessions_seen,
                    files_changed: resp.total_tool_use_count as i32,
                    duration_ms,
                });
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
