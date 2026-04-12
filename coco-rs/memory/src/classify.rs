//! Memory file classification helpers.
//!
//! TS: utils/memoryFileDetection.ts — isAutoMemFile, isAutoManagedMemoryFile,
//! memoryScopeForPath, isMemoryDirectory, detectSessionFileType.

use std::path::Path;

/// Scope of a memory file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    /// Personal memory (default).
    Personal,
    /// Team-shared memory.
    Team,
}

/// Type of a session-related file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileType {
    /// Session memory (insights).
    Memory,
    /// Session transcript.
    Transcript,
    /// Unknown/other.
    Other,
}

/// Check if a path is an auto-memory file.
///
/// Returns true if the path is within a `.claude/*/memory/` or
/// `~/.claude/projects/*/memory/` directory.
pub fn is_auto_mem_file(path: &Path, memory_dir: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let mem_str = memory_dir.to_string_lossy();
    path_str.starts_with(mem_str.as_ref())
}

/// Check if a path is any auto-managed memory file.
///
/// Includes: auto-memory, team memory, agent memory, session memory.
pub fn is_auto_managed_memory_file(path: &Path, memory_dir: &Path) -> bool {
    if is_auto_mem_file(path, memory_dir) {
        return true;
    }
    // Check team memory subdirectory
    let team_dir = memory_dir.join("team");
    if path.starts_with(&team_dir) {
        return true;
    }
    // Check session memory files
    let path_str = path.to_string_lossy();
    path_str.contains("session_memory.json")
}

/// Determine the scope of a memory file.
pub fn memory_scope_for_path(path: &Path, memory_dir: &Path) -> MemoryScope {
    let team_dir = memory_dir.join("team");
    if path.starts_with(&team_dir) {
        MemoryScope::Team
    } else {
        MemoryScope::Personal
    }
}

/// Check if a path is a memory-related directory.
pub fn is_memory_directory(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "memory"
        && path.parent().is_some_and(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == ".claude" || n.contains("projects"))
        })
}

/// Detect the type of a session-related file.
pub fn detect_session_file_type(path: &Path) -> SessionFileType {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match name {
        "session_memory.json" => SessionFileType::Memory,
        "transcript.jsonl" | "transcript.json" => SessionFileType::Transcript,
        _ => SessionFileType::Other,
    }
}

/// Check if a path should bypass dangerous directory restrictions.
///
/// Auto-memory paths get a write carve-out so the agent can save memories
/// without triggering filesystem permission prompts.
///
/// Security: Only applies when no custom path override is active
/// (SDK overrides disable the carve-out).
pub fn should_bypass_dangerous_dirs(
    path: &Path,
    memory_dir: &Path,
    has_path_override: bool,
) -> bool {
    if has_path_override {
        return false;
    }
    is_auto_mem_file(path, memory_dir)
}

#[cfg(test)]
#[path = "classify.test.rs"]
mod tests;
