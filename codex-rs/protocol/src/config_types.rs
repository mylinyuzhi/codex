use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum_macros::Display;
use strum_macros::EnumIter;
use ts_rs::TS;

/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#get-started-with-reasoning
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    JsonSchema,
    TS,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    #[default]
    Medium,
    High,
}

/// A summary of the reasoning performed by the model. This can be useful for
/// debugging and understanding the model's reasoning process.
/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#reasoning-summaries
#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningSummary {
    #[default]
    Auto,
    Concise,
    Detailed,
    /// Option to disable reasoning summaries.
    None,
}

/// Controls output length/detail on GPT-5 models via the Responses API.
/// Serialized with lowercase values to match the OpenAI API.
#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Verbosity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(
    Deserialize, Debug, Clone, Copy, PartialEq, Default, Serialize, Display, JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SandboxMode {
    #[serde(rename = "read-only")]
    #[default]
    ReadOnly,

    #[serde(rename = "workspace-write")]
    WorkspaceWrite,

    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ForcedLoginMethod {
    Chatgpt,
    Api,
}

/// Web search provider backend selection.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    JsonSchema,
    TS,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum WebSearchProvider {
    /// DuckDuckGo HTML scraping (free, no API key required)
    #[default]
    DuckDuckGo,
    /// Tavily AI-optimized search API (requires TAVILY_API_KEY)
    Tavily,
    /// OpenAI native web_search tool (only for GPT models)
    OpenAI,
}

/// Web search configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, TS)]
pub struct WebSearchConfig {
    /// Search provider backend
    #[serde(default)]
    pub provider: WebSearchProvider,
    /// Maximum number of search results to return (1-20)
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: WebSearchProvider::default(),
            max_results: default_max_results(),
        }
    }
}

fn default_max_results() -> usize {
    5
}

/// Web fetch configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, TS)]
pub struct WebFetchConfig {
    /// Maximum content length in characters (default 100,000)
    #[serde(default = "default_max_content_length")]
    pub max_content_length: usize,
    /// Request timeout in seconds (default 30)
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// User-Agent header for HTTP requests
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            max_content_length: default_max_content_length(),
            timeout_secs: default_timeout_secs(),
            user_agent: default_user_agent(),
        }
    }
}

fn default_max_content_length() -> usize {
    100_000
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_user_agent() -> String {
    format!("codex-rs/{}", env!("CARGO_PKG_VERSION"))
}
