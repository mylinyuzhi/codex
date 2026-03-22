//! File read tracking policy for consistent read-state management.
//!
//! This module centralizes read-state source classification and tracking logic
//! used by both tool execution and history reconstruction. This ensures consistent
//! behavior across code paths when determining which tools contribute to file-read
//! tracking state.
//!
//! # Claude Code Alignment
//!
//! This matches Claude Code v2.1.38's read tracking behavior:
//! - `isReadStateSourceTool` - Tools that emit FileRead modifiers
//! - `shouldSkipTrackedFile` - Whether to skip a file in restoration
//! - `isFullContentReadTool` - Whether tool reads full content
//! - `isCacheableRead` - Whether read can be cached for already-read detection
//! - `categorizeReadKind` - Categorize read based on tool and parameters
//!
//! # Architecture
//!
//! Core functions (`is_read_state_source_tool`, `normalize_path`,
//! `collect_cleared_read_paths_from_input`) are re-exported from `cocode-message`
//! where they're used during micro-compaction.
//!
//! This module adds additional functions used by system-reminder generators
//! and history reconstruction.

use std::path::Path;

use cocode_protocol::ToolName;

// Re-export core functions from cocode-message crate
pub use cocode_message::collect_cleared_read_paths;
pub use cocode_message::collect_cleared_read_paths_from_input;
pub use cocode_message::is_read_state_source_tool;
pub use cocode_message::is_stronger_kind;
pub use cocode_message::normalize_path;

use cocode_protocol::FileReadKind;

/// Check if a tool performs a full content read (vs metadata-only).
///
/// Full content reads are cacheable for already-read detection,
/// while metadata-only reads (Glob, Grep) are not.
///
/// # Returns
///
/// - `true` for tools that read full file content
/// - `false` for tools that only access metadata or partial content
pub fn is_full_content_read_tool(tool_name: &str) -> bool {
    tool_name == ToolName::Read.as_str() || tool_name == ToolName::ReadManyFiles.as_str()
}

/// Check if a file should be skipped for read tracking.
///
/// Some files are internal to the system and should not be tracked:
/// - Session memory files
/// - Plan files
/// - Auto memory files
/// - Tool result persistence files
///
/// # Arguments
///
/// * `path` - The file path to check
/// * `plan_file_path` - Optional plan file path
/// * `session_memory_path` - Optional session memory path
/// * `extra_internal_paths` - Additional internal paths to skip
///
/// # Returns
///
/// `true` if the file should be skipped in read tracking.
pub fn should_skip_tracked_file(
    path: &Path,
    plan_file_path: Option<&Path>,
    session_memory_path: Option<&Path>,
    extra_internal_paths: &[std::path::PathBuf],
) -> bool {
    let path_str = path.to_string_lossy();

    // Skip session memory files
    if let Some(session_mem) = session_memory_path
        && path == session_mem
    {
        return true;
    }
    if path_str.contains("session-memory") && path_str.contains("summary.md") {
        return true;
    }

    // Skip plan files
    if let Some(plan_path) = plan_file_path
        && path == plan_path
    {
        return true;
    }
    if path_str.contains(".cocode/plans/") {
        return true;
    }

    // Skip auto memory files
    if let Some(filename) = path.file_name().and_then(|n| n.to_str())
        && (filename == "MEMORY.md" || filename.starts_with("memory-"))
    {
        return true;
    }

    // Skip tool result persistence files
    if path_str.contains("tool-results/") {
        return true;
    }

    // Skip extra internal paths
    if extra_internal_paths.contains(&path.to_path_buf()) {
        return true;
    }

    false
}

/// Check if a read should be considered cacheable for already-read detection.
///
/// A read is cacheable if:
/// 1. It's a full content read (not partial, not metadata-only)
/// 2. The file hasn't been modified since it was read
///
/// # Arguments
///
/// * `tool_name` - The name of the tool that performed the read
/// * `has_offset` - Whether the read had an offset (partial read)
/// * `has_limit` - Whether the read had a limit (partial read)
///
/// # Returns
///
/// `true` if the read should be considered for already-read caching.
pub fn is_cacheable_read(tool_name: &str, has_offset: bool, has_limit: bool) -> bool {
    // Only full content reads are cacheable
    if !is_full_content_read_tool(tool_name) {
        return false;
    }

    // Partial reads are not cacheable
    if has_offset || has_limit {
        return false;
    }

    true
}

/// Categorize the read kind based on tool and parameters.
///
/// # Read Kinds
///
/// - `FullContent` - Complete file read (cacheable)
/// - `PartialContent` - Partial read with offset/limit (not cacheable)
/// - `MetadataOnly` - No content read (Glob, Grep)
///
/// # Arguments
///
/// * `tool_name` - The name of the tool that performed the read
/// * `has_offset` - Whether the read had an offset
/// * `has_limit` - Whether the read had a limit
///
/// # Returns
///
/// The appropriate `FileReadKind` for this read operation.
pub fn categorize_read_kind(tool_name: &str, has_offset: bool, has_limit: bool) -> FileReadKind {
    // Glob and Grep don't read actual file content
    if tool_name == ToolName::Glob.as_str() || tool_name == ToolName::Grep.as_str() {
        return FileReadKind::MetadataOnly;
    }

    // Check for partial reads
    if has_offset || has_limit {
        return FileReadKind::PartialContent;
    }

    // Full content read
    FileReadKind::FullContent
}

/// Decision for how to handle an @mentioned file.
///
/// This enum represents the three possible paths for @mention handling,
/// matching Claude Code v2.1.38's behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionReadDecision {
    /// File was already read and unchanged - skip reading, emit silent attachment.
    AlreadyReadUnchanged,
    /// File has line range specified - force re-read with range.
    NeedsReadLineRange,
    /// File needs to be read normally.
    NeedsRead,
}

/// Resolve the read decision for an @mentioned file.
///
/// This function determines how to handle an @mentioned file based on:
/// 1. Whether it has a line range (always re-read)
/// 2. Whether it's already in the tracker and unchanged
///
/// # Arguments
///
/// * `tracker` - Optional file tracker to check cache
/// * `path` - Path to the file
/// * `has_line_range` - Whether the mention has a line range
///
/// # Returns
///
/// The appropriate `MentionReadDecision` for this file.
pub fn resolve_mention_read_decision(
    tracker: Option<&cocode_tools::FileTracker>,
    path: &Path,
    has_line_range: bool,
) -> MentionReadDecision {
    // Line range mentions must always be re-read
    if has_line_range {
        return MentionReadDecision::NeedsReadLineRange;
    }

    // Check if file is in tracker and unchanged
    if let Some(tracker) = tracker
        && tracker.is_already_read_unchanged(path)
    {
        return MentionReadDecision::AlreadyReadUnchanged;
    }

    MentionReadDecision::NeedsRead
}

#[cfg(test)]
#[path = "file_read_tracking_policy.test.rs"]
mod tests;
