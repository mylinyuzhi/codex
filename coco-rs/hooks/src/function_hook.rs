//! In-process function hooks — TS parity for `addFunctionHook`.
//!
//! Function hooks are TypeScript-style in-memory callbacks that
//! evaluate a predicate over the current message history when their
//! event fires. Unlike the four settings-loaded handler types
//! ([`HookHandler::Command`](super::HookHandler::Command) /
//! [`Prompt`](super::HookHandler::Prompt) /
//! [`Http`](super::HookHandler::Http) /
//! [`Agent`](super::HookHandler::Agent)) which deserialize from
//! `settings.json`, function hooks are **registered in code** during
//! session bootstrap and live only for the lifetime of the session.
//!
//! TS source: `utils/hooks/sessionHooks.ts` (`addFunctionHook` /
//! `removeFunctionHook`) — function hooks land in
//! `AppState.sessionHooks` (a per-session map), separate from settings
//! hooks. Coco-rs mirrors that split by storing them in a separate
//! field on [`crate::HookRegistry`] rather than as a new variant on
//! [`HookHandler`] — the latter would break `Serialize` / `Deserialize`
//! round-tripping of settings-derived hooks.
//!
//! ## Use cases
//!
//! 1. **`StructuredOutput` Stop enforcement** — `hookHelpers.ts:70-83`.
//!    Prevent Stop until the model successfully calls the
//!    `StructuredOutput` tool. Wired in
//!    [`coco_tools::register_structured_output_tool`] and friends.
//! 2. **Swarm teammate init** — `utils/swarm/teammateInit.ts:98`.
//!    Block Stop until team config is acknowledged. (Pending port.)
//!
//! ## Concurrency
//!
//! Predicates are synchronous (TS callback is sync) but
//! [`crate::orchestration`] drives them via
//! [`tokio::task::spawn_blocking`] so a CPU-heavy predicate can't
//! starve the tokio runtime, and bounded by
//! [`tokio::time::timeout`] with the hook's configured timeout.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use coco_messages::Message;
use coco_types::HookEventType;

/// Predicate evaluated against the current message history.
///
/// Implementations must be `Send + Sync + Debug` because the registry
/// holds them in `Arc<dyn FunctionHookPredicate>` and the orchestrator
/// drives execution from arbitrary tokio worker threads. `Debug` is
/// load-bearing — registry diagnostics print this value when a hook
/// fires.
///
/// Implementations are conventionally **pure**: no I/O, no mutation of
/// shared state. If state is required, the predicate type can hold an
/// `Arc<Mutex<…>>` field and use interior mutability — but doing so is
/// an antipattern for the common case (TS predicates are pure scans
/// over `messages`).
pub trait FunctionHookPredicate: Send + Sync + fmt::Debug {
    /// Return `true` when the condition is satisfied (the event is
    /// permitted to proceed) or `false` when it is not (a Stop / etc.
    /// should be blocked and the configured `error_message` injected
    /// into the conversation).
    ///
    /// `messages` is the session's current message history snapshot at
    /// the moment the hook fires. Predicates are expected to scan
    /// recent assistant turns; they MUST treat the slice as read-only
    /// (the Arc-wrapping is for cheap sharing, not interior mutation).
    ///
    /// ## Performance + cancellation contract
    ///
    /// Implementations MUST be **fast** (sub-millisecond expected) and
    /// **pure**. The orchestrator drives `evaluate` via
    /// [`tokio::task::spawn_blocking`] under a
    /// [`tokio::time::timeout`]. When the timeout fires, the
    /// `JoinHandle` is dropped but the spawned blocking thread keeps
    /// running until `evaluate` returns — `spawn_blocking` cannot
    /// interrupt synchronous code. The hook is reported as failed
    /// (block-Stop semantics) but the predicate thread leaks until it
    /// returns. A predicate that hangs indefinitely leaks an OS
    /// thread for the lifetime of the process.
    fn evaluate(&self, messages: &[Arc<Message>]) -> bool;

    /// Stable identifier for logs / telemetry. Use a static `&str`
    /// where possible so this is allocation-free.
    fn name(&self) -> &str;
}

/// Events that actually dispatch function hooks today.
///
/// Settings hooks fire for every variant of [`HookEventType`], but the
/// function-hook execution path lives only in [`crate::orchestration`]
/// entry points that thread message history (currently
/// [`crate::orchestration::execute_stop`]). Registering for any other
/// event would silently never fire — worse than rejecting the call —
/// so [`crate::HookRegistry::register_function_hook`] refuses the
/// registration with [`RegisterFunctionHookError::UnsupportedEvent`].
///
/// Add the event here when you also wire `evaluate_function_hooks`
/// into the matching entry point in `orchestration.rs`.
pub const FUNCTION_HOOK_SUPPORTED_EVENTS: &[HookEventType] = &[HookEventType::Stop];

/// Reasons [`crate::HookRegistry::register_function_hook`] can refuse a
/// registration. Both variants are programmer errors at the call site,
/// not user-facing problems — surface them as panics in tests and
/// `tracing::error!` + abort in production.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterFunctionHookError {
    /// The supplied id is already registered. Re-registering the same
    /// id would create a silent duplicate (matched both by lookup and
    /// nuked together by `remove_function_hook`). Programmer error.
    #[error("function hook with id {0:?} is already registered")]
    DuplicateId(String),
    /// The supplied [`HookEventType`] is not in
    /// [`FUNCTION_HOOK_SUPPORTED_EVENTS`]. Function hooks for this
    /// event would persist but never fire — surface the bug at
    /// registration time instead.
    #[error(
        "function hooks are only dispatched for events in \
         FUNCTION_HOOK_SUPPORTED_EVENTS; refused {0:?}"
    )]
    UnsupportedEvent(HookEventType),
}

/// A registered function hook.
///
/// Lives in [`crate::HookRegistry::function_hooks`]; cloned cheaply
/// (the predicate behind `Arc`, other fields are small owned values).
#[derive(Clone)]
pub struct FunctionHook {
    /// Unique id, used by [`crate::HookRegistry::remove_function_hook`]
    /// to remove a single registration. TS parity:
    /// `FunctionHook.id`.
    pub id: String,
    pub event: HookEventType,
    /// TS-parity matcher string; `None` means "fire on any matcher"
    /// (TS treats `''` as wildcard).
    pub matcher: Option<String>,
    pub timeout: Duration,
    /// Predicate to evaluate. Cloned `Arc` on every fire — no fn-pointer
    /// indirection cost beyond the dyn dispatch.
    pub predicate: Arc<dyn FunctionHookPredicate>,
    /// Text to inject into the conversation when `predicate.evaluate()`
    /// returns `false`. TS: `FunctionHook.errorMessage`.
    pub error_message: String,
}

impl fmt::Debug for FunctionHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionHook")
            .field("id", &self.id)
            .field("event", &self.event)
            .field("matcher", &self.matcher)
            .field("timeout", &self.timeout)
            .field("predicate", &self.predicate)
            .field("error_message", &self.error_message)
            .finish()
    }
}

#[cfg(test)]
#[path = "function_hook.test.rs"]
mod tests;
