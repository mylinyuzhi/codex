//! File context resolver for shared file resolution logic.
//!
//! This module provides shared functionality for resolving file mentions and
//! reading file content, used by both `at_mentioned_files` and `already_read_files`
//! generators.
//!
//! # Features
//!
//! - File mention resolution (relative/absolute paths)
//! - Content reading with limits
//! - Already-read detection helpers
//! - Token estimation for file content

use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Result of resolving a file mention.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    /// The resolved absolute path.
    pub path: PathBuf,
    /// Whether this is a partial read (has line range).
    pub is_partial: bool,
    /// Line start (1-indexed), if specified.
    pub line_start: Option<i32>,
    /// Line end (1-indexed), if specified.
    pub line_end: Option<i32>,
}

impl ResolvedFile {
    /// Resolve a file mention relative to a working directory.
    ///
    /// Handles:
    /// - Absolute paths (used as-is)
    /// - Relative paths (resolved against cwd)
    /// - Home directory expansion (~)
    pub fn from_mention(path_str: &str, cwd: &Path) -> Self {
        let path = if path_str.starts_with('~') {
            // Expand home directory
            if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(path_str.replacen('~', &home.to_string_lossy(), 1))
            } else {
                PathBuf::from(path_str)
            }
        } else if Path::new(path_str).is_absolute() {
            PathBuf::from(path_str)
        } else {
            cwd.join(path_str)
        };

        Self {
            path,
            is_partial: false,
            line_start: None,
            line_end: None,
        }
    }

    /// Add line range to the resolved file.
    pub fn with_line_range(mut self, start: Option<i32>, end: Option<i32>) -> Self {
        self.is_partial = start.is_some() || end.is_some();
        self.line_start = start;
        self.line_end = end;
        self
    }
}

/// Result of reading a file with limits applied.
#[derive(Debug)]
pub enum ReadFileResult {
    /// File content successfully read.
    Content(String),
    /// File is too large.
    TooLarge { size: i64, max: i64 },
    /// File not found or read error.
    Error(String),
}

/// Configuration for file reading limits.
#[derive(Debug, Clone)]
pub struct FileReadConfig {
    /// Maximum file size in bytes.
    pub max_file_size: i64,
    /// Maximum number of lines to read.
    pub max_lines: i32,
    /// Maximum line length before truncation.
    pub max_line_length: i32,
}

impl Default for FileReadConfig {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_lines: 2000,
            max_line_length: 2000,
        }
    }
}

/// Read file content with optional line range and limits.
///
/// Returns formatted content with line numbers (matching Read tool output).
pub fn read_file_with_limits(
    path: &Path,
    line_start: Option<i32>,
    line_end: Option<i32>,
    config: &FileReadConfig,
) -> ReadFileResult {
    // Check file size first
    match fs::metadata(path) {
        Ok(metadata) => {
            let file_size = metadata.len() as i64;
            if file_size > config.max_file_size {
                return ReadFileResult::TooLarge {
                    size: file_size,
                    max: config.max_file_size,
                };
            }
        }
        Err(e) => {
            return ReadFileResult::Error(format!("Failed to read file metadata: {e}"));
        }
    }

    // Read content
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ReadFileResult::Error(format!("Failed to read file: {e}"));
        }
    };

    // Format with optional line range
    let result = format_content_with_lines(&content, line_start, line_end, config);
    ReadFileResult::Content(result)
}

/// Format file content with line numbers and limits.
fn format_content_with_lines(
    content: &str,
    line_start: Option<i32>,
    line_end: Option<i32>,
    config: &FileReadConfig,
) -> String {
    let lines: Vec<&str> = content.lines().collect();

    match (line_start, line_end) {
        (Some(start), Some(end)) => {
            // Extract specific line range (1-indexed)
            let start_idx = (start - 1).max(0) as usize;
            let end_idx = (end as usize).min(lines.len());

            if start_idx >= lines.len() {
                return String::new();
            }

            let mut result = String::new();
            for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
                let line_num = start_idx + i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
            }
            result
        }
        (Some(start), None) => {
            // From line start to EOF (with max_lines limit)
            let start_idx = (start - 1).max(0) as usize;
            if start_idx >= lines.len() {
                return String::new();
            }

            let mut result = String::new();
            let mut count = 0;
            for (i, line) in lines[start_idx..].iter().enumerate() {
                if count >= config.max_lines {
                    let remaining = lines.len() - start_idx - count as usize;
                    result.push_str(&format!("\n... truncated ({remaining} more lines)\n"));
                    break;
                }
                let line_num = start_idx + i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
                count += 1;
            }
            result
        }
        _ => {
            // Full file with line numbers, respecting max_lines
            let mut result = String::new();
            let mut line_count = 0;
            for (i, line) in lines.iter().enumerate() {
                if line_count >= config.max_lines {
                    let remaining = lines.len() - config.max_lines as usize;
                    result.push_str(&format!("\n... truncated ({remaining} more lines)\n"));
                    break;
                }
                let line_num = i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
                line_count += 1;
            }
            result
        }
    }
}

/// Truncate a line if it exceeds the max length.
fn truncate_line(line: &str, max_length: i32) -> String {
    if line.len() > max_length as usize {
        format!("{}...", &line[..max_length as usize])
    } else {
        line.to_string()
    }
}

/// Estimate token count for content.
///
/// Uses ~4 characters per token approximation.
pub fn estimate_tokens(content: &str) -> usize {
    content.len() / 4
}

/// Check if a file should be considered for already-read caching.
///
/// A file is cacheable if:
/// 1. It exists
/// 2. It's a regular file (not a directory)
/// 3. The read would be a full content read (not partial)
pub fn is_cacheable_file(path: &Path, is_partial: bool) -> bool {
    if is_partial {
        return false;
    }

    match fs::metadata(path) {
        Ok(metadata) => metadata.is_file(),
        Err(_) => false,
    }
}

/// Deduplicate file mentions by normalized path.
///
/// When multiple @mentions reference the same file (possibly with different
/// path representations like relative vs absolute), this function returns
/// a unique set of resolved paths.
///
/// # Arguments
///
/// * `mentions` - Iterator of resolved file paths
///
/// # Returns
///
/// A vector of unique paths, preserving the first occurrence order.
pub fn deduplicate_mentions<'a, I>(mentions: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = &'a PathBuf>,
{
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for path in mentions {
        // Normalize the path for comparison
        let normalized = if let Ok(canonical) = path.canonicalize() {
            canonical
        } else {
            path.clone()
        };

        if seen.insert(normalized) {
            result.push(path.clone());
        }
    }

    result
}

/// Check if a mention has a line range (always needs re-read).
///
/// Line range mentions cannot be cached because they represent partial reads.
pub fn has_line_range(line_start: Option<i32>, line_end: Option<i32>) -> bool {
    line_start.is_some() || line_end.is_some()
}

/// Check if a read is cacheable for already-read detection.
///
/// A read is cacheable if:
/// 1. It's a full content read (no line range)
/// 2. It's from a cacheable tool (Read, ReadManyFiles)
/// 3. The file exists and is a regular file
pub fn is_cacheable_read_for_mention(tool_name: &str, path: &Path, has_line_range: bool) -> bool {
    use crate::file_read_tracking_policy::is_full_content_read_tool;

    if has_line_range {
        return false;
    }

    if !is_full_content_read_tool(tool_name) {
        return false;
    }

    is_cacheable_file(path, false)
}

#[cfg(test)]
#[path = "file_context_resolver.test.rs"]
mod tests;
