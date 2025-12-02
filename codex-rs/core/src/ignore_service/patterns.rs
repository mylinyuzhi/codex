//! Default ignore patterns for file operations.
//!
//! Provides common patterns aligned with gemini-cli for consistent
//! file filtering across agent tools.

/// Common ignore patterns for basic file exclusions.
/// These directories are typically ignored in development projects.
pub const COMMON_IGNORE_PATTERNS: &[&str] = &[
    "**/node_modules/**",
    "**/.git/**",
    "**/bower_components/**",
    "**/.svn/**",
    "**/.hg/**",
];

/// Binary file extension patterns typically excluded from text processing.
pub const BINARY_FILE_PATTERNS: &[&str] = &[
    "**/*.bin",
    "**/*.exe",
    "**/*.dll",
    "**/*.so",
    "**/*.dylib",
    "**/*.class",
    "**/*.jar",
    "**/*.war",
    "**/*.zip",
    "**/*.tar",
    "**/*.gz",
    "**/*.bz2",
    "**/*.rar",
    "**/*.7z",
    "**/*.doc",
    "**/*.docx",
    "**/*.xls",
    "**/*.xlsx",
    "**/*.ppt",
    "**/*.pptx",
];

/// Common directory patterns typically ignored in development.
pub const COMMON_DIRECTORY_EXCLUDES: &[&str] = &[
    "**/.vscode/**",
    "**/.idea/**",
    "**/dist/**",
    "**/build/**",
    "**/coverage/**",
    "**/__pycache__/**",
];

/// System and environment file patterns.
pub const SYSTEM_FILE_EXCLUDES: &[&str] = &["**/.DS_Store", "**/.env"];

/// Get all default exclude patterns combined.
pub fn get_all_default_excludes() -> Vec<&'static str> {
    let mut patterns = Vec::new();
    patterns.extend(COMMON_IGNORE_PATTERNS);
    patterns.extend(BINARY_FILE_PATTERNS);
    patterns.extend(COMMON_DIRECTORY_EXCLUDES);
    patterns.extend(SYSTEM_FILE_EXCLUDES);
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_patterns_not_empty() {
        assert!(!COMMON_IGNORE_PATTERNS.is_empty());
    }

    #[test]
    fn test_binary_patterns_not_empty() {
        assert!(!BINARY_FILE_PATTERNS.is_empty());
    }

    #[test]
    fn test_get_all_default_excludes() {
        let all = get_all_default_excludes();
        assert!(all.len() > 10);
        assert!(all.contains(&"**/node_modules/**"));
        assert!(all.contains(&"**/*.exe"));
    }
}
