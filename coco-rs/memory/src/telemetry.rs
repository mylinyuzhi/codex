//! Memory subsystem telemetry events.
//!
//! Each variant maps to one TS `tengu_*` event. Emission goes through
//! [`MemoryTelemetryEmitter`] so call sites stay free of OTel imports.

/// Memory event taxonomy.
///
/// TS: scattered `logEvent('tengu_*', ...)` calls in
/// `memdir/memdir.ts`, `services/extractMemories/`, `services/autoDream/`,
/// `services/SessionMemory/`.
#[derive(Debug, Clone)]
pub enum MemoryEvent {
    /// Memory directory loaded into the system prompt.
    /// TS: `tengu_memdir_loaded`.
    MemdirLoaded {
        line_count: i64,
        byte_count: i64,
        was_truncated: bool,
        was_byte_truncated: bool,
        has_team: bool,
    },

    /// Memory subsystem is gated off.
    /// TS: `tengu_memdir_disabled`.
    MemdirDisabled { reason: DisableReason },

    /// Extraction agent ran a tool that wasn't allow-listed.
    /// TS: `tengu_auto_mem_tool_denied`.
    ExtractionToolDenied { tool_name: String },

    /// Background extraction skipped because the main agent already
    /// wrote memory files this turn.
    /// TS: `tengu_extract_memories_skipped_direct_write`.
    ExtractionSkippedDirectWrite { message_count: i32 },

    /// Background extraction completed.
    /// TS: `tengu_extract_memories_extraction`.
    ExtractionCompleted {
        turn_count: i32,
        input_tokens: i64,
        output_tokens: i64,
        files_written: i32,
        duration_ms: i64,
    },

    /// Auto-dream consolidation fired.
    /// TS: `tengu_auto_dream_fired`.
    AutoDreamFired {
        hours_since_last: i64,
        sessions_since_last: i32,
    },

    /// Auto-dream consolidation completed.
    /// TS: `tengu_auto_dream_completed`.
    AutoDreamCompleted {
        sessions_reviewed: i32,
        files_changed: i32,
        duration_ms: i64,
    },

    /// Session-memory extraction fired.
    /// TS: `tengu_session_memory_extraction`.
    SessionMemoryExtracted {
        input_tokens: i64,
        output_tokens: i64,
        duration_ms: i64,
    },
}

/// Reason auto-memory was disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisableReason {
    EnvVar,
    Settings,
    BareMode,
    RemoteMode,
    FeatureGate,
}

impl DisableReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnvVar => "env_var",
            Self::Settings => "settings",
            Self::BareMode => "bare_mode",
            Self::RemoteMode => "remote_mode",
            Self::FeatureGate => "feature_gate",
        }
    }
}

/// Trait the memory crate uses to emit events. Implemented by
/// `coco-otel`-backed adapters; tests use [`NoopEmitter`].
pub trait MemoryTelemetryEmitter: Send + Sync {
    fn emit(&self, event: MemoryEvent);
}

/// Default emitter — drops events on the floor.
#[derive(Debug, Default)]
pub struct NoopEmitter;

impl MemoryTelemetryEmitter for NoopEmitter {
    fn emit(&self, _event: MemoryEvent) {}
}

/// Adapter that maps [`MemoryEvent`] onto an [`coco_otel::OtelManager`].
///
/// Each TS `tengu_*` event lands as a counter; numeric payload fields
/// (token counts, durations, file counts) are emitted as histograms /
/// `record_duration` so dashboards can chart distribution. Tag values
/// preserve the TS event names so downstream pipelines that already
/// know `tengu_extract_memories_extraction` keep working.
#[derive(Clone)]
pub struct OtelEmitter {
    manager: std::sync::Arc<coco_otel::OtelManager>,
}

impl std::fmt::Debug for OtelEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelEmitter").finish()
    }
}

impl OtelEmitter {
    pub fn new(manager: std::sync::Arc<coco_otel::OtelManager>) -> Self {
        Self { manager }
    }
}

impl MemoryTelemetryEmitter for OtelEmitter {
    fn emit(&self, event: MemoryEvent) {
        match event {
            MemoryEvent::MemdirLoaded {
                line_count,
                byte_count,
                was_truncated,
                was_byte_truncated,
                has_team,
            } => {
                let truncated = bool_str(was_truncated);
                let byte_truncated = bool_str(was_byte_truncated);
                let team = bool_str(has_team);
                self.manager.counter(
                    "tengu_memdir_loaded",
                    1,
                    &[
                        ("was_truncated", truncated),
                        ("was_byte_truncated", byte_truncated),
                        ("has_team", team),
                    ],
                );
                self.manager.histogram("memdir.line_count", line_count, &[]);
                self.manager.histogram("memdir.byte_count", byte_count, &[]);
            }
            MemoryEvent::MemdirDisabled { reason } => {
                self.manager
                    .counter("tengu_memdir_disabled", 1, &[("reason", reason.as_str())]);
            }
            MemoryEvent::ExtractionToolDenied { tool_name } => {
                self.manager.counter(
                    "tengu_auto_mem_tool_denied",
                    1,
                    &[("tool", tool_name.as_str())],
                );
            }
            MemoryEvent::ExtractionSkippedDirectWrite { message_count } => {
                self.manager
                    .counter("tengu_extract_memories_skipped_direct_write", 1, &[]);
                self.manager
                    .histogram("extract.message_count", message_count as i64, &[]);
            }
            MemoryEvent::ExtractionCompleted {
                turn_count,
                input_tokens,
                output_tokens,
                files_written,
                duration_ms,
            } => {
                self.manager
                    .counter("tengu_extract_memories_extraction", 1, &[]);
                self.manager
                    .histogram("extract.turn_count", turn_count as i64, &[]);
                self.manager
                    .histogram("extract.input_tokens", input_tokens, &[]);
                self.manager
                    .histogram("extract.output_tokens", output_tokens, &[]);
                self.manager
                    .histogram("extract.files_written", files_written as i64, &[]);
                self.manager.record_duration(
                    "extract.duration",
                    std::time::Duration::from_millis(duration_ms.max(0) as u64),
                    &[],
                );
            }
            MemoryEvent::AutoDreamFired {
                hours_since_last,
                sessions_since_last,
            } => {
                self.manager.counter("tengu_auto_dream_fired", 1, &[]);
                self.manager
                    .histogram("dream.hours_since_last", hours_since_last, &[]);
                self.manager.histogram(
                    "dream.sessions_since_last",
                    sessions_since_last as i64,
                    &[],
                );
            }
            MemoryEvent::AutoDreamCompleted {
                sessions_reviewed,
                files_changed,
                duration_ms,
            } => {
                self.manager.counter("tengu_auto_dream_completed", 1, &[]);
                self.manager
                    .histogram("dream.sessions_reviewed", sessions_reviewed as i64, &[]);
                self.manager
                    .histogram("dream.files_changed", files_changed as i64, &[]);
                self.manager.record_duration(
                    "dream.duration",
                    std::time::Duration::from_millis(duration_ms.max(0) as u64),
                    &[],
                );
            }
            MemoryEvent::SessionMemoryExtracted {
                input_tokens,
                output_tokens,
                duration_ms,
            } => {
                self.manager
                    .counter("tengu_session_memory_extraction", 1, &[]);
                self.manager
                    .histogram("session_memory.input_tokens", input_tokens, &[]);
                self.manager
                    .histogram("session_memory.output_tokens", output_tokens, &[]);
                self.manager.record_duration(
                    "session_memory.duration",
                    std::time::Duration::from_millis(duration_ms.max(0) as u64),
                    &[],
                );
            }
        }
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}
