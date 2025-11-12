//! Hook decisions and effects

use crate::context::CommandMutation;
use std::collections::HashMap;
use std::fmt;

/// Result returned by hook actions
#[derive(Debug, Clone)]
pub struct HookResult {
    /// Decision about what should happen next
    pub decision: HookDecision,

    /// Side effects to apply
    pub effects: Vec<HookEffect>,
}

/// Hook decision (internal representation)
///
/// This is more detailed than the protocol's HookDecision to support
/// internal control flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// Continue to the next hook
    Continue,

    /// Abort the operation with an error
    Abort { reason: String },

    /// Ask the user for confirmation
    AskUser { prompt: String },

    /// Skip this hook (e.g., due to cache hit)
    Skip,

    /// Trigger retry logic
    Retry { reason: String },
}

/// Side effects that modify shared state
///
/// Effects are applied after a hook executes successfully, allowing
/// subsequent hooks to see the modifications.
#[derive(Debug, Clone)]
pub enum HookEffect {
    /// Mark the operation as approved
    SetApproved(bool),

    /// Set the sandbox type
    SetSandbox(Option<crate::context::SandboxType>),

    /// Add a command mutation
    MutateCommand(CommandMutation),

    /// Add or override environment variables
    MutateEnv(HashMap<String, String>),

    /// Add metadata to the context
    AddMetadata {
        key: String,
        value: serde_json::Value,
    },

    /// Cache a decision (for future lookups)
    CacheDecision {
        key: String,
        // Using Box<dyn Any> would require trait objects, simplified to String for now
        value: String,
    },

    /// Emit a log message
    Log { level: LogLevel, message: String },
}

/// Log level for hook messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl HookResult {
    /// Create a result that continues execution with the given effects
    pub fn continue_with(effects: Vec<HookEffect>) -> Self {
        Self {
            decision: HookDecision::Continue,
            effects,
        }
    }

    /// Create a result that aborts execution
    pub fn abort(reason: impl Into<String>) -> Self {
        Self {
            decision: HookDecision::Abort {
                reason: reason.into(),
            },
            effects: vec![],
        }
    }

    /// Create a result that skips this hook
    pub fn skip() -> Self {
        Self {
            decision: HookDecision::Skip,
            effects: vec![],
        }
    }

    /// Create a result that asks the user
    pub fn ask_user(prompt: impl Into<String>) -> Self {
        Self {
            decision: HookDecision::AskUser {
                prompt: prompt.into(),
            },
            effects: vec![],
        }
    }

    /// Create a result that triggers a retry
    pub fn retry(reason: impl Into<String>) -> Self {
        Self {
            decision: HookDecision::Retry {
                reason: reason.into(),
            },
            effects: vec![],
        }
    }
}

impl fmt::Display for HookDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookDecision::Continue => write!(f, "Continue"),
            HookDecision::Abort { reason } => write!(f, "Abort: {}", reason),
            HookDecision::AskUser { prompt } => write!(f, "AskUser: {}", prompt),
            HookDecision::Skip => write!(f, "Skip"),
            HookDecision::Retry { reason } => write!(f, "Retry: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_result_builders() {
        let continue_result = HookResult::continue_with(vec![]);
        assert!(matches!(continue_result.decision, HookDecision::Continue));

        let abort_result = HookResult::abort("Test error");
        match abort_result.decision {
            HookDecision::Abort { reason } => assert_eq!(reason, "Test error"),
            _ => panic!("Expected Abort decision"),
        }

        let skip_result = HookResult::skip();
        assert!(matches!(skip_result.decision, HookDecision::Skip));
    }

    #[test]
    fn test_hook_effects() {
        let effects = vec![
            HookEffect::SetApproved(true),
            HookEffect::Log {
                level: LogLevel::Info,
                message: "Test log".to_string(),
            },
        ];

        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], HookEffect::SetApproved(true)));
    }

    #[test]
    fn test_decision_display() {
        assert_eq!(HookDecision::Continue.to_string(), "Continue");
        assert_eq!(
            HookDecision::Abort {
                reason: "test".to_string()
            }
            .to_string(),
            "Abort: test"
        );
    }
}
