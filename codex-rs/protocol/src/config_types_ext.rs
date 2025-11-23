use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum_macros::Display;
use strum_macros::EnumIter;
use ts_rs::TS;

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

/// Common LLM sampling parameters that apply across different model providers.
/// These parameters control the model's generation behavior and can be set at
/// both global (Config) and provider (ModelProviderInfo) levels, with provider
/// settings overriding global defaults.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, JsonSchema, TS)]
pub struct ModelParameters {
    /// Controls randomness in the output. Higher values make output more random.
    /// Range: 0.0-2.0 (OpenAI), 0.0-1.0 (Anthropic)
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Nucleus sampling: only tokens with cumulative probability <= top_p are considered.
    /// Range: 0.0-1.0
    #[serde(default)]
    pub top_p: Option<f32>,

    /// Penalizes tokens based on their frequency in the text so far.
    /// Reduces repetition. Range: -2.0 to 2.0 (OpenAI)
    #[serde(default)]
    pub frequency_penalty: Option<f32>,

    /// Penalizes tokens based on whether they appear in the text so far.
    /// Encourages topic diversity. Range: -2.0 to 2.0 (OpenAI)
    #[serde(default)]
    pub presence_penalty: Option<f32>,

    /// Maximum number of tokens to generate in the completion.
    /// Overrides model-specific defaults.
    /// Note: Different APIs may use different parameter names (max_tokens, max_output_tokens, etc.)
    /// but we use the industry-standard 'max_tokens' in configuration.
    #[serde(default)]
    pub max_tokens: Option<i64>,

    /// Enable thinking process in response (Gemini-specific).
    /// When true, the model will include its thinking process in the response.
    /// Ignored by non-Gemini adapters.
    #[serde(default)]
    pub include_thoughts: Option<bool>,

    /// Thinking token budget for Gemini models (Gemini-specific).
    ///
    /// Special values:
    /// - `-1` (default): Dynamic thinking - model decides when and how much to think
    /// - `0`: Disable thinking (Gemini 2.5 Flash only; 2.5 Pro does not support disabling)
    /// - `> 0`: Fixed token budget
    ///   - Gemini 2.5 Pro: 128 to 32768
    ///   - Gemini 2.5 Flash: 1 to 24576
    ///
    /// If not set, Gemini uses dynamic thinking by default (equivalent to -1).
    /// Ignored by non-Gemini adapters.
    #[serde(default)]
    pub budget_tokens: Option<i32>,
}
