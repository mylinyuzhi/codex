//! `HookEventsSource` impl on [`crate::async_registry::AsyncHookRegistry`].
//!
//! Bridges completed async-hook responses into the per-turn reminder
//! pipeline. Each call to [`HookEventsSource::drain`] delegates to
//! `AsyncHookRegistry::collect_responses()` — which marks responses
//! delivered — and maps each response to a `HookEvent::AsyncResponse`.
//!
//! **Scope**: only async hook responses are produced here.
//! Synchronous hook events (`HookEvent::Success` / `BlockingError` /
//! `AdditionalContext` / `StoppedContinuation`) are emitted directly
//! by the synchronous orchestration path, not by this registry.
//! Future work: add a sync-side buffer + trait impl if those events
//! need reminder-channel surfacing.
//!
//! TS parity: `getAsyncHookResponseAttachments()` at
//! `/lyz/codespace/3rd/claude-code/src/utils/attachments.ts:3464`
//! drains the async hook registry the same way.

use async_trait::async_trait;

use crate::async_registry::AsyncHookRegistry;
use crate::async_registry::AsyncHookResponse;
use coco_system_reminder::HookEvent;
use coco_system_reminder::HookEventsSource;

#[async_trait]
impl HookEventsSource for AsyncHookRegistry {
    async fn drain(&self, _agent_id: Option<&str>) -> Vec<HookEvent> {
        // `collect_responses` drains (marks delivered), so repeat
        // calls don't re-emit. Matches TS drain-on-read semantics.
        self.collect_responses()
            .await
            .into_iter()
            .map(to_hook_event)
            .collect()
    }
}

/// Map a completed async-hook response to the `async_hook_response`
/// reminder shape. `stdout` becomes `system_message`; `stderr` flows
/// into `additional_context` when non-empty. Empty fields are `None`
/// so the generator can short-circuit.
fn to_hook_event(r: AsyncHookResponse) -> HookEvent {
    let system_message = if r.stdout.is_empty() {
        None
    } else {
        Some(r.stdout)
    };
    let additional_context = if r.stderr.is_empty() {
        None
    } else {
        // Prefix with hook name so the model knows which hook surfaced
        // the stderr — TS bundles this inside `hookSpecificOutput`.
        Some(format!("[{}] {}", r.hook_name, r.stderr))
    };
    HookEvent::AsyncResponse {
        system_message,
        additional_context,
    }
}

#[cfg(test)]
#[path = "reminder_source.test.rs"]
mod tests;
