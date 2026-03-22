//! Directory scanner for skill discovery.
//!
//! Walks a directory tree to find skill directories (those containing a
//! `SKILL.md` file). Supports configurable depth limits and detects
//! symlink cycles via canonical path tracking.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;

/// The expected skill file name in each skill directory.
const SKILL_MD: &str = "SKILL.md";

/// Scans directory trees for skill directories.
///
/// Walks each root directory looking for directories that contain a
/// `SKILL.md` file. Symlink cycles are detected by tracking canonical
/// paths. Errors during scanning (e.g., permission denied) are logged
/// and skipped.
pub struct SkillScanner {
    /// Maximum depth to walk into the directory tree.
    pub max_scan_depth: i32,

    /// Maximum number of skill directories to discover per root.
    pub max_skills_dirs_per_root: i32,
}

impl Default for SkillScanner {
    fn default() -> Self {
        Self {
            max_scan_depth: 6,
            max_skills_dirs_per_root: 2000,
        }
    }
}

impl SkillScanner {
    /// Creates a new scanner with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans a single root directory for skill directories.
    ///
    /// Returns a list of absolute paths to directories containing `SKILL.md`.
    /// Symlink cycles are detected and skipped. Errors are logged but do not
    /// cause the scan to abort.
    pub fn scan(&self, root: &Path) -> Vec<PathBuf> {
        let mut results = Vec::new();
        let mut seen_canonical = HashSet::<PathBuf>::new();

        // Convert max_scan_depth to usize for walkdir; clamp negative values to 0
        let depth = self.max_scan_depth.max(0) as usize;
        let max_results = self.max_skills_dirs_per_root.max(0) as usize;

        let walker = WalkDir::new(root)
            .max_depth(depth)
            .follow_links(true)
            .into_iter();

        for entry in walker {
            if results.len() >= max_results {
                tracing::warn!(
                    root = %root.display(),
                    limit = self.max_skills_dirs_per_root,
                    "reached skill directory scan limit, stopping"
                );
                break;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        "skipping inaccessible entry during skill scan"
                    );
                    continue;
                }
            };

            // Only process directories
            if !entry.file_type().is_dir() {
                continue;
            }

            let dir_path = entry.path();

            // Check for symlink cycles by tracking canonical paths
            match dir_path.canonicalize() {
                Ok(canonical) => {
                    if !seen_canonical.insert(canonical) {
                        tracing::debug!(
                            path = %dir_path.display(),
                            "skipping symlink cycle"
                        );
                        continue;
                    }
                }
                Err(err) => {
                    tracing::debug!(
                        path = %dir_path.display(),
                        error = %err,
                        "failed to canonicalize path, skipping"
                    );
                    continue;
                }
            }

            // Check if this directory contains SKILL.md
            let skill_md = dir_path.join(SKILL_MD);
            if skill_md.is_file() {
                results.push(dir_path.to_path_buf());
            }
        }

        results
    }

    /// Scans multiple root directories for skill directories.
    ///
    /// Results from all roots are concatenated. Duplicates across roots
    /// are not removed here; use [`crate::dedup`] for that.
    pub fn scan_roots(&self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut all = Vec::new();
        for root in roots {
            if root.is_dir() {
                let found = self.scan(root);
                tracing::debug!(
                    root = %root.display(),
                    count = found.len(),
                    "scanned root for skills"
                );
                all.extend(found);
            } else {
                tracing::debug!(
                    root = %root.display(),
                    "skill scan root does not exist or is not a directory"
                );
            }
        }
        all
    }
}

#[cfg(test)]
#[path = "scanner.test.rs"]
mod tests;
