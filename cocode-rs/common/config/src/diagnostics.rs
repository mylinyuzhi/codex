//! Rich error diagnostics for JSON configuration files.
//!
//! Provides gcc-style annotated error messages with file path, line, column,
//! and caret annotations when JSON config files fail to parse or validate.
//!
//! # Example output
//!
//! ```text
//! ~/.cocode/config.json:5:10: unknown field "modl", expected "models"
//!     |
//!   5 |   "modl": {
//!     |    ^^^^
//! ```
//!
//! Inspired by codex-rs's config diagnostics system.

use std::path::Path;
use std::path::PathBuf;

/// 1-based position in a text file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: usize,
    pub column: usize,
}

/// Range in a text file (start is inclusive, end is inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

/// A config error with precise source location.
#[derive(Debug, Clone)]
pub struct ConfigDiagnostic {
    pub path: PathBuf,
    pub range: TextRange,
    pub message: String,
    /// The serde path to the error (e.g., `.models.main`).
    pub serde_path: String,
}

/// Deserialize JSON with path-aware error reporting.
///
/// On success, returns the deserialized value. On failure, returns a
/// `ConfigDiagnostic` with precise location info including the serde path.
pub fn deserialize_json_with_diagnostics<T: serde::de::DeserializeOwned>(
    contents: &str,
    path: &Path,
) -> Result<T, ConfigDiagnostic> {
    let deserializer = &mut serde_json::Deserializer::from_str(contents);
    serde_path_to_error::deserialize(deserializer).map_err(|err| {
        let serde_path = err.path().to_string();
        let inner = err.inner();

        // serde_json::Error provides 1-based line and column directly.
        let line = inner.line();
        let column = inner.column();

        let message = if serde_path == "." {
            inner.to_string()
        } else {
            format!("at {serde_path}: {inner}")
        };

        ConfigDiagnostic {
            path: path.to_path_buf(),
            range: TextRange {
                start: TextPosition { line, column },
                end: TextPosition { line, column },
            },
            message,
            serde_path,
        }
    })
}

/// Format a diagnostic as a gcc-style annotated error message.
///
/// Produces output like:
/// ```text
/// path/to/file.json:5:10: error message
///     |
///   5 |   "modl": {
///     |    ^^^^
/// ```
pub fn format_diagnostic(diag: &ConfigDiagnostic, contents: &str) -> String {
    let mut output = String::new();

    // Header: path:line:column: message
    let path_display = diag.path.display();
    let line = diag.range.start.line;
    let col = diag.range.start.column;
    output.push_str(&format!("{path_display}:{line}:{col}: {}\n", diag.message));

    // Try to extract the error line from contents.
    let lines: Vec<&str> = contents.lines().collect();
    let line_idx = line.saturating_sub(1);
    if line_idx < lines.len() {
        let source_line = lines[line_idx];
        let line_num_width = format!("{line}").len();
        let gutter_pad = " ".repeat(line_num_width + 1);

        // Gutter line
        output.push_str(&format!("{gutter_pad}|\n"));

        // Source line
        output.push_str(&format!("{line:>line_num_width$} | {source_line}\n"));

        // Caret line
        if col > 0 {
            let caret_offset = col - 1; // Convert 1-based column to 0-based offset
            let highlight_len = compute_highlight_len(source_line, caret_offset);
            let carets = "^".repeat(highlight_len.max(1));
            let leading = " ".repeat(caret_offset);
            output.push_str(&format!("{gutter_pad}| {leading}{carets}\n"));
        }
    }

    output
}

/// Compute the length of the highlight region starting at `offset` in a line.
///
/// Tries to highlight the current JSON token (string key, number, etc.).
fn compute_highlight_len(line: &str, offset: usize) -> usize {
    let bytes = line.as_bytes();
    if offset >= bytes.len() {
        return 1;
    }

    let start_byte = bytes[offset];
    match start_byte {
        // Quoted string: highlight until closing quote
        b'"' => {
            let rest = &bytes[offset + 1..];
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == b'\\' {
                    i += 2; // skip escaped character
                } else if rest[i] == b'"' {
                    return i + 2; // include both quotes
                } else {
                    i += 1;
                }
            }
            1
        }
        // Number or alphanumeric: highlight contiguous chars
        b'0'..=b'9' | b'-' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.' => {
            let rest = &bytes[offset..];
            rest.iter()
                .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
                .count()
                .max(1)
        }
        _ => 1,
    }
}

#[cfg(test)]
#[path = "diagnostics.test.rs"]
mod tests;
