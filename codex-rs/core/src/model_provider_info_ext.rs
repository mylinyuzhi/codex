//! Registry of model providers supported by Codex.
//!
//! Providers can be defined in two places:
//!   1. Built-in defaults compiled into the binary so Codex works out-of-the-box.
//!   2. User-defined entries inside `~/.codex/config.toml` under the `model_providers`
//!      key. These override or extend the defaults at runtime.

use codex_protocol::config_types_ext::ModelParameters;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Serializable representation of a provider definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ModelProviderInfoExt {
    /// Whether to use streaming responses (SSE), defaults to true.
    #[serde(default = "default_streaming")]
    pub streaming: bool,

    /// Optional: Custom adapter for protocol transformation.
    /// Adapters enable support for providers with different API formats (e.g.,
    /// Anthropic Messages API, Google Gemini) while reusing the existing HTTP layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,

    /// Optional: Configuration for the adapter
    ///
    /// Provider-specific settings that customize the adapter's behavior.
    /// The structure is flexible and adapter-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_config: Option<HashMap<String, serde_json::Value>>,

    /// Optional: Model name for this provider configuration
    ///
    /// When set, this model name will be used in API requests for this provider.
    /// This allows multiple ModelProviderInfo entries to share the same adapter
    /// and base_url but use different models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,

    /// Optional: Common LLM sampling parameters for this provider
    ///
    /// These parameters control the model's generation behavior. If specified,
    /// they override global defaults from the Config struct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_parameters: Option<ModelParameters>,

    /// HTTP request total timeout in milliseconds (per-provider override).
    ///
    /// Overrides the global `http_request_timeout_ms` setting for this provider.
    /// Useful for slow gateways that need longer timeouts.
    ///
    /// If not set, uses global config or defaults to 600000ms (10 minutes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_ms: Option<u64>,
}

fn default_streaming() -> bool {
    true
}

impl Default for ModelProviderInfoExt {
    fn default() -> Self {
        Self {
            streaming: default_streaming(),
            adapter: None,
            adapter_config: None,
            model_name: None,
            model_parameters: None,
            request_timeout_ms: None,
        }
    }
}
