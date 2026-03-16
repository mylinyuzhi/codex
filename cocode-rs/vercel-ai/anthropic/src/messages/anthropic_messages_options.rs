use serde::Deserialize;
use serde_json::Value;
use vercel_ai_provider::ProviderOptions;

/// Anthropic-specific thinking configuration.
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
pub struct CacheControlConfig {
    #[serde(rename = "type")]
    pub cache_type: String,
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
pub fn extract_anthropic_options(
    provider_options: &Option<ProviderOptions>,
) -> AnthropicProviderOptions {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("anthropic"))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<AnthropicProviderOptions>(v).ok())
        .unwrap_or_default()
}
