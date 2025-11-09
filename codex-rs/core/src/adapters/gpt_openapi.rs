//! GPT OpenAPI adapter for enterprise LLM gateway
//!
//! This adapter is designed for enterprise internal LLM gateways that are
//! compatible with OpenAI format. It uses the Responses API and performs
//! minimal transformation since the gateway already speaks OpenAI protocol.
//!
//! # Use Cases
//!
//! - **Enterprise LLM Gateway**: Internal API gateway that proxies to various LLM providers
//! - **OpenAI-compatible**: Gateway that already uses OpenAI-compatible request/response format
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
/// OpenAI-compatible. It performs minimal transformation and uses the Responses API.
///
/// # Features
///
/// - **Minimal overhead**: Direct passthrough of requests/responses
/// - **Multi-configuration**: One adapter, multiple provider configurations
/// - **Responses API**: Uses OpenAI Responses API format
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
/// - Accepts OpenAI-compatible request format
/// - Returns OpenAI-compatible streaming responses
/// - Supports standard SSE (Server-Sent Events) streaming
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
        // Auto-detect API format and use appropriate parser
        // Enterprise gateways may use either Chat Completions or Responses API

        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Determine parser type from context or by inspecting chunk
        let parser_type = if let Some(ptype) = context.get_str("parser_type") {
            ptype
        } else {
            // First chunk - auto-detect format
            let detected = if chunk.contains(r#""event":"response."#) {
                "responses"
            } else {
                "chat"
            };
            context.set("parser_type", json!(detected));
            detected
        };

        let events = if parser_type == "responses" {
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
            events
        } else {
            // Use Chat Completions parser
            let state_key = "chat_parser_state";
            let mut parser = if let Some(state_json) = context.state.get(state_key) {
                serde_json::from_value(state_json.clone())?
            } else {
                super::openai_common::ChatCompletionsParserState::new()
            };

            let events = parser.parse_chunk(chunk)?;
            context
                .state
                .insert(state_key.to_string(), serde_json::to_value(&parser)?);
            events
        };

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

        // Real Chat Completions format
        let chunk = r#"{"choices":[{"delta":{"content":"Hello from gateway"}}]}"#;
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

        // Add content first
        adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":"Test"}}]}"#, &mut ctx)
            .unwrap();

        // Real Chat Completions format for completion
        let chunk = r#"{"id":"resp-gateway-123","choices":[{"finish_reason":"stop"}]}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone"),
        }
        match &events[1] {
            ResponseEvent::Completed { token_usage, .. } => {
                assert!(token_usage.is_none());
            }
            _ => panic!("Expected Completed"),
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
    fn test_multiple_events_in_single_chunk() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();

        // Real Chat Completions format with both delta and finish_reason
        let chunk =
            r#"{"id":"resp-123","choices":[{"delta":{"content":"Done"},"finish_reason":"stop"}]}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 3);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Done"),
            _ => panic!("Expected OutputTextDelta"),
        }
        match &events[1] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone"),
        }
        match &events[2] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_done_signal() {
        let adapter = GptOpenapiAdapter::new();
        let mut ctx = AdapterContext::new();

        // Add some content
        adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":"Test"}}]}"#, &mut ctx)
            .unwrap();

        // Send [DONE]
        let events = adapter
            .transform_response_chunk("[DONE]", &mut ctx)
            .unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone"),
        }
        match &events[1] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed"),
        }
    }
}
