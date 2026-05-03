//! Memory file classification — scope (personal/team) + auto-managed
//! detection used by the secret-guard, permission carve-outs, and the
//! file-history skip list.
//!
//! TS: `utils/memoryFileDetection.ts`.

use std::path::Path;

use super::validate::is_within_memory_dir;

/// Scope of a memory file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    Personal,
    Team,
}

/// Type of a session-related file (transcript vs session memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileType {
    Memory,
    Transcript,
    Other,
}

/// Predicate: is `path` under `memory_dir.personal`?
pub fn is_auto_mem_file(path: &Path, memory_dir: &Path) -> bool {
    is_within_memory_dir(path, memory_dir)
}

/// Anything we manage automatically: personal memory, team memory,
/// session memory file. Used by secret-guard / permission carve-outs.
pub fn is_auto_managed_memory_file(path: &Path, memory_dir: &Path) -> bool {
    is_auto_mem_file(path, memory_dir) || path.to_string_lossy().contains("session_memory")
}

/// Personal vs team — checked by directory layout. Team is a strict
/// subdir of personal so the team check goes first.
pub fn memory_scope_for_path(path: &Path, memory_dir: &Path) -> MemoryScope {
    let team = memory_dir.join("team");
    if is_within_memory_dir(path, &team) {
        MemoryScope::Team
    } else {
        MemoryScope::Personal
    }
}

/// Permission carve-out: auto-memory paths skip the dangerous-dirs
/// prompt so the agent can save without an extra confirmation. Disabled
/// when the operator installed a custom path override.
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
#[path = "scope.test.rs"]
mod tests;
