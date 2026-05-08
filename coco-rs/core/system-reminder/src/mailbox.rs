//! Inter-turn reminder mailbox.
//!
//! Several reminders ([`AttachmentType::CommandPermissions`],
//! [`AttachmentType::DynamicSkill`], [`AttachmentType::StructuredOutput`],
//! [`AttachmentType::TeammateShutdownBatch`] — see `audit-gaps.md` Round 13)
//! fire as a side-effect of subsystem events that don't have the engine
//! turn-loop on the call stack: a slash command modifying permissions, a
//! skill loader matching a path-glob, a tool returning structured JSON, the
//! swarm coordinator shutting down teammates. Each of those events needs
//! to push a reminder body into *the next* turn's reminder pipeline without
//! blocking on it.
//!
//! The mailbox is the seam: producers call [`ReminderMailboxRef::put_*`]
//! from anywhere, and the engine [`ReminderMailbox::drain`]s the queue at
//! the top of every turn into [`crate::TurnReminderInput`]. "Latest snapshot
//! wins" semantics — putting the same key twice in one turn keeps only
//! the most recent value, matching TS behaviour where these reminders
//! describe *current* subsystem state, not a stream of events.
//!
//! ## Architecture
//!
//! Two traits split producer/consumer access so accidental drains from a
//! producer-only callsite don't compile:
//!
//! - [`ReminderMailboxRef`] — producer-facing; subsystems hold
//!   `Arc<dyn ReminderMailboxRef>` and only see `put_*` methods.
//! - [`ReminderMailbox`] (concrete) — engine-facing; exposes
//!   [`ReminderMailbox::drain`]. The engine holds `Arc<ReminderMailbox>`
//!   and produces an `Arc<dyn ReminderMailboxRef>` via
//!   [`ReminderMailbox::handle`] for plumbing into subsystems.
//!
//! [`NoOpReminderMailbox`] is the default for tests / non-production
//! contexts that never observe the next-turn reminder.

use std::sync::Arc;
use std::sync::Mutex;

/// Snapshot of every "latest event" reminder body waiting to fire on the
/// next turn. Cleared by [`ReminderMailbox::drain`] each turn.
///
/// Each field is the **already-formatted text body** the corresponding
/// generator emits — producers do the formatting because they own the
/// domain shape; the mailbox is just a typed forwarding queue.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReminderMailboxState {
    /// Body for [`crate::AttachmentType::CommandPermissions`]. Set by the
    /// slash-command handler when the user adds/removes a permission rule.
    /// TS source: `processSlashCommand.tsx:909`.
    pub command_permissions: Option<String>,
    /// Body for [`crate::AttachmentType::DynamicSkill`]. Set by the skill
    /// loader when path-glob matching activates a skill.
    /// TS source: `attachments.ts:2589`.
    pub dynamic_skill: Option<String>,
    /// Body for [`crate::AttachmentType::StructuredOutput`]. Set after a
    /// tool returns a structured JSON result so the model sees the schema.
    /// TS source: `services/tools/toolExecution.ts:1276`.
    pub structured_output: Option<String>,
    /// Body for [`crate::AttachmentType::TeammateShutdownBatch`]. Set by
    /// the swarm coordinator when multiple teammates shut down in a batch.
    /// TS source: `collapseTeammateShutdowns.ts:43`.
    pub teammate_shutdown_batch: Option<String>,
}

/// Producer-facing trait for [`ReminderMailbox`].
///
/// Subsystems hold `Arc<dyn ReminderMailboxRef>` (cloneable, sharable)
/// and call `put_*` on it. Implementations must be cheap and infallible —
/// these calls run inside slash-command handlers, tool result paths, etc.,
/// where blocking would surprise users.
pub trait ReminderMailboxRef: Send + Sync + std::fmt::Debug {
    fn put_command_permissions(&self, body: String);
    fn put_dynamic_skill(&self, body: String);
    fn put_structured_output(&self, body: String);
    fn put_teammate_shutdown_batch(&self, body: String);
}

/// Concrete mailbox. Engine holds `Arc<ReminderMailbox>` and:
///
/// 1. Hands an `Arc<dyn ReminderMailboxRef>` to subsystems
///    via [`Self::handle`] (just the same Arc, type-erased).
/// 2. Calls [`Self::drain`] at the top of every turn.
#[derive(Debug, Default)]
pub struct ReminderMailbox {
    inner: Mutex<ReminderMailboxState>,
}

impl ReminderMailbox {
    /// Build a new empty mailbox wrapped in an `Arc` ready to share.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Type-erase `Arc<Self>` to `Arc<dyn ReminderMailboxRef>` for
    /// plumbing into subsystems.
    pub fn handle(self: Arc<Self>) -> Arc<dyn ReminderMailboxRef> {
        self
    }

    /// Drain every queued snapshot, leaving the mailbox empty. Called by
    /// the engine at the top of each turn. Any producer write that races
    /// the drain lands in the *next* turn — strictly safe given the
    /// "latest snapshot wins" semantics.
    pub fn drain(&self) -> ReminderMailboxState {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }

    /// Inspect the current state without draining (test/debug helper).
    /// Production callers should prefer [`Self::drain`] which clears as
    /// part of consumption.
    #[cfg(test)]
    pub fn peek(&self) -> ReminderMailboxState {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ReminderMailboxRef for ReminderMailbox {
    fn put_command_permissions(&self, body: String) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.command_permissions = Some(body);
        }
    }
    fn put_dynamic_skill(&self, body: String) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.dynamic_skill = Some(body);
        }
    }
    fn put_structured_output(&self, body: String) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.structured_output = Some(body);
        }
    }
    fn put_teammate_shutdown_batch(&self, body: String) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.teammate_shutdown_batch = Some(body);
        }
    }
}

/// No-op mailbox for tests and contexts that never read the next-turn
/// reminder (subagent forks, headless one-shots, etc.). Every `put_*`
/// silently drops.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpReminderMailbox;

impl ReminderMailboxRef for NoOpReminderMailbox {
    fn put_command_permissions(&self, _body: String) {}
    fn put_dynamic_skill(&self, _body: String) {}
    fn put_structured_output(&self, _body: String) {}
    fn put_teammate_shutdown_batch(&self, _body: String) {}
}

/// Convenience: a default `Arc<dyn ReminderMailboxRef>` that drops every
/// put. Useful for `..Default::default()` builders that want a sane
/// empty value.
pub fn noop_reminder_mailbox() -> Arc<dyn ReminderMailboxRef> {
    Arc::new(NoOpReminderMailbox)
}

#[cfg(test)]
#[path = "mailbox.test.rs"]
mod tests;
