//! User-visible memory notices — TS parity with the
//! `appendSystemMessage(createMemorySavedMessage(...))` calls in
//! `services/extractMemories/extractMemories.ts:491-496` and
//! `services/autoDream/autoDream.ts:240-247`.
//!
//! After a successful extract / dream run, the service pushes a
//! [`MemoryUserNotice`] onto the runtime's [`NoticeInbox`]. The engine
//! drains the inbox at the end of each turn and constructs a
//! [`coco_messages::SystemMemorySavedMessage`] in `history`, so the
//! user sees a "Saved 3 memories: …" / "Improved 2 memories: …" line
//! in their transcript.
//!
//! Why a separate channel from [`crate::telemetry::MemoryEvent`]:
//! telemetry is fire-and-forget (counters, histograms); notices need
//! ordered, drainable, per-turn delivery into the conversation
//! history. Mixing them would force the OTel emitter to know about
//! transcript writes.

use std::sync::Arc;
use std::sync::Mutex;

/// Display verb for the user-visible message. Mirrors TS
/// `createMemorySavedMessage` default ("Saved") + the
/// `autoDream.ts:247` override (`verb: 'Improved'`) for dream
/// consolidations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeVerb {
    Saved,
    Improved,
}

impl NoticeVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Saved => "Saved",
            Self::Improved => "Improved",
        }
    }
}

/// One queued user-visible notice.
#[derive(Debug, Clone)]
pub struct MemoryUserNotice {
    /// Paths the subagent wrote / improved. Excludes `MEMORY.md`
    /// (the index is mechanical and not user-relevant).
    pub written_paths: Vec<String>,
    pub verb: NoticeVerb,
}

/// Append-only mailbox shared between memory services and the engine
/// drain hook. Cheap to clone (`Arc` inside).
#[derive(Debug, Default, Clone)]
pub struct NoticeInbox {
    inner: Arc<Mutex<Vec<MemoryUserNotice>>>,
}

impl NoticeInbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a notice. Best-effort — silently drops on poisoned
    /// mutex (the runtime is shutting down anyway).
    pub fn push(&self, notice: MemoryUserNotice) {
        if let Ok(mut g) = self.inner.lock() {
            g.push(notice);
        }
    }

    /// Take everything queued and clear the inbox. Called by the
    /// engine once per turn from `finalize_turn_post_tools`.
    pub fn drain(&self) -> Vec<MemoryUserNotice> {
        self.inner
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    /// Test helper — peek the count without draining.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Test helper — `true` when no notices are queued. Mirrors the
    /// `Vec::is_empty` ergonomic alongside `len` so clippy stays happy
    /// (`len_without_is_empty`).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[path = "notice.test.rs"]
mod tests;
