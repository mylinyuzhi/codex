//! Memory file loading and management.
//!
//! Handles reading `MEMORY.md` with 200-line truncation, listing topic
//! files, and loading individual memory files.

use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use snafu::ResultExt;
use tracing::debug;

use crate::error::auto_memory_error as err;

/// The MEMORY.md filename.
pub const MEMORY_MD_FILENAME: &str = "MEMORY.md";

/// Parsed MEMORY.md index.
#[derive(Debug, Clone)]
pub struct MemoryIndex {
    /// Raw file content (possibly truncated).
    pub raw_content: String,
    /// Total line count of the original file.
    pub line_count: i32,
    /// Whether the content was truncated.
    pub was_truncated: bool,
    /// Last modification time.
    pub last_modified: Option<SystemTime>,
}

/// A loaded memory file entry.
/// Parsed YAML frontmatter from a memory file.
#[derive(Debug, Clone, Default)]
pub struct MemoryFrontmatter {
    /// Memory name.
    pub name: Option<String>,
    /// One-line description used for relevance matching.
    pub description: Option<String>,
    /// Memory type: user, feedback, project, reference.
    pub memory_type: Option<String>,
}

/// A loaded memory file entry.
#[derive(Debug, Clone)]
pub struct AutoMemoryEntry {
    /// Path to the file.
    pub path: PathBuf,
    /// File content (possibly truncated).
    pub content: String,
    /// Parsed frontmatter (if any).
    pub frontmatter: Option<MemoryFrontmatter>,
    /// Last modification time.
    pub last_modified: Option<SystemTime>,
    /// Line count of loaded content.
    pub line_count: i32,
    /// Whether the content was truncated.
    pub was_truncated: bool,
}

impl AutoMemoryEntry {
    /// Get the description from frontmatter, if present.
    pub fn description(&self) -> Option<&str> {
        self.frontmatter
            .as_ref()
            .and_then(|f| f.description.as_deref())
    }

    /// Get the memory type from frontmatter, if present.
    pub fn memory_type(&self) -> Option<&str> {
        self.frontmatter
            .as_ref()
            .and_then(|f| f.memory_type.as_deref())
    }
}

/// Load and parse the MEMORY.md index from a memory directory.
///
/// Returns `None` if the file does not exist. Truncates content
/// to `max_lines` and appends a warning if truncated.
pub fn load_memory_index(dir: &Path, max_lines: i32) -> crate::Result<Option<MemoryIndex>> {
    let path = dir.join(MEMORY_MD_FILENAME);
    if !path.exists() {
        debug!(path = %path.display(), "MEMORY.md not found");
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path).context(err::ReadFileSnafu {
        path: path.display().to_string(),
    })?;

    let last_modified = path.metadata().ok().and_then(|m| m.modified().ok());
    let content = strip_html_comments(&raw);

    let total_lines = content.lines().count() as i32;
    let (truncated_content, was_truncated) = truncate_content(&content, max_lines);

    debug!(
        path = %path.display(),
        total_lines,
        was_truncated,
        "Loaded MEMORY.md"
    );

    Ok(Some(MemoryIndex {
        raw_content: truncated_content,
        line_count: total_lines,
        was_truncated,
        last_modified,
    }))
}

/// List all `.md` files in the memory directory (excluding MEMORY.md).
///
/// Returns files sorted by modification time (newest first).
pub fn list_memory_files(dir: &Path) -> crate::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(dir).context(err::ListDirSnafu {
        path: dir.display().to_string(),
    })?;

    let mut files: Vec<(PathBuf, SystemTime)> = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            let path = entry.path();
            path.extension().is_some_and(|ext| ext == "md")
                && path
                    .file_name()
                    .is_some_and(|name| name != MEMORY_MD_FILENAME)
        })
        .map(|entry| {
            let path = entry.path();
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (path, mtime)
        })
        .collect();

    // Sort newest first
    files.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(files.into_iter().map(|(path, _)| path).collect())
}

/// Load a single memory file with line truncation.
pub fn load_memory_file(
    path: &Path,
    max_lines: i32,
    max_frontmatter_lines: i32,
) -> crate::Result<AutoMemoryEntry> {
    let raw = std::fs::read_to_string(path).context(err::ReadFileSnafu {
        path: path.display().to_string(),
    })?;

    let last_modified = path.metadata().ok().and_then(|m| m.modified().ok());
    // Parse frontmatter from raw content (before stripping comments)
    // so that commented-out frontmatter fields are not accidentally used.
    let frontmatter = parse_frontmatter(&raw, max_frontmatter_lines);
    let content = strip_html_comments(&raw);
    let (truncated, was_truncated) = truncate_content(&content, max_lines);
    let line_count = truncated.lines().count() as i32;

    Ok(AutoMemoryEntry {
        path: path.to_path_buf(),
        content: truncated,
        frontmatter,
        last_modified,
        line_count,
        was_truncated,
    })
}

/// Truncate content to a maximum number of lines.
///
/// Returns `(content, was_truncated)`. If truncated, appends a warning.
/// If `max_lines <= 0`, returns empty content with `was_truncated = true`
/// (unless content is already empty).
pub fn truncate_content(content: &str, max_lines: i32) -> (String, bool) {
    if max_lines <= 0 {
        return (String::new(), !content.is_empty());
    }

    let total = content.lines().count() as i32;

    if total <= max_lines {
        return (content.to_string(), false);
    }

    let mut result: String = content
        .lines()
        .take(max_lines as usize)
        .collect::<Vec<_>>()
        .join("\n");
    result.push_str(&format!(
        "\n\n... (truncated — {total} total lines, showing first {max_lines}. \
         Keep MEMORY.md concise to stay under the {max_lines}-line limit.)"
    ));
    (result, true)
}

/// Parse YAML frontmatter from a memory file.
///
/// Extracts `name`, `description`, and `type` fields from between `---`
/// delimiters. This is a lightweight parser that handles the subset of
/// YAML used by memory files:
///
/// - Single-line scalar values only (no multi-line `|` or `>` blocks)
/// - Outer single/double quotes are stripped
/// - Embedded colons in values work correctly (we split on the first `:` after the key)
/// - Multi-line quoted strings and YAML anchors/aliases are NOT supported
///
/// This is intentional — memory files use a simple frontmatter format and
/// a full YAML parser (serde_yaml) would add unnecessary weight.
fn parse_frontmatter(content: &str, max_frontmatter_lines: i32) -> Option<MemoryFrontmatter> {
    let mut lines = content.lines();
    let first = lines.next()?;
    if first.trim() != "---" {
        return None;
    }

    let mut fm = MemoryFrontmatter::default();
    let mut found_any = false;

    // Limit scan to avoid parsing body content as frontmatter
    // when the closing `---` delimiter is missing.
    for line in lines.take(max_frontmatter_lines.max(1) as usize) {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(val) = extract_yaml_value(trimmed, "description") {
            fm.description = Some(val);
            found_any = true;
        } else if let Some(val) = extract_yaml_value(trimmed, "name") {
            fm.name = Some(val);
            found_any = true;
        } else if let Some(val) = extract_yaml_value(trimmed, "type") {
            fm.memory_type = Some(val);
            found_any = true;
        }
    }

    if found_any { Some(fm) } else { None }
}

/// Extract a YAML scalar value from a `key: value` line.
fn extract_yaml_value(line: &str, key: &str) -> Option<String> {
    let val = line.strip_prefix(key)?.strip_prefix(':')?;
    let val = val.trim().trim_matches('"').trim_matches('\'');
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Strip HTML comments (`<!-- ... -->`) from content.
///
/// Aligns with Claude Code's `stripHtmlComments` (o14) which removes
/// HTML comment blocks from memory file content before injection.
/// Multi-line comments are supported.
pub fn strip_html_comments(content: &str) -> String {
    if !content.contains("<!--") {
        return content.to_string();
    }

    let mut result = String::with_capacity(content.len());
    let mut remainder = content;

    while let Some(start) = remainder.find("<!--") {
        // Copy everything before the comment
        result.push_str(&remainder[..start]);

        // Find the end of the comment
        match remainder[start + 4..].find("-->") {
            Some(end_offset) => {
                // Skip past the closing -->
                remainder = &remainder[start + 4 + end_offset + 3..];
            }
            None => {
                // Unclosed comment — keep the rest as-is
                result.push_str(&remainder[start..]);
                remainder = "";
                break;
            }
        }
    }

    // Append anything after the last comment
    result.push_str(remainder);
    result
}

#[cfg(test)]
#[path = "memory_file.test.rs"]
mod tests;
