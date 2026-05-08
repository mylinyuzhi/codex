//! Memory path validation — traversal, null bytes, UNC, drive root,
//! tilde, fullwidth Unicode, URL-encoded traversal.
//!
//! TS: `memdir/paths.ts:validateMemoryPath` + `sanitizePathKey`.

use std::path::Path;
use std::path::PathBuf;

/// Validation error for memory paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathValidationError {
    #[error("path is empty")]
    Empty,
    #[error("path contains null bytes")]
    NullByte,
    #[error("path contains '..' traversal")]
    Traversal,
    #[error("path must be relative")]
    AbsolutePath,
    #[error("UNC paths are not allowed")]
    UncPath,
    #[error("Windows drive-root paths are not allowed")]
    DriveRoot,
    #[error("path contains a bare or non-home tilde")]
    Tilde,
    #[error("path contains fullwidth or other Unicode-traversal characters")]
    UnicodeTraversal,
    #[error("path escapes the memory directory")]
    Escape,
}

impl coco_error::StackError for PathValidationError {
    fn debug_fmt(&self, layer: usize, buf: &mut Vec<String>) {
        buf.push(format!("{layer}: {self}"));
    }

    fn next(&self) -> Option<&dyn coco_error::StackError> {
        None
    }
}

impl coco_error::ErrorExt for PathValidationError {
    fn status_code(&self) -> coco_error::StatusCode {
        coco_error::StatusCode::InvalidArguments
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Validate a relative memory file path.
///
/// Rejects: empty, null bytes, UNC (`\\server\share`), absolute paths,
/// Windows drive roots (`C:\foo` / `C:foo`), bare/relative tilde, `..`
/// traversal (literal, URL-encoded `%2e%2e`, fullwidth `．．`), and
/// fullwidth solidus `／`.
pub fn validate_memory_path(path: &str) -> Result<(), PathValidationError> {
    if path.is_empty() {
        return Err(PathValidationError::Empty);
    }
    if path.contains('\0') {
        return Err(PathValidationError::NullByte);
    }
    if path.starts_with("\\\\") {
        return Err(PathValidationError::UncPath);
    }
    if path.starts_with('/') {
        return Err(PathValidationError::AbsolutePath);
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return Err(PathValidationError::DriveRoot);
    }
    // Tilde: TS only accepts `~/` and `~\\`. Bare `~` and `~/..` are out.
    if path.starts_with('~') && !path.starts_with("~/") && !path.starts_with("~\\") {
        return Err(PathValidationError::Tilde);
    }
    if path.contains('\u{FF0E}') || path.contains('\u{FF0F}') {
        return Err(PathValidationError::UnicodeTraversal);
    }
    let decoded = path
        .replace("%2e", ".")
        .replace("%2E", ".")
        .replace("%2f", "/")
        .replace("%2F", "/");
    for component in decoded.split(['/', '\\']) {
        if component == ".." {
            return Err(PathValidationError::Traversal);
        }
    }
    Ok(())
}

/// Lexically resolve `path` against `memory_dir` and verify the result
/// stays inside the memory dir. No filesystem access — symlink-aware
/// validation is in [`super::symlink`].
pub fn validate_resolved_path(
    path: &Path,
    memory_dir: &Path,
) -> Result<PathBuf, PathValidationError> {
    let resolved = memory_dir.join(path);
    let normalized = lexical_normalize(&resolved);
    let mem_normalized = lexical_normalize(memory_dir);
    if !normalized.starts_with(&mem_normalized) {
        return Err(PathValidationError::Escape);
    }
    Ok(normalized)
}

/// Predicate variant: is `path` already within `memory_dir`?
pub fn is_within_memory_dir(path: &Path, memory_dir: &Path) -> bool {
    lexical_normalize(path).starts_with(lexical_normalize(memory_dir))
}

/// Sanitize a free-form string for use as a memory file basename.
pub fn sanitize_path_key(key: &str) -> String {
    key.to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                Some(c)
            } else if c == ' ' {
                Some('_')
            } else {
                None
            }
        })
        .collect()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "validate.test.rs"]
mod tests;
