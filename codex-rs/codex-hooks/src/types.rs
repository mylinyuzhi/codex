//! Core types for the hook system

use codex_protocol::hooks::HookEventName;

/// Hook execution phase
///
/// Phases provide a finer-grained classification than event names for internal use.
/// Multiple event names can map to the same phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPhase {
    /// Before tool execution (first attempt)
    PreToolUse,

    /// Just before spawning the execution environment (sandbox transform)
    PreExecution,

    /// After tool execution
    PostToolUse,

    /// When an error occurs (retry logic)
    OnError,

    /// Other lifecycle events
    Other(HookEventName),
}

// Manual PartialOrd/Ord implementation that ignores the HookEventName value
impl PartialOrd for HookPhase {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HookPhase {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use HookPhase::*;
        match (self, other) {
            (PreToolUse, PreToolUse) => std::cmp::Ordering::Equal,
            (PreToolUse, _) => std::cmp::Ordering::Less,
            (_, PreToolUse) => std::cmp::Ordering::Greater,

            (PreExecution, PreExecution) => std::cmp::Ordering::Equal,
            (PreExecution, PostToolUse) => std::cmp::Ordering::Less,
            (PreExecution, OnError) => std::cmp::Ordering::Less,
            (PreExecution, Other(_)) => std::cmp::Ordering::Less,
            (PostToolUse, PreExecution) => std::cmp::Ordering::Greater,
            (OnError, PreExecution) => std::cmp::Ordering::Greater,
            (Other(_), PreExecution) => std::cmp::Ordering::Greater,

            (PostToolUse, PostToolUse) => std::cmp::Ordering::Equal,
            (PostToolUse, OnError) => std::cmp::Ordering::Less,
            (PostToolUse, Other(_)) => std::cmp::Ordering::Less,
            (OnError, PostToolUse) => std::cmp::Ordering::Greater,
            (Other(_), PostToolUse) => std::cmp::Ordering::Greater,

            (OnError, OnError) => std::cmp::Ordering::Equal,
            (OnError, Other(_)) => std::cmp::Ordering::Less,
            (Other(_), OnError) => std::cmp::Ordering::Greater,

            (Other(_), Other(_)) => std::cmp::Ordering::Equal,
        }
    }
}

impl From<HookEventName> for HookPhase {
    fn from(event: HookEventName) -> Self {
        match event {
            HookEventName::PreToolUse => HookPhase::PreToolUse,
            HookEventName::PostToolUse => HookPhase::PostToolUse,
            other => HookPhase::Other(other),
        }
    }
}

/// Hook priority (lower number = earlier execution)
pub type HookPriority = i32;

/// Priority constants for common use cases
pub const PRIORITY_FIRST: HookPriority = -1000;
pub const PRIORITY_EARLY: HookPriority = -100;
pub const PRIORITY_NORMAL: HookPriority = 0;
pub const PRIORITY_LATE: HookPriority = 100;
pub const PRIORITY_LAST: HookPriority = 1000;

/// Hook metadata
#[derive(Debug, Clone)]
pub struct HookMetadata {
    /// Unique identifier
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Execution phase
    pub phase: HookPhase,

    /// Execution priority (lower = earlier)
    pub priority: HookPriority,

    /// Whether this hook is currently enabled
    pub enabled: bool,
}

impl HookMetadata {
    pub fn new(id: impl Into<String>, name: impl Into<String>, phase: HookPhase) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            phase,
            priority: PRIORITY_NORMAL,
            enabled: true,
        }
    }

    pub fn with_priority(mut self, priority: HookPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_phase_ordering() {
        assert!(HookPhase::PreToolUse < HookPhase::PostToolUse);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(PRIORITY_FIRST < PRIORITY_EARLY);
        assert!(PRIORITY_EARLY < PRIORITY_NORMAL);
        assert!(PRIORITY_NORMAL < PRIORITY_LATE);
        assert!(PRIORITY_LATE < PRIORITY_LAST);
    }

    #[test]
    fn test_hook_metadata_builder() {
        let metadata = HookMetadata::new("test", "Test Hook", HookPhase::PreToolUse)
            .with_priority(PRIORITY_EARLY)
            .with_enabled(false);

        assert_eq!(metadata.id, "test");
        assert_eq!(metadata.priority, PRIORITY_EARLY);
        assert!(!metadata.enabled);
    }
}
