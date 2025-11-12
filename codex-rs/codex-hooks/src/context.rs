//! Hook execution context

use codex_protocol::hooks::HookEventContext;
use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Unified hook context
///
/// Provides access to:
/// - Event metadata (read-only)
/// - Shared mutable state (for hook coordination)
/// - Type-safe extensions
pub struct HookContext {
    /// Event that triggered this hook (read-only, Claude Code format)
    pub event: HookEventContext,

    /// Shared mutable state (accessible across hooks)
    pub state: Arc<RwLock<HookState>>,

    /// Type-safe extension data
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

/// Shared state that can be modified by hooks
///
/// This state is passed through the hook execution chain, allowing hooks
/// to coordinate and communicate.
#[derive(Debug, Default, Clone)]
pub struct HookState {
    // === Approval state ===
    /// Whether the operation has been approved by a previous hook
    pub already_approved: bool,

    /// Reason for approval/denial
    pub approval_reason: Option<String>,

    // === Sandbox state ===
    /// Selected sandbox type
    pub sandbox_type: Option<SandboxType>,

    /// Whether sandbox transformation has been applied
    pub sandbox_transformed: bool,

    // === Command mutations ===
    /// Command transformations to apply
    pub command_mutations: Vec<CommandMutation>,

    /// Environment variable additions/overrides
    pub env_mutations: HashMap<String, String>,

    // === Generic metadata ===
    /// Arbitrary key-value data
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Sandbox type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxType {
    None,
    MacosSeatbelt,
    LinuxSeccomp,
    WindowsRestrictedToken,
}

/// Command mutation operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandMutation {
    /// Add arguments before the command
    WrapWithPrefix(Vec<String>),

    /// Add arguments after the command
    WrapWithSuffix(Vec<String>),

    /// Completely replace the command
    Replace(Vec<String>),
}

impl HookContext {
    /// Create a new hook context
    pub fn new(event: HookEventContext) -> Self {
        Self {
            event,
            state: Arc::new(RwLock::new(HookState::default())),
            extensions: HashMap::new(),
        }
    }

    /// Insert type-safe extension data
    pub fn insert_extension<T: Any + Send + Sync>(&mut self, value: T) {
        self.extensions.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Get type-safe extension data
    pub fn get_extension<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Convenience method: get tool name from event data
    pub fn tool_name(&self) -> Option<&str> {
        use codex_protocol::hooks::HookEventData;

        match &self.event.event_data {
            HookEventData::PreToolUse { tool_name, .. }
            | HookEventData::PostToolUse { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        }
    }

    /// Convenience method: get tool input from event data
    pub fn tool_input(&self) -> Option<&serde_json::Value> {
        use codex_protocol::hooks::HookEventData;

        match &self.event.event_data {
            HookEventData::PreToolUse { tool_input, .. } => Some(tool_input),
            _ => None,
        }
    }

    /// Convenience method: check if already approved
    pub async fn is_approved(&self) -> bool {
        self.state.read().await.already_approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::hooks::{HookEventData, HookEventName};

    #[tokio::test]
    async fn test_hook_context_state_access() {
        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::PreToolUse {
                tool_name: "test_tool".to_string(),
                tool_input: serde_json::json!({}),
            },
        };

        let ctx = HookContext::new(event);

        // Initial state
        assert!(!ctx.is_approved().await);

        // Modify state
        {
            let mut state = ctx.state.write().await;
            state.already_approved = true;
            state.approval_reason = Some("Test approval".to_string());
        }

        // Read modified state
        assert!(ctx.is_approved().await);
        {
            let state = ctx.state.read().await;
            assert_eq!(state.approval_reason, Some("Test approval".to_string()));
        }
    }

    #[test]
    fn test_hook_context_extensions() {
        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::Other,
        };

        let mut ctx = HookContext::new(event);

        // Insert extension data
        ctx.insert_extension("test_data".to_string());
        ctx.insert_extension(42i32);

        // Retrieve extension data
        assert_eq!(
            ctx.get_extension::<String>(),
            Some(&"test_data".to_string())
        );
        assert_eq!(ctx.get_extension::<i32>(), Some(&42));
        assert_eq!(ctx.get_extension::<bool>(), None);
    }

    #[test]
    fn test_tool_name_extraction() {
        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::PreToolUse {
                tool_name: "local_shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
            },
        };

        let ctx = HookContext::new(event);
        assert_eq!(ctx.tool_name(), Some("local_shell"));
    }

    #[test]
    fn test_command_mutations() {
        let mutation =
            CommandMutation::WrapWithPrefix(vec!["sandbox-exec".to_string(), "-p".to_string()]);

        match mutation {
            CommandMutation::WrapWithPrefix(args) => {
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], "sandbox-exec");
            }
            _ => panic!("Wrong mutation type"),
        }
    }
}
