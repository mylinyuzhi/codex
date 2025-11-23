//! GPT OpenAPI adapter for enterprise LLM gateway
//!
//! This adapter is designed for enterprise internal LLM gateways that are
//! compatible with OpenAI Responses API format. It performs minimal transformation
//! since the gateway already speaks OpenAI protocol.
//!
//! # Requirements
//!
//! **This adapter ONLY supports `wire_api = "responses"`**. Configuration validation
//! will reject providers with `wire_api = "chat"`.
//!

use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestContext;
use crate::adapters::RequestMetadata;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_json::json;

/// Token usage details for complete (non-streaming) responses
#[derive(Debug, Deserialize)]
struct CompleteResponseUsage {
    input_tokens: i64,
    input_tokens_details: Option<CompleteResponseInputTokensDetails>,
    output_tokens: i64,
    output_tokens_details: Option<CompleteResponseOutputTokensDetails>,
}

/// Input token details for complete responses
#[derive(Debug, Deserialize)]
struct CompleteResponseInputTokensDetails {
    cached_tokens: i64,
}

/// Output token details for complete responses
#[derive(Debug, Deserialize)]
struct CompleteResponseOutputTokensDetails {
    reasoning_tokens: i64,
}

#[derive(Debug)]
pub struct GptAdapter;

impl GptAdapter {
    /// Create a new GPT OpenAPI adapter instance
    pub fn new() -> Self {
        Self
    }


    /// Parse complete (non-streaming) Responses API JSON response
    fn parse_complete_responses_json(body: &str) -> Result<Vec<ResponseEvent>> {
        let data: JsonValue = serde_json::from_str(body)?;

        let mut events = Vec::new();

        // Extract response ID
        let response_id = data
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        tracing::debug!("response_id '{}'", response_id);

        // Parse output items
        if let Some(output_array) = data.get("output").and_then(|o| o.as_array()) {
            for item_data in output_array {
                if let Some(item) = Self::parse_output_item(item_data)? {
                    events.push(ResponseEvent::OutputItemDone(item));
                }
            }
        }

        // Parse token usage with proper nested structure handling
        let token_usage = data.get("usage").and_then(|u| {
            serde_json::from_value::<CompleteResponseUsage>(u.clone())
                .ok()
                .map(|usage| crate::protocol::TokenUsage {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage
                        .input_tokens_details
                        .map(|d| d.cached_tokens)
                        .unwrap_or(0),
                    output_tokens: usage.output_tokens,
                    reasoning_output_tokens: usage
                        .output_tokens_details
                        .map(|d| d.reasoning_tokens)
                        .unwrap_or(0),
                    total_tokens: usage.input_tokens + usage.output_tokens,
                })
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
        tracing::debug!(
            item_data = ?item_data,
            "Parsing output item from LLM response"
        );

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
                            "output_text" => {
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

impl Default for GptAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GptAdapter {
    fn name(&self) -> &str {
        "gpt_openapi"
    }

    fn supports_previous_response_id(&self) -> bool {
        // GptAdapter supports OpenAI-compatible gateways
        // that implement previous_response_id for conversation continuity
        true
    }

    fn validate_provider(&self, provider: &ModelProviderInfo) -> Result<()> {
        // GptAdapter only supports Responses API format
        // Reject configurations using Chat Completions API
        if provider.wire_api != crate::model_provider_info::WireApi::Responses {
            return Err(crate::error::CodexErr::Fatal(format!(
                "GptAdapter requires wire_api = \"responses\". \
                     Current configuration uses wire_api = \"{:?}\". \
                     Please update your config to set wire_api = \"responses\" \
                     for provider '{}'.",
                provider.wire_api, provider.name
            )));
        }
        Ok(())
    }

    fn build_request_metadata(
        &self,
        _prompt: &Prompt,
        _provider: &ModelProviderInfo,
        context: &RequestContext,
    ) -> Result<RequestMetadata> {
        let mut metadata = RequestMetadata::default();

        // Build extra header with session_id JSON
        // Format: {"session_id": "{conversation_id}"}
        let extra_json = json!({
            "session_id": context.conversation_id
        });

        metadata
            .headers
            .insert("extra".to_string(), extra_json.to_string());

        Ok(metadata)
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        // Get model name from provider config
        let model = provider.ext.model_name.as_ref().ok_or_else(|| {
            crate::error::CodexErr::Fatal(
                "Provider must specify model_name when using gpt_openapi adapter".into(),
            )
        })?;

        // Apply incremental filtering when previous_response_id exists
        let input = if let Some(prev_id) = &prompt.previous_response_id {
            match crate::adapters::filter_incremental_input(&prompt.input) {
                None => {
                    // No LLM items found - first turn, use full input
                    tracing::debug!(
                        previous_response_id = %prev_id,
                        input_len = prompt.input.len(),
                        "First turn (no LLM items) - using full input"
                    );
                    &prompt.input[..]
                }
                Some(slice) if slice.is_empty() => {
                    // LLM item is last - no user input after, error state
                    let item_types = crate::adapters::get_item_type_names(&prompt.input);
                    tracing::warn!(
                        previous_response_id = %prev_id,
                        original_len = prompt.input.len(),
                        item_types = ?item_types,
                        "Incremental mode error: no user input after last LLM response",
                    );
                    return Err(crate::error::CodexErr::Fatal(format!(
                        "Incremental mode error: no user input after last LLM response (previous_response_id={})",
                        prev_id
                    )));
                }
                Some(slice) => {
                    // Normal incremental mode - use filtered slice
                    let item_types = crate::adapters::get_item_type_names(slice);
                    tracing::debug!(
                        previous_response_id = %prev_id,
                        original_len = prompt.input.len(),
                        filtered_len = slice.len(),
                        filtered_count = prompt.input.len() - slice.len(),
                        item_types = ?item_types,
                        "Using incremental input mode"
                    );
                    slice
                }
            }
        } else {
            // No previous_response_id - use full input
            &prompt.input[..]
        };

        // Minimal transformation - pass through in OpenAI-compatible format
        let mut request = json!({
            "model": model,
            "input": input,  // Use filtered input
            "stream": provider.ext.streaming,
            "store": true,
            // ResponseAPI required always pass instructions
            "instructions": prompt.base_instructions_override,
        });

        // Bind tools if present
        let tools_json = crate::tools::spec::create_tools_json_for_responses_api(&prompt.tools)?;
        if !tools_json.is_empty() {
            request["tools"] = json!(tools_json);
            request["parallel_tool_calls"] = json!(prompt.parallel_tool_calls);
        }

        // Apply effective model parameters from context
        // Note: Adapters decide how to map these to API-specific names
        let params = &context.effective_parameters;
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
        }else{
            tracing::debug!(
                "instructions" = prompt.base_instructions_override,
                "Using instructions"
            )
        }

        // Add reasoning parameters if present (for Responses API)
        if let Some(effort) = context.reasoning_effort {
            request["reasoning"] = json!({
                "effort": effort,
                "summary": context.reasoning_summary,
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
        // GptAdapter only supports Responses API format
        // (wire_api validation ensures this is configured correctly)

        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Branch based on streaming configuration
        if provider.ext.streaming {
            // ========== Streaming mode: Parse SSE chunks ==========
            // Use Responses API parser with stateful context
            let state_key = "responses_parser_state";
            let mut parser = if let Some(state_json) = context.state.get(state_key) {
                serde_json::from_value(state_json.clone())?
            } else {
                crate::adapters::openai_common::ResponsesApiParserState::new()
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
        let adapter = GptAdapter::new();
        assert_eq!(adapter.name(), "gpt_openapi");
    }

    #[test]
    fn test_default_trait() {
        let adapter = GptAdapter::default();
        assert_eq!(adapter.name(), "gpt_openapi");
    }

    #[test]
    fn test_validate_provider_accepts_responses_api() {
        let adapter = GptAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Responses;
        provider.name = "test_provider".to_string();

        let result = adapter.validate_provider(&provider);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_provider_rejects_chat_api() {
        let adapter = GptAdapter::new();
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
        let adapter = GptAdapter::new();
        let prompt = Prompt {
            input: vec![],
            ..Default::default()
        };
        let mut provider = ModelProviderInfo::default();
        provider.ext.model_name = Some("gpt-4".to_string());

        let context = crate::adapters::RequestContext {
            conversation_id: "test-conv".to_string(),
            session_source: "Test".to_string(),
            effective_parameters: Default::default(),
            reasoning_effort: None,
            reasoning_summary: None,
        };

        let request = adapter
            .transform_request(&prompt, &context, &provider)
            .unwrap();

        assert_eq!(request["stream"], json!(true));
        assert_eq!(request["model"], json!("gpt-4"));
        assert!(request.get("input").is_some());
    }

    #[test]
    fn test_transform_response_empty_chunk() {
        let adapter = GptAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        let events = adapter
            .transform_response_chunk("", &mut ctx, &provider)
            .unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_transform_response_text_delta() {
        let adapter = GptAdapter::new();
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
        let adapter = GptAdapter::new();
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
        let chunk = r#"{"event":"response.done","response":{"id":"resp-gateway-123"},"usage":{"input_tokens":10,"output_tokens":5,"input_tokens_details":{"cached_tokens":2},"output_tokens_details":{"reasoning_tokens":3}}}"#;
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
        let adapter = GptAdapter::new();
        let mut ctx = AdapterContext::new();
        let provider = ModelProviderInfo::default();

        let chunk = r#"{"invalid json"#;
        let result = adapter.transform_response_chunk(chunk, &mut ctx, &provider);

        assert!(result.is_err());
    }

    #[test]
    fn test_responses_api_text_and_done() {
        let adapter = GptAdapter::new();
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
        let adapter = GptAdapter::new();
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

    #[test]
    fn test_parse_complete_responses_with_token_details() {
        let adapter = GptAdapter::new();
        let mut ctx = AdapterContext::new();
        let mut provider = ModelProviderInfo::default();
        provider.ext.streaming = false; // Non-streaming mode

        // Complete non-streaming response with full token details
        let body = r#"{
            "id": "resp-123",
            "model": "gpt-4",
            "output": [{
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "content": [{"type": "text", "text": "Hello"}]
            }],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "input_tokens_details": {"cached_tokens": 20},
                "output_tokens_details": {"reasoning_tokens": 15}
            },
            "status": "completed"
        }"#;

        let events = adapter
            .transform_response_chunk(body, &mut ctx, &provider)
            .unwrap();

        // Should have OutputItemDone + Completed events
        assert_eq!(events.len(), 2);

        // First event should be OutputItemDone
        match &events[0] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone, got {:?}", events[0]),
        }

        // Second event should be Completed with full token details
        match &events[1] {
            ResponseEvent::Completed {
                response_id,
                token_usage,
            } => {
                assert_eq!(response_id, "resp-123");
                let usage = token_usage.as_ref().expect("Should have token usage");
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
                assert_eq!(usage.cached_input_tokens, 20);
                assert_eq!(usage.reasoning_output_tokens, 15);
                assert_eq!(usage.total_tokens, 150);
            }
            _ => panic!("Expected Completed, got {:?}", events[1]),
        }
    }
}
