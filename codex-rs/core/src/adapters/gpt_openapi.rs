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
            return Err(crate::error::CodexErr::Fatal(
                format!(
                    "GptOpenapiAdapter requires wire_api = \"responses\". \
                     Current configuration uses wire_api = \"{:?}\". \
                     Please update your config to set wire_api = \"responses\" \
                     for provider '{}'.",
                    provider.wire_api,
                    provider.name
                )
            ));
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
            "stream": true,
        });

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
    ) -> Result<Vec<ResponseEvent>> {
        // GptOpenapiAdapter only supports Responses API format
        // (wire_api validation ensures this is configured correctly)

        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Use Responses API parser
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

        let events = adapter.transform_response_chunk("", &mut ctx).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_transform_response_text_delta() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();

        // Responses API format - JSON with event field
        let chunk = r#"{"event":"response.output_text.delta","delta":"Hello from gateway"}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

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

        // Add content first using Responses API format
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_text.delta","delta":"Test"}"#,
                &mut ctx,
            )
            .unwrap();

        // Responses API format for completion - just returns Completed event
        let chunk = r#"{"event":"response.done","response":{"id":"resp-gateway-123"},"usage":{"input_tokens":10,"output_tokens":5}}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

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

        let chunk = r#"{"invalid json"#;
        let result = adapter.transform_response_chunk(chunk, &mut ctx);

        assert!(result.is_err());
    }

    #[test]
    fn test_responses_api_text_and_done() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();

        // First, add an output item to set current_item
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_item.added","item":{"type":"message","id":"msg-1"}}"#,
                &mut ctx,
            )
            .unwrap();

        // Send text delta
        let events1 = adapter
            .transform_response_chunk(r#"{"event":"response.output_text.delta","delta":"Done"}"#, &mut ctx)
            .unwrap();
        assert_eq!(events1.len(), 1);
        match &events1[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Done"),
            _ => panic!("Expected OutputTextDelta, got {:?}", events1[0]),
        }

        // Send output_item done event - now returns OutputItemDone because current_item exists
        let events2 = adapter
            .transform_response_chunk(r#"{"event":"response.output_item.done"}"#, &mut ctx)
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

        // Add some content using Responses API format
        adapter
            .transform_response_chunk(
                r#"{"event":"response.output_text.delta","delta":"Test"}"#,
                &mut ctx,
            )
            .unwrap();

        // Send response.done event - returns only Completed event
        let events = adapter
            .transform_response_chunk(
                r#"{"event":"response.done","response":{"id":"resp-123"}}"#,
                &mut ctx,
            )
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed, got {:?}", events[0]),
        }
    }
}
