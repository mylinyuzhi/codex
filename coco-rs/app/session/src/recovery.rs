//! Conversation recovery for session resume/fork.
//!
//! TS: utils/conversationRecovery.ts — reload conversation from
//! transcript JSONL, build the message chain by walking parent_uuid
//! from the newest non-sidechain leaf back to the root, then
//! reconstruct typed `coco_messages::Message` values preserving
//! `tool_use` / `tool_result` content blocks so the resumed model
//! sees the same DAG it left.

use crate::storage::ContentReplacementRecord;
use crate::storage::Entry;
use crate::storage::MetadataEntry;
use crate::storage::TranscriptEntry;
use crate::storage::build_file_history_snapshot_chain;
use crate::storage::content_replacements_for_chain;
use crate::storage::messages_from_transcript_entry;
use coco_messages::Message;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::BufRead;
use std::path::Path;

/// Conversation loaded from transcript for session resume.
#[derive(Debug)]
pub struct ConversationForResume {
    pub messages: Vec<Message>,
    pub model: String,
    pub turn_count: i32,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    /// Plan slug extracted from transcript (for plan resume).
    pub plan_slug: Option<String>,
    /// Whether the session had sidechain entries.
    pub has_sidechain: bool,
    /// Stored execution mode (`coordinator` / `normal`) from the last
    /// `Mode` metadata entry — drives coordinator-mode reconcile on resume.
    pub mode: Option<String>,
}

/// Full session state for resume, anchored to one selected conversation chain.
#[derive(Debug)]
pub struct SessionResumeState {
    pub messages: Vec<Message>,
    pub selected_chain_uuids: HashSet<String>,
    pub content_replacements: Vec<ContentReplacementRecord>,
    pub agent_content_replacements: HashMap<String, Vec<ContentReplacementRecord>>,
    pub file_history_snapshots: Vec<serde_json::Value>,
    pub model: String,
    pub turn_count: i32,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub plan_slug: Option<String>,
    pub has_sidechain: bool,
    /// Stored execution mode (`coordinator` / `normal`) from the last
    /// `Mode` metadata entry — drives coordinator-mode reconcile on resume.
    pub mode: Option<String>,
}

/// Load a conversation from a session transcript for resume.
///
/// Reads the JSONL transcript, walks the `parent_uuid` chain backward
/// from the newest non-sidechain leaf, then reconstructs the message
/// list in chronological order. Falls back to top-to-bottom read order
/// when no parent_uuid links are present (transcripts written by
/// older builds, fixture data).
///
/// TS parity: `loadConversationForResume` →
/// `loadTranscriptFile` → `buildConversationChain`
/// (`utils/conversationRecovery.ts`).
pub fn load_conversation_for_resume(
    transcript_path: &Path,
) -> crate::Result<ConversationForResume> {
    let resume_state = load_session_state_for_resume(transcript_path)?;
    Ok(ConversationForResume {
        messages: resume_state.messages,
        model: resume_state.model,
        turn_count: resume_state.turn_count,
        total_input_tokens: resume_state.total_input_tokens,
        total_output_tokens: resume_state.total_output_tokens,
        plan_slug: resume_state.plan_slug,
        has_sidechain: resume_state.has_sidechain,
        mode: resume_state.mode,
    })
}

pub fn load_session_state_for_resume(transcript_path: &Path) -> crate::Result<SessionResumeState> {
    if !transcript_path.exists() {
        return Err(crate::SessionError::TranscriptNotFound {
            path: transcript_path.to_path_buf(),
        });
    }

    let mut entries: Vec<TranscriptEntry> = Vec::new();
    let mut metadata_entries: Vec<Entry> = Vec::new();
    let mut plan_slug: Option<String> = None;
    let mut has_sidechain = false;

    let file = std::fs::File::open(transcript_path)?;
    let reader = std::io::BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let entry_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !is_transcript_message_type(entry_type) {
            if let Ok(meta) = serde_json::from_value::<MetadataEntry>(value) {
                metadata_entries.push(Entry::Metadata(meta));
            }
            continue;
        }
        if value
            .get("is_sidechain")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            has_sidechain = true;
            continue;
        }
        if plan_slug.is_none()
            && let Some(slug) = value.get("slug").and_then(|v| v.as_str())
            && !slug.is_empty()
        {
            plan_slug = Some(slug.to_string());
        }
        let Ok(te) = serde_json::from_value::<TranscriptEntry>(value) else {
            continue;
        };
        entries.push(te);
    }
    // TS-parity: `sessionStorage.ts:3654-3657` — a compact_boundary entry
    // keeps every pre-boundary message in the `messages` map and only
    // resets marble-origami state; the boundary message itself stamps
    // `parentUuid: null` / `logicalParentUuid: prev` so the chain walk
    // naturally terminates without us dropping data. Truncating the
    // entries Vec here would break `applyPreservedSegmentRelinks`
    // (`sessionStorage.ts:1855-1865`) which still resolves UUIDs from
    // the pre-boundary range. microcompact_boundary is NOT a chain
    // break in TS (see `messages.ts:4608-4612`); it's a plain system
    // message that stays inline.

    // Pass 2: build a uuid → entry index and the set of parent uuids
    // so we can identify leaves (uuids that no other entry points at).
    // The walk picks the latest non-sidechain leaf by timestamp; on
    // tie or empty index we fall back to disk order.
    let mut by_uuid: HashMap<String, usize> = HashMap::new();
    let mut parent_uuids: HashSet<String> = HashSet::new();
    for (idx, e) in entries.iter().enumerate() {
        if !e.uuid.is_empty() {
            by_uuid.insert(e.uuid.clone(), idx);
        }
        if let Some(p) = &e.parent_uuid
            && !p.is_empty()
        {
            parent_uuids.insert(p.clone());
        }
    }

    // Find leaves the way TS does (`sessionStorage.ts:3768-3784`):
    // 1) terminal = entries whose uuid is no one's parent;
    // 2) for each terminal, walk back via parent_uuid to its nearest
    //    user/assistant ancestor — attachments / system / progress are
    //    skipped because they can't anchor a turn;
    // 3) among the resulting set of valid leaf uuids, pick the latest
    //    by timestamp using strict `>` (TS `findLatestMessage`,
    //    `sessionStorage.ts:2046-2060`, is first-wins on tie).
    //
    // No-parent-links fixture: fall back to disk order so every
    // persisted message round-trips.
    let any_parent_link = !parent_uuids.is_empty();
    let chain_indices: Vec<usize> = if any_parent_link {
        let mut leaf_idxs: Vec<usize> = Vec::new();
        let mut leaf_seen: HashSet<usize> = HashSet::new();
        for (idx, terminal) in entries.iter().enumerate() {
            if terminal.uuid.is_empty() || parent_uuids.contains(&terminal.uuid) {
                continue;
            }
            // Walk back to nearest user/assistant ancestor.
            let mut visited: HashSet<String> = HashSet::new();
            let mut cursor = Some(idx);
            while let Some(i) = cursor {
                let entry = &entries[i];
                if !entry.uuid.is_empty() && !visited.insert(entry.uuid.clone()) {
                    break;
                }
                if entry.entry_type == "user" || entry.entry_type == "assistant" {
                    if leaf_seen.insert(i) {
                        leaf_idxs.push(i);
                    }
                    break;
                }
                cursor = entry
                    .parent_uuid
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .and_then(|p| by_uuid.get(p).copied());
            }
        }
        // First-wins tie-break: keep the first index whose timestamp
        // strictly exceeds the running max. Mirrors TS `t > maxTime`.
        let leaf_idx = leaf_idxs
            .into_iter()
            .fold(None::<usize>, |best, idx| match best {
                Some(b) if entries[idx].timestamp > entries[b].timestamp => Some(idx),
                Some(b) => Some(b),
                None => Some(idx),
            });
        match leaf_idx {
            Some(idx) => {
                let mut walked: Vec<usize> = Vec::new();
                let mut visited: HashSet<String> = HashSet::new();
                let mut cursor = Some(idx);
                while let Some(i) = cursor {
                    let entry = &entries[i];
                    if !entry.uuid.is_empty() && !visited.insert(entry.uuid.clone()) {
                        break;
                    }
                    walked.push(i);
                    cursor = entry
                        .parent_uuid
                        .as_deref()
                        .filter(|p| !p.is_empty())
                        .and_then(|p| by_uuid.get(p).copied());
                }
                walked.reverse();
                walked
            }
            None => (0..entries.len()).collect(),
        }
    } else {
        (0..entries.len()).collect()
    };

    // Pass 3: reconstruct typed messages, aggregating model + token +
    // turn counters along the way. `latest_model` mirrors TS's "newest
    // assistant model wins" rule used by the resume picker.
    let mut messages: Vec<Message> = Vec::with_capacity(chain_indices.len());
    let mut selected_chain_uuids: HashSet<String> = HashSet::new();
    let mut latest_model = String::new();
    let mut total_input: i64 = 0;
    let mut total_output: i64 = 0;
    let mut turn_count: i32 = 0;

    for idx in chain_indices {
        let te = &entries[idx];
        if !te.uuid.is_empty() {
            selected_chain_uuids.insert(te.uuid.clone());
        }
        if let Some(m) = &te.model
            && !m.is_empty()
        {
            latest_model.clone_from(m);
        }
        if let Some(usage) = &te.usage {
            total_input += usage.input_tokens;
            total_output += usage.output_tokens;
        }
        let entry_messages = messages_from_transcript_entry(te);
        if !entry_messages.is_empty() {
            if te.entry_type == "assistant" {
                turn_count += 1;
            }
            messages.extend(entry_messages);
        }
    }

    for msg in &messages {
        if let Some(uuid) = msg.uuid() {
            selected_chain_uuids.insert(uuid.to_string());
        }
    }

    let session_id = entries
        .iter()
        .find_map(|entry| (!entry.session_id.is_empty()).then(|| entry.session_id.clone()))
        .unwrap_or_else(|| {
            transcript_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_string()
        });
    let content_replacements = content_replacements_for_chain(&metadata_entries, &session_id, None);
    // TS-parity: `sessionStorage.ts:3682-3693` routes content-replacement
    // records by `agentId` presence — no per-message-uuid scope. Records
    // are keyed by `tool_use_id` (TS `toolResultStorage.ts:475-479`),
    // which is globally unique within a session.
    let mut agent_content_replacements: HashMap<String, Vec<ContentReplacementRecord>> =
        HashMap::new();
    for entry in &metadata_entries {
        let Entry::Metadata(MetadataEntry::ContentReplacement {
            session_id: entry_session_id,
            agent_id: Some(agent_id),
            replacements,
        }) = entry
        else {
            continue;
        };
        if entry_session_id != &session_id {
            continue;
        }
        if !replacements.is_empty() {
            agent_content_replacements
                .entry(agent_id.clone())
                .or_default()
                .extend(replacements.iter().cloned());
        }
    }
    // Build the conversation-ordered chain of message UUIDs for the
    // file-history replay. TS `buildFileHistorySnapshotChain`
    // (`utils/sessionStorage.ts:2248-2272`) walks the resolved
    // conversation chain; without that order, isSnapshotUpdate
    // overwrites can hit the wrong index.
    let chain_message_uuids: Vec<String> = messages
        .iter()
        .filter_map(|m| m.uuid().map(std::string::ToString::to_string))
        .collect();
    let file_history_snapshots =
        build_file_history_snapshot_chain(&metadata_entries, &chain_message_uuids);

    // Last-wins `Mode` metadata entry — the session's stored
    // coordinator/normal mode, replayed so resume can reconcile it.
    let latest_mode = metadata_entries.iter().rev().find_map(|entry| match entry {
        Entry::Metadata(MetadataEntry::Mode { mode, .. }) => Some(mode.clone()),
        _ => None,
    });

    Ok(SessionResumeState {
        messages,
        selected_chain_uuids,
        content_replacements,
        agent_content_replacements,
        file_history_snapshots,
        model: latest_model,
        turn_count,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        plan_slug,
        has_sidechain,
        mode: latest_mode,
    })
}

fn is_transcript_message_type(entry_type: &str) -> bool {
    matches!(entry_type, "user" | "assistant" | "system" | "attachment")
}

/// Check if a session can be resumed (transcript exists and is valid).
pub fn can_resume_session(transcript_path: &Path) -> bool {
    if !transcript_path.exists() {
        return false;
    }
    std::fs::read_to_string(transcript_path)
        .map(|content| {
            content.lines().any(|line| {
                !line.trim().is_empty() && serde_json::from_str::<serde_json::Value>(line).is_ok()
            })
        })
        .unwrap_or(false)
}

/// Fork a conversation — create a copy of the transcript for a new session.
pub fn fork_conversation(source_path: &Path, dest_path: &Path) -> crate::Result<()> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source_path, dest_path)?;
    Ok(())
}

#[cfg(test)]
#[path = "recovery.test.rs"]
mod tests;
