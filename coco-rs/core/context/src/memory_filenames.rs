//! Memory-file basename matching with case-insensitive lookup.
//!
//! coco-rs supports both `CLAUDE.md` (TS-original) and `AGENTS.md`
//! (cross-ecosystem convention shared with Codex / Cursor / similar
//! agents). Filenames match case-insensitively so `Claude.md`,
//! `agents.md`, `CLAUDE.MD` etc. all load identically across
//! case-sensitive (Linux ext4) and case-insensitive (macOS APFS,
//! Windows NTFS) filesystems.
//!
//! Divergence from TS: TS only loads files literally named `CLAUDE.md`
//! / `CLAUDE.local.md` with exact case. coco-rs broadens both axes.
//! The `<system-reminder>` template, dedup keys (absolute path
//! strings), and trigger pipeline are unchanged — only filename
//! resolution is broader.

use std::path::Path;
use std::path::PathBuf;

/// Memory-file basenames at directory roots. Matched case-insensitively.
pub const MEMORY_FILE_CANDIDATES: &[&str] = &["CLAUDE.md", "AGENTS.md"];

/// Local (gitignored) variants. Matched case-insensitively.
pub const MEMORY_LOCAL_FILE_CANDIDATES: &[&str] = &["CLAUDE.local.md", "AGENTS.local.md"];

/// Case-insensitively find any of `candidates` directly under `dir`.
///
/// Returns disk-cased absolute paths (so `Claude.md` on disk surfaces
/// as `Claude.md`, not normalized) in deterministic alphabetical order
/// of the lowercased basename — repeated calls on identical trees
/// produce identical sequences.
///
/// Empty result on read errors (`ENOENT`, `EACCES`, `ENOTDIR`) — matches
/// TS `processMdRules` defensive read behavior in `claudemd.ts:730-738`.
/// Directory entries that happen to share a candidate name are skipped;
/// only regular files (or symlinks resolving to files) are returned.
pub fn find_memory_files(dir: &Path, candidates: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut hits: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        if !candidates.iter().any(|c| name.eq_ignore_ascii_case(c)) {
            continue;
        }
        let path = entry.path();
        // Use fs::metadata (follows symlinks) instead of DirEntry::metadata
        // (which calls symlink_metadata) so a symlink pointing at a file
        // counts as a file. Without this, a workspace where CLAUDE.md is a
        // symlink to a shared rule file would silently drop.
        let is_file = std::fs::metadata(&path)
            .map(|m| m.is_file())
            .unwrap_or(false);
        if !is_file {
            continue;
        }
        hits.push((name.to_ascii_lowercase(), path));
    }

    hits.sort_by(|a, b| a.0.cmp(&b.0));
    hits.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
#[path = "memory_filenames.test.rs"]
mod tests;
