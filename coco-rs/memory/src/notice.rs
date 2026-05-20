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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoticeVerb {
    /// ExtractService wrote new memory files — primary turn-end
    /// save event. TS: default `createMemorySavedMessage` verb.
    Saved,
    /// DreamService merged / pruned existing memories — auto-dream
    /// success. TS: `autoDream.ts:247` `verb: 'Improved'`.
    Improved,
    /// Main agent (or user via `/memory` editor) directly edited a
    /// memory file via `Edit`/`Write`/`NotebookEdit`. Emitted by the
    /// engine's post-write classification pass — Gap 4 / TS
    /// `useMemoryUpdateNotification`. Distinct from `Saved`/`Improved`
    /// so the TUI can render it with a different color and copy
    /// ("Memory updated: foo.md") that hints the user, not a subagent,
    /// owned the change.
    ManualEdit,
}

impl NoticeVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Saved => "Saved",
            Self::Improved => "Improved",
            Self::ManualEdit => "Updated",
        }
    }

    /// Cross-source dedup priority — higher wins when the same path
    /// shows up in multiple notices from one drain. A fork's
    /// `Saved`/`Improved` is a more precise signal than the engine's
    /// post-write `ManualEdit` classification (which fires for any
    /// Edit/Write of a memory-managed file). When both fire for the
    /// same path same turn the user should see one toast, not two.
    fn priority(self) -> u8 {
        match self {
            Self::Saved => 3,
            Self::Improved => 2,
            Self::ManualEdit => 1,
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

    /// Append a notice. **Panics on poisoned mutex** — a poisoned
    /// `Mutex` means a prior `push`/`drain` panicked mid-write,
    /// leaving the `Vec` in an unknown state. Silently dropping the
    /// notice would lose user-visible "memory saved" toasts forever
    /// (TODO at the prior call site explicitly acknowledged this).
    /// Library-internal invariant — surface the bug at test time.
    pub fn push(&self, notice: MemoryUserNotice) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(notice);
    }

    /// Take everything queued and clear the inbox. Called by the
    /// engine once per turn from `finalize_turn_post_tools`.
    ///
    /// Cross-source dedup: when the same path appears under multiple
    /// notices (e.g. extract's `Saved` and the engine's `ManualEdit`
    /// for the same Edit call), keep the higher-priority verb
    /// (`Saved > Improved > ManualEdit`). The original notice order is
    /// preserved across distinct paths. This stops the TUI from
    /// rendering both "Saved foo.md" and "Updated foo.md" in the same
    /// turn — they describe the same write observed twice.
    pub fn drain(&self) -> Vec<MemoryUserNotice> {
        let raw = {
            let mut g = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *g)
        };

        // First pass: pick the winning verb per path. Stored
        // in a HashMap that's only read for membership / priority
        // comparison — the second pass below preserves the original
        // FIFO ordering of distinct paths.
        let mut winners: std::collections::HashMap<String, NoticeVerb> =
            std::collections::HashMap::new();
        for notice in &raw {
            for path in &notice.written_paths {
                let new_pri = notice.verb.priority();
                winners
                    .entry(path.clone())
                    .and_modify(|existing| {
                        if new_pri > existing.priority() {
                            *existing = notice.verb;
                        }
                    })
                    .or_insert(notice.verb);
            }
        }

        // Second pass: emit each unique path exactly once, under the
        // winning verb. Group by verb so a single notice carries all
        // paths sharing the same verb (matching the pre-dedup shape).
        let mut grouped: std::collections::HashMap<NoticeVerb, Vec<String>> =
            std::collections::HashMap::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for notice in &raw {
            for path in &notice.written_paths {
                if !seen.insert(path.clone()) {
                    continue;
                }
                let verb = winners.get(path).copied().unwrap_or(notice.verb);
                grouped.entry(verb).or_default().push(path.clone());
            }
        }

        // Emit in the verb priority order so the user sees real
        // memory saves before manual edits — keeps the higher-signal
        // notices on top.
        let mut out: Vec<MemoryUserNotice> = Vec::new();
        for verb in [
            NoticeVerb::Saved,
            NoticeVerb::Improved,
            NoticeVerb::ManualEdit,
        ] {
            if let Some(paths) = grouped.remove(&verb)
                && !paths.is_empty()
            {
                out.push(MemoryUserNotice {
                    written_paths: paths,
                    verb,
                });
            }
        }
        out
    }

    /// Test helper — peek the count without draining.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("NoticeInbox mutex poisoned — invariant broken")
            .len()
    }

    /// Test helper — `true` when no notices are queued.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[path = "notice.test.rs"]
mod tests;
