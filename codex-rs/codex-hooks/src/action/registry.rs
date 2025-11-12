//! Global registry for native hook functions

use super::native::NativeHookFn;
use crate::context::HookContext;
use crate::decision::HookResult;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Global registry for native hook functions
pub struct NativeHookRegistry {
    functions: HashMap<String, NativeHookFn>,
}

impl NativeHookRegistry {
    fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Register a native hook function
    pub fn register(&mut self, id: String, function: NativeHookFn) {
        self.functions.insert(id, function);
    }

    /// Get a registered function
    pub fn get(&self, id: &str) -> Option<NativeHookFn> {
        self.functions.get(id).cloned()
    }

    /// List all registered function IDs
    pub fn list(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }

    /// Check if a function is registered
    pub fn contains(&self, id: &str) -> bool {
        self.functions.contains_key(id)
    }
}

// Global singleton
static NATIVE_REGISTRY: Lazy<RwLock<NativeHookRegistry>> =
    Lazy::new(|| RwLock::new(NativeHookRegistry::new()));

/// Register a native hook function (public API)
///
/// # Example
///
/// ```ignore
/// use codex_hooks::action::registry::register_native_hook;
/// use codex_hooks::decision::HookResult;
///
/// register_native_hook("my_security_check", |ctx| {
///     // Your security logic here
///     if is_safe(ctx) {
///         HookResult::continue_with(vec![])
///     } else {
///         HookResult::abort("Unsafe operation detected")
///     }
/// });
/// ```
pub fn register_native_hook<F>(id: impl Into<String>, function: F)
where
    F: Fn(&HookContext) -> HookResult + Send + Sync + 'static,
{
    let id = id.into();
    NATIVE_REGISTRY
        .write()
        .expect("Failed to acquire registry lock")
        .register(id, Arc::new(function));
}

/// Get a registered native hook function
pub fn get_native_hook(id: &str) -> Option<NativeHookFn> {
    NATIVE_REGISTRY
        .read()
        .expect("Failed to acquire registry lock")
        .get(id)
}

/// List all registered native hook functions
pub fn list_native_hooks() -> Vec<String> {
    NATIVE_REGISTRY
        .read()
        .expect("Failed to acquire registry lock")
        .list()
}

/// Check if a function is registered
pub fn contains_native_hook(id: &str) -> bool {
    NATIVE_REGISTRY
        .read()
        .expect("Failed to acquire registry lock")
        .contains(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_operations() {
        register_native_hook("test_fn_1", |_| HookResult::skip());
        register_native_hook("test_fn_2", |_| HookResult::continue_with(vec![]));

        assert!(contains_native_hook("test_fn_1"));
        assert!(contains_native_hook("test_fn_2"));
        assert!(!contains_native_hook("nonexistent"));

        let hooks = list_native_hooks();
        assert!(hooks.contains(&"test_fn_1".to_string()));
        assert!(hooks.contains(&"test_fn_2".to_string()));

        let func = get_native_hook("test_fn_1").unwrap();
        let event = codex_protocol::hooks::HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: codex_protocol::hooks::HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: codex_protocol::hooks::HookEventData::Other,
        };
        let ctx = HookContext::new(event);
        let result = func(&ctx);
        assert!(matches!(
            result.decision,
            crate::decision::HookDecision::Skip
        ));
    }
}
