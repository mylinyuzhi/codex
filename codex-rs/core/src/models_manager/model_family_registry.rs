//! Global registry for model family configurations.
//!
//! This module provides a registry that combines code-defined model families
//! with user-defined families from `~/.codex/model_families.toml`.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;

use super::model_family::ModelFamily;
use super::model_family_config::ModelFamilyConfig;

/// Name of the model families configuration file.
pub const MODEL_FAMILIES_TOML: &str = "model_families.toml";

/// Global model family registry.
static FAMILY_REGISTRY: OnceLock<RwLock<ModelFamilyRegistry>> = OnceLock::new();

/// Registry for model family configurations.
///
/// Combines user-defined families from TOML with code-defined defaults.
#[derive(Debug)]
pub struct ModelFamilyRegistry {
    /// User-configured families loaded from model_families.toml.
    user_families: HashMap<String, ModelFamilyConfig>,
    /// Path to codex home directory for resolving relative paths.
    codex_home: PathBuf,
}

impl Default for ModelFamilyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelFamilyRegistry {
    /// Get the global registry instance.
    pub fn global() -> &'static RwLock<Self> {
        FAMILY_REGISTRY.get_or_init(|| RwLock::new(Self::new()))
    }

    /// Create a new empty registry.
    fn new() -> Self {
        Self {
            user_families: HashMap::new(),
            codex_home: PathBuf::new(),
        }
    }

    /// Load user-defined model families from configuration file.
    ///
    /// Reads `~/.codex/model_families.toml` and populates the registry.
    /// Does nothing if the file doesn't exist.
    pub fn load_from_file(&mut self, codex_home: &Path) -> std::io::Result<()> {
        self.codex_home = codex_home.to_path_buf();
        let config_path = codex_home.join(MODEL_FAMILIES_TOML);

        if !config_path.exists() {
            tracing::debug!(
                "Model families config not found at {}",
                config_path.display()
            );
            return Ok(());
        }

        let content = std::fs::read_to_string(&config_path)?;
        let families: HashMap<String, ModelFamilyConfig> = toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        tracing::info!(
            "Loaded {} custom model families from {}",
            families.len(),
            config_path.display()
        );

        self.user_families = families;
        Ok(())
    }

    /// Resolve a model family by ID.
    ///
    /// Resolution priority:
    /// 1. User-defined family from model_families.toml
    /// 2. Code-defined family via `find_family_for_model()`
    /// 3. Default fallback via `derive_default_model_family()`
    pub fn resolve(&self, family_id: &str) -> ModelFamily {
        // Check user-configured families first
        if let Some(config) = self.user_families.get(family_id) {
            return self.build_from_config(family_id, config);
        }

        // Fall back to code-defined families
        super::model_family::find_family_for_model(family_id)
    }

    /// Build a ModelFamily from user configuration.
    fn build_from_config(&self, id: &str, config: &ModelFamilyConfig) -> ModelFamily {
        // Start with default family
        let mut family = super::model_family::derive_default_model_family(id);

        // Apply user configuration overrides
        family.slug = id.to_string();
        family.family = id.to_string();

        if let Some(context_window) = config.context_window {
            family.context_window = Some(context_window);
        }

        if let Some(auto_compact) = config.auto_compact_token_limit {
            family.auto_compact_token_limit = Some(auto_compact);
        }

        if config.supports_reasoning_summaries {
            family.supports_reasoning_summaries = true;
        }

        if config.supports_parallel_tool_calls {
            family.supports_parallel_tool_calls = true;
        }

        if let Some(effort) = &config.default_reasoning_effort {
            family.default_reasoning_effort = Some(effort.clone());
        }

        // Resolve base_instructions (inline or file)
        if let Some(instructions) = config.resolve_base_instructions(&self.codex_home) {
            family.base_instructions = instructions;
        }

        family
    }

    /// Check if a family ID exists in user configuration.
    pub fn has_user_family(&self, family_id: &str) -> bool {
        self.user_families.contains_key(family_id)
    }
}

/// Resolve a model family by ID using the global registry.
///
/// This is the main entry point for model family resolution.
///
/// # Resolution Priority
/// 1. User-defined family from `~/.codex/model_families.toml`
/// 2. Code-defined family via `find_family_for_model()`
/// 3. Default fallback
pub fn resolve_model_family(family_id: &str) -> ModelFamily {
    ModelFamilyRegistry::global()
        .read()
        .expect("model family registry lock poisoned")
        .resolve(family_id)
}

/// Initialize the global registry with families from the config file.
///
/// Should be called early during application startup.
pub fn init_registry(codex_home: &Path) -> std::io::Result<()> {
    let mut registry = ModelFamilyRegistry::global()
        .write()
        .expect("model family registry lock poisoned");
    registry.load_from_file(codex_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_falls_back_to_code_defined() {
        let registry = ModelFamilyRegistry::new();

        // Should fall back to code-defined family
        let family = registry.resolve("gpt-5.1-codex-max");
        // The model_family! macro sets both slug and family to the same value
        assert_eq!(family.slug, "gpt-5.1-codex-max");
        // Code-defined gpt-5.1-codex-max has reasoning summaries enabled
        assert!(family.supports_reasoning_summaries);
    }

    #[test]
    fn test_user_config_takes_precedence() {
        let codex_home = tempdir().unwrap();

        // Write a user config
        let config_content = r#"
[custom-model]
display_name = "Custom Model"
context_window = 32000
supports_reasoning_summaries = true
base_instructions = "Custom instructions"
"#;
        std::fs::write(codex_home.path().join(MODEL_FAMILIES_TOML), config_content).unwrap();

        let mut registry = ModelFamilyRegistry::new();
        registry.load_from_file(codex_home.path()).unwrap();

        let family = registry.resolve("custom-model");
        assert_eq!(family.slug, "custom-model");
        assert_eq!(family.context_window, Some(32000));
        assert!(family.supports_reasoning_summaries);
        assert_eq!(family.base_instructions, "Custom instructions");
    }

    #[test]
    fn test_missing_config_file_is_ok() {
        let codex_home = tempdir().unwrap();
        let mut registry = ModelFamilyRegistry::new();

        // Should not error if file doesn't exist
        let result = registry.load_from_file(codex_home.path());
        assert!(result.is_ok());
        assert!(registry.user_families.is_empty());
    }
}
