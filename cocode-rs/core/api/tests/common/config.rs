//! Test configuration loading from environment variables.
//!
//! Loads provider configuration from `.env` files to construct `ProviderInfo`
//! for live integration tests of the core/api layer.
//!
//! # Configuration Modes
//!
//! ## Flat env vars (simple)
//! ```text
//! COCODE_API_TEST_{PROVIDER}_API_KEY=sk-xxx
//! COCODE_API_TEST_{PROVIDER}_MODEL=gpt-4o-mini
//! COCODE_API_TEST_{PROVIDER}_BASE_URL=https://api.openai.com/v1
//! COCODE_API_TEST_{PROVIDER}_WIRE_API=responses
//! ```
//!
//! ## Full JSON (advanced)
//! ```text
//! COCODE_API_TEST_{PROVIDER}_CONFIG={"name":"openai","api":"openai",...}
//! ```
//!
//! # Capability Gating
//!
//! ```text
//! COCODE_API_TEST_CAPABILITIES=text,streaming,tools         # global
//! COCODE_API_TEST_OPENAI_CAPABILITIES=text,streaming,tools  # per-provider
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use cocode_protocol::ProviderApi;
use cocode_protocol::ProviderInfo;
use cocode_protocol::WireApi;

/// Environment variable prefix.
const ENV_PREFIX: &str = "COCODE_API_TEST";

/// All available test capabilities.
const ALL_CAPABILITIES: &[&str] = &["text", "streaming", "tools", "cross_provider"];

/// Ensure .env file is loaded exactly once.
static ENV_LOADED: OnceLock<bool> = OnceLock::new();

/// Test configuration for a provider, wrapping a full `ProviderInfo`.
#[derive(Debug, Clone)]
pub struct ProviderTestConfig {
    /// Provider name identifier (e.g., "openai", "anthropic").
    pub provider: String,
    /// Complete runtime provider configuration.
    pub provider_info: ProviderInfo,
    /// Model slug to test with.
    pub model_slug: String,
    /// Whether this provider is enabled (has API key + model).
    pub enabled: bool,
    /// Enabled test capabilities.
    pub capabilities: HashSet<String>,
}

impl ProviderTestConfig {
    /// Check if a capability is enabled.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Load .env file once per test run.
fn ensure_env_loaded() {
    ENV_LOADED.get_or_init(|| {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        let env_file = std::env::var(format!("{ENV_PREFIX}_ENV_FILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let test_env = crate_root.join(".env.test");
                if test_env.exists() {
                    test_env
                } else {
                    crate_root.join(".env")
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

/// Get env var for a specific provider and field.
fn get_env(provider: &str, field: &str) -> Option<String> {
    let key = format!("{}_{}_{}", ENV_PREFIX, provider.to_uppercase(), field);
    std::env::var(&key).ok().filter(|v| !v.is_empty())
}

/// Map virtual provider names to config source.
fn config_provider_name(provider: &str) -> &str {
    match provider {
        "openai_chat" => "openai",
        other => other,
    }
}

/// Get env var with fallback to mapped provider.
fn get_env_with_fallback(provider: &str, field: &str) -> Option<String> {
    if let Some(val) = get_env(provider, field) {
        return Some(val);
    }
    let config_name = config_provider_name(provider);
    if config_name != provider {
        return get_env(config_name, field);
    }
    None
}

/// Parse comma-separated capabilities.
fn parse_capabilities(value: &str) -> HashSet<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return HashSet::new();
    }
    value
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Load capabilities for a provider.
fn load_capabilities(provider: &str) -> HashSet<String> {
    // Per-provider override
    let key = format!("{}_{}_CAPABILITIES", ENV_PREFIX, provider.to_uppercase());
    if let Ok(caps) = std::env::var(&key) {
        return parse_capabilities(&caps);
    }

    // Mapped provider fallback
    let config_name = config_provider_name(provider);
    if config_name != provider
        && let Some(caps) = get_env(config_name, "CAPABILITIES")
    {
        return parse_capabilities(&caps);
    }

    // Global
    let global_key = format!("{ENV_PREFIX}_CAPABILITIES");
    if let Ok(caps) = std::env::var(&global_key)
        && !caps.is_empty()
    {
        return parse_capabilities(&caps);
    }

    // Default: all capabilities
    ALL_CAPABILITIES.iter().map(|s| (*s).to_string()).collect()
}

/// Map provider name to ProviderApi.
fn provider_api_for(provider: &str) -> ProviderApi {
    match provider {
        "openai" | "openai_chat" => ProviderApi::Openai,
        "anthropic" => ProviderApi::Anthropic,
        "gemini" => ProviderApi::Gemini,
        "volcengine" => ProviderApi::Volcengine,
        "zai" => ProviderApi::Zai,
        _ => ProviderApi::OpenaiCompat,
    }
}

/// Default base URL for a provider api.
fn default_base_url(provider_api: ProviderApi) -> &'static str {
    match provider_api {
        ProviderApi::Openai => "https://api.openai.com/v1",
        ProviderApi::Anthropic => "https://api.anthropic.com",
        ProviderApi::Gemini => "https://generativelanguage.googleapis.com",
        ProviderApi::Volcengine => "https://ark.cn-beijing.volces.com/api/v3",
        ProviderApi::Zai => "https://api.z.ai/api/paas/v4",
        ProviderApi::OpenaiCompat => "",
    }
}

/// Load test configuration for a specific provider.
///
/// Returns `None` if no configuration exists at all.
pub fn load_provider_config(provider: &str) -> Option<ProviderTestConfig> {
    ensure_env_loaded();

    let capabilities = load_capabilities(provider);

    // Mode 1: Full JSON config
    if let Some(json_str) = get_env_with_fallback(provider, "CONFIG") {
        return load_from_json(provider, &json_str, capabilities);
    }

    // Mode 2: Flat env vars
    load_from_flat(provider, capabilities)
}

/// Load from full JSON ProviderInfo string.
fn load_from_json(
    provider: &str,
    json_str: &str,
    capabilities: HashSet<String>,
) -> Option<ProviderTestConfig> {
    let provider_info: ProviderInfo = match serde_json::from_str(json_str) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Failed to parse JSON config for {provider}: {e}");
            return None;
        }
    };

    let model_slug = get_env_with_fallback(provider, "MODEL")
        .or_else(|| provider_info.model_slugs().first().map(ToString::to_string))
        .unwrap_or_default();

    let enabled = provider_info.has_api_key() && !model_slug.is_empty();

    Some(ProviderTestConfig {
        provider: provider.to_string(),
        provider_info,
        model_slug,
        enabled,
        capabilities,
    })
}

/// Load from flat env vars, assembling a ProviderInfo.
fn load_from_flat(provider: &str, capabilities: HashSet<String>) -> Option<ProviderTestConfig> {
    let api_key = get_env_with_fallback(provider, "API_KEY");
    let auth_token = get_env_with_fallback(provider, "AUTH_TOKEN");
    let model = get_env_with_fallback(provider, "MODEL");
    let base_url = get_env_with_fallback(provider, "BASE_URL");

    // Either API key or auth token is required
    let has_auth = api_key.is_some() || auth_token.is_some();
    let enabled = has_auth && model.is_some();

    // Return None if no configuration at all
    if !has_auth && model.is_none() && base_url.is_none() {
        return None;
    }

    let api = provider_api_for(provider);
    let model_slug = model.unwrap_or_default();

    let base_url = base_url.unwrap_or_else(|| default_base_url(api).to_string());

    // Wire API: explicit override or auto-detect
    let wire_api = get_env_with_fallback(provider, "WIRE_API")
        .map(|v| match v.to_lowercase().as_str() {
            "chat" => WireApi::Chat,
            _ => WireApi::Responses,
        })
        .unwrap_or_else(|| {
            if provider == "openai_chat" {
                WireApi::Chat
            } else {
                WireApi::default()
            }
        });

    let streaming = get_env_with_fallback(provider, "STREAMING")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(true);

    let mut provider_info = ProviderInfo::new(provider, api, base_url)
        .with_api_key(api_key.unwrap_or_default())
        .with_wire_api(wire_api)
        .with_streaming(streaming);

    // Build options from individual env vars + OPTIONS JSON
    let mut options = if let Some(opts_str) = get_env_with_fallback(provider, "OPTIONS") {
        serde_json::from_str::<serde_json::Value>(&opts_str)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    // Auth token (Anthropic via gateways like DashScope)
    if let Some(token) = &auth_token {
        options.insert(
            "auth_token".into(),
            serde_json::Value::String(token.clone()),
        );
    }

    // User-Agent header
    let user_agent = get_env_with_fallback(provider, "USER_AGENT");
    if user_agent.is_some() || options.contains_key("headers") {
        let headers_obj = options
            .entry("headers")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(ua) = &user_agent
            && let Some(map) = headers_obj.as_object_mut()
        {
            map.insert("User-Agent".into(), serde_json::Value::String(ua.clone()));
        }
    }

    // Include usage (OpenAI-compatible streaming)
    if let Some(val) = get_env_with_fallback(provider, "INCLUDE_USAGE") {
        let include = val.eq_ignore_ascii_case("true") || val == "1";
        options.insert("include_usage".into(), serde_json::Value::Bool(include));
    }

    if !options.is_empty() {
        provider_info = provider_info.with_options(serde_json::Value::Object(options));
    }

    Some(ProviderTestConfig {
        provider: provider.to_string(),
        provider_info,
        model_slug,
        enabled,
        capabilities,
    })
}

/// All known provider names for discovery.
fn all_provider_names() -> &'static [&'static str] {
    &[
        "openai",
        "openai_chat",
        "anthropic",
        "gemini",
        "volcengine",
        "zai",
        "openai_compat",
    ]
}

/// List all providers that are configured and enabled.
#[allow(dead_code)]
pub fn list_configured_providers() -> Vec<String> {
    ensure_env_loaded();

    all_provider_names()
        .iter()
        .filter_map(|p| {
            load_provider_config(p).and_then(|c| if c.enabled { Some(c.provider) } else { None })
        })
        .collect()
}

/// List all configured providers with the `cross_provider` capability.
pub fn list_cross_provider_configs() -> Vec<ProviderTestConfig> {
    ensure_env_loaded();

    all_provider_names()
        .iter()
        .filter_map(|p| {
            load_provider_config(p).filter(|c| c.enabled && c.has_capability("cross_provider"))
        })
        .collect()
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
