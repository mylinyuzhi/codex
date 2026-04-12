//! Memory file discovery and loading.
//!
//! TS: memdir/ — CLAUDE.md management, auto-extraction, session memory.

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

/// Type of memory file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Managed by the system.
    Managed,
    /// User-level (~/.claude/memory/).
    User,
    /// Project-level (.claude/memory/).
    Project,
    /// Local (gitignored) memory.
    Local,
    /// Auto-extracted memory.
    AutoMem,
    /// Team memory.
    TeamMem,
}

/// A discovered memory file with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFileInfo {
    pub path: PathBuf,
    pub memory_type: MemoryType,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub globs: Option<Vec<String>>,
}

/// Discover memory files for a working directory.
pub fn get_memory_files(cwd: &Path) -> Vec<MemoryFileInfo> {
    let mut files = Vec::new();

    // Project memory: .claude/memory/
    let project_mem_dir = cwd.join(".claude/memory");
    collect_memory_files(&project_mem_dir, MemoryType::Project, &mut files);

    // Auto-memory: ~/.claude/projects/<sanitized>/memory/
    let auto_mem_dir = resolve_auto_memory_dir(cwd);
    collect_memory_files(&auto_mem_dir, MemoryType::AutoMem, &mut files);

    // Team memory: auto_mem_dir/team/
    let team_mem_dir = auto_mem_dir.join("team");
    collect_memory_files(&team_mem_dir, MemoryType::TeamMem, &mut files);

    // User memory: ~/.claude/memory/ (or COCO_CONFIG_DIR)
    if let Ok(home) = std::env::var("HOME") {
        let user_mem_dir = PathBuf::from(home).join(".claude/memory");
        collect_memory_files(&user_mem_dir, MemoryType::User, &mut files);
    }

    files
}

/// Collect `.md` files from a directory into the memory file list.
fn collect_memory_files(dir: &Path, mem_type: MemoryType, out: &mut Vec<MemoryFileInfo>) {
    if !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.push(MemoryFileInfo {
                    path,
                    memory_type: mem_type,
                    content,
                    parent: None,
                    globs: None,
                });
            }
        }
    }
}

/// Resolve the auto-memory directory for a project.
///
/// Path: `~/.claude/projects/<sanitized-cwd>/memory/`
fn resolve_auto_memory_dir(cwd: &Path) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let sanitized = cwd
        .to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '\\'], "-");
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(sanitized)
        .join("memory")
}

/// Check if a path belongs to any memory-managed directory.
///
/// Used by the permission system to grant write carve-outs.
pub fn is_memory_managed_path(path: &Path, cwd: &Path) -> bool {
    let auto_dir = resolve_auto_memory_dir(cwd);
    let project_dir = cwd.join(".claude/memory");

    path.starts_with(&auto_dir)
        || path.starts_with(&project_dir)
        || path.to_string_lossy().contains(".claude/memory")
}

/// Determine the memory type for a given path.
pub fn classify_memory_path(path: &Path, cwd: &Path) -> Option<MemoryType> {
    let auto_dir = resolve_auto_memory_dir(cwd);
    let project_dir = cwd.join(".claude/memory");
    let team_dir = auto_dir.join("team");

    if path.starts_with(&team_dir) {
        Some(MemoryType::TeamMem)
    } else if path.starts_with(&auto_dir) {
        Some(MemoryType::AutoMem)
    } else if path.starts_with(&project_dir) {
        Some(MemoryType::Project)
    } else if path.to_string_lossy().contains("/.claude/memory/") {
        Some(MemoryType::User)
    } else {
        None
    }
}
