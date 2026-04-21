pub mod builtin;

use coco_types::ProviderApi;
use coco_types::WireApi;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::env;

/// Per-provider configuration for API key resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: String,
    pub api: ProviderApi,
    /// Environment variable name for this provider's API key.
    pub env_key: String,
    /// Fallback API key from config file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            api: ProviderApi::Anthropic,
            env_key: String::new(),
            api_key: None,
            base_url: String::new(),
            default_model: None,
        }
    }
}

impl ProviderConfig {
    /// Resolve API key for this provider.
    /// Priority: env var > config file api_key.
    pub fn resolve_api_key(&self) -> Option<String> {
        env::env_opt(&self.env_key).or_else(|| self.api_key.clone())
    }

    /// Layer `override_cfg` onto `self`: every non-empty / Some field in
    /// `override_cfg` wins, every empty / None field leaves `self`
    /// untouched so builtin defaults (e.g. `default_model`) are preserved
    /// when the user only overrides a subset.
    ///
    /// Note: `api` is always taken from the override because it's a non-
    /// optional enum with no "unset" sentinel — users who partially
    /// override a builtin (e.g. just changing `base_url`) but forget to
    /// set `api` will get `ProviderApi::Anthropic` by serde default. For
    /// the five builtins this happens to match; for unknown providers
    /// users should set `api` explicitly.
    pub fn merge_from(&mut self, override_cfg: &Self) {
        if !override_cfg.name.is_empty() {
            self.name.clone_from(&override_cfg.name);
        }
        self.api = override_cfg.api;
        if !override_cfg.env_key.is_empty() {
            self.env_key.clone_from(&override_cfg.env_key);
        }
        if override_cfg.api_key.is_some() {
            self.api_key.clone_from(&override_cfg.api_key);
        }
        if !override_cfg.base_url.is_empty() {
            self.base_url.clone_from(&override_cfg.base_url);
        }
        if override_cfg.default_model.is_some() {
            self.default_model.clone_from(&override_cfg.default_model);
        }
    }
}

/// Resolved provider configuration at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub api: ProviderApi,
    pub base_url: String,
    pub api_key: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: i64,
    #[serde(default = "default_true")]
    pub streaming: bool,
    #[serde(default = "default_wire_api")]
    pub wire_api: WireApi,
    /// Models registered under this provider.
    #[serde(default)]
    pub models: HashMap<String, ProviderModel>,
    /// SDK client construction options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
    #[serde(default)]
    pub interceptors: Vec<String>,
}

fn default_wire_api() -> WireApi {
    WireApi::Chat
}

fn default_timeout() -> i64 {
    600
}

fn default_true() -> bool {
    true
}

/// A model entry within a provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderModel {
    /// Merged ModelInfo.
    #[serde(flatten)]
    pub model_info: crate::model::ModelInfo,
    /// API model name if different from model_id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_model_name: Option<String>,
    /// Per-provider per-model options.
    pub model_options: HashMap<String, serde_json::Value>,
}
