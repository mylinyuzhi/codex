//! Agent ignore service for consistent file filtering.
//!
//! Provides a shared service for handling agent-specific ignore files
//! (`.agentignore`, `.agentsignore`) along with standard `.gitignore` support.

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::path::Path;

use super::patterns::BINARY_FILE_PATTERNS;
use super::patterns::COMMON_DIRECTORY_EXCLUDES;
use super::patterns::COMMON_IGNORE_PATTERNS;
use super::patterns::SYSTEM_FILE_EXCLUDES;

/// Configuration for ignore behavior.
#[derive(Debug, Clone)]
pub struct IgnoreConfig {
    /// Whether to respect .gitignore files (default: true)
    pub respect_gitignore: bool,
    /// Whether to respect .agentignore/.agentsignore files (default: true)
    pub respect_agent_ignore: bool,
    /// Whether to include hidden files (default: false)
    pub include_hidden: bool,
    /// Whether to follow symbolic links (default: false)
    pub follow_links: bool,
    /// Additional custom exclude patterns
    pub custom_excludes: Vec<String>,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            respect_agent_ignore: true,
            include_hidden: false,
            follow_links: false,
            custom_excludes: Vec::new(),
        }
    }
}

/// Shared service for handling agent ignore patterns.
///
/// This service provides consistent file filtering behavior across
/// all file-related tools (glob_files, list_dir, grep_files, etc.).
#[derive(Debug)]
pub struct AgentIgnoreService {
    config: IgnoreConfig,
}

impl AgentIgnoreService {
    /// Create a new service with the given configuration.
    pub fn new(config: IgnoreConfig) -> Self {
        Self { config }
    }

    /// Create a new service with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(IgnoreConfig::default())
    }

    /// Create a WalkBuilder with all ignore rules applied.
    ///
    /// The WalkBuilder is configured to:
    /// - Respect .gitignore if enabled
    /// - Respect .agentignore and .agentsignore if enabled
    /// - Apply custom exclude patterns
    pub fn create_walk_builder(&self, root: &Path) -> WalkBuilder {
        let mut builder = WalkBuilder::new(root);

        // Configure gitignore
        if self.config.respect_gitignore {
            builder.git_ignore(true).git_global(true).git_exclude(true);
        } else {
            builder
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false);
        }

        // Configure agent ignore files (.agentignore, .agentsignore)
        if self.config.respect_agent_ignore {
            builder.add_custom_ignore_filename(".agentignore");
            builder.add_custom_ignore_filename(".agentsignore");
        }

        // Configure hidden files and symlinks
        builder
            .hidden(!self.config.include_hidden)
            .follow_links(self.config.follow_links)
            .require_git(false);

        // Apply custom excludes via OverrideBuilder
        if !self.config.custom_excludes.is_empty() {
            if let Ok(overrides) = self.build_overrides(root) {
                builder.overrides(overrides);
            }
        }

        builder
    }

    /// Build override matcher for custom excludes.
    fn build_overrides(&self, root: &Path) -> Result<ignore::overrides::Override, ignore::Error> {
        let mut override_builder = OverrideBuilder::new(root);
        for pattern in &self.config.custom_excludes {
            // Prefix with ! for exclusion
            override_builder.add(&format!("!{pattern}"))?;
        }
        override_builder.build()
    }

    /// Get core ignore patterns for basic operations.
    pub fn get_core_patterns(&self) -> Vec<&'static str> {
        COMMON_IGNORE_PATTERNS.to_vec()
    }

    /// Get all default exclude patterns.
    pub fn get_default_excludes(&self) -> Vec<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = IgnoreConfig::default();
        assert!(config.respect_gitignore);
        assert!(config.respect_agent_ignore);
        assert!(!config.include_hidden);
        assert!(!config.follow_links);
        assert!(config.custom_excludes.is_empty());
    }

    #[test]
    fn test_service_with_defaults() {
        let service = AgentIgnoreService::with_defaults();
        assert!(service.config().respect_gitignore);
        assert!(service.config().respect_agent_ignore);
    }

    #[test]
    fn test_get_core_patterns() {
        let service = AgentIgnoreService::with_defaults();
        let patterns = service.get_core_patterns();
        assert!(patterns.contains(&"**/node_modules/**"));
        assert!(patterns.contains(&"**/.git/**"));
    }

    #[test]
    fn test_get_default_excludes() {
        let service = AgentIgnoreService::with_defaults();
        let excludes = service.get_default_excludes();
        assert!(excludes.len() > 10);
        assert!(excludes.contains(&"**/*.exe"));
        assert!(excludes.contains(&"**/.DS_Store"));
    }

    #[test]
    fn test_create_walk_builder() {
        let temp = tempdir().expect("create temp dir");
        let service = AgentIgnoreService::with_defaults();
        let _builder = service.create_walk_builder(temp.path());
        // Just verify it doesn't panic
    }

    #[test]
    fn test_custom_excludes() {
        let temp = tempdir().expect("create temp dir");
        let config = IgnoreConfig {
            custom_excludes: vec!["*.log".to_string(), "*.tmp".to_string()],
            ..Default::default()
        };
        let service = AgentIgnoreService::new(config);
        let _builder = service.create_walk_builder(temp.path());
        // Just verify it doesn't panic
    }
}
