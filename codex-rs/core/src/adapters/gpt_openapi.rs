//! GPT OpenAPI adapter for enterprise LLM gateway
//!
//! This adapter is designed for enterprise internal LLM gateways that are
//! compatible with OpenAI Responses API format. It performs minimal transformation
//! since the gateway already speaks OpenAI protocol.
//!
//! # Requirements
//!
//! **This adapter ONLY supports `wire_api = "responses"`**. Configuration validation
//! will reject providers with `wire_api = "chat"`. If your gateway uses Chat Completions
//! format, use the `passthrough` adapter instead.
//!
//! # Use Cases
//!
//! - **Enterprise LLM Gateway**: Internal API gateway that proxies to various LLM providers
//! - **Responses API Gateway**: Gateway that uses OpenAI Responses API format
//! - **Multi-tenant**: One adapter implementation with multiple provider configurations
//!
//! # Example Configuration
//!
//! ```toml
//! # Configuration for production gateway
//! [model_providers.enterprise_prod]
//! name = "Enterprise Production Gateway"
//! base_url = "https://api.enterprise.com/v1"
//! env_key = "ENTERPRISE_API_KEY"
//! adapter = "gpt_openapi"
//! wire_api = "responses"  # Required: gpt_openapi only supports Responses API
//! model_name = "gpt-4"
//!
//! [model_providers.enterprise_prod.adapter_config]
//! api_version = "v1"
//! timeout = 60
//!
//! # Configuration for staging gateway with different model
//! [model_providers.enterprise_staging]
//! name = "Enterprise Staging Gateway"
//! base_url = "https://api-staging.enterprise.com/v1"
//! env_key = "ENTERPRISE_STAGING_KEY"
//! adapter = "gpt_openapi"
//! wire_api = "responses"  # Required: gpt_openapi only supports Responses API
//! model_name = "gpt-3.5-turbo"
//!
//! [model_providers.enterprise_staging.adapter_config]
//! api_version = "v1"
//! timeout = 30
//! ```

use super::AdapterContext;
use super::ProviderAdapter;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use serde_json::Value as JsonValue;
use serde_json::json;

/// GPT OpenAPI adapter for enterprise LLM gateways
///
/// This adapter is optimized for enterprise internal gateways that are already
/// OpenAI Responses API compatible. It performs minimal transformation.
///
/// # Requirements
///
/// **ONLY supports `wire_api = "responses"`**. Config validation will reject `wire_api = "chat"`.
///
/// # Features
///
/// - **Minimal overhead**: Direct passthrough of requests/responses
/// - **Multi-configuration**: One adapter, multiple provider configurations
/// - **Responses API only**: Uses OpenAI Responses API format exclusively
/// - **Flexible**: Supports different model names and endpoints per provider
///
/// # Dynamic Headers & Metadata
///
/// This adapter uses the **default `build_request_metadata()` implementation**,
/// which means it does NOT add any dynamic HTTP headers or query parameters.
///
/// For enterprise gateways that need dynamic headers (e.g., session tracking,
/// log correlation), you have two options:
///
/// 1. **Create a custom adapter** that extends this one:
///    ```rust,ignore
///    impl ProviderAdapter for CustomEnterpriseAdapter {
///        fn build_request_metadata(
///            &self,
///            _prompt: &Prompt,
///            _provider: &ModelProviderInfo,
///            context: &RequestContext,
///        ) -> Result<RequestMetadata> {
///            let mut metadata = RequestMetadata::default();
///            metadata.headers.insert(
///                "x-log-id".to_string(),
///                context.conversation_id.clone(),
///            );
///            Ok(metadata)
///        }
///    }
///    ```
///
/// 2. **Use static headers** in provider configuration:
///    ```toml
///    [model_providers.enterprise]
///    adapter = "gpt_openapi"
///    http_headers = { "x-team-id" = "ai-team" }
///    ```
///
/// # Implementation Notes
///
/// The adapter assumes the gateway:
/// - Accepts OpenAI Responses API request format (with `input`, `previous_response_id`, etc.)
/// - Returns OpenAI Responses API streaming responses (events like `response.created`, `response.output_item.delta`)
/// - Supports standard SSE (Server-Sent Events) streaming
/// - Uses `/responses` endpoint (automatically determined by `wire_api` setting)
#[derive(Debug)]
pub struct GptOpenapiAdapter;

impl GptOpenapiAdapter {
    /// Create a new GPT OpenAPI adapter instance
    pub fn new() -> Self {
        Self
    }

    /// Parse complete (non-streaming) Responses API JSON response
    ///
    /// Expected format:
    /// ```json
    /// {
    ///   "id": "resp-123",
    ///   "model": "gpt-4",
    ///   "output": [
    ///     {
    ///       "type": "message",
    ///       "id": "msg-1",
    ///       "content": [
    ///         { "type": "text", "text": "Hello" }
    ///       ]
    ///     }
    ///   ],
    ///   "usage": {
    ///     "input_tokens": 10,
    ///     "output_tokens": 5
    ///   }
    /// }
    /// ```
    fn parse_complete_responses_json(body: &str) -> Result<Vec<ResponseEvent>> {
        let data: JsonValue = serde_json::from_str(body)?;

        let mut events = Vec::new();

        // Extract response ID
        let response_id = data
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        // Parse output items
        if let Some(output_array) = data.get("output").and_then(|o| o.as_array()) {
            for item_data in output_array {
                if let Some(item) = Self::parse_output_item(item_data)? {
                    events.push(ResponseEvent::OutputItemDone(item));
                }
            }
        }

        // Parse token usage
        let token_usage = data.get("usage").map(|u| {
            let input_tokens = u
                .get("input_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let output_tokens = u
                .get("output_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let cached_input_tokens = u
                .get("input_tokens_cache_read")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let reasoning_output_tokens = u
                .get("reasoning_output_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let total_tokens = input_tokens + output_tokens;

            crate::protocol::TokenUsage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_output_tokens,
                total_tokens,
            }
        });

        // Add completion event
        events.push(ResponseEvent::Completed {
            response_id,
            token_usage,
        });

        Ok(events)
    }

    /// Parse a single output item from complete response
    ///
    /// Returns None if item type is not recognized or parsing fails
    fn parse_output_item(
        item_data: &JsonValue,
    ) -> Result<Option<codex_protocol::models::ResponseItem>> {
        let item_type = item_data.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                let id = item_data
                    .get("id")
                    .and_then(|i| i.as_str())
                    .map(std::string::ToString::to_string);

                let role = item_data
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("assistant")
                    .to_string();

                // Parse content array
                let mut content = Vec::new();
                if let Some(content_array) = item_data.get("content").and_then(|c| c.as_array()) {
                    for content_item in content_array {
                        let content_type = content_item
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("");

                        match content_type {
                            "text" => {
                                if let Some(text) =
                                    content_item.get("text").and_then(|t| t.as_str())
                                {
                                    content.push(codex_protocol::models::ContentItem::OutputText {
                                        text: text.to_string(),
                                    });
                                }
                            }
                            _ => {
                                // Skip unknown content types
                            }
                        }
                    }
                }

                Ok(Some(codex_protocol::models::ResponseItem::Message {
                    id,
                    role,
                    content,
                }))
            }

            "function_call" => {
                let id = item_data
                    .get("id")
                    .and_then(|i| i.as_str())
                    .map(std::string::ToString::to_string);

                let name = item_data
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();

                let call_id = item_data
                    .get("call_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();

                let arguments = item_data
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}")
                    .to_string();

                Ok(Some(codex_protocol::models::ResponseItem::FunctionCall {
                    id,
                    name,
                    call_id,
                    arguments,
                }))
            }

            "reasoning" => {
                let id = item_data
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();

                // Parse summary array
                let mut summary = Vec::new();
                if let Some(summary_array) = item_data.get("summary").and_then(|s| s.as_array()) {
                    for summary_item in summary_array {
                        if let Some(text) = summary_item.get("text").and_then(|t| t.as_str()) {
                            summary.push(
                                codex_protocol::models::ReasoningItemReasoningSummary::SummaryText {
                                    text: text.to_string(),
                                },
                            );
                        }
                    }
                }

                // Parse content array (optional)
                let content = if let Some(content_array) =
                    item_data.get("content").and_then(|c| c.as_array())
                {
                    let mut content_items = Vec::new();
                    for content_item in content_array {
                        if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                            content_items.push(
                                codex_protocol::models::ReasoningItemContent::ReasoningText {
                                    text: text.to_string(),
                                },
                            );
                        }
                    }
                    if content_items.is_empty() {
                        None
                    } else {
                        Some(content_items)
                    }
                } else {
                    None
                };

                let encrypted_content = item_data
                    .get("encrypted_content")
                    .and_then(|e| e.as_str())
                    .map(std::string::ToString::to_string);

                Ok(Some(codex_protocol::models::ResponseItem::Reasoning {
                    id,
                    summary,
                    content,
                    encrypted_content,
                }))
            }

            _ => {
                // Unknown item type, skip it
                Ok(None)
            }
        }
    }
}

impl Default for GptOpenapiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GptOpenapiAdapter {
    fn name(&self) -> &str {
        "gpt_openapi"
    }

    fn supports_previous_response_id(&self) -> bool {
        // GptOpenapiAdapter supports OpenAI-compatible gateways
        // that implement previous_response_id for conversation continuity
        true
    }

    fn validate_provider(&self, provider: &ModelProviderInfo) -> Result<()> {
        // GptOpenapiAdapter only supports Responses API format
        // Reject configurations using Chat Completions API
        if provider.wire_api != crate::model_provider_info::WireApi::Responses {
            return Err(crate::error::CodexErr::Fatal(format!(
                "GptOpenapiAdapter requires wire_api = \"responses\". \
                     Current configuration uses wire_api = \"{:?}\". \
                     Please update your config to set wire_api = \"responses\" \
                     for provider '{}'.",
                provider.wire_api, provider.name
            )));
        }
        Ok(())
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        // Get model name from provider config
        let model = provider.model_name.as_ref().ok_or_else(|| {
            crate::error::CodexErr::Fatal(
                "Provider must specify model_name when using gpt_openapi adapter".into(),
            )
        })?;

        // Minimal transformation - pass through in OpenAI-compatible format
        let mut request = json!({
            "model": model,
            "input": prompt.input,
            "stream": provider.streaming,
        });

        // Bind tools if present
        let tools_json = crate::tools::spec::create_tools_json_for_responses_api(&prompt.tools)?;
        if !tools_json.is_empty() {
            request["tools"] = json!(tools_json);
            request["parallel_tool_calls"] = json!(prompt.parallel_tool_calls);
        }

        // Apply effective model parameters
        // Note: Adapters decide how to map these to API-specific names
        let params = &prompt.effective_parameters;
        if let Some(temp) = params.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(top_p) = params.top_p {
            request["top_p"] = json!(top_p);
        }
        if let Some(freq_penalty) = params.frequency_penalty {
            request["frequency_penalty"] = json!(freq_penalty);
        }
        if let Some(pres_penalty) = params.presence_penalty {
            request["presence_penalty"] = json!(pres_penalty);
        }
        if let Some(max_tokens) = params.max_tokens {
            // Note: Using max_output_tokens for Responses API
            request["max_output_tokens"] = json!(max_tokens);
        }

        // Add previous_response_id if present (for Responses API conversation continuity)
        if let Some(prev_id) = &prompt.previous_response_id {
            request["previous_response_id"] = json!(prev_id);
        }

        // Add reasoning parameters if present (for Responses API)
        if let Some(effort) = prompt.reasoning_effort {
            request["reasoning"] = json!({
                "effort": effort,
                "summary": prompt.reasoning_summary,
            });

            // Request encrypted content for reasoning models
            request["include"] = json!(["reasoning.encrypted_content"]);
        }

        Ok(request)
    }

    fn transform_response_chunk(
        &self,
        chunk: &str,
        context: &mut AdapterContext,
        provider: &ModelProviderInfo,
    ) -> Result<Vec<ResponseEvent>> {
        // GptOpenapiAdapter only supports Responses API format
        // (wire_api validation ensures this is configured correctly)

        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Branch based on streaming configuration
        if provider.streaming {
            // ========== Streaming mode: Parse SSE chunks ==========
            // Use Responses API parser with stateful context
            let state_key = "responses_parser_state";
            let mut parser = if let Some(state_json) = context.state.get(state_key) {
                serde_json::from_value(state_json.clone())?
            } else {
                super::openai_common::ResponsesApiParserState::new()
            };

            let events = parser.parse_chunk(chunk)?;
            context
                .state
                .insert(state_key.to_string(), serde_json::to_value(&parser)?);

            Ok(events)
        } else {
            // ========== Non-streaming mode: Parse complete JSON ==========
            // chunk contains the full response body, parse it once
            Self::parse_complete_responses_json(chunk)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_adapter_name() {
        let adapter = GptOpenapiAdapter::new();
        assert_eq!(adapter.name(), "gpt_openapi");
    }

    #[test]
    fn test_default_trait() {
        let adapter = GptOpenapiAdapter::default();
        assert_eq!(adapter.name(), "gpt_openapi");
    }

    #[test]
    fn test_validate_provider_accepts_responses_api() {
        let adapter = GptOpenapiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Responses;
        provider.name = "test_provider".to_string();

        let result = adapter.validate_provider(&provider);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_provider_rejects_chat_api() {
        let adapter = GptOpenapiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Chat;
        provider.name = "test_provider".to_string();

        let result = adapter.validate_provider(&provider);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            crate::error::CodexErr::Fatal(msg) => {
                assert!(msg.contains("requires wire_api = \"responses\""));
                assert!(msg.contains("test_provider"));
            }
            _ => panic!("Expected CodexErr::Fatal"),
        }
    }

    #[test]
    fn test_transform_request_basic() {
        let adapter = GptOpenapiAdapter::new();
        let prompt = Prompt {
            input: vec![],
            ..Default::default()
        };
        let mut provider = ModelProviderInfo::default();
        provider.model_name = Some("gpt-4".to_string());

        let request = adapter.transform_request(&prompt, &provider).unwrap();

        assert_eq!(request["stream"], json!(true));
        assert_eq!(request["model"], json!("gpt-4"));
        assert!(request.get("input").is_some());
    }

    #[test]
    fn test_transform_response_empty_chunk() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        let events = adapter
            .transform_response_chunk("", &mut ctx, &provider)
            .unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_transform_response_text_delta() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        // Responses API format - JSON with event field
        let chunk = r#"{"event":"response.output_text.delta","delta":"Hello from gateway"}"#;
        let events = adapter
            .transform_response_chunk(chunk, &mut ctx, &provider)
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Hello from gateway"),
            _ => panic!("Expected OutputTextDelta"),
        }
    }

    #[test]
    fn test_transform_response_completion() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        // Add content first using Responses API format
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_text.delta","delta":"Test"}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();

        // Responses API format for completion - just returns Completed event
        let chunk = r#"{"event":"response.done","response":{"id":"resp-gateway-123"},"usage":{"input_tokens":10,"output_tokens":5}}"#;
        let events = adapter
            .transform_response_chunk(chunk, &mut ctx, &provider)
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::Completed { token_usage, .. } => {
                assert!(token_usage.is_some());
            }
            _ => panic!("Expected Completed, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_transform_response_invalid_json() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        let chunk = r#"{"invalid json"#;
        let result = adapter.transform_response_chunk(chunk, &mut ctx, &provider);

        assert!(result.is_err());
    }

    #[test]
    fn test_responses_api_text_and_done() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        // First, add an output item to set current_item
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_item.added","item":{"type":"message","id":"msg-1"}}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();

        // Send text delta
        let events1 = adapter
            .transform_response_chunk(
                r#"{"event":"response.output_text.delta","delta":"Done"}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();
        assert_eq!(events1.len(), 1);
        match &events1[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Done"),
            _ => panic!("Expected OutputTextDelta, got {:?}", events1[0]),
        }

        // Send output_item done event - now returns OutputItemDone because current_item exists
        let events2 = adapter
            .transform_response_chunk(
                r#"{"event":"response.output_item.done"}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();
        assert_eq!(events2.len(), 1);
        match &events2[0] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone, got {:?}", events2[0]),
        }
    }

    #[test]
    fn test_done_signal() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        // Add some content using Responses API format
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_text.delta","delta":"Test"}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();

        // Send response.done event - returns only Completed event
        let events = adapter
            .transform_response_chunk(
                r#"{"event":"response.done","response":{"id":"resp-123"}}"#,
                &mut ctx,
                &provider,
            )
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed, got {:?}", events[0]),
        }
    }
}
