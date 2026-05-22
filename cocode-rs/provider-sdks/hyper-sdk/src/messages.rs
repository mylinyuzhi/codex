//! Message types for conversations.

use crate::options::ProviderOptions;
use crate::tools::ToolResultContent;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// Role of a message in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System instructions/context.
    System,
    /// User input.
    User,
    /// Assistant response.
    Assistant,
    /// Tool/function result.
    Tool,
}

/// Source for an image in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image data.
    Base64 {
        /// Base64-encoded data.
        data: String,
        /// MIME type (e.g., "image/png", "image/jpeg").
        media_type: String,
    },
    /// URL to an image.
    Url {
        /// Image URL.
        url: String,
    },
}

/// Image detail level for vision models.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    /// Low detail mode (faster, uses fewer tokens).
    Low,
    /// High detail mode (slower, uses more tokens).
    High,
    /// Auto-select detail level.
    #[default]
    Auto,
}

/// A block of content within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Image content for vision models.
    Image {
        /// Image source (base64 or URL).
        source: ImageSource,
        /// Optional detail level.
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// Tool/function call from assistant.
    ToolUse {
        /// Unique ID for this tool call.
        id: String,
        /// Name of the tool being called.
        name: String,
        /// Arguments as JSON.
        input: Value,
    },
    /// Result of a tool call.
    ToolResult {
        /// ID of the tool call this is responding to.
        tool_use_id: String,
        /// Result content.
        content: ToolResultContent,
        /// Whether this represents an error.
        #[serde(default)]
        is_error: bool,
        /// Whether this result is for a custom tool (vs a function tool).
        /// OpenAI requires `custom_tool_call_output` for custom tools.
        #[serde(default)]
        is_custom: bool,
    },
    /// Thinking/reasoning content (for extended thinking models).
    Thinking {
        /// The thinking content.
        content: String,
        /// Optional signature for verification.
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

impl ContentBlock {
    /// Create a text content block.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::ContentBlock;
    ///
    /// let block = ContentBlock::text("Hello, world!");
    /// assert_eq!(block.as_text(), Some("Hello, world!"));
    /// ```
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    /// Create an image content block from base64 data.
    pub fn image_base64(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        ContentBlock::Image {
            source: ImageSource::Base64 {
                data: data.into(),
                media_type: media_type.into(),
            },
            detail: None,
        }
    }

    /// Create an image content block from URL.
    pub fn image_url(url: impl Into<String>) -> Self {
        ContentBlock::Image {
            source: ImageSource::Url { url: url.into() },
            detail: None,
        }
    }

    /// Create a tool use content block.
    pub fn tool_use(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        ContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Create a tool result content block (for function tools).
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: ToolResultContent,
        is_error: bool,
    ) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content,
            is_error,
            is_custom: false,
        }
    }

    /// Create a custom tool result content block.
    ///
    /// Custom tool results use `custom_tool_call_output` when sent to OpenAI,
    /// instead of `function_call_output`.
    pub fn custom_tool_result(
        tool_use_id: impl Into<String>,
        content: ToolResultContent,
        is_error: bool,
    ) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content,
            is_error,
            is_custom: true,
        }
    }

    /// Create a thinking content block.
    pub fn thinking(content: impl Into<String>) -> Self {
        ContentBlock::Thinking {
            content: content.into(),
            signature: None,
        }
    }

    /// Extract text if this is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Check if this is a tool use block.
    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse { .. })
    }

    /// Check if this is a thinking block.
    pub fn is_thinking(&self) -> bool {
        matches!(self, ContentBlock::Thinking { .. })
    }
}

/// Unified provider metadata for a message.
///
/// Tracks message origin and preserves provider-specific extension data.
/// This design consolidates all provider-related information in one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderMetadata {
    /// Provider that generated this message (e.g., "openai", "anthropic").
    /// Required for assistant messages, None for user messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_provider: Option<String>,

    /// Model that generated this message (e.g., "gpt-4o", "claude-sonnet-4").
    /// Required for assistant messages, None for user messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_model: Option<String>,

    /// Provider-specific extensions keyed by provider name.
    ///
    /// Allows preserving metadata from multiple providers across conversation history.
    /// Examples:
    /// - `{"openai": {"finish_reason_detail": "length"}}`
    /// - `{"anthropic": {"cache_hit": true}}`
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extensions: HashMap<String, Value>,
}

impl ProviderMetadata {
    /// Create empty metadata (for user messages).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create metadata with source information.
    pub fn with_source(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            source_provider: Some(provider.into()),
            source_model: Some(model.into()),
            extensions: HashMap::new(),
        }
    }

    /// Check if this message was generated by the specified provider.
    pub fn is_from_provider(&self, provider: &str) -> bool {
        self.source_provider.as_deref() == Some(provider)
    }

    /// Check if this message was generated by the specified provider and model.
    pub fn is_from(&self, provider: &str, model: &str) -> bool {
        self.source_provider.as_deref() == Some(provider)
            && self.source_model.as_deref() == Some(model)
    }

    /// Get extension data for a specific provider.
    pub fn get_extension(&self, provider: &str) -> Option<&Value> {
        self.extensions.get(provider)
    }

    /// Set extension data for a specific provider.
    pub fn set_extension(&mut self, provider: impl Into<String>, data: Value) {
        self.extensions.insert(provider.into(), data);
    }

    /// Remove extension data for a specific provider.
    pub fn remove_extension(&mut self, provider: &str) -> Option<Value> {
        self.extensions.remove(provider)
    }

    /// Check if metadata is empty (no source, no extensions).
    pub fn is_empty(&self) -> bool {
        self.source_provider.is_none() && self.source_model.is_none() && self.extensions.is_empty()
    }
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender.
    pub role: Role,
    /// Content blocks.
    pub content: Vec<ContentBlock>,
    /// Provider-specific options for THIS request (runtime, not persisted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    /// Unified provider metadata (source tracking + extensions).
    #[serde(default, skip_serializing_if = "ProviderMetadata::is_empty")]
    pub metadata: ProviderMetadata,
}

impl Message {
    /// Create a new message with the given role and content blocks.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{Message, ContentBlock, Role};
    ///
    /// let msg = Message::new(Role::User, vec![ContentBlock::text("Hello")]);
    /// assert_eq!(msg.role, Role::User);
    /// assert_eq!(msg.text(), "Hello");
    /// ```
    pub fn new(role: Role, content: Vec<ContentBlock>) -> Self {
        Self {
            role,
            content,
            provider_options: None,
            metadata: ProviderMetadata::new(),
        }
    }

    /// Create a user message with text content.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{Message, Role};
    ///
    /// let msg = Message::user("What is 2 + 2?");
    /// assert_eq!(msg.role, Role::User);
    /// assert_eq!(msg.text(), "What is 2 + 2?");
    /// ```
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentBlock::text(text)])
    }

    /// Create an assistant message with text content.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{Message, Role};
    ///
    /// let msg = Message::assistant("The answer is 4.");
    /// assert_eq!(msg.role, Role::Assistant);
    /// assert_eq!(msg.text(), "The answer is 4.");
    /// ```
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentBlock::text(text)])
    }

    /// Create a system message with text content.
    ///
    /// # Example
    ///
    /// ```
    /// use hyper_sdk::{Message, Role};
    ///
    /// let msg = Message::system("You are a helpful assistant.");
    /// assert_eq!(msg.role, Role::System);
    /// ```
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentBlock::text(text)])
    }

    /// Create a user message with text and an image.
    pub fn user_with_image(text: impl Into<String>, image: ImageSource) -> Self {
        Self::new(
            Role::User,
            vec![
                ContentBlock::text(text),
                ContentBlock::Image {
                    source: image,
                    detail: None,
                },
            ],
        )
    }

    /// Create a tool result message.
    pub fn tool_result(tool_use_id: impl Into<String>, content: ToolResultContent) -> Self {
        Self::new(
            Role::Tool,
            vec![ContentBlock::tool_result(tool_use_id, content, false)],
        )
    }

    /// Create a tool error message.
    pub fn tool_error(tool_use_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self::new(
            Role::Tool,
            vec![ContentBlock::tool_result(
                tool_use_id,
                ToolResultContent::Text(error.into()),
                true,
            )],
        )
    }

    /// Set provider-specific options for this message.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Get all text content from this message concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all tool use blocks from this message.
    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.content.iter().filter(|b| b.is_tool_use()).collect()
    }

    /// Set the source provider and model for this message.
    pub fn with_source(mut self, provider: impl Into<String>, model: impl Into<String>) -> Self {
        self.metadata = ProviderMetadata::with_source(provider, model);
        self
    }

    /// Get source provider (convenience accessor).
    pub fn source_provider(&self) -> Option<&str> {
        self.metadata.source_provider.as_deref()
    }

    /// Get source model (convenience accessor).
    pub fn source_model(&self) -> Option<&str> {
        self.metadata.source_model.as_deref()
    }

    /// Strip all thinking signatures from this message.
    ///
    /// This is useful when switching providers, as thinking signatures
    /// are provider-specific and cannot be verified by other providers.
    pub fn strip_thinking_signatures(&mut self) {
        for block in &mut self.content {
            if let ContentBlock::Thinking { signature, .. } = block {
                *signature = None;
            }
        }
    }

    /// Sanitize this message for use with a target provider and model.
    ///
    /// If the message was generated by a different provider or model,
    /// this will strip thinking signatures to avoid verification errors.
    /// Both provider AND model must match to preserve signatures, since
    /// different models from the same provider may have incompatible signatures.
    pub fn sanitize_for_target(&mut self, target_provider: &str, target_model: &str) {
        if !self.metadata.is_from(target_provider, target_model) {
            self.strip_thinking_signatures();
        }
    }

    /// Convert message content to be compatible with target provider.
    ///
    /// This method sanitizes the message for cross-provider compatibility:
    /// 1. Strips thinking signatures if source differs from target
    /// 2. Clears provider-specific options
    /// 3. Preserves source tracking in metadata for debugging
    pub fn convert_for_provider(&mut self, target_provider: &str, target_model: &str) {
        let is_same_provider = self.metadata.is_from_provider(target_provider);
        let is_same_model = self.metadata.is_from(target_provider, target_model);

        // 1. Strip thinking signatures if provider/model differs
        if !is_same_model {
            self.strip_thinking_signatures();
        }

        // 2. Clear provider-specific options that won't be understood
        if !is_same_provider {
            self.provider_options = None;
        }

        // 3. Clear extensions from other providers (optional, configurable)
        // Keep source tracking, but remove runtime extensions from different providers
        if !is_same_provider {
            // Preserve extensions for the target provider only
            let target_ext = self.metadata.extensions.remove(target_provider);
            self.metadata.extensions.clear();
            if let Some(ext) = target_ext {
                self.metadata
                    .extensions
                    .insert(target_provider.to_string(), ext);
            }
        }
    }
}

#[cfg(test)]
#[path = "messages.test.rs"]
mod tests;
