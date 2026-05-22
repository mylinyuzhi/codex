//! Language model call options (V4).

use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use super::prompt::LanguageModelV4Prompt;
use super::tool::LanguageModelV4Tool;
use super::tool_choice::LanguageModelV4ToolChoice;
use crate::json_schema::JSONSchema;
use crate::shared::ProviderOptions;

/// Provider-agnostic reasoning effort level.
///
/// Controls how much reasoning effort the model should apply.
/// Providers map this to their specific reasoning configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningLevel {
    /// Use the provider's default reasoning behavior.
    ProviderDefault,
    /// Disable reasoning.
    None,
    /// Minimal reasoning effort.
    Minimal,
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Maximum reasoning effort.
    Xhigh,
}

impl ReasoningLevel {
    /// Returns the string representation matching the serde serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProviderDefault => "provider-default",
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

/// Response format configuration.
///
/// The output can either be text or JSON. Default is text.
/// If JSON is selected, a schema can optionally be provided to guide the LLM.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponseFormat {
    /// Text output format.
    #[default]
    Text,
    /// JSON output format with optional schema.
    Json {
        /// JSON schema that the generated output should conform to.
        #[serde(skip_serializing_if = "Option::is_none")]
        schema: Option<JSONSchema>,
        /// Name of output that should be generated.
        /// Used by some providers for additional LLM guidance.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Description of the output that should be generated.
        /// Used by some providers for additional LLM guidance.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl ResponseFormat {
    /// Create a text response format.
    pub fn text() -> Self {
        Self::Text
    }

    /// Create a JSON response format.
    pub fn json() -> Self {
        Self::Json {
            schema: None,
            name: None,
            description: None,
        }
    }

    /// Create a JSON response format with a schema.
    pub fn json_with_schema(schema: JSONSchema) -> Self {
        Self::Json {
            schema: Some(schema),
            name: None,
            description: None,
        }
    }

    /// Add a name to the JSON response format.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        if let Self::Json { name: n, .. } = &mut self {
            *n = Some(name.into());
        }
        self
    }

    /// Add a description to the JSON response format.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        if let Self::Json { description: d, .. } = &mut self {
            *d = Some(description.into());
        }
        self
    }
}

/// Options for a language model call.
#[derive(Debug, Clone, Default)]
pub struct LanguageModelV4CallOptions {
    /// The prompt to send to the model.
    pub prompt: LanguageModelV4Prompt,
    /// The maximum number of tokens to generate.
    pub max_output_tokens: Option<u64>,
    /// The temperature for sampling.
    pub temperature: Option<f32>,
    /// The top-p for nucleus sampling.
    pub top_p: Option<f32>,
    /// The top-k for sampling.
    pub top_k: Option<u64>,
    /// The stop sequences.
    pub stop_sequences: Option<Vec<String>>,
    /// The tools available to the model.
    pub tools: Option<Vec<LanguageModelV4Tool>>,
    /// The tool choice configuration.
    pub tool_choice: Option<LanguageModelV4ToolChoice>,
    /// The frequency penalty.
    pub frequency_penalty: Option<f32>,
    /// The presence penalty.
    pub presence_penalty: Option<f32>,
    /// The seed for deterministic sampling.
    pub seed: Option<u64>,
    /// Provider-agnostic reasoning effort level.
    pub reasoning: Option<ReasoningLevel>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Response format for structured output.
    pub response_format: Option<ResponseFormat>,
    /// Include raw chunks in the stream. Only applicable for streaming calls.
    pub include_raw_chunks: Option<bool>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Headers to include in the request.
    pub headers: Option<HashMap<String, String>>,
}

impl LanguageModelV4CallOptions {
    /// Create new call options.
    pub fn new(prompt: LanguageModelV4Prompt) -> Self {
        Self {
            prompt,
            ..Default::default()
        }
    }

    /// Set the maximum output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set the top-p.
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set the stop sequences.
    pub fn with_stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(stop_sequences);
        self
    }

    /// Set the tools.
    pub fn with_tools(mut self, tools: Vec<LanguageModelV4Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelV4ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Set provider options.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set the response format.
    pub fn with_response_format(mut self, response_format: ResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Set whether to include raw chunks in the stream.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }

    /// Set headers.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Set the reasoning level.
    pub fn with_reasoning(mut self, reasoning: ReasoningLevel) -> Self {
        self.reasoning = Some(reasoning);
        self
    }
}

#[cfg(test)]
#[path = "call_options.test.rs"]
mod tests;
