//! Eager memory-file discovery — root→CWD walk loaded once at session
//! start.
//!
//! TS source: `utils/claudemd.ts:790-960` (`getMemoryFiles`).
//! Per-file lazy traversal lives in [`crate::nested_memory`] and is
//! driven by file-read triggers, not this module.
//!
//! **Naming**: TS calls these `CLAUDE.md` files. coco-rs supports both
//! `CLAUDE.md` and `AGENTS.md` (Codex / Cursor convention) at every
//! eager-load position, matched case-insensitively via
//! [`crate::memory_filenames::find_memory_files`]. The struct is named
//! `MemoryFile` to reflect this — `ClaudeMdFile` is no longer used.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use crate::memory_filenames::MEMORY_FILE_CANDIDATES;
use crate::memory_filenames::MEMORY_LOCAL_FILE_CANDIDATES;
use crate::memory_filenames::find_memory_files;
use crate::memory_imports::expand_imports;

/// A discovered memory file (`CLAUDE.md`, `AGENTS.md`, or local variant).
#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub path: PathBuf,
    pub content: String,
    pub source: MemoryFileSource,
}

/// Where a memory file was found in the eager load.
///
/// Per-file lazy traversal (driven by file-read triggers) emits
/// `Project` for each loaded file regardless of relative depth — the
/// `path` field carries the precise location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFileSource {
    /// `~/.coco/CLAUDE.md` (or AGENTS.md) — user-global.
    UserGlobal,
    /// `<dir>/.claude/CLAUDE.md` — project config dir.
    ProjectConfig,
    /// `<dir>/CLAUDE.md` or `<dir>/AGENTS.md` — root-level project file.
    Project,
    /// `<dir>/CLAUDE.local.md` or `<dir>/AGENTS.local.md` — gitignored.
    Local,
}

/// Discover all memory files for the given working directory.
///
/// Walk order (TS parity, `claudemd.ts:790-960`):
/// 1. User-global `~/.coco/{CLAUDE,AGENTS}.md` (case-insensitive).
/// 2. From filesystem root walking down to `cwd` inclusive, in each dir:
///    - `<dir>/.claude/CLAUDE.md` (project config dir — claude-code-specific path; AGENTS.md not added here)
///    - `<dir>/{CLAUDE,AGENTS}.md` (case-insensitive)
///    - `<dir>/{CLAUDE,AGENTS}.local.md` (case-insensitive)
///
/// Files closer to `cwd` are loaded last → highest model-attention
/// priority (TS header comment: "Files are loaded in reverse order of
/// priority"). Duplicates resolved via canonicalized-path dedup (e.g.
/// when CWD == filesystem root or when symlinks loop back).
///
/// Per-file lazy traversal — adding `<between-cwd-and-file>/CLAUDE.md`
/// and conditional `.claude/rules/*.md` matches — happens in
/// [`crate::nested_memory`] driven by [`coco_tool_runtime`] file-read
/// triggers, not this function.
pub fn discover_memory_files(cwd: &Path) -> Vec<MemoryFile> {
    let mut files: Vec<MemoryFile> = Vec::new();
    // Shared `processed` set for the whole eager pass so:
    //   1. canonical-path dedup across positions (user-global, project,
    //      .claude/, local) without rescanning `files` quadratically.
    //   2. an `@import` chain that resolves into another would-be-loaded
    //      file is not double-loaded.
    let mut processed: HashSet<PathBuf> = HashSet::new();

    // 1. User-global ~/.coco/{CLAUDE,AGENTS}.md
    if let Some(home) = dirs_home() {
        let coco_dir = home.join(".coco");
        for path in find_memory_files(&coco_dir, MEMORY_FILE_CANDIDATES) {
            try_push(
                &path,
                MemoryFileSource::UserGlobal,
                &mut files,
                &mut processed,
            );
        }
    }

    // 2. Walk root→cwd inclusive. Build dirs from cwd up, then reverse
    //    so loading proceeds root→cwd (TS `claudemd.ts:850-857`).
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut current = cwd.to_path_buf();
    loop {
        dirs.push(current.clone());
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }
    dirs.reverse();

    for dir in &dirs {
        // .claude/CLAUDE.md (project config dir — claude-code-specific
        // path; we don't extend with AGENTS.md here since `.claude/`
        // is the config dir convention, not a memory dir).
        let dot_claude = dir.join(".claude").join("CLAUDE.md");
        try_push(
            &dot_claude,
            MemoryFileSource::ProjectConfig,
            &mut files,
            &mut processed,
        );

        // <dir>/{CLAUDE,AGENTS}.md (case-insensitive)
        for path in find_memory_files(dir, MEMORY_FILE_CANDIDATES) {
            try_push(&path, MemoryFileSource::Project, &mut files, &mut processed);
        }

        // <dir>/{CLAUDE,AGENTS}.local.md (case-insensitive)
        for path in find_memory_files(dir, MEMORY_LOCAL_FILE_CANDIDATES) {
            try_push(&path, MemoryFileSource::Local, &mut files, &mut processed);
        }
    }

    files
}

/// Backward-compat alias. New code should use [`discover_memory_files`].
#[doc(hidden)]
pub fn discover_claude_md_files(cwd: &Path) -> Vec<MemoryFile> {
    discover_memory_files(cwd)
}

fn try_push(
    path: &Path,
    source: MemoryFileSource,
    files: &mut Vec<MemoryFile>,
    processed: &mut HashSet<PathBuf>,
) {
    if !path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    // expand_imports owns the canonical-path dedup against `processed`
    // and the `@import` recursion's own cycle break. Returns the parent
    // first followed by transitively-included files.
    for (p, c) in expand_imports(path, &content, processed, 0) {
        files.push(MemoryFile {
            path: p,
            content: c,
            source,
        });
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
#[path = "memory_discovery.test.rs"]
mod tests;
