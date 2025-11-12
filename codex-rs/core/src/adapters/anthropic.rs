//! Anthropic Claude adapter for Messages API
//!
//! This adapter transforms requests/responses between codex-rs format and
//! Anthropic's Messages API format.
//!
//! # API Documentation
//!
//! - Messages API: https://docs.anthropic.com/en/api/messages
//! - Streaming: https://docs.anthropic.com/en/api/messages-streaming

use super::AdapterConfig;
use super::AdapterContext;
use super::ProviderAdapter;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::model_provider_info::ModelProviderInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use serde_json::Value as JsonValue;
use serde_json::json;

/// Anthropic Claude adapter
///
/// Transforms requests/responses for Anthropic's Messages API.
///
/// # Configuration
///
/// ```toml
/// [model_providers.anthropic]
/// name = "Anthropic Claude"
/// base_url = "https://api.anthropic.com/v1"
/// env_key = "ANTHROPIC_API_KEY"
/// adapter = "anthropic"
///
/// [model_providers.anthropic.adapter_config]
/// api_version = "2023-06-01"
/// default_max_tokens = 4096
/// ```
#[derive(Debug, Clone)]
pub struct AnthropicAdapter {
    /// Anthropic API version
    api_version: String,
    /// Default max tokens for responses
    default_max_tokens: i64,
}

impl Default for AnthropicAdapter {
    fn default() -> Self {
        Self {
            api_version: "2023-06-01".to_string(),
            default_max_tokens: 4096,
        }
    }
}

impl AnthropicAdapter {
    /// Create a new Anthropic adapter with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert codex-rs messages to Anthropic format
    fn convert_messages(&self, items: &[ResponseItem]) -> Vec<JsonValue> {
        let mut messages = Vec::new();

        for item in items {
            match item {
                ResponseItem::Message { role, content, .. } => {
                    // Skip system messages (handled separately in instructions)
                    if role == "system" {
                        continue;
                    }

                    let mut anthropic_content = Vec::new();

                    for content_item in content {
                        match content_item {
                            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                                anthropic_content.push(json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                            ContentItem::InputImage { image_url } => {
                                // Anthropic requires base64-encoded images
                                // For now, we'll include a placeholder
                                // TODO: Implement proper image handling
                                anthropic_content.push(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "url",
                                        "url": image_url
                                    }
                                }));
                            }
                        }
                    }

                    if !anthropic_content.is_empty() {
                        messages.push(json!({
                            "role": role,
                            "content": anthropic_content
                        }));
                    }
                }
                // Skip non-message items for now
                // TODO: Handle tool calls when implementing tool support
                _ => {}
            }
        }

        messages
    }
}

impl ProviderAdapter for AnthropicAdapter {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn endpoint_path(&self) -> Option<&str> {
        // Anthropic uses /v1/messages instead of /v1/chat/completions
        Some("/v1/messages")
    }

    fn build_request_metadata(
        &self,
        _prompt: &super::Prompt,
        _provider: &ModelProviderInfo,
        _context: &super::RequestContext,
    ) -> CodexResult<super::RequestMetadata> {
        let mut metadata = super::RequestMetadata::default();

        // Add Anthropic API version header
        // This was previously hardcoded in client.rs but now belongs in the adapter
        metadata
            .headers
            .insert("anthropic-version".to_string(), self.api_version.clone());

        Ok(metadata)
    }

    fn configure(&mut self, config: &AdapterConfig) -> CodexResult<()> {
        if let Some(version) = config.get_string("api_version") {
            self.api_version = version.to_string();
        }

        if let Some(max_tokens) = config.get_i64("default_max_tokens") {
            if max_tokens <= 0 {
                return Err(CodexErr::Fatal(format!(
                    "default_max_tokens must be positive, got {max_tokens}"
                )));
            }
            self.default_max_tokens = max_tokens;
        }

        Ok(())
    }

    fn validate_config(&self, config: &AdapterConfig) -> CodexResult<()> {
        // Validate API version format
        if let Some(version) = config.get_string("api_version")
            && !version.starts_with("20")
        {
            return Err(CodexErr::Fatal(format!(
                "api_version should be in format YYYY-MM-DD, got {version}"
            )));
        }

        Ok(())
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        provider: &ModelProviderInfo,
    ) -> CodexResult<JsonValue> {
        let messages = self.convert_messages(&prompt.input);

        // Get model name from provider config, fallback to default
        let model = provider
            .model_name
            .as_deref()
            .unwrap_or("claude-3-5-sonnet-20241022");

        let mut request = json!({
            "model": model,
            "messages": messages,
            "max_tokens": self.default_max_tokens,
            "stream": true
        });

        // Add system prompt from base_instructions_override if present
        if let Some(instructions) = &prompt.base_instructions_override {
            request["system"] = json!(instructions);
        }

        Ok(request)
    }

    fn transform_response_chunk(
        &self,
        chunk: &str,
        context: &mut AdapterContext,
    ) -> CodexResult<Vec<ResponseEvent>> {
        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Parse JSON event
        let event: JsonValue = serde_json::from_str(chunk)
            .map_err(|e| CodexErr::Fatal(format!("Failed to parse Anthropic event: {e}")))?;

        let event_type = event["type"].as_str().unwrap_or("");

        // MEMORY MANAGEMENT:
        // AnthropicAdapter uses lightweight key-value tracking (< 100 bytes per request)
        // - Stores message_id and current_block_index as simple JSON values
        // - Unlike PassthroughAdapter, doesn't accumulate large text buffers
        // - State is automatically freed when request completes (context drops)

        match event_type {
            "message_start" => {
                // Extract message ID and store in context for later retrieval
                // Memory cost: ~20 bytes (string "msg_xxx")
                if let Some(message_id) = event["message"]["id"].as_str() {
                    context.set("message_id", json!(message_id));
                }
                Ok(vec![ResponseEvent::Created])
            }

            "content_block_start" => {
                // Track which content block we're processing
                // Memory cost: ~20 bytes (integer index)
                // This helps correlate delta events with their blocks
                if let Some(index) = event["index"].as_i64() {
                    context.set("current_block_index", json!(index));
                }
                Ok(vec![])
            }

            "content_block_delta" => {
                // Extract text delta
                // Note: We don't accumulate text in context - just pass through deltas
                // Text accumulation happens in the response processor, not in adapter state
                if let Some(text) = event["delta"]["text"].as_str() {
                    Ok(vec![ResponseEvent::OutputTextDelta(text.to_string())])
                } else {
                    Ok(vec![])
                }
            }

            "content_block_stop" => {
                // Content block finished - clean up block tracking
                // Removes current_block_index from context
                context.remove("current_block_index");
                Ok(vec![])
            }

            "message_delta" => {
                // Message metadata update (usage stats, etc.)
                Ok(vec![])
            }

            "message_stop" => {
                // Message complete - retrieve stored message_id
                let message_id = context
                    .get_str("message_id")
                    .unwrap_or("unknown")
                    .to_string();

                // Extract token usage if available
                let token_usage = None; // TODO: Parse usage from event

                Ok(vec![ResponseEvent::Completed {
                    response_id: message_id,
                    token_usage,
                }])
            }

            "error" => {
                let error_message = event["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");
                Err(CodexErr::Fatal(format!(
                    "Anthropic API error: {error_message}"
                )))
            }

            _ => {
                // Unknown event type - log and ignore
                tracing::debug!("Unknown Anthropic event type: {event_type}");
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_anthropic_adapter_name() {
        let adapter = AnthropicAdapter::new();
        assert_eq!(adapter.name(), "anthropic");
    }

    #[test]
    fn test_configure_api_version() {
        let mut adapter = AnthropicAdapter::new();
        let mut config = AdapterConfig::new();
        config.set("api_version", json!("2024-01-01"));

        adapter.configure(&config).unwrap();
        assert_eq!(adapter.api_version, "2024-01-01");
    }

    #[test]
    fn test_configure_max_tokens() {
        let mut adapter = AnthropicAdapter::new();
        let mut config = AdapterConfig::new();
        config.set("default_max_tokens", json!(8192));

        adapter.configure(&config).unwrap();
        assert_eq!(adapter.default_max_tokens, 8192);
    }

    #[test]
    fn test_configure_invalid_max_tokens() {
        let mut adapter = AnthropicAdapter::new();
        let mut config = AdapterConfig::new();
        config.set("default_max_tokens", json!(-100));

        let result = adapter.configure(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_valid_version() {
        let adapter = AnthropicAdapter::new();
        let mut config = AdapterConfig::new();
        config.set("api_version", json!("2023-06-01"));

        assert!(adapter.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_invalid_version() {
        let adapter = AnthropicAdapter::new();
        let mut config = AdapterConfig::new();
        config.set("api_version", json!("invalid"));

        assert!(adapter.validate_config(&config).is_err());
    }

    #[test]
    fn test_transform_response_message_start() {
        let adapter = AnthropicAdapter::new();
        let mut ctx = AdapterContext::new();

        let chunk = r#"{"type":"message_start","message":{"id":"msg_123"}}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ResponseEvent::Created));
        assert_eq!(ctx.get_str("message_id"), Some("msg_123"));
    }

    #[test]
    fn test_transform_response_content_delta() {
        let adapter = AnthropicAdapter::new();
        let mut ctx = AdapterContext::new();

        let chunk = r#"{"type":"content_block_delta","delta":{"text":"Hello"}}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Hello"),
            _ => panic!("Expected OutputTextDelta"),
        }
    }

    #[test]
    fn test_transform_response_message_stop() {
        let adapter = AnthropicAdapter::new();
        let mut ctx = AdapterContext::new();
        ctx.set("message_id", json!("msg_456"));

        let chunk = r#"{"type":"message_stop"}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::Completed { response_id, .. } => {
                assert_eq!(response_id, "msg_456");
            }
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_transform_response_error() {
        let adapter = AnthropicAdapter::new();
        let mut ctx = AdapterContext::new();

        let chunk = r#"{"type":"error","error":{"message":"API key invalid"}}"#;
        let result = adapter.transform_response_chunk(chunk, &mut ctx);

        assert!(result.is_err());
    }
}
