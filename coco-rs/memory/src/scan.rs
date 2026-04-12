//! Enhanced memory scanning with mtime tracking and manifest formatting.
//!
//! TS: memdir/memoryScan.ts — scanMemoryFiles, formatMemoryManifest.

use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::MemoryFrontmatter;
use crate::parse_frontmatter;
use crate::staleness;

/// Maximum number of memory files to scan (sorted by mtime, newest first).
const MAX_SCAN_FILES: usize = 200;

/// A scanned memory file with metadata.
#[derive(Debug, Clone)]
pub struct ScannedMemory {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// Parsed frontmatter (name, description, type).
    pub frontmatter: Option<MemoryFrontmatter>,
    /// File modification time in milliseconds since epoch.
    pub mtime_ms: i64,
    /// Pre-computed header string for display (age + relative path).
    pub header: String,
    /// File size in bytes.
    pub size_bytes: i64,
}

/// Scan a memory directory for all `.md` files (excluding MEMORY.md).
///
/// Returns files sorted by mtime (newest first), capped at `MAX_SCAN_FILES`.
pub fn scan_memory_files(memory_dir: &Path) -> Vec<ScannedMemory> {
    let mut files = Vec::new();

    if !memory_dir.is_dir() {
        return files;
    }

    let entries = match std::fs::read_dir(memory_dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "MEMORY.md") {
            continue;
        }

        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let size_bytes = metadata.len() as i64;

        // Read and parse frontmatter (lightweight — only first few lines)
        let frontmatter = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| parse_frontmatter(&content).0);

        // Build header: "[age] path"
        let age = staleness::memory_age(mtime_ms);
        let rel_path = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let header = format!("[{age}] {rel_path}");

        files.push(ScannedMemory {
            path,
            frontmatter,
            mtime_ms,
            header,
            size_bytes,
        });
    }

    // Sort by mtime descending (newest first)
    files.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));

    // Cap at max files
    files.truncate(MAX_SCAN_FILES);

    files
}

/// Format scanned memories as a text manifest for injection into extraction prompts.
///
/// TS: memoryScan.ts formatMemoryManifest — one line per file:
/// `- [type] filename (age): description`
pub fn format_memory_manifest(memories: &[ScannedMemory]) -> String {
    let mut lines = Vec::with_capacity(memories.len() + 1);
    lines.push("## Existing Memory Files".to_string());

    for mem in memories {
        let filename = mem
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.md");
        let mem_type = mem
            .frontmatter
            .as_ref()
            .map_or("unknown", |fm| fm.memory_type.as_str());
        let desc = mem
            .frontmatter
            .as_ref()
            .map_or("", |fm| fm.description.as_str());
        let age = staleness::memory_age(mem.mtime_ms);

        lines.push(format!("- [{mem_type}] {filename} ({age}): {desc}"));
    }

    lines.join("\n")
}

/// Scan both personal and team memory directories.
pub fn scan_all_memory_files(personal_dir: &Path, team_dir: Option<&Path>) -> Vec<ScannedMemory> {
    let mut all = scan_memory_files(personal_dir);

    if let Some(team) = team_dir {
        let team_files = scan_memory_files(team);
        all.extend(team_files);
    }

    // Re-sort combined list
    all.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    all.truncate(MAX_SCAN_FILES);

    all
}

#[cfg(test)]
#[path = "scan.test.rs"]
mod tests;
