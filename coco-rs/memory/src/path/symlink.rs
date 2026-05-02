//! Symlink-aware path resolution for write-validation.
//!
//! TS: `memdir/teamMemPaths.ts:realpathDeepestExisting`. Walks up the
//! ancestry until canonicalize succeeds, then re-appends the dropped
//! components. Lets us validate paths whose tail doesn't exist yet
//! (we're about to create them) without choking on partial trees.

use std::path::Path;
use std::path::PathBuf;

/// Resolve symlinks for the deepest existing ancestor of `path`.
///
/// If `path` itself exists, returns its canonical form. Otherwise walks
/// `parent()` until canonicalize succeeds and re-appends the leaf
/// components. Returns `None` only when no ancestor is canonicalizable
/// (effectively never on a normal filesystem).
pub fn realpath_deepest_existing(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }
    let mut remaining: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    loop {
        let parent = cursor.parent()?;
        if let Some(name) = cursor.file_name() {
            remaining.push(name);
        }
        if let Ok(real) = parent.canonicalize() {
            let mut out = real;
            for component in remaining.iter().rev() {
                out.push(component);
            }
            return Some(out);
        }
        cursor = parent;
    }
}

#[cfg(test)]
#[path = "symlink.test.rs"]
mod tests;
