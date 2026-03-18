//! Test configuration loading from environment variables.
//!
//! This module handles loading test credentials from `.env` files,
//! with support for per-provider configuration and graceful skipping
//! when credentials are not available.
//!
//! # Environment Variable Naming
//!
//! ```text
//! VERCEL_AI_TEST_{PROVIDER}_{FIELD}
//! ```
//!
//! Examples:
//! - `VERCEL_AI_TEST_OPENAI_API_KEY`
//! - `VERCEL_AI_TEST_ANTHROPIC_MODEL`
//! - `VERCEL_AI_TEST_GOOGLE_BASE_URL`
//!
//! # Capability Gating
//!
//! Control which test categories run per provider (or globally):
//!
//! ```text
//! # Global: enable only these capabilities for all providers
//! VERCEL_AI_TEST_CAPABILITIES=text,streaming,tools
//!
//! # Per-provider override (takes precedence over global)
//! VERCEL_AI_TEST_OPENAI_CAPABILITIES=text,streaming,tools,vision
//! VERCEL_AI_TEST_ANTHROPIC_CAPABILITIES=text,tools
//! ```
//!
//! Available capabilities: `text`, `streaming`, `tools`, `vision`, `cross_provider`.
//! If neither env var is set, all capabilities are enabled by default.
//!
//! # .env File Loading Priority
//!
//! 1. Path from `VERCEL_AI_TEST_ENV_FILE` environment variable
//! 2. `.env.test` in crate root
//! 3. `.env` in crate root

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Environment variable for custom .env file path.
const ENV_FILE_VAR: &str = "VERCEL_AI_TEST_ENV_FILE";

/// Default .env file location (relative to crate root).
const DEFAULT_ENV_FILE: &str = ".env.test";

/// Fallback .env file location.
const FALLBACK_ENV_FILE: &str = ".env";

/// Environment variable prefix for test configuration.
const ENV_PREFIX: &str = "VERCEL_AI_TEST";

/// All available test capabilities.
const ALL_CAPABILITIES: &[&str] = &["text", "streaming", "tools", "vision", "cross_provider"];

/// Ensure .env file is loaded exactly once per test run.
static ENV_LOADED: OnceLock<bool> = OnceLock::new();

/// Test configuration for a specific LLM provider.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Provider name (e.g., "openai", "anthropic").
    pub provider: String,
    /// API key for authentication.
    pub api_key: String,
    /// Model name to use.
    pub model: String,
    /// Optional custom endpoint URL.
    pub base_url: Option<String>,
    /// Whether this provider is enabled (has required credentials).
    pub enabled: bool,
    /// Enabled capabilities for this provider.
    pub capabilities: HashSet<String>,
}

impl TestConfig {
    /// Check if a capability is enabled for this provider.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Load .env file once per test run.
fn ensure_env_loaded() {
    ENV_LOADED.get_or_init(|| {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Priority: ENV_FILE_VAR > .env.test > .env
        let env_file = std::env::var(ENV_FILE_VAR)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let test_env = crate_root.join(DEFAULT_ENV_FILE);
                if test_env.exists() {
                    test_env
                } else {
                    crate_root.join(FALLBACK_ENV_FILE)
                }
            });

        if env_file.exists() {
            if dotenvy::from_path(&env_file).is_ok() {
                eprintln!("Loaded test config from: {}", env_file.display());
            }
        } else {
            eprintln!(
                "No .env file found at {}, tests will be skipped",
                env_file.display()
            );
        }

        true
    });
}

/// Get environment variable for a specific provider and field.
fn get_provider_env(provider: &str, field: &str) -> Option<String> {
    let key = format!("{}_{}_{}", ENV_PREFIX, provider.to_uppercase(), field);
    std::env::var(&key).ok().filter(|v| !v.is_empty())
}

/// Parse a comma-separated capabilities string into a HashSet.
fn parse_capabilities(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Load capabilities for a provider.
///
/// Resolution order:
/// 1. `VERCEL_AI_TEST_{PROVIDER}_CAPABILITIES` (per-provider)
/// 2. `VERCEL_AI_TEST_CAPABILITIES` (global)
/// 3. All capabilities enabled (default)
fn load_capabilities(provider: &str) -> HashSet<String> {
    // Per-provider override
    if let Some(caps) = get_provider_env(provider, "CAPABILITIES") {
        return parse_capabilities(&caps);
    }

    // Global setting
    let global_key = format!("{ENV_PREFIX}_CAPABILITIES");
    if let Ok(caps) = std::env::var(&global_key)
        && !caps.is_empty()
    {
        return parse_capabilities(&caps);
    }

    // Default: all capabilities
    ALL_CAPABILITIES.iter().map(|s| (*s).to_string()).collect()
}

/// Load test configuration for a specific provider.
///
/// Returns `None` if the provider is not configured (no API key).
/// Returns `Some(config)` with `enabled = false` if partial config exists.
pub fn load_test_config(provider: &str) -> Option<TestConfig> {
    ensure_env_loaded();

    let api_key = get_provider_env(provider, "API_KEY");
    let model = get_provider_env(provider, "MODEL");
    let base_url = get_provider_env(provider, "BASE_URL");

    // API key is required for a provider to be enabled
    let enabled = api_key.is_some() && model.is_some();

    // Return None if no configuration at all
    if api_key.is_none() && model.is_none() && base_url.is_none() {
        return None;
    }

    let capabilities = load_capabilities(provider);

    Some(TestConfig {
        provider: provider.to_string(),
        api_key: api_key.unwrap_or_default(),
        model: model.unwrap_or_default(),
        base_url,
        enabled,
        capabilities,
    })
}

/// List all providers that are configured (have API keys).
pub fn list_configured_providers() -> Vec<String> {
    ensure_env_loaded();

    let providers = ["openai", "anthropic", "google"];

    providers
        .iter()
        .filter_map(|p| {
            load_test_config(p).and_then(|c| if c.enabled { Some(c.provider) } else { None })
        })
        .collect()
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
