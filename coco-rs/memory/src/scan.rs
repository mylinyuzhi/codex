//! Single Scanner — the only entry point for listing memory files.
//!
//! TS: `memdir/memoryScan.ts`. Walks the directory, reads only the first
//! 30 lines of each `.md` (enough for frontmatter), sorts by mtime
//! descending, caps at 200 files.

use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::store::MemoryFrontmatter;
use crate::store::parse_memory_frontmatter;

/// Hard cap on files scanned in a single pass. Anything past this is
/// silently dropped after mtime-sort.
pub const MAX_SCANNED_FILES: usize = 200;

/// Lines read for frontmatter parsing. Prevents loading large bodies
/// when only the manifest fields are needed.
pub const FRONTMATTER_MAX_LINES: usize = 30;

/// One scanned memory file.
#[derive(Debug, Clone)]
pub struct ScannedMemory {
    pub path: PathBuf,
    pub filename: String,
    pub mtime_ms: i64,
    pub size_bytes: i64,
    pub frontmatter: Option<MemoryFrontmatter>,
}

/// Scan a directory for memory files (`*.md` excluding `MEMORY.md`).
///
/// Sorted newest-first by mtime, capped at 200. Errors (unreadable
/// directory, broken symlinks) are swallowed — return an empty vec.
pub fn scan_memory_files(dir: &Path) -> Vec<ScannedMemory> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !filename.ends_with(".md") || filename == crate::store::ENTRYPOINT_NAME {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let size_bytes = metadata.len() as i64;
        let frontmatter = read_frontmatter_only(&path);
        out.push(ScannedMemory {
            path,
            filename,
            mtime_ms,
            size_bytes,
            frontmatter,
        });
    }
    out.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    out.truncate(MAX_SCANNED_FILES);
    out
}

/// Format scanned memories as a manifest line list for prompt injection.
///
/// One line per file: `- [type] filename (age): description`. Mirrors TS
/// `formatMemoryManifest`.
pub fn format_memory_manifest(memories: &[ScannedMemory]) -> String {
    let mut lines = Vec::with_capacity(memories.len() + 1);
    lines.push("## Existing Memory Files".to_string());
    if memories.is_empty() {
        lines.push("_(none)_".to_string());
        return lines.join("\n");
    }
    for mem in memories {
        let ty = mem
            .frontmatter
            .as_ref()
            .map_or("unknown", |fm| fm.memory_type.as_str());
        let desc = mem
            .frontmatter
            .as_ref()
            .map_or("", |fm| fm.description.as_str());
        let age = memory_age_string(mem.mtime_ms);
        lines.push(format!("- [{ty}] {} ({age}): {desc}", mem.filename));
    }
    lines.join("\n")
}

/// Days since `mtime_ms`. 0 = today, 1 = yesterday, etc.
pub fn memory_age_days(mtime_ms: i64) -> i64 {
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff = now_ms.saturating_sub(mtime_ms);
    if diff <= 0 {
        0
    } else {
        diff / (24 * 60 * 60 * 1000)
    }
}

/// Human-readable age. TS `memoryAge`: `today` / `yesterday` / `N days ago`.
pub fn memory_age_string(mtime_ms: i64) -> String {
    match memory_age_days(mtime_ms) {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        n => format!("{n} days ago"),
    }
}

/// Return mtime in ms since epoch, or `None` if the file is missing.
pub fn file_mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn read_frontmatter_only(path: &Path) -> Option<MemoryFrontmatter> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut bounded = String::new();
    for (i, line) in content.lines().enumerate() {
        if i >= FRONTMATTER_MAX_LINES {
            break;
        }
        bounded.push_str(line);
        bounded.push('\n');
    }
    parse_memory_frontmatter(&bounded)
}

#[cfg(test)]
#[path = "scan.test.rs"]
mod tests;
