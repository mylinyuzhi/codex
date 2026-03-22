//! File read tracking policy for micro-compact path clearing.
//!
//! This module provides core functions for:
//! - Identifying tools that contribute to file-read tracking state
//! - Collecting file paths to clear during micro-compaction
//! - Path normalization for consistent tracking
//!
//! # Claude Code Alignment
//!
//! This matches Claude Code v2.1.38's core read tracking behavior:
//! - `isReadStateSourceTool` - Tools that emit FileRead modifiers
//! - `collectClearedReadPaths` - Paths to clear during micro-compact
//!
//! # Architecture Note
//!
//! This crate (`cocode-message`) keeps only the core functions needed for
//! micro-compaction and message processing. The more complex state
//! reconstruction logic (`build_file_read_state_from_modifiers`,
//! `merge_file_read_state`, etc.) lives in `cocode-system-reminder`'s
//! `history_file_read_state` module, which is the appropriate place for
//! history-dependent operations.

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use cocode_protocol::ToolName;

/// Tools that contribute to file-read tracking state.
const READ_STATE_SOURCE_TOOLS: &[&str] = &[
    ToolName::Read.as_str(),
    ToolName::ReadManyFiles.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Grep.as_str(),
];

/// Normalize a path for consistent tracking across all file operations.
///
/// Ensures paths are consistent across different representations by:
/// 1. Making relative paths absolute (using current working directory)
/// 2. Resolving `.` and `..` components without requiring the file to exist
///
/// This is critical for file read tracking, where the same file may be
/// referenced via different relative paths (e.g., `./src/lib.rs` vs `src/lib.rs`).
///
/// # Example
///
/// ```
/// use cocode_message::normalize_path;
/// use std::path::PathBuf;
///
/// // Resolves .. without needing the file to exist
/// let normalized = normalize_path("/project/src/../lib/file.rs");
/// assert_eq!(normalized, PathBuf::from("/project/lib/file.rs"));
/// ```
///
/// # Claude Code Alignment
///
/// This matches Claude Code v2.1.38's path normalization behavior for
/// consistent file tracking across relative and absolute path references.
pub fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let raw = path.as_ref();

    // First, make it absolute if it isn't already
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(raw)
    } else {
        raw.to_path_buf()
    };

    // Now normalize by processing components
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {
                // Skip current directory markers
            }
            Component::ParentDir => {
                // Pop the last component if possible
                if !normalized.pop() {
                    // Can't go above root, keep the ..
                    normalized.push(component.as_os_str());
                }
            }
            _ => {
                normalized.push(component.as_os_str());
            }
        }
    }

    normalized
}

/// Check if a tool contributes to file-read tracking state.
///
/// These tools emit `ContextModifier::FileRead` that should be tracked
/// and potentially cleaned up during micro-compaction.
///
/// # Included Tools
///
/// - `Read` - Full file reads
/// - `ReadManyFiles` - Batch file reads
/// - `Glob` - File pattern matching (metadata only)
/// - `Grep` - Content search (partial reads)
///
/// # Claude Code Alignment
///
/// This matches Claude Code v2.1.38's `isReadStateSourceTool` function.
pub fn is_read_state_source_tool(tool_name: &str) -> bool {
    READ_STATE_SOURCE_TOOLS.contains(&tool_name)
}

/// Check if one FileReadKind is stronger than another.
///
/// Strength ordering: FullContent > PartialContent > MetadataOnly
///
/// Used during collision resolution when the same path appears in
/// multiple tool calls - prefer the stronger read kind.
pub fn is_stronger_kind(
    a: &cocode_protocol::FileReadKind,
    b: &cocode_protocol::FileReadKind,
) -> bool {
    use cocode_protocol::FileReadKind;
    matches!(
        (a, b),
        (
            FileReadKind::FullContent,
            FileReadKind::PartialContent | FileReadKind::MetadataOnly
        ) | (FileReadKind::PartialContent, FileReadKind::MetadataOnly)
    )
}

/// Collect cleared read paths from tool call (primary API).
///
/// This is the preferred function for collecting paths during micro-compaction.
/// It uses modifier paths (from `ContextModifier::FileRead`) as the primary source,
/// with fallback to parsing tool input only when modifiers are not available.
///
/// # Arguments
///
/// * `tool_name` - Name of the tool
/// * `modifier_paths` - Paths extracted from `ContextModifier::FileRead` in tool call
/// * `fallback_input_path` - Single path from tool input (used if no modifier paths)
///
/// # Returns
///
/// List of file paths that were read by this tool call.
///
/// # Claude Code Alignment
///
/// Modifier paths are the source of truth because they come from structured
/// `ContextModifier::FileRead` data, which is more reliable than parsing
/// tool input JSON (which may have varying formats).
pub fn collect_cleared_read_paths(
    tool_name: &str,
    modifier_paths: &[PathBuf],
    fallback_input_path: Option<&str>,
) -> Vec<PathBuf> {
    if !is_read_state_source_tool(tool_name) {
        return Vec::new();
    }

    // Prefer modifier paths (from ContextModifier::FileRead)
    if !modifier_paths.is_empty() {
        return modifier_paths.to_vec();
    }

    // Fall back to single path if provided
    if let Some(path_str) = fallback_input_path {
        return vec![PathBuf::from(path_str)];
    }

    Vec::new()
}

/// Collect cleared read paths from tool call input (extended API).
///
/// Used during micro-compact to identify which file paths should be removed
/// from the FileTracker when tool results are compacted.
///
/// # Arguments
///
/// * `tool_name` - Name of the tool
/// * `modifier_paths` - Paths extracted from `ContextModifier::FileRead` in tool call
/// * `input` - Tool input JSON (fallback source)
///
/// # Returns
///
/// List of file paths that were read by this tool call.
///
/// # Note
///
/// Prefer `collect_cleared_read_paths` when modifier paths are already extracted.
/// This function is kept for compatibility and cases where full tool input parsing
/// is needed (e.g., ReadManyFiles with paths array).
pub fn collect_cleared_read_paths_from_input(
    tool_name: &str,
    modifier_paths: &[PathBuf],
    input: &serde_json::Value,
) -> Vec<PathBuf> {
    if !is_read_state_source_tool(tool_name) {
        return Vec::new();
    }

    // Prefer modifier paths (from ContextModifier::FileRead)
    if !modifier_paths.is_empty() {
        return modifier_paths.to_vec();
    }

    // Fall back to parsing tool input
    let mut paths = Vec::new();

    if tool_name == ToolName::Read.as_str() {
        if let Some(path_str) = input.get("file_path").and_then(|v| v.as_str()) {
            paths.push(PathBuf::from(path_str));
        }
    } else if tool_name == ToolName::ReadManyFiles.as_str()
        && let Some(paths_arr) = input.get("paths").and_then(|v| v.as_array())
    {
        for path_val in paths_arr {
            if let Some(path_str) = path_val.as_str() {
                paths.push(PathBuf::from(path_str));
            }
        }
    }
    // Glob and Grep have a `path` parameter for the search directory
    // but we don't track these as file reads since they're metadata-only

    paths
}

#[cfg(test)]
#[path = "read_tracking_policy.test.rs"]
mod tests;
