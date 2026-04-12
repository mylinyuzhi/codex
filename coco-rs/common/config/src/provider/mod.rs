pub mod builtin;

use coco_types::ProviderApi;
use coco_types::WireApi;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::env;

/// Per-provider configuration for API key resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
