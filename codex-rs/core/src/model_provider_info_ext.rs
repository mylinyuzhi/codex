//! Registry of model providers supported by Codex.
//!
//! Providers can be defined in two places:
//!   1. Built-in defaults compiled into the binary so Codex works out-of-the-box.
//!   2. User-defined entries inside `~/.codex/config.toml` under the `model_providers`
//!      key. These override or extend the defaults at runtime.

use codex_protocol::config_types_ext::ModelParameters;
use serde::Deserialize;
use serde::Serialize;

use crate::models_manager::model_family::ModelFamily;
use crate::models_manager::resolve_model_family;
use crate::thinking::UltrathinkConfig;

/// Serializable representation of a provider definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ModelProviderInfoExt {
    /// Whether to use streaming responses (SSE), defaults to true.
    #[serde(default = "default_streaming")]
    pub streaming: bool,

    /// Optional: LLM provider implementation to use.
    /// Providers handle request transformation and API communication.
    /// Built-in options: "openai_responses", "openai_chat"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Optional: Request interceptors to apply.
    /// Interceptors modify requests before sending (e.g., header injection).
    /// Built-in: "session_id_header" (injects session_id into "extra" header)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interceptors: Vec<String>,

    /// Optional: Model name for this provider configuration
    ///
    /// When set, this model name will be used in API requests for this provider.
    /// This allows multiple ModelProviderInfo entries to share the same provider
    /// and base_url but use different models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,

    /// Optional: Common LLM sampling parameters for this provider
    ///
    /// These parameters control the model's generation behavior. If specified,
    /// they override global defaults from the Config struct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_parameters: Option<ModelParameters>,

    /// Optional: Ultrathink configuration for this provider
    ///
    /// Configures the reasoning effort and budget when ultrathink mode is
    /// activated via Tab toggle or "ultrathink" keyword in message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ultrathink_config: Option<UltrathinkConfig>,

    /// HTTP request total timeout in milliseconds (per-provider override).
    ///
    /// Overrides the global `http_request_timeout_ms` setting for this provider.
    /// Useful for slow gateways that need longer timeouts.
    ///
    /// If not set, uses global config or defaults to 600000ms (10 minutes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_ms: Option<u64>,

    /// Explicit model family ID for metadata resolution.
    ///
    /// When set, this ID is used to look up the model family from
    /// `~/.codex/model_families.toml` or code-defined families.
    ///
    /// Use this when the API model name (`model_name`) differs from the
    /// logical model family. For example, Volcengine Ark endpoints use
    /// `ep-xxx` as model_name but belong to the `deepseek-r1` family.
    ///
    /// Example config:
    /// ```toml
    /// [model_providers.volcengine_ark.ext]
    /// model_name = "ep-20250109-xxxxx"  # Sent to API
    /// model_family_id = "deepseek-r1"   # Used for metadata lookup
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_family_id: Option<String>,

    /// Model family for this provider (derived at runtime, not serialized).
    /// Used by providers to get proper system instructions fallback.
    #[serde(skip)]
    pub model_family: Option<ModelFamily>,
}

fn default_streaming() -> bool {
    true
}

impl Default for ModelProviderInfoExt {
    fn default() -> Self {
        Self {
            streaming: default_streaming(),
            provider: None,
            interceptors: Vec::new(),
            model_name: None,
            model_parameters: None,
            ultrathink_config: None,
            request_timeout_ms: None,
            model_family_id: None,
            model_family: None,
        }
    }
}

impl ModelProviderInfoExt {
    /// Derive model_family from model_family_id or model_name.
    ///
    /// Should be called after config loading to populate the model_family field.
    ///
    /// Resolution priority:
    /// 1. `model_family_id` - explicit family ID (looked up in model_families.toml or code)
    /// 2. `model_name` - derived from the model name
    ///
    /// This allows the API model (`model_name`) to differ from the logical family
    /// (`model_family_id`). For example, Volcengine Ark uses `ep-xxx` as the API
    /// model but belongs to the `deepseek-r1` family.
    pub fn derive_model_family(&mut self) {
        // Priority 1: Explicit model_family_id
        if let Some(family_id) = &self.model_family_id {
            self.model_family = Some(resolve_model_family(family_id));
            return;
        }

        // Priority 2: Derive from model_name
        if let Some(model_name) = &self.model_name {
            self.model_family = Some(resolve_model_family(model_name));
        }
    }
}
