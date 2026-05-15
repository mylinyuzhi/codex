//! Session transcript persistence via JSONL rollout format.
//!
//! TS: utils/sessionStorage.ts — JSONL append-only transcript at
//! `~/.coco/projects/{sanitized_cwd}/{session_id}.jsonl`.
//!
//! Each line is a self-contained JSON entry: transcript messages
//! (user/assistant/system), metadata entries (custom-title, tag,
//! last-prompt), and compaction markers. The file is append-only
//! during normal operation; compaction rewrites are handled separately.

use serde::Deserialize;
use serde::Serialize;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Maximum transcript file size we will fully read into memory (50 MB).
/// Matches the TS `MAX_TRANSCRIPT_READ_BYTES` constant.
const MAX_TRANSCRIPT_READ_BYTES: u64 = 50 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Entry types
// ---------------------------------------------------------------------------

/// Closed set of `entry_type` discriminators we write to the JSONL.
/// Centralised so `build_transcript_entry` (write side) and
/// `reconstruct_message` (read side) can't drift, per the
/// "no hardcoded strings for closed sets" rule in CLAUDE.md.
pub mod entry_kind {
    pub const USER: &str = "user";
    pub const ASSISTANT: &str = "assistant";
    pub const SYSTEM: &str = "system";
    pub const ATTACHMENT: &str = "attachment";
    pub const TOOL_RESULT: &str = "tool_result";
}

/// Token usage for a single transcript entry. Field names mirror
/// TS `Usage` so transcripts are byte-compatible with `claude-code`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
}

/// A transcript message entry (user, assistant, system, attachment).
///
/// On-disk shape mirrors TS `SerializedMessage` from
/// `types/logs.ts`: camelCase keys (`parentUuid`, `sessionId`,
/// `isSidechain`, `gitBranch`, `costUsd`) so a JSONL written by
/// coco-rs is wire-compatible with `claude-code`'s.
///
/// `timestamp` is an ISO 8601 / RFC 3339 string — the leaf walk in
/// `recovery.rs` sorts leaves by lexicographic timestamp, which is
/// only correct for that format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    pub uuid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub is_sidechain: bool,
    /// The raw message payload (role + content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<serde_json::Value>,
    /// Token usage for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TranscriptUsage>,
    /// Model used for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Estimated cost in USD for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Catch-all for fields we don't model explicitly.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Metadata entries that live alongside transcript messages in the JSONL.
///
/// Variants use `kebab-case` for the `type` discriminator (TS-aligned:
/// `custom-title`, `last-prompt`, `marble-origami-commit`); inner
/// fields use camelCase so the on-disk shape matches TS Claude Code
/// (`{type:"custom-title", sessionId, customTitle}`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MetadataEntry {
    #[serde(rename_all = "camelCase")]
    CustomTitle {
        session_id: String,
        custom_title: String,
    },
    #[serde(rename_all = "camelCase")]
    Tag { session_id: String, tag: String },
    #[serde(rename_all = "camelCase")]
    LastPrompt {
        session_id: String,
        last_prompt: String,
    },
    #[serde(rename_all = "camelCase")]
    Summary { leaf_uuid: String, summary: String },
    #[serde(rename_all = "camelCase")]
    CostSummary {
        session_id: String,
        total_input_tokens: i64,
        total_output_tokens: i64,
        total_cost_usd: f64,
        #[serde(default)]
        model_usage: std::collections::HashMap<String, ModelCostEntry>,
    },
    /// File-history snapshot recorded by the rewind subsystem.
    ///
    /// TS: `FileHistorySnapshotMessage` from `types/logs.ts:188` —
    /// `{type: 'file-history-snapshot', messageId, snapshot, isSnapshotUpdate}`.
    /// Replayed on resume by `buildFileHistorySnapshotChain`
    /// (`utils/sessionStorage.ts:2248`) to rebuild the rewind picker
    /// and the disk-backup mapping. The `snapshot` payload is a
    /// passthrough JSON blob to keep `coco-session` free of a
    /// `coco-context` dependency — `coco-context::FileHistorySnapshot`
    /// owns the typed shape and (de)serializes through this Value.
    #[serde(rename_all = "camelCase")]
    FileHistorySnapshot {
        message_id: String,
        snapshot: serde_json::Value,
        #[serde(default)]
        is_snapshot_update: bool,
    },
    /// Staged context-collapse commit (TS `'marble-origami-commit'`).
    ///
    /// Persists one committed range so resume can replay the splice.
    /// `payload` is a passthrough JSON blob produced by
    /// `coco_compact::staged::CommitEntry` — keeping it untyped here
    /// avoids a `coco-session → coco-compact` dependency.
    #[serde(rename = "marble-origami-commit")]
    MarbleOrigamiCommit {
        #[serde(flatten)]
        payload: serde_json::Value,
    },
    /// Staged context-collapse snapshot (TS `'marble-origami-snapshot'`).
    /// Last-wins by `sessionId` on resume.
    #[serde(rename = "marble-origami-snapshot")]
    MarbleOrigamiSnapshot {
        #[serde(flatten)]
        payload: serde_json::Value,
    },
    /// Tool-result budget replacement record. TS writes these so a
    /// resumed session can replay the exact `<persisted-output>`
    /// replacement string for a tool_use_id and preserve prompt-cache
    /// stability.
    #[serde(rename = "content-replacement", rename_all = "camelCase")]
    ContentReplacement {
        #[serde(flatten)]
        record: ContentReplacementRecord,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContentReplacementRecord {
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_use_id: String,
        replacement: String,
    },
}

impl ContentReplacementRecord {
    pub fn tool_result(tool_use_id: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self::ToolResult {
            tool_use_id: tool_use_id.into(),
            replacement: replacement.into(),
        }
    }

    pub fn tool_use_id(&self) -> &str {
        match self {
            Self::ToolResult { tool_use_id, .. } => tool_use_id,
        }
    }

    pub fn replacement(&self) -> &str {
        match self {
            Self::ToolResult { replacement, .. } => replacement,
        }
    }
}

/// Per-model cost breakdown within a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostEntry {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub request_count: i32,
}

/// Union of all entry kinds that can appear in a JSONL transcript.
/// Deserialization tries transcript message first, then metadata.
//
// `TranscriptEntry` is ~344 bytes and dominates the enum size; box it so
// the metadata / unknown variants don't drag every `Vec<Entry>` allocation
// up to that footprint.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Transcript(Box<TranscriptEntry>),
    Metadata(MetadataEntry),
    /// Unparseable line — kept so we never silently drop data.
    Unknown(serde_json::Value),
}

impl Serialize for Entry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Entry::Transcript(t) => t.serialize(serializer),
            Entry::Metadata(m) => m.serialize(serializer),
            Entry::Unknown(v) => v.serialize(serializer),
        }
    }
}

// ---------------------------------------------------------------------------
// Transcript metadata (lightweight summary)
// ---------------------------------------------------------------------------

/// Lightweight metadata extracted from a transcript file without loading
/// every message. Mirrors the TS `LiteMetadata` / `LogOption` fields used
/// by the session picker (`--resume`). Camel-case serde so the shape
/// matches TS `LogOption`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMetadata {
    pub session_id: String,
    pub first_prompt: String,
    pub message_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub is_sidechain: bool,
    pub created_at: String,
    pub modified_at: String,
    /// File size in bytes.
    pub file_size: u64,
}

// ---------------------------------------------------------------------------
// AgentMetadata
// ---------------------------------------------------------------------------

/// Per-agent metadata sidecar. Mirrors TS `AgentMetadata`
/// (`utils/sessionStorage.ts:264-272`) — written when a background
/// AgentTool spawn registers, read when the model invokes
/// `agent/resume` to rehydrate the spawn.
///
/// Persisted as `<sessions_dir>/<session_id>/subagents/agent-<id>.meta.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentMetadata {
    /// Agent type used at original spawn (e.g. `general-purpose`,
    /// `Explore`). Resume reads this to route correctly when
    /// `subagent_type` is omitted from the resume request.
    pub agent_type: String,
    /// Worktree path if the agent was spawned with `isolation:
    /// "worktree"`. Resume restores this as the cwd_override when
    /// the directory still exists; missing directory falls back to
    /// the parent's cwd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// Original task description from the AgentTool input. Resumed
    /// agent's notification surfaces this so the panel doesn't show
    /// a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// TranscriptStore
// ---------------------------------------------------------------------------

/// Manages reading and writing JSONL session transcripts.
///
/// Path layout: `{sessions_dir}/{session_id}.jsonl`
///
/// The TS codebase nests transcripts under a sanitized project path
/// (`~/.coco/projects/{sanitized_cwd}/{id}.jsonl`). We keep the
/// sessions dir configurable so callers can reproduce that layout or
/// use a flat directory.
pub struct TranscriptStore {
    sessions_dir: PathBuf,
}

impl TranscriptStore {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    /// Resolve the JSONL path for a session.
    pub fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.jsonl"))
    }

    /// Resolve the per-agent transcript JSONL path used by
    /// background AgentTool spawns for resume.
    ///
    /// Layout: `<sessions_dir>/<session_id>/subagents/agent-<agent_id>.jsonl`.
    /// Mirrors TS `getAgentTranscriptPath` (`utils/sessionStorage.ts:247-258`)
    /// — different from the per-session transcript path so concurrent
    /// agent and main session writes never compete for the same file.
    pub fn agent_transcript_path(&self, session_id: &str, agent_id: &str) -> PathBuf {
        self.sessions_dir
            .join(session_id)
            .join("subagents")
            .join(format!("agent-{agent_id}.jsonl"))
    }

    /// Resolve the per-agent metadata sidecar path. Mirrors TS
    /// `getAgentMetadataPath` (`utils/sessionStorage.ts:260-262`).
    pub fn agent_metadata_path(&self, session_id: &str, agent_id: &str) -> PathBuf {
        self.sessions_dir
            .join(session_id)
            .join("subagents")
            .join(format!("agent-{agent_id}.meta.json"))
    }

    /// Resolve the per-session tool-result storage directory.
    ///
    /// Layout: `<sessions_dir>/<session_id>/tool-results`, matching
    /// TS's session-scoped artifact layout.
    pub fn tool_results_session_dir(&self, session_id: &str) -> PathBuf {
        self.session_artifact_dir(session_id).join("tool-results")
    }

    /// Remove stale files under every session's `tool-results/` artifact dir.
    ///
    /// Mirrors TS `utils/cleanup.ts::cleanupOldSessionFiles`: direct files under
    /// `tool-results/` and one-level nested tool directories are unlinked when
    /// their file mtime is older than the retention cutoff; then empty tool,
    /// `tool-results`, and session artifact directories are removed best-effort.
    ///
    /// Returns the number of files removed.
    pub fn cleanup_tool_results_older_than(
        &self,
        older_than: std::time::Duration,
    ) -> crate::Result<i32> {
        let cutoff = std::time::SystemTime::now()
            .checked_sub(older_than)
            .ok_or(crate::SessionError::DurationOverflow)?;
        if !self.sessions_dir.exists() {
            return Ok(0);
        }

        let mut removed = 0;
        for entry in std::fs::read_dir(&self.sessions_dir)? {
            let Ok(entry) = entry else { continue };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let session_dir = entry.path();
            let tool_results_dir = session_dir.join("tool-results");
            let tool_entries = match std::fs::read_dir(&tool_results_dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    try_remove_empty_dir(&session_dir);
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            for tool_entry in tool_entries {
                let Ok(tool_entry) = tool_entry else { continue };
                let Ok(tool_file_type) = tool_entry.file_type() else {
                    continue;
                };
                let tool_path = tool_entry.path();
                if tool_file_type.is_file() {
                    if unlink_if_older_than(&tool_path, cutoff)? {
                        removed += 1;
                    }
                } else if tool_file_type.is_dir() {
                    let tool_files = match std::fs::read_dir(&tool_path) {
                        Ok(files) => files,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e.into()),
                    };
                    for tool_file in tool_files {
                        let Ok(tool_file) = tool_file else { continue };
                        let Ok(tool_file_type) = tool_file.file_type() else {
                            continue;
                        };
                        if !tool_file_type.is_file() {
                            continue;
                        }
                        if unlink_if_older_than(&tool_file.path(), cutoff)? {
                            removed += 1;
                        }
                    }
                    try_remove_empty_dir(&tool_path);
                }
            }

            try_remove_empty_dir(&tool_results_dir);
            try_remove_empty_dir(&session_dir);
        }

        Ok(removed)
    }

    /// Resolve the per-session artifact root.
    ///
    /// Tool-result helpers receive this root and append
    /// `tool-results/` themselves.
    pub fn session_artifact_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(session_id)
    }

    /// Append raw `Message` JSON values to a background agent's
    /// per-spawn transcript, one entry per line. Used by
    /// `coco_coordinator::agent_handle_spawn` on bg-spawn
    /// completion to persist the conversation history for resume.
    ///
    /// Each value is the serialised `coco_messages::Message` —
    /// simpler than TS's full `TranscriptEntry` with parent_uuid
    /// chain because coco-rs's `MessageHistory.messages` is in
    /// conversation order; resume just reads back the Vec.
    pub fn append_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
        messages: &[serde_json::Value],
    ) -> crate::Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let path = self.agent_transcript_path(session_id, agent_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        use std::io::Write;
        for msg in messages {
            let line = serde_json::to_string(msg)?;
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    /// Load every line of a background agent's per-spawn transcript
    /// in conversation order. Returns `Ok(None)` when the file
    /// doesn't exist (no prior spawn). Lines that fail to parse
    /// are dropped with a debug log — resume is best-effort, a
    /// corrupted entry shouldn't take the whole spawn down.
    pub fn load_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> crate::Result<Option<Vec<serde_json::Value>>> {
        let path = self.agent_transcript_path(session_id, agent_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut out = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => out.push(v),
                Err(_) => {
                    // Malformed entry — best-effort skip; resume is
                    // tolerant. `i` is intentionally unused to avoid
                    // requiring tracing in this leaf crate.
                    let _ = i;
                }
            }
        }
        Ok(Some(out))
    }

    /// Write an agent's metadata sidecar (`agent-<id>.meta.json`).
    /// Mirrors TS `writeAgentMetadata` (`utils/sessionStorage.ts:283-290`).
    pub fn write_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
        metadata: &AgentMetadata,
    ) -> crate::Result<()> {
        let path = self.agent_metadata_path(session_id, agent_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string(metadata)?;
        std::fs::write(&path, body)?;
        Ok(())
    }

    /// Read an agent's metadata sidecar; returns `Ok(None)` when
    /// the file doesn't exist. Mirrors TS `readAgentMetadata`.
    pub fn read_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> crate::Result<Option<AgentMetadata>> {
        let path = self.agent_metadata_path(session_id, agent_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    /// Append a single entry to the transcript file (creates dirs if needed).
    pub fn append_entry(&self, session_id: &str, entry: &Entry) -> crate::Result<()> {
        let path = self.transcript_path(session_id);
        append_entry_to_file(&path, entry)
    }

    /// Append a transcript message, auto-filling session-level fields.
    pub fn append_message(&self, session_id: &str, entry: &TranscriptEntry) -> crate::Result<()> {
        self.append_entry(session_id, &Entry::Transcript(Box::new(entry.clone())))
    }

    /// Append a metadata entry (custom-title, tag, last-prompt, summary).
    pub fn append_metadata(&self, session_id: &str, entry: &MetadataEntry) -> crate::Result<()> {
        self.append_entry(session_id, &Entry::Metadata(entry.clone()))
    }

    /// Persist a file-history snapshot to the JSONL transcript.
    ///
    /// `snapshot_json` is the `FileHistorySnapshot`'s serialized JSON
    /// shape (see `coco-context::FileHistorySnapshot`). Mirrors TS
    /// `insertFileHistorySnapshot` (`utils/sessionStorage.ts:1085`).
    /// `is_snapshot_update == true` means we're rewriting an existing
    /// snapshot in place (TS `tracked_edit` updates a not-yet-flushed
    /// snapshot); resume's chain builder uses last-wins on (`message_id`,
    /// `is_snapshot_update`) ordering.
    pub fn insert_file_history_snapshot(
        &self,
        session_id: &str,
        message_id: &str,
        snapshot_json: serde_json::Value,
        is_snapshot_update: bool,
    ) -> crate::Result<()> {
        self.append_metadata(
            session_id,
            &MetadataEntry::FileHistorySnapshot {
                message_id: message_id.to_string(),
                snapshot: snapshot_json,
                is_snapshot_update,
            },
        )
    }

    /// Persist a marble-origami commit entry to the transcript.
    ///
    /// `payload` is the serialized [`coco_compact::staged::CommitEntry`]
    /// (camelCase). TS:
    /// `utils/sessionStorage.ts:1541 recordContextCollapseCommit`.
    pub fn append_marble_origami_commit(
        &self,
        session_id: &str,
        payload: serde_json::Value,
    ) -> crate::Result<()> {
        self.append_metadata(session_id, &MetadataEntry::MarbleOrigamiCommit { payload })
    }

    /// Persist a marble-origami snapshot entry to the transcript
    /// (last-wins on resume). TS:
    /// `utils/sessionStorage.ts:1563 recordContextCollapseSnapshot`.
    pub fn append_marble_origami_snapshot(
        &self,
        session_id: &str,
        payload: serde_json::Value,
    ) -> crate::Result<()> {
        self.append_metadata(
            session_id,
            &MetadataEntry::MarbleOrigamiSnapshot { payload },
        )
    }

    pub fn insert_content_replacement(
        &self,
        session_id: &str,
        records: &[ContentReplacementRecord],
    ) -> crate::Result<()> {
        for record in records {
            self.append_metadata(
                session_id,
                &MetadataEntry::ContentReplacement {
                    record: record.clone(),
                },
            )?;
        }
        Ok(())
    }

    pub fn load_content_replacements(
        &self,
        session_id: &str,
    ) -> crate::Result<Vec<ContentReplacementRecord>> {
        let entries = self.load_entries(session_id)?;
        Ok(entries
            .into_iter()
            .filter_map(|e| match e {
                Entry::Metadata(MetadataEntry::ContentReplacement { record }) => Some(record),
                _ => None,
            })
            .collect())
    }

    /// Replay marble-origami entries on resume. Returns
    /// `(commits_in_order, last_snapshot_or_none)` filtered by
    /// `session_id` (snapshot last-wins). Each `serde_json::Value` is
    /// the original camelCase payload — caller deserializes back into
    /// `coco_compact::staged::{CommitEntry, SnapshotEntry}`.
    ///
    /// TS: `utils/sessionStorage.ts:2345-2351 loadAllLogs` filter +
    /// last-wins logic.
    pub fn load_marble_origami_entries(
        &self,
        session_id: &str,
    ) -> crate::Result<(Vec<serde_json::Value>, Option<serde_json::Value>)> {
        let entries = self.load_entries(session_id)?;
        let mut commits = Vec::new();
        let mut last_snapshot: Option<serde_json::Value> = None;
        for e in entries {
            let Entry::Metadata(meta) = e else { continue };
            match meta {
                MetadataEntry::MarbleOrigamiCommit { payload } => {
                    if matches_session(&payload, session_id) {
                        commits.push(payload);
                    }
                }
                MetadataEntry::MarbleOrigamiSnapshot { payload } => {
                    if matches_session(&payload, session_id) {
                        last_snapshot = Some(payload);
                    }
                }
                _ => {}
            }
        }
        Ok((commits, last_snapshot))
    }

    /// Replay file-history snapshots from the transcript JSONL.
    ///
    /// Returns the ordered chain of snapshot JSON values keyed by
    /// `message_id`, with later `is_snapshot_update == true` entries
    /// overwriting earlier ones at the same id (TS `last-wins` rule
    /// from `buildFileHistorySnapshotChain` in
    /// `utils/sessionStorage.ts:2248-2272`).
    ///
    /// Caller deserializes each Value into the typed snapshot
    /// (decoupled so coco-session doesn't depend on coco-context).
    pub fn load_file_history_snapshots(
        &self,
        session_id: &str,
    ) -> crate::Result<Vec<serde_json::Value>> {
        let entries = self.load_entries(session_id)?;
        Ok(build_file_history_snapshot_chain(&entries))
    }

    /// Load all entries from a transcript file.
    ///
    /// Skips blank and malformed lines (logged as `Unknown`). Refuses to
    /// read files larger than [`MAX_TRANSCRIPT_READ_BYTES`] to prevent OOM.
    pub fn load_entries(&self, session_id: &str) -> crate::Result<Vec<Entry>> {
        let path = self.transcript_path(session_id);
        load_entries_from_file(&path)
    }

    /// Load only transcript messages (user/assistant/system/attachment),
    /// filtering out metadata and unknown entries.
    pub fn load_transcript_messages(
        &self,
        session_id: &str,
    ) -> crate::Result<Vec<TranscriptEntry>> {
        let entries = self.load_entries(session_id)?;
        Ok(entries
            .into_iter()
            .filter_map(|e| match e {
                Entry::Transcript(t) => Some(*t),
                _ => None,
            })
            .collect())
    }

    /// Extract lightweight metadata from a transcript without loading all
    /// messages. Reads the first and last few KB of the file (like the TS
    /// `readLiteMetadata`).
    pub fn read_metadata(&self, session_id: &str) -> crate::Result<TranscriptMetadata> {
        let path = self.transcript_path(session_id);
        read_transcript_metadata(&path, session_id)
    }

    /// List all session IDs that have transcript files, newest first.
    pub fn list_sessions(&self) -> crate::Result<Vec<TranscriptMetadata>> {
        list_transcript_sessions(&self.sessions_dir)
    }

    /// List sessions, excluding sidechain transcripts.
    pub fn list_main_sessions(&self) -> crate::Result<Vec<TranscriptMetadata>> {
        let all = self.list_sessions()?;
        Ok(all.into_iter().filter(|m| !m.is_sidechain).collect())
    }

    /// Check whether a transcript file exists for the given session.
    pub fn exists(&self, session_id: &str) -> bool {
        self.transcript_path(session_id).exists()
    }

    /// Delete a transcript file.
    pub fn delete(&self, session_id: &str) -> crate::Result<()> {
        let path = self.transcript_path(session_id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

fn unlink_if_older_than(path: &Path, cutoff: std::time::SystemTime) -> crate::Result<bool> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let Ok(mtime) = metadata.modified() else {
        return Ok(false);
    };
    if mtime >= cutoff {
        return Ok(false);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn try_remove_empty_dir(path: &Path) {
    let _ = std::fs::remove_dir(path);
}

// ---------------------------------------------------------------------------
// File-level helpers
// ---------------------------------------------------------------------------

/// Walk transcript entries and reconstruct the file-history snapshot
/// chain. Implements TS `buildFileHistorySnapshotChain`:
/// `is_snapshot_update == false` appends a new snapshot for that
/// `message_id`; `is_snapshot_update == true` overwrites the prior
/// snapshot for the same id at its position in the vec (last-wins).
pub fn build_file_history_snapshot_chain(entries: &[Entry]) -> Vec<serde_json::Value> {
    use std::collections::HashMap;
    let mut snapshots: Vec<serde_json::Value> = Vec::new();
    let mut index_by_message_id: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        let Entry::Metadata(MetadataEntry::FileHistorySnapshot {
            message_id,
            snapshot,
            is_snapshot_update,
        }) = entry
        else {
            continue;
        };
        if *is_snapshot_update && let Some(&idx) = index_by_message_id.get(message_id) {
            snapshots[idx] = snapshot.clone();
            continue;
        }
        index_by_message_id.insert(message_id.clone(), snapshots.len());
        snapshots.push(snapshot.clone());
    }
    snapshots
}

/// Whether a marble-origami payload's `sessionId` field equals
/// `session_id`. Untyped match — payload is camelCase JSON.
fn matches_session(payload: &serde_json::Value, session_id: &str) -> bool {
    payload
        .get("sessionId")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == session_id)
}

/// Append a single JSON entry as one JSONL line.
fn append_entry_to_file(path: &Path, entry: &Entry) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Load and parse all JSONL entries from a file.
fn load_entries_from_file(path: &Path) -> crate::Result<Vec<Entry>> {
    if !path.exists() {
        return Err(crate::SessionError::TranscriptNotFound {
            path: path.to_path_buf(),
        });
    }

    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_TRANSCRIPT_READ_BYTES {
        return Err(crate::SessionError::generic(format!(
            "transcript file too large ({} bytes, max {MAX_TRANSCRIPT_READ_BYTES}): {}",
            meta.len(),
            path.display(),
        )));
    }

    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        entries.push(parse_entry(&line));
    }

    Ok(entries)
}

/// Parse a single JSONL line into an [`Entry`].
fn parse_entry(line: &str) -> Entry {
    // Try metadata first (tagged enum with "type" discriminator).
    if let Ok(meta) = serde_json::from_str::<MetadataEntry>(line) {
        return Entry::Metadata(meta);
    }
    // Try transcript message.
    if let Ok(transcript) = serde_json::from_str::<TranscriptEntry>(line) {
        return Entry::Transcript(Box::new(transcript));
    }
    // Fallback: preserve the raw JSON value.
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => Entry::Unknown(v),
        Err(_) => Entry::Unknown(serde_json::Value::String(line.to_string())),
    }
}

/// Lite-read window: when a transcript exceeds this size, scan only
/// the first and last `LITE_READ_WINDOW` bytes rather than loading
/// the whole file. Matches TS `LITE_METADATA_WINDOW = 64 KB` —
/// session-picker metadata (`first_prompt`, `custom_title`, `tag`,
/// `last_prompt`, `git_branch`, `cwd`) lives at the top of the
/// transcript, while the re-appended-on-exit values land near the
/// tail; 64 KB at each end is enough to capture both without
/// streaming the multi-megabyte body.
const LITE_READ_WINDOW: u64 = 64 * 1024;

/// Read lightweight metadata from a transcript file without loading all
/// messages. Scans the first and last portion of the file.
fn read_transcript_metadata(path: &Path, session_id: &str) -> crate::Result<TranscriptMetadata> {
    if !path.exists() {
        return Err(crate::SessionError::TranscriptNotFound {
            path: path.to_path_buf(),
        });
    }

    let file_meta = std::fs::metadata(path)?;
    let file_size = file_meta.len();

    let created_at = file_meta
        .created()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();

    let modified_at = file_meta
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();

    // For small files (≤ 2× the lite window) load everything; for
    // larger transcripts scan only the head and tail. The head pass
    // captures `first_prompt`, `cwd`, `git_branch`, and the
    // sidechain/message-count signal; the tail pass picks up the
    // metadata entries (`custom-title`, `tag`, `last-prompt`) that
    // TS re-appends on exit so they survive head-truncation.
    let content = if file_size > LITE_READ_WINDOW * 2 {
        read_head_and_tail(path, LITE_READ_WINDOW)?
    } else {
        std::fs::read_to_string(path)?
    };
    let lines: Vec<&str> = content.lines().collect();

    let mut first_prompt = String::new();
    let mut custom_title: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut last_prompt: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut is_sidechain = false;
    let mut message_count: i32 = 0;

    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let entry = parse_entry(line);
        match &entry {
            Entry::Transcript(t) => {
                if t.entry_type == "user" || t.entry_type == "assistant" {
                    message_count += 1;
                }
                if first_prompt.is_empty() && t.entry_type == "user" {
                    first_prompt = extract_text_content(t);
                }
                if t.is_sidechain {
                    is_sidechain = true;
                }
                if cwd.is_none() && !t.cwd.is_empty() {
                    cwd = Some(t.cwd.clone());
                }
                if t.git_branch.is_some() {
                    git_branch.clone_from(&t.git_branch);
                }
            }
            Entry::Metadata(m) => match m {
                MetadataEntry::CustomTitle {
                    custom_title: ct, ..
                } => {
                    custom_title = Some(ct.clone());
                }
                MetadataEntry::Tag { tag: t, .. } => {
                    tag = Some(t.clone());
                }
                MetadataEntry::LastPrompt {
                    last_prompt: lp, ..
                } => {
                    last_prompt = Some(lp.clone());
                }
                MetadataEntry::Summary { .. }
                | MetadataEntry::CostSummary { .. }
                | MetadataEntry::FileHistorySnapshot { .. }
                | MetadataEntry::MarbleOrigamiCommit { .. }
                | MetadataEntry::MarbleOrigamiSnapshot { .. }
                | MetadataEntry::ContentReplacement { .. } => {}
            },
            Entry::Unknown(_) => {}
        }
    }

    Ok(TranscriptMetadata {
        session_id: session_id.to_string(),
        first_prompt,
        message_count,
        custom_title,
        tag,
        last_prompt,
        git_branch,
        cwd,
        is_sidechain,
        created_at,
        modified_at,
        file_size,
    })
}

/// Read the first `window` bytes and the last `window` bytes of
/// `path`, joining them with a single newline. Drops any partial
/// JSONL lines at the seams (the byte right after the head window
/// and the byte at the start of the tail window may sit mid-record)
/// so the caller's `parse_entry` loop only sees complete lines.
///
/// TS parity: `readSessionLite()` in `utils/listSessionsImpl.ts`.
fn read_head_and_tail(path: &Path, window: u64) -> crate::Result<String> {
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;

    let mut file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();

    let head_len = window.min(total);
    let mut head_buf = vec![0u8; head_len as usize];
    file.read_exact(&mut head_buf)?;
    // Truncate at the last newline so we don't carry a partial
    // record into `parse_entry` (which would surface as `Unknown`).
    if let Some(idx) = find_last_newline(&head_buf) {
        head_buf.truncate(idx);
    }

    let tail_len = window.min(total.saturating_sub(head_len));
    let mut tail_buf = vec![0u8; tail_len as usize];
    if tail_len > 0 {
        file.seek(SeekFrom::End(-(tail_len as i64)))?;
        file.read_exact(&mut tail_buf)?;
        // Skip leading partial line (everything up to the first '\n').
        if let Some(idx) = tail_buf.iter().position(|b| *b == b'\n') {
            tail_buf.drain(..=idx);
        } else {
            // No newline in the tail window — every byte belongs to a
            // single oversized line; drop it.
            tail_buf.clear();
        }
    }

    let mut combined = Vec::with_capacity(head_buf.len() + 1 + tail_buf.len());
    combined.extend_from_slice(&head_buf);
    combined.push(b'\n');
    combined.extend_from_slice(&tail_buf);
    String::from_utf8(combined)
        .map_err(|e| crate::SessionError::generic(format!("transcript not utf-8: {e}")))
}

/// Index of the rightmost newline byte in `buf`, or `None` when no
/// newline is present. Used by [`read_head_and_tail`] to drop partial
/// records at the head/tail seams.
fn find_last_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().rposition(|b| *b == b'\n')
}

/// List all transcript sessions from a directory, newest first.
fn list_transcript_sessions(sessions_dir: &Path) -> crate::Result<Vec<TranscriptMetadata>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for entry in std::fs::read_dir(sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if session_id.is_empty() {
                continue;
            }
            match read_transcript_metadata(&path, &session_id) {
                Ok(meta) => results.push(meta),
                Err(_) => {
                    // Skip corrupt / unreadable files.
                    continue;
                }
            }
        }
    }

    // Newest first by modified_at (descending). Parse numerically so
    // mixed-width millisecond timestamps still compare correctly.
    results.sort_by(|a, b| {
        let a_ms = a.modified_at.parse::<u128>().unwrap_or(0);
        let b_ms = b.modified_at.parse::<u128>().unwrap_or(0);
        b_ms.cmp(&a_ms)
    });
    Ok(results)
}

/// Extract a short text snippet from a transcript entry's message content.
fn extract_text_content(entry: &TranscriptEntry) -> String {
    let Some(message) = &entry.message else {
        return String::new();
    };

    // Message has a "content" field that is either a string or an array.
    let Some(content) = message.get("content") else {
        return String::new();
    };

    if let Some(text) = content.as_str() {
        return truncate_prompt(text);
    }

    // Array content: find the first text block.
    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(text) = block.get("text").and_then(|t| t.as_str())
            {
                return truncate_prompt(text);
            }
        }
    }

    String::new()
}

/// Truncate a prompt string for display (matching TS 200-char limit).
fn truncate_prompt(text: &str) -> String {
    let flat = text.replace('\n', " ");
    let trimmed = flat.trim();
    if trimmed.len() > 200 {
        format!("{}...", &trimmed[..200].trim())
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Cost restoration
// ---------------------------------------------------------------------------

/// Summary of costs restored from transcript entries.
#[derive(Debug, Clone, Default)]
pub struct RestoredCostSummary {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost_usd: f64,
    pub model_usage: std::collections::HashMap<String, ModelCostEntry>,
}

/// Reconstruct total costs from transcript entries on session resume.
///
/// Scans all entries for usage data and aggregates per-model costs.
/// If a CostSummary metadata entry exists, uses that directly.
pub fn restore_cost_from_transcript(entries: &[Entry]) -> RestoredCostSummary {
    // Check for explicit CostSummary first (most accurate).
    for entry in entries.iter().rev() {
        if let Entry::Metadata(MetadataEntry::CostSummary {
            total_input_tokens,
            total_output_tokens,
            total_cost_usd,
            model_usage,
            ..
        }) = entry
        {
            return RestoredCostSummary {
                total_input_tokens: *total_input_tokens,
                total_output_tokens: *total_output_tokens,
                total_cost_usd: *total_cost_usd,
                model_usage: model_usage.clone(),
            };
        }
    }

    // Fallback: aggregate from individual transcript entries.
    let mut summary = RestoredCostSummary::default();
    for entry in entries {
        if let Entry::Transcript(t) = entry {
            if let Some(ref usage) = t.usage {
                summary.total_input_tokens += usage.input_tokens;
                summary.total_output_tokens += usage.output_tokens;
            }
            if let Some(cost) = t.cost_usd {
                summary.total_cost_usd += cost;
            }
            if let (Some(model), Some(usage)) = (&t.model, &t.usage) {
                let entry = summary
                    .model_usage
                    .entry(model.clone())
                    .or_insert(ModelCostEntry {
                        input_tokens: 0,
                        output_tokens: 0,
                        cost_usd: 0.0,
                        request_count: 0,
                    });
                entry.input_tokens += usage.input_tokens;
                entry.output_tokens += usage.output_tokens;
                entry.cost_usd += t.cost_usd.unwrap_or(0.0);
                entry.request_count += 1;
            }
        }
    }
    summary
}

#[cfg(test)]
#[path = "storage.test.rs"]
mod tests;
