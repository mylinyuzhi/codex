//! Hook manager
//!
//! Central registry and trigger system for hooks.

use crate::context::HookContext;
use crate::context::HookState;
use crate::decision::HookDecision;
use crate::executor::HookExecutor;
use crate::types::HookPhase;
use crate::types::HookPriority;
use codex_protocol::hooks::HookEventContext;
use once_cell::sync::Lazy;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Hook manager
///
/// Manages hook registration and execution. Thread-safe and async-friendly.
pub struct HookManager {
    executors: BTreeMap<(HookPhase, HookPriority), Arc<HookExecutor>>,
    enabled: bool,
}

impl HookManager {
    /// Create a new hook manager
    pub fn new() -> Self {
        Self {
            executors: BTreeMap::new(),
            enabled: false,
        }
    }

    /// Register a hook executor for a specific phase and priority
    pub fn register(
        &mut self,
        phase: HookPhase,
        priority: HookPriority,
        executor: Arc<HookExecutor>,
    ) {
        self.executors.insert((phase, priority), executor);
    }

    /// Enable or disable the hook system
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if hooks are enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Trigger hooks for an event
    ///
    /// This is a synchronous operation (blocks until all hooks complete).
    /// Returns an error if any hook blocks execution.
    pub async fn trigger(&self, event: HookEventContext) -> Result<(), HookError> {
        if !self.enabled {
            return Ok(());
        }

        let phase: HookPhase = event.hook_event_name.into();
        let ctx = HookContext::new(event);

        // Execute hooks in priority order
        for ((exec_phase, priority), executor) in &self.executors {
            if *exec_phase != phase {
                continue;
            }

            tracing::debug!(
                "Triggering hooks for phase {:?}, priority {}",
                exec_phase,
                priority
            );

            let result = executor.execute(&ctx).await;

            // Handle final decision
            match result.final_decision {
                HookDecision::Abort { reason } => {
                    return Err(HookError::Aborted(reason));
                }
                HookDecision::AskUser { prompt } => {
                    return Err(HookError::UserConfirmationRequired(prompt));
                }
                HookDecision::Retry { reason } => {
                    return Err(HookError::RetryRequested(reason));
                }
                HookDecision::Continue | HookDecision::Skip => {
                    // Continue to next hook
                }
            }
        }

        Ok(())
    }

    /// Get the current hook state after triggering
    ///
    /// This can be used to extract effects applied by hooks.
    pub async fn get_state_from_context(ctx: &HookContext) -> HookState {
        ctx.state.read().await.clone()
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Hook execution errors
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("Hook aborted operation: {0}")]
    Aborted(String),

    #[error("Hook requires user confirmation: {0}")]
    UserConfirmationRequired(String),

    #[error("Hook requested retry: {0}")]
    RetryRequested(String),

    #[error("Hook execution failed: {0}")]
    ExecutionFailed(String),
}

// Global singleton
static HOOK_MANAGER: Lazy<RwLock<HookManager>> = Lazy::new(|| RwLock::new(HookManager::new()));

/// Initialize the global hook manager with a configuration
///
/// This should be called once at application startup.
pub async fn initialize(manager: HookManager) {
    *HOOK_MANAGER.write().await = manager;
}

/// Trigger a hook event (public API)
///
/// This is the main entry point for triggering hooks from application code.
pub async fn trigger_hook(event: HookEventContext) -> Result<(), HookError> {
    HOOK_MANAGER.read().await.trigger(event).await
}

/// Enable hooks globally
pub async fn enable_hooks() {
    HOOK_MANAGER.write().await.set_enabled(true);
}

/// Disable hooks globally
pub async fn disable_hooks() {
    HOOK_MANAGER.write().await.set_enabled(false);
}

/// Check if hooks are enabled
pub async fn is_enabled() -> bool {
    HOOK_MANAGER.read().await.is_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::bash::BashAction;
    use crate::action::HookAction;
    use crate::executor::HookExecutor;
    use crate::types::PRIORITY_NORMAL;
    use codex_protocol::hooks::HookEventData;
    use codex_protocol::hooks::HookEventName;

    #[tokio::test]
    async fn test_manager_registration() {
        let mut manager = HookManager::new();
        manager.set_enabled(true);

        let actions: Vec<Arc<dyn HookAction>> =
            vec![Arc::new(BashAction::new("echo 'test'".to_string(), 5000))];

        let executor = Arc::new(HookExecutor::new(actions, false));
        manager.register(HookPhase::PreToolUse, PRIORITY_NORMAL, executor);

        assert!(manager.is_enabled());
    }

    #[tokio::test]
    async fn test_manager_disabled() {
        let manager = HookManager::new();

        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::Other,
        };

        // Should succeed without executing (disabled)
        let result = manager.trigger(event).await;
        assert!(result.is_ok());
    }
}
