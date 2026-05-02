//! Staged context-collapse strategy (TS `services/contextCollapse/`).
//!
//! TS background: an Anthropic-internal strategy that pre-stages spans
//! of the conversation (`StagedRange`s) and commits them to compressed
//! placeholders (`CommitEntry`) when a token threshold is crossed.
//! When a prompt-too-long error fires it can drain the staged queue
//! eagerly. The external Claude Code build strips the runtime impl
//! but persists `marble-origami-commit` / `marble-origami-snapshot`
//! transcript lines for cross-session continuity.
//!
//! coco-rs ships a self-designed re-port behind
//! `compact.experimental.staged_compact.*` with the same persistence
//! shape so transcripts remain interchangeable. Wire format:
//! camelCase JSON, type discriminator preserved verbatim.
//!
//! TS schema sources:
//!   - types/logs.ts:255-269 `ContextCollapseCommitEntry`
//!   - types/logs.ts:282-295 `ContextCollapseSnapshotEntry`
//!   - utils/sessionStorage.ts:1541-1581 record helpers
//!
//! State machine:
//! ```text
//!   ─ STAGED ─────► COMMITTED ─► (transcript)
//!         │
//!         └─ WITHHELD (PTL 413) ─► RECOVERED (drain) ─► COMMITTED
//! ```

use coco_messages::Message;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One staged span awaiting commit.
///
/// TS: `staged[]` items inside `ContextCollapseSnapshotEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedRange {
    /// First archived message UUID (inclusive).
    pub start_uuid: Uuid,
    /// Last archived message UUID (inclusive).
    pub end_uuid: Uuid,
    /// Pre-computed summary text — used when the stage is committed.
    pub summary: String,
    /// Risk score (0..1). Higher = more eager spawn priority.
    pub risk: f64,
    /// Unix epoch milliseconds when this stage was created.
    pub staged_at: i64,
}

/// Committed collapse — archived span replaced by a placeholder.
///
/// TS: `ContextCollapseCommitEntry` (logs.ts:255).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitEntry {
    /// `'marble-origami-commit'`.
    #[serde(rename = "type")]
    pub type_: String,
    pub session_id: Uuid,
    /// Per-session monotonic id; 16-char uppercase hex by convention.
    pub collapse_id: String,
    pub summary_uuid: Uuid,
    pub summary_content: String,
    pub summary: String,
    pub first_archived_uuid: Uuid,
    pub last_archived_uuid: Uuid,
}

/// Latest snapshot of pending staged ranges + spawn-clock state.
///
/// TS: `ContextCollapseSnapshotEntry` (logs.ts:282). Last-wins by
/// `session_id` on resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    /// `'marble-origami-snapshot'`.
    #[serde(rename = "type")]
    pub type_: String,
    pub session_id: Uuid,
    pub staged: Vec<StagedRange>,
    /// Whether the spawn trigger is armed.
    pub armed: bool,
    /// Token count at the last spawn — drives spawn-clock recovery.
    pub last_spawn_tokens: i64,
}

impl CommitEntry {
    pub const TYPE: &'static str = "marble-origami-commit";

    pub fn new(
        session_id: Uuid,
        collapse_id: String,
        summary_uuid: Uuid,
        summary_content: String,
        summary: String,
        first_archived_uuid: Uuid,
        last_archived_uuid: Uuid,
    ) -> Self {
        Self {
            type_: Self::TYPE.to_string(),
            session_id,
            collapse_id,
            summary_uuid,
            summary_content,
            summary,
            first_archived_uuid,
            last_archived_uuid,
        }
    }
}

impl SnapshotEntry {
    pub const TYPE: &'static str = "marble-origami-snapshot";

    pub fn empty(session_id: Uuid) -> Self {
        Self {
            type_: Self::TYPE.to_string(),
            session_id,
            staged: Vec::new(),
            armed: false,
            last_spawn_tokens: 0,
        }
    }
}

/// In-memory ledger holding committed entries + the latest snapshot.
///
/// One ledger per session. Run-time API mirrors the TS module-level
/// `let committed: CommitEntry[]` + `let snapshot: SnapshotEntry`.
#[derive(Debug, Default)]
pub struct StagedCompactLedger {
    pub commits: Vec<CommitEntry>,
    pub snapshot: Option<SnapshotEntry>,
}

impl StagedCompactLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay persisted entries on session resume. Mirrors TS
    /// `restoreFromEntries(commits, snapshot)` (contextCollapse/persist.ts).
    pub fn restore_from_entries(
        &mut self,
        commits: Vec<CommitEntry>,
        snapshot: Option<SnapshotEntry>,
    ) {
        self.commits = commits;
        self.snapshot = snapshot;
    }

    /// Stage a new range (called by the ctx-agent / threshold detector).
    pub fn stage(&mut self, session_id: Uuid, range: StagedRange) {
        let snap = self
            .snapshot
            .get_or_insert_with(|| SnapshotEntry::empty(session_id));
        snap.staged.push(range);
    }

    /// Commit a staged range: remove from `staged`, append to `commits`.
    /// Returns the produced [`CommitEntry`] so callers can persist it.
    pub fn commit(
        &mut self,
        session_id: Uuid,
        index: usize,
        summary_uuid: Uuid,
        summary_content: String,
    ) -> Option<CommitEntry> {
        let snap = self.snapshot.as_mut()?;
        if index >= snap.staged.len() {
            return None;
        }
        let staged = snap.staged.remove(index);
        let collapse_id = next_collapse_id(&self.commits);
        let entry = CommitEntry::new(
            session_id,
            collapse_id,
            summary_uuid,
            summary_content,
            staged.summary.clone(),
            staged.start_uuid,
            staged.end_uuid,
        );
        self.commits.push(entry.clone());
        Some(entry)
    }

    /// Drain *all* staged ranges into commits — TS `recoverFromOverflow`.
    /// Returns the number of newly-committed entries.
    pub fn drain_overflow(
        &mut self,
        session_id: Uuid,
        mut summary_uuid_for: impl FnMut(&StagedRange) -> Uuid,
    ) -> Vec<CommitEntry> {
        let Some(snap) = self.snapshot.as_mut() else {
            return Vec::new();
        };
        let drained: Vec<StagedRange> = snap.staged.drain(..).collect();
        let mut produced = Vec::with_capacity(drained.len());
        for staged in drained {
            let collapse_id = next_collapse_id(&self.commits);
            let summary_uuid = summary_uuid_for(&staged);
            let entry = CommitEntry::new(
                session_id,
                collapse_id,
                summary_uuid,
                placeholder_text(&staged),
                staged.summary.clone(),
                staged.start_uuid,
                staged.end_uuid,
            );
            self.commits.push(entry.clone());
            produced.push(entry);
        }
        produced
    }

    /// Reset everything — TS `resetContextCollapse()` (REPL.tsx:3686).
    /// Called on rewind / autocompact failure when UUID mappings go stale.
    pub fn reset(&mut self) {
        self.commits.clear();
        self.snapshot = None;
    }

    /// Whether any committed collapse exists.
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty() && self.snapshot.as_ref().is_none_or(|s| s.staged.is_empty())
    }
}

/// Wire format `<collapsed id="…">summary</collapsed>` placeholder.
pub fn placeholder_text(range: &StagedRange) -> String {
    format!(
        "<collapsed id=\"{}\">{}</collapsed>",
        range.start_uuid.as_simple(),
        range.summary
    )
}

/// Apply committed collapses to a message slice, splicing each
/// `[first_archived_uuid..=last_archived_uuid]` range with a single
/// synthetic placeholder user message carrying `summary_uuid`.
///
/// TS: `services/contextCollapse/index.ts:applyCollapsesIfNeeded` —
/// invoked before each prompt build so the LLM sees collapsed spans
/// instead of the raw archived turns.
///
/// Behavior:
///   - Commits are processed in their `commits` order. Later commits
///     can overlap or be nested; we replay deterministically and skip
///     a commit whose archived range is already missing (already
///     collapsed by a previous one).
///   - Returns `(new_messages, applied_count)` where `applied_count`
///     is how many commits actually rewrote at least one message.
///   - When `commits` is empty, returns the input unchanged with
///     `applied_count = 0`.
pub fn apply_collapses_if_needed(
    messages: &[Message],
    commits: &[CommitEntry],
) -> (Vec<Message>, usize) {
    if commits.is_empty() {
        return (messages.to_vec(), 0);
    }
    let mut working: Vec<Message> = messages.to_vec();
    let mut applied = 0usize;
    for commit in commits {
        let Some(start) = working.iter().position(|m| {
            m.uuid()
                .copied()
                .is_some_and(|u| u == commit.first_archived_uuid)
        }) else {
            continue;
        };
        let Some(end) = working.iter().position(|m| {
            m.uuid()
                .copied()
                .is_some_and(|u| u == commit.last_archived_uuid)
        }) else {
            continue;
        };
        if end < start {
            continue;
        }
        let placeholder_msg = coco_messages::create_user_message_with_uuid(
            commit.summary_uuid,
            &commit.summary_content,
        );
        working.splice(start..=end, std::iter::once(placeholder_msg));
        applied += 1;
    }
    (working, applied)
}

/// Generate the next 16-char uppercase-hex collapse id.
///
/// TS encodes the id as a base-16 counter that resets at `0xFFFFFFFFFFFFFFFF`;
/// we approximate with a per-vector monotonic counter to avoid global state.
fn next_collapse_id(commits: &[CommitEntry]) -> String {
    let n = commits.len() as u64;
    format!("{:016X}", n.wrapping_add(1))
}

#[cfg(test)]
#[path = "staged.test.rs"]
mod tests;
