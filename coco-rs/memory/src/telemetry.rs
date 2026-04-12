//! Auto-memory telemetry events.
//!
//! TS: Multiple files emit `tengu_*` analytics events for observability.
//! These events track memory system health, extraction efficiency,
//! and usage patterns.

/// Telemetry event types for the auto-memory system.
#[derive(Debug, Clone)]
pub enum MemoryEvent {
    /// Memory directory loaded — tracks file and subdirectory counts.
    ///
    /// TS: `tengu_memdir_loaded`
    MemdirLoaded {
        file_count: i32,
        subdir_count: i32,
        has_team: bool,
    },

    /// Auto-memory disabled — tracks the reason.
    ///
    /// TS: `tengu_memdir_disabled`
    MemdirDisabled { reason: DisableReason },

    /// Team memory disabled.
    ///
    /// TS: `tengu_team_memdir_disabled`
    TeamMemdirDisabled,

    /// Extraction agent denied a tool call.
    ///
    /// TS: `tengu_auto_mem_tool_denied`
    ToolDenied { tool_name: String, reason: String },

    /// Background extraction was skipped because the main agent
    /// already wrote memories during this turn.
    ///
    /// TS: `tengu_extract_memories_skipped_direct_write`
    ExtractionSkippedDirectWrite { message_count: i32 },

    /// Memory extraction completed.
    ///
    /// TS: `tengu_extract_memories_extraction`
    ExtractionCompleted {
        turn_count: i32,
        input_tokens: i64,
        output_tokens: i64,
        files_written: i32,
        files_updated: i32,
    },

    /// Auto-dream consolidation fired.
    ///
    /// TS: `tengu_auto_dream_fired`
    AutoDreamFired {
        hours_since_last: i64,
        sessions_since_last: i32,
    },
}

/// Reason auto-memory was disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisableReason {
    /// Disabled via environment variable.
    EnvVar,
    /// Disabled via settings.
    Settings,
    /// Disabled in bare mode (--bare).
    BareMode,
    /// Disabled in remote mode.
    RemoteMode,
    /// Feature gate not enabled.
    FeatureGate,
}

impl DisableReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EnvVar => "env_var",
            Self::Settings => "settings",
            Self::BareMode => "bare_mode",
            Self::RemoteMode => "remote_mode",
            Self::FeatureGate => "feature_gate",
        }
    }
}

/// Trait for emitting memory telemetry events.
///
/// Implemented by the application's telemetry system (e.g., OpenTelemetry).
/// The memory crate calls `emit()` when events occur.
pub trait MemoryTelemetryEmitter: Send + Sync {
    fn emit(&self, event: MemoryEvent);
}

/// No-op telemetry emitter (default when telemetry is disabled).
pub struct NoopEmitter;

impl MemoryTelemetryEmitter for NoopEmitter {
    fn emit(&self, _event: MemoryEvent) {}
}
