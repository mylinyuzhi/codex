//! History file read state reconstruction.
//!
//! This module provides utilities for rebuilding file tracking state from
//! message history, enabling proper recovery after compaction, rewind, and
//! session resumption operations.
//!
//! # Claude Code Alignment
//!
//! This matches Claude Code v2.1.38's behavior:
//! - `buildFileReadStateFromTurns` - Extract state from ContextModifier::FileRead
//! - Collision handling: prefer newer by read_turn, then stronger FileReadKind
//! - State reconstruction is the source of truth for tracker rebuilds
//!
//! # Types
//!
//! - `ReadFileState`: Alias for `cocode_tools::FileReadState` - file read state snapshot
//! - `ReadStateKind`: Alias for `cocode_protocol::FileReadKind` - read operation type
//! - `FileReadStateEntry`: Entry tuple (path, state) for collections

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use cocode_protocol::ContextModifier;
use cocode_protocol::FileReadKind;
use cocode_tools::FileReadState;

// Use shared policy functions from this crate
use crate::file_read_tracking_policy::is_read_state_source_tool;
use crate::file_read_tracking_policy::is_stronger_kind;

// ============================================================================
// Type Aliases (API compatibility with reference branch)
// ============================================================================

/// Alias for file read state, matching reference branch API.
///
/// This is the same as `cocode_tools::FileReadState` but re-exported
/// here for convenience and API consistency with the reference branch.
pub type ReadFileState = FileReadState;

/// Alias for read state kind, matching reference branch API.
///
/// This is the same as `cocode_protocol::FileReadKind` but re-exported
/// here for convenience and API consistency with the reference branch.
pub type ReadStateKind = FileReadKind;

/// Default maximum number of file entries to restore during rebuild.
pub const BUILD_STATE_DEFAULT_MAX_ENTRIES: usize = 10;

/// Rebuilt file read state entry.
///
/// Contains the path and the reconstructed read state.
pub type FileReadStateEntry = (PathBuf, FileReadState);

/// Build file read state from tool call modifiers.
///
/// This function extracts file read information from `ContextModifier::FileRead`
/// entries in tool calls. It handles collisions by:
/// 1. Preferring newer entries (higher `read_turn`)
/// 2. For same-turn collisions, preferring stronger `FileReadKind`
///    (FullContent > PartialContent > MetadataOnly)
///
/// # Arguments
///
/// * `tool_calls` - Iterator of (name, modifiers, turn_number, is_completed) tuples
/// * `max_entries` - Maximum number of entries to return (LRU-style eviction)
///
/// # Returns
///
/// A vector of (path, state) pairs, ordered by most recent read.
pub fn build_file_read_state_from_modifiers<'a>(
    tool_calls: impl Iterator<Item = (&'a str, &'a [ContextModifier], i32, bool)>,
    max_entries: usize,
) -> Vec<FileReadStateEntry> {
    // Collect all file reads with their turn numbers
    let mut reads_by_path: HashMap<PathBuf, (i32, FileReadState)> = HashMap::new();

    for (tool_name, modifiers, turn_number, is_completed) in tool_calls {
        // Only process tools that contribute to file-read state
        if !is_read_state_source_tool(tool_name) {
            continue;
        }

        // Only process completed tool calls
        if !is_completed {
            continue;
        }

        // Extract file info from ContextModifier::FileRead
        for modifier in modifiers {
            if let ContextModifier::FileRead {
                path,
                content,
                file_mtime_ms,
                offset,
                limit,
                read_kind,
            } = modifier
            {
                let entry = build_read_state_from_modifier(
                    content.clone(),
                    file_mtime_ms.map(ms_to_system_time),
                    turn_number,
                    *offset,
                    *limit,
                    *read_kind,
                );

                if let Some(state) = entry {
                    // Handle collision: prefer newer or stronger kind
                    let should_update = match reads_by_path.get(path) {
                        None => true,
                        Some((existing_turn, existing_state)) => {
                            if turn_number > *existing_turn {
                                true
                            } else if turn_number == *existing_turn {
                                // Same turn: prefer stronger read kind
                                is_stronger_kind(&state.kind, &existing_state.kind)
                            } else {
                                false
                            }
                        }
                    };

                    if should_update {
                        reads_by_path.insert(path.clone(), (turn_number, state));
                    }
                }
            }
        }
    }

    // Sort by turn number (most recent first) and limit to max_entries
    let mut entries: Vec<_> = reads_by_path.into_iter().collect();
    entries.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    entries.truncate(max_entries);

    // Extract just the (path, state) pairs
    entries
        .into_iter()
        .map(|(path, (_, state))| (path, state))
        .collect()
}

/// Build file read state from turns directly.
///
/// This is a convenience wrapper around `build_file_read_state_from_modifiers`
/// that accepts turn data directly. Used for rebuilding tracker state after
/// compaction, rewind, or session resumption.
///
/// # Arguments
///
/// * `turns` - Iterator of (turn_number, tool_calls) tuples where tool_calls
///   is an iterator of (tool_name, modifiers, is_completed) tuples
/// * `max_entries` - Maximum number of entries to return (LRU-style eviction)
///
/// # Returns
///
/// A vector of (path, state) pairs, ordered by most recent read.
///
/// # Example
///
/// ```ignore
/// use cocode_system_reminder::build_file_read_state_from_turns;
///
/// let turns = message_history.turns();
/// let state = build_file_read_state_from_turns(
///     turns.iter().map(|t| (t.number, t.tool_calls.iter())),
///     100
/// );
/// ```
pub fn build_file_read_state_from_turns<'a>(
    turns: impl Iterator<
        Item = (
            i32,
            impl Iterator<Item = (&'a str, &'a [ContextModifier], bool)>,
        ),
    >,
    max_entries: usize,
) -> Vec<FileReadStateEntry> {
    // Flatten turns into tool_calls iterator format for the modifiers function
    let tool_calls = turns.flat_map(|(turn_number, tool_calls)| {
        tool_calls.map(move |(name, modifiers, is_completed)| {
            (name, modifiers, turn_number, is_completed)
        })
    });

    build_file_read_state_from_modifiers(tool_calls, max_entries)
}

/// Merge file read state from two sources.
///
/// When loading persisted state and rebuilding from history, this function
/// merges them intelligently, preferring newer entries.
///
/// # Arguments
///
/// * `base` - Base state (typically from persistence)
/// * `incoming` - Incoming state (typically from history rebuild)
///
/// # Returns
///
/// Merged state with collisions resolved by preferring newer entries.
pub fn merge_file_read_state(
    base: Vec<FileReadStateEntry>,
    incoming: Vec<FileReadStateEntry>,
) -> Vec<FileReadStateEntry> {
    let mut merged: HashMap<PathBuf, FileReadState> = HashMap::new();

    // Add base entries first
    for (path, state) in base {
        merged.insert(path, state);
    }

    // Merge incoming entries (they may be newer)
    for (path, state) in incoming {
        match merged.get(&path) {
            Some(existing) => {
                // Prefer newer (higher read_turn)
                if state.read_turn > existing.read_turn {
                    merged.insert(path, state);
                }
            }
            None => {
                merged.insert(path, state);
            }
        }
    }

    // Sort by read_turn (most recent first)
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by(|a, b| b.1.read_turn.cmp(&a.1.read_turn));
    entries
}

/// Build a FileReadState from modifier data.
///
/// This constructs a complete FileReadState from the components stored
/// in a ContextModifier::FileRead.
///
/// # Arguments
///
/// * `content` - File content (may be empty for compacted files)
/// * `file_mtime` - File modification time at read time
/// * `read_turn` - Turn number when the file was read
/// * `offset` - Line offset (None if from start)
/// * `limit` - Line limit (None if no limit)
/// * `read_kind` - Kind of read operation
///
/// # Returns
///
/// A FileReadState if the read is valid, None otherwise.
pub fn build_read_state_from_modifier(
    content: String,
    file_mtime: Option<SystemTime>,
    read_turn: i32,
    offset: Option<i64>,
    limit: Option<i64>,
    read_kind: FileReadKind,
) -> Option<FileReadState> {
    // Skip metadata-only reads (no content to track)
    if read_kind == FileReadKind::MetadataOnly {
        return None;
    }

    let content_hash = if !content.is_empty() {
        Some(FileReadState::compute_hash(&content))
    } else {
        None
    };

    Some(FileReadState {
        content: if content.is_empty() {
            None
        } else {
            Some(content)
        },
        timestamp: SystemTime::now(),
        file_mtime,
        content_hash,
        offset,
        limit,
        kind: read_kind,
        access_count: 1,
        read_turn,
    })
}

/// Convert milliseconds since Unix epoch to SystemTime.
fn ms_to_system_time(ms: i64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64)
}

use crate::types::FileReadInfo;

/// Extract file reads from a list of FileReadInfo (from system reminders).
///
/// This is used when processing the `file_reads` field from AtMentionedFiles
/// reminders to update the FileTracker.
pub fn file_read_infos_to_states(infos: &[FileReadInfo]) -> Vec<FileReadStateEntry> {
    infos
        .iter()
        .filter_map(|info| {
            let state = build_read_state_from_modifier(
                info.content.clone(),
                info.mtime,
                info.turn_number,
                info.offset,
                info.limit,
                info.read_kind,
            );
            state.map(|s| (info.path.clone(), s))
        })
        .collect()
}

#[cfg(test)]
#[path = "history_file_read_state.test.rs"]
mod tests;
