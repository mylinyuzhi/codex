//! File ignore service for consistent file filtering.

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;

use crate::config::IgnoreConfig;

/// Ignore file names (ripgrep native support)
pub const IGNORE_FILES: &[&str] = &[".ignore"];
use crate::patterns::BINARY_FILE_PATTERNS;
use crate::patterns::COMMON_DIRECTORY_EXCLUDES;
use crate::patterns::COMMON_IGNORE_PATTERNS;
use crate::patterns::SYSTEM_FILE_EXCLUDES;

/// Service for handling file ignore patterns.
///
/// Provides consistent file filtering behavior across all file-related
/// operations (glob, list_dir, grep, file_search, etc.).
///
/// # Example
///
/// ```rust,no_run
/// use cocode_file_ignore::{IgnoreService, IgnoreConfig};
/// use std::path::Path;
///
/// let service = IgnoreService::with_defaults();
/// let walker = service.create_walk_builder(Path::new("."));
///
/// for entry in walker.build() {
///     match entry {
///         Ok(e) => println!("{}", e.path().display()),
///         Err(e) => eprintln!("Error: {}", e),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct IgnoreService {
    config: IgnoreConfig,
}

impl IgnoreService {
    /// Create a new service with the given configuration.
    pub fn new(config: IgnoreConfig) -> Self {
        Self { config }
    }

    /// Create a new service with default configuration.
    ///
    /// Defaults:
    /// - Respects `.gitignore` files
    /// - Respects `.ignore` files (ripgrep native support)
    /// - Excludes hidden files
    /// - Does not follow symlinks
    pub fn with_defaults() -> Self {
        Self::new(IgnoreConfig::default())
    }

    /// Create a WalkBuilder with all ignore rules applied.
    ///
    /// The returned WalkBuilder is configured to:
    /// - Respect `.gitignore` if enabled
    /// - Respect `.ignore` if enabled (ripgrep native support)
    /// - Handle hidden files according to config
    /// - Apply custom exclude patterns
    ///
    /// # Arguments
    ///
    /// * `root` - The root directory to start walking from
    ///
    /// # Returns
    ///
    /// A configured `WalkBuilder` ready for traversal.
    pub fn create_walk_builder(&self, root: &Path) -> WalkBuilder {
        let mut builder = WalkBuilder::new(root);

        // Configure gitignore handling
        if self.config.respect_gitignore {
            builder.git_ignore(true).git_global(true).git_exclude(true);
        } else {
            builder
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false);
        }

        // Configure .ignore files (ripgrep native support)
        if self.config.respect_ignore {
            builder.add_custom_ignore_filename(".ignore");
        }

        // Configure hidden files and symlinks
        builder
            .hidden(!self.config.include_hidden)
            .follow_links(self.config.follow_links)
            .require_git(false); // Don't require git repo

        // Apply custom exclude patterns
        if !self.config.custom_excludes.is_empty()
            && let Ok(overrides) = self.build_overrides(root)
        {
            builder.overrides(overrides);
        }

        builder
    }

    /// Build override matcher for custom exclude patterns.
    fn build_overrides(&self, root: &Path) -> Result<ignore::overrides::Override, ignore::Error> {
        let mut override_builder = OverrideBuilder::new(root);
        for pattern in &self.config.custom_excludes {
            // Prefix with ! for exclusion in override syntax
            override_builder.add(&format!("!{pattern}"))?;
        }
        override_builder.build()
    }

    /// Get common ignore patterns for basic operations.
    ///
    /// Returns patterns like `**/node_modules/**`, `**/.git/**`, etc.
    pub fn get_core_patterns() -> &'static [&'static str] {
        COMMON_IGNORE_PATTERNS
    }

    /// Get all default exclude patterns combined.
    ///
    /// Includes:
    /// - Common ignore patterns (node_modules, .git, etc.)
    /// - Binary file patterns (*.exe, *.dll, etc.)
    /// - Common directory excludes (dist, build, etc.)
    /// - System file excludes (.DS_Store, etc.)
    pub fn get_default_excludes() -> Vec<&'static str> {
        let mut patterns = Vec::new();
        patterns.extend(COMMON_IGNORE_PATTERNS);
        patterns.extend(BINARY_FILE_PATTERNS);
        patterns.extend(COMMON_DIRECTORY_EXCLUDES);
        patterns.extend(SYSTEM_FILE_EXCLUDES);
        patterns
    }

    /// Get the current configuration.
    pub fn config(&self) -> &IgnoreConfig {
        &self.config
    }
}

impl Default for IgnoreService {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Find all .ignore files for a given root path.
///
/// This is useful for external tools that need explicit file paths
/// rather than built-in ignore handling.
///
/// Note: ripgrep natively supports .ignore files, so this function
/// is typically not needed when using rg directly.
///
/// Searches:
/// 1. UP - from root through parent directories (for project-level ignores)
/// 2. DOWN - into subdirectories (for nested ignores like src/.ignore)
///
/// # Arguments
///
/// * `root` - The root directory to search from
///
/// # Returns
///
/// A vector of paths to found ignore files.
pub fn find_ignore_files(root: &Path) -> Vec<PathBuf> {
    let mut ignore_files = Vec::new();

    // 1. Walk UP to parent directories (for project-level ignores)
    // Stop at git root or max depth to avoid walking all the way to filesystem root
    const MAX_PARENT_DEPTH: usize = 20;
    let mut current = Some(root.to_path_buf());
    let mut depth = 0;
    while let Some(dir) = current {
        for name in IGNORE_FILES {
            let path = dir.join(name);
            if path.exists() {
                ignore_files.push(path);
            }
        }
        depth += 1;
        // Stop at git root or max depth
        if depth >= MAX_PARENT_DEPTH || dir.join(".git").exists() {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }

    // 2. Walk DOWN into subdirectories (for nested ignores)
    if root.is_dir() {
        for entry in WalkDir::new(root)
            .max_depth(10) // Limit depth to avoid performance issues
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();
                if IGNORE_FILES.iter().any(|&n| n == name) {
                    let path = entry.path().to_path_buf();
                    // Avoid duplicates (root was already added in step 1)
                    if !ignore_files.contains(&path) {
                        ignore_files.push(path);
                    }
                }
            }
        }
    }

    ignore_files
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
