//! Passthrough adapter - uses standard OpenAI Chat Completions format
//!
//! This adapter is useful for:
//! - Testing the adapter framework
//! - OpenAI-compatible APIs that already speak Chat Completions format
//! - Debugging adapter flow
//! - Serving as a reference implementation
//!
//! # Format
//!
//! This adapter expects standard OpenAI Chat Completions streaming format:
//! ```json
//! {
//!   "choices": [{
//!     "delta": {"content": "Hello"},
//!     "finish_reason": null
//!   }]
//! }
//! ```
//!
//! See [`super::openai_common::ChatCompletionsParserState`] for details.

use super::AdapterContext;
use super::ProviderAdapter;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use serde_json::Value as JsonValue;
use serde_json::json;

/// Passthrough adapter that uses standard OpenAI Chat Completions format
///
/// This adapter uses the full Chat Completions parser extracted from the
/// built-in implementation, supporting:
/// - Text deltas (`choices[0].delta.content`)
/// - Reasoning content (`choices[0].delta.reasoning`)
/// - Tool calls (`choices[0].delta.tool_calls`)
/// - `[DONE]` signal handling
///
/// # Use Cases
///
/// - **Testing**: Verify the adapter framework works correctly
/// - **Debugging**: Trace request/response flow through the adapter system
/// - **OpenAI-compatible**: Use with providers that already speak OpenAI format
///
/// # Dynamic Headers & Metadata
///
/// This adapter uses the **default `build_request_metadata()` implementation**,
/// which means it does NOT add any dynamic HTTP headers or query parameters.
///
/// If you need to add custom headers (e.g., session tracking, log correlation),
/// you should:
/// 1. Create a custom adapter that implements `build_request_metadata()`
/// 2. Use static headers in `ModelProviderInfo.http_headers` configuration
///
/// # Example
///
/// ```toml
/// [model_providers.my_openai_compatible_provider]
/// name = "My Provider"
/// base_url = "https://api.example.com/v1"
/// env_key = "MY_PROVIDER_KEY"
/// adapter = "passthrough"  # Use passthrough adapter
/// wire_api = "chat"        # Uses /chat/completions endpoint
/// ```
#[derive(Debug)]
pub struct PassthroughAdapter;

impl ProviderAdapter for PassthroughAdapter {
    fn name(&self) -> &str {
        "passthrough"
    }

    fn supports_previous_response_id(&self) -> bool {
        // PassthroughAdapter supports OpenAI Responses API format
        // which includes previous_response_id for conversation continuity
        true
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        _provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        // Pass through unchanged - standard OpenAI format
        //
        // Supports both Chat Completions and Responses API formats
        let mut request = json!({
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
        //
        // Responses API events have "event" field: {"event":"response.output_text.delta",...}
        // Chat Completions has "choices" field: {"choices":[{"delta":{"content":"..."}}]}
        //
        // Strategy: Check first chunk to determine format, then stick with that parser

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

    #[test]
    fn test_passthrough_name() {
        let adapter = PassthroughAdapter;
        assert_eq!(adapter.name(), "passthrough");
    }

    #[test]
    fn test_transform_request() {
        let adapter = PassthroughAdapter;
        let prompt = Prompt {
            input: vec![],
            ..Default::default()
        };
        let provider = ModelProviderInfo::default();

        let request = adapter.transform_request(&prompt, &provider).unwrap();

        // Should have stream: true
        assert_eq!(request["stream"], json!(true));

        // Should have input field
        assert!(request.get("input").is_some());
    }

    #[test]
    fn test_transform_response_empty_chunk() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        let events = adapter.transform_response_chunk("", &mut ctx).unwrap();
        assert!(events.is_empty(), "Empty chunk should produce no events");
    }

    #[test]
    fn test_transform_response_whitespace_chunk() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        let events = adapter
            .transform_response_chunk("   \n  \t  ", &mut ctx)
            .unwrap();
        assert!(
            events.is_empty(),
            "Whitespace-only chunk should produce no events"
        );
    }

    #[test]
    fn test_transform_response_text_delta() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        // Real Chat Completions format
        let chunk = r#"{"choices":[{"delta":{"content":"Hello world"}}]}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Hello world"),
            _ => panic!("Expected OutputTextDelta, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_transform_response_completion() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        // Add some content first
        adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":"Test"}}]}"#, &mut ctx)
            .unwrap();

        // Then finish
        let chunk = r#"{"choices":[{"finish_reason":"stop"}]}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            ResponseEvent::OutputItemDone(_) => {}
            _ => panic!("Expected OutputItemDone, got {:?}", events[0]),
        }
        match &events[1] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed, got {:?}", events[1]),
        }
    }

    #[test]
    fn test_transform_response_done_signal() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        // Add some content
        adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#, &mut ctx)
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

    #[test]
    fn test_transform_response_invalid_json() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        let chunk = r#"{"invalid": json missing brace"#;
        let result = adapter.transform_response_chunk(chunk, &mut ctx);

        assert!(result.is_err(), "Invalid JSON should return error");
    }

    #[test]
    fn test_context_preserves_state() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        // First chunk
        adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#, &mut ctx)
            .unwrap();

        // Second chunk - state should be preserved
        let events = adapter
            .transform_response_chunk(r#"{"choices":[{"delta":{"content":" world"}}]}"#, &mut ctx)
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, " world"),
            _ => panic!("Expected OutputTextDelta"),
        }

        // Finish - should combine both chunks
        let events = adapter
            .transform_response_chunk(r#"{"choices":[{"finish_reason":"stop"}]}"#, &mut ctx)
            .unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            ResponseEvent::OutputItemDone(item) => {
                use codex_protocol::models::ContentItem;
                use codex_protocol::models::ResponseItem;
                match item {
                    ResponseItem::Message { content, .. } => {
                        assert_eq!(content.len(), 1);
                        match &content[0] {
                            ContentItem::OutputText { text } => {
                                assert_eq!(text, "Hello world");
                            }
                            _ => panic!("Expected OutputText"),
                        }
                    }
                    _ => panic!("Expected Message"),
                }
            }
            _ => panic!("Expected OutputItemDone"),
        }
    }

    #[test]
    fn test_reasoning_delta() {
        let adapter = PassthroughAdapter;
        let mut ctx = AdapterContext::new();

        let chunk = r#"{"choices":[{"delta":{"reasoning":"Thinking..."}}]}"#;
        let events = adapter.transform_response_chunk(chunk, &mut ctx).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::ReasoningContentDelta(text) => assert_eq!(text, "Thinking..."),
            _ => panic!("Expected ReasoningContentDelta"),
        }
    }
}
