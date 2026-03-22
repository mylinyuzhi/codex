//! Request types for generation.

use crate::messages::Message;
use crate::options::AnthropicOptions;
use crate::options::GeminiOptions;
use crate::options::OpenAIOptions;
use crate::options::ProviderOptions;
use crate::options::ProviderOptionsData;
use crate::options::VolcengineOptions;
use crate::options::ZaiOptions;
use crate::tools::ToolChoice;
use crate::tools::ToolDefinition;
use serde::Deserialize;
use serde::Serialize;

/// Request for text generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// Messages in the conversation.
    pub messages: Vec<Message>,
    /// Sampling temperature (0.0-2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    /// Top-p nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling (Anthropic-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    /// Presence penalty (-2.0 to 2.0, OpenAI-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Frequency penalty (-2.0 to 2.0, OpenAI-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Tools available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// How the model should choose tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Provider-specific options.
    ///
    /// Skipped in serialization because:
    /// 1. Options are provider-specific and not portable across providers
    /// 2. Deserialization would require knowing the target provider type upfront
    /// 3. Options may contain non-serializable types in the future
    ///
    /// For persistence, store the request parameters separately and reconstruct
    /// the provider options when deserializing for a specific provider.
    #[serde(skip)]
    pub provider_options: Option<ProviderOptions>,
}

impl GenerateRequest {
    /// Create a new request with the given messages.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, Message};
    ///
    /// let request = GenerateRequest::new(vec![
    ///     Message::system("You are a helpful assistant."),
    ///     Message::user("Hello!"),
    /// ]);
    /// assert_eq!(request.messages.len(), 2);
    /// ```
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            temperature: None,
            max_tokens: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            provider_options: None,
        }
    }

    /// Create a request from a single text prompt.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::GenerateRequest;
    ///
    /// let request = GenerateRequest::from_text("What is the meaning of life?")
    ///     .temperature(0.7)
    ///     .max_tokens(1000);
    ///
    /// assert_eq!(request.temperature, Some(0.7));
    /// assert_eq!(request.max_tokens, Some(1000));
    /// ```
    pub fn from_text(text: impl Into<String>) -> Self {
        Self::new(vec![Message::user(text)])
    }

    /// Set the sampling temperature.
    pub fn temperature(mut self, t: f64) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Set the maximum tokens to generate.
    pub fn max_tokens(mut self, n: i32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Set top-p nucleus sampling.
    pub fn top_p(mut self, p: f64) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Set top-k sampling (Anthropic-specific).
    pub fn top_k(mut self, k: i32) -> Self {
        self.top_k = Some(k);
        self
    }

    /// Set presence penalty (-2.0 to 2.0, OpenAI-specific).
    pub fn presence_penalty(mut self, p: f64) -> Self {
        self.presence_penalty = Some(p);
        self
    }

    /// Set frequency penalty (-2.0 to 2.0, OpenAI-specific).
    pub fn frequency_penalty(mut self, p: f64) -> Self {
        self.frequency_penalty = Some(p);
        self
    }

    /// Set available tools.
    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set tool choice behavior.
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set provider-specific options (generic).
    ///
    /// For better IDE support and type safety hints, prefer using the
    /// typed methods like [`with_openai_options()`](Self::with_openai_options).
    pub fn provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set provider-specific options using any type implementing ProviderOptionsData.
    ///
    /// This is a convenience method that boxes the options automatically.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, OpenAIOptions};
    ///
    /// let request = GenerateRequest::from_text("Hello")
    ///     .with_provider_options(OpenAIOptions::new());
    /// ```
    pub fn with_provider_options(mut self, options: impl ProviderOptionsData + 'static) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Set OpenAI-specific options.
    ///
    /// This method provides IDE auto-completion for OpenAI-specific options
    /// and makes the code more self-documenting.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, OpenAIOptions};
    /// use hyper_sdk::options::openai::ReasoningEffort;
    ///
    /// let request = GenerateRequest::from_text("Solve this math problem")
    ///     .with_openai_options(
    ///         OpenAIOptions::new()
    ///             .with_reasoning_effort(ReasoningEffort::High)
    ///     );
    /// ```
    pub fn with_openai_options(mut self, options: OpenAIOptions) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Set Anthropic-specific options.
    ///
    /// This method provides IDE auto-completion for Anthropic-specific options.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, AnthropicOptions};
    ///
    /// let request = GenerateRequest::from_text("Think step by step")
    ///     .with_anthropic_options(
    ///         AnthropicOptions::new()
    ///             .with_thinking_budget(10000)
    ///     );
    /// ```
    pub fn with_anthropic_options(mut self, options: AnthropicOptions) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Set Google Gemini-specific options.
    ///
    /// This method provides IDE auto-completion for Gemini-specific options.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, GeminiOptions};
    /// use hyper_sdk::options::gemini::ThinkingLevel;
    ///
    /// let request = GenerateRequest::from_text("Research this topic")
    ///     .with_gemini_options(
    ///         GeminiOptions::new()
    ///             .with_grounding(true)
    ///             .with_thinking_level(ThinkingLevel::High)
    ///     );
    /// ```
    pub fn with_gemini_options(mut self, options: GeminiOptions) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Set Volcengine Ark-specific options.
    ///
    /// This method provides IDE auto-completion for Volcengine-specific options.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, VolcengineOptions};
    ///
    /// let request = GenerateRequest::from_text("Complex reasoning task")
    ///     .with_volcengine_options(
    ///         VolcengineOptions::new()
    ///             .with_thinking_budget(2048)
    ///     );
    /// ```
    pub fn with_volcengine_options(mut self, options: VolcengineOptions) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Set Z.AI / ZhipuAI-specific options.
    ///
    /// This method provides IDE auto-completion for Z.AI-specific options.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{GenerateRequest, ZaiOptions};
    ///
    /// let request = GenerateRequest::from_text("Deep reasoning task")
    ///     .with_zai_options(
    ///         ZaiOptions::new()
    ///             .with_thinking_budget(4096)
    ///             .with_do_sample(true)
    ///     );
    /// ```
    pub fn with_zai_options(mut self, options: ZaiOptions) -> Self {
        self.provider_options = Some(Box::new(options));
        self
    }

    /// Add a message to the request.
    pub fn add_message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Check if tools are configured.
    pub fn has_tools(&self) -> bool {
        self.tools.as_ref().is_some_and(|t| !t.is_empty())
    }

    /// Strip thinking signatures from all messages in this request.
    ///
    /// This is useful when switching providers, as thinking signatures
    /// are provider-specific and cannot be verified by other providers.
    pub fn strip_all_thinking_signatures(&mut self) {
        for msg in &mut self.messages {
            msg.strip_thinking_signatures();
        }
    }

    /// Sanitize all messages for use with a target provider and model.
    ///
    /// For each message that was generated by a different provider or model,
    /// this will strip thinking signatures to avoid verification errors.
    pub fn sanitize_for_target(&mut self, target_provider: &str, target_model: &str) {
        for msg in &mut self.messages {
            msg.sanitize_for_target(target_provider, target_model);
        }
    }
}

impl Default for GenerateRequest {
    fn default() -> Self {
        Self::new(vec![])
    }
}

#[cfg(test)]
#[path = "request.test.rs"]
mod tests;
