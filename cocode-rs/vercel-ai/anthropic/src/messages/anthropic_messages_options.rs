use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use vercel_ai_provider::ProviderOptions;

/// Anthropic-specific thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    /// For Sonnet 4.6, Opus 4.6, and newer models.
    #[serde(rename = "adaptive")]
    Adaptive,
    /// For models before Opus 4.6 (except Sonnet 4.6 which still supports it).
    #[serde(rename = "enabled")]
    Enabled {
        #[serde(rename = "budgetTokens")]
        budget_tokens: Option<u64>,
    },
    /// Disable thinking.
    #[serde(rename = "disabled")]
    Disabled,
}

/// Structured output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StructuredOutputMode {
    OutputFormat,
    JsonTool,
    Auto,
}

/// Effort level.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Max,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Speed mode (Opus 4.6 only).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    Fast,
    Standard,
}

impl Speed {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Standard => "standard",
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub name: String,
    pub url: String,
    pub authorization_token: Option<String>,
    pub tool_configuration: Option<McpToolConfiguration>,
}

/// MCP tool configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolConfiguration {
    pub enabled: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
}

/// Container configuration for agent skills.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfig {
    pub id: Option<String>,
    pub skills: Option<Vec<ContainerSkill>>,
}

/// A skill in a container.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSkill {
    #[serde(rename = "type")]
    pub skill_type: String,
    pub skill_id: String,
    pub version: Option<String>,
}

/// Cache control configuration.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct CacheControlConfig {
    #[serde(rename = "type")]
    pub cache_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// Anthropic-specific provider options.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicProviderOptions {
    /// Whether to send reasoning to the model.
    pub send_reasoning: Option<bool>,
    /// Structured output mode.
    pub structured_output_mode: Option<StructuredOutputMode>,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Disable parallel tool use.
    pub disable_parallel_tool_use: Option<bool>,
    /// Cache control settings.
    pub cache_control: Option<CacheControlConfig>,
    /// MCP servers.
    pub mcp_servers: Option<Vec<McpServerConfig>>,
    /// Container/agent skills configuration.
    pub container: Option<ContainerConfig>,
    /// Enable/disable tool streaming.
    pub tool_streaming: Option<bool>,
    /// Effort level.
    pub effort: Option<Effort>,
    /// Speed mode (Opus 4.6 only).
    pub speed: Option<Speed>,
    /// Extra beta features.
    pub anthropic_beta: Option<Vec<String>>,
    /// Context management configuration.
    pub context_management: Option<Value>,
}

/// Extract Anthropic-specific options from generic provider options.
///
/// Parses from both the canonical `"anthropic"` key and any custom provider name key,
/// merging them (custom overrides canonical). The provider name prefix is extracted
/// from the full provider string (e.g., `"my-proxy.messages"` → `"my-proxy"`).
pub fn extract_anthropic_options(
    provider_options: &Option<ProviderOptions>,
    provider: &str,
) -> AnthropicProviderOptions {
    let opts = match provider_options.as_ref() {
        Some(opts) => opts,
        None => return AnthropicProviderOptions::default(),
    };

    // Parse canonical "anthropic" key
    let canonical: AnthropicProviderOptions = opts
        .0
        .get("anthropic")
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Extract custom provider name prefix (e.g., "my-proxy.messages" → "my-proxy")
    let provider_name = match provider.find('.') {
        Some(idx) => &provider[..idx],
        None => provider,
    };

    if provider_name == "anthropic" {
        return canonical;
    }

    // Parse custom provider key and merge (custom overrides canonical)
    let custom: Option<AnthropicProviderOptions> = opts
        .0
        .get(provider_name)
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value(v).ok());

    match custom {
        Some(custom) => merge_anthropic_options(canonical, custom),
        None => canonical,
    }
}

/// Merge two option structs: custom values override canonical.
fn merge_anthropic_options(
    canonical: AnthropicProviderOptions,
    custom: AnthropicProviderOptions,
) -> AnthropicProviderOptions {
    AnthropicProviderOptions {
        send_reasoning: custom.send_reasoning.or(canonical.send_reasoning),
        structured_output_mode: custom
            .structured_output_mode
            .or(canonical.structured_output_mode),
        thinking: custom.thinking.or(canonical.thinking),
        disable_parallel_tool_use: custom
            .disable_parallel_tool_use
            .or(canonical.disable_parallel_tool_use),
        cache_control: custom.cache_control.or(canonical.cache_control),
        mcp_servers: custom.mcp_servers.or(canonical.mcp_servers),
        container: custom.container.or(canonical.container),
        tool_streaming: custom.tool_streaming.or(canonical.tool_streaming),
        effort: custom.effort.or(canonical.effort),
        speed: custom.speed.or(canonical.speed),
        anthropic_beta: custom.anthropic_beta.or(canonical.anthropic_beta),
        context_management: custom.context_management.or(canonical.context_management),
    }
}
