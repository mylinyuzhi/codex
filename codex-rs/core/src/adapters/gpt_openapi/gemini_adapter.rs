//! Gemini adapter for OpenAPI-style Gemini models
//!
//! This adapter implements support for Google Gemini models via OpenAI-compatible
//! Chat Completions API format. It supports:
//!
//! - Text and multimodal messages (images)
//! - Standard function calling
//! - Gemini-specific thinking/reasoning with configurable token budgets
//! - Non-streaming mode (streaming support planned for future)
//!
//! # Gemini Thinking Budget
//!
//! Gemini models support a "thinking" parameter that controls reasoning:
//!
//! - **Default (unset)**: Dynamic thinking - model decides when and how much to think
//! - **`budget_tokens = -1`**: Explicit dynamic thinking
//! - **`budget_tokens = 0`**: Disable thinking (Gemini 2.5 Flash only)
//! - **`budget_tokens > 0`**: Fixed token budget (128-32768 for Pro, 0-24576 for Flash)
//!
//! Configure via `ModelParameters.budget_tokens` and `include_thoughts`.

use crate::adapters::AdapterContext;
use crate::adapters::ProviderAdapter;
use crate::adapters::RequestContext;
use crate::adapters::RequestMetadata;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::CodexErr;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use codex_protocol::config_types_ext::ModelParameters;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use serde_json::Value as JsonValue;
use serde_json::json;

/// Gemini adapter for OpenAPI-compatible Chat Completions format
///
/// This adapter is designed for Gemini models accessed via OpenAI-compatible
/// Chat Completions API endpoints. It performs request/response transformation
/// while preserving Gemini-specific features like thinking budgets.
#[derive(Debug, Clone)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    /// Create a new Gemini adapter instance
    pub fn new() -> Self {
        Self
    }

    /// Build Gemini thinking parameter from ModelParameters
    ///
    /// Returns None if budget_tokens is not set (use Gemini default).
    /// Returns Some with thinking config if budget_tokens is set.
    ///
    /// # Validation
    ///
    /// - Accepts -1 (dynamic thinking)
    /// - Accepts 0 (disable thinking, Flash only)
    /// - Accepts 1-32768 (fixed budget)
    /// - Rejects values < -1 or > 32768
    fn build_thinking_param(params: &ModelParameters) -> Result<Option<JsonValue>> {
        let budget = match params.budget_tokens {
            None => return Ok(None), // Use Gemini default (dynamic)
            Some(b) => b,
        };

        // Validate range (union of Pro and Flash ranges)
        if budget < -1 || budget > 32768 {
            return Err(CodexErr::Fatal(format!(
                "Invalid budget_tokens: {}. Valid range: -1 (dynamic), 0 (disable for Flash), or 1-32768",
                budget
            )));
        }

        Ok(Some(json!({
            "include_thoughts": params.include_thoughts.unwrap_or(true),
            "budget_tokens": budget
        })))
    }

    /// Convert ResponseItem slice to Gemini messages array format
    ///
    /// Optionally prepends a system message if instructions are provided.
    ///
    /// Transforms:
    /// - System instructions → {role: "system", content: "..."}
    /// - ResponseItem::Message → {role, content: [...]}
    /// - ResponseItem::FunctionCall → assistant message with tool_calls
    /// - ResponseItem::FunctionCallOutput → tool role message
    /// - ResponseItem::Reasoning → skipped (from previous responses)
    fn transform_response_items_to_messages(
        items: &[ResponseItem],
        system_instructions: Option<&str>,
    ) -> Result<Vec<JsonValue>> {
        let mut messages = Vec::new();

        // Prepend system message if instructions are provided
        // This is more efficient than insert(0, ...) which requires O(n) shift
        if let Some(instructions) = system_instructions {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }

        for item in items {
            match item {
                ResponseItem::Message { role, content, .. } => {
                    messages.push(json!({
                        "role": role,
                        "content": Self::transform_content_items(content)?
                    }));
                }
                ResponseItem::FunctionCall {
                    name,
                    arguments,
                    call_id,
                    ..
                } => {
                    messages.push(json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments
                            }
                        }]
                    }));
                }
                ResponseItem::FunctionCallOutput {
                    call_id, output, ..
                } => {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": output.content
                    }));
                }
                ResponseItem::Reasoning { .. } => {
                    // Skip reasoning from previous responses
                }
                _ => {
                    // Skip other item types
                }
            }
        }

        Ok(messages)
    }

    /// Transform ContentItem slice to Gemini content format
    ///
    /// Supports:
    /// - Text (InputText, OutputText)
    /// - Images (InputImage with data URL or https URL)
    fn transform_content_items(items: &[ContentItem]) -> Result<Vec<JsonValue>> {
        let mut content = Vec::new();

        for item in items {
            match item {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                    content.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
                ContentItem::InputImage { image_url } => {
                    content.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": image_url
                        }
                    }));
                }
            }
        }

        Ok(content)
    }

    /// Parse complete (non-streaming) Chat Completions JSON response
    ///
    /// Extracts:
    /// 1. Reasoning content (if present) → OutputItemDone(Reasoning)
    /// 2. Tool calls (if present) → OutputItemDone(FunctionCall) for each
    /// 3. Message content (if present) → OutputItemDone(Message)
    /// 4. Token usage → Completed event
    fn parse_complete_chat_json(body: &str) -> Result<Vec<ResponseEvent>> {
        let data: JsonValue = serde_json::from_str(body)?;
        let mut events = Vec::new();

        // Check for error
        if let Some(error) = data.get("error") {
            return Err(Self::parse_gemini_error(error)?);
        }

        // Extract choices array with strict validation
        let choices = data
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                CodexErr::Stream(
                    "Missing or invalid 'choices' array in response".into(),
                    None,
                )
            })?;

        // Ensure choices array is not empty
        if choices.is_empty() {
            return Err(CodexErr::Stream(
                "Empty 'choices' array in response".into(),
                None,
            ));
        }

        // Extract message from choices[0]
        let message = choices[0].get("message").ok_or_else(|| {
            CodexErr::Stream("Missing 'message' field in choices[0]".into(), None)
        })?;

        // 1. Parse thinking content FIRST (if present)
        if let Some(reasoning) = message.get("reasoning").and_then(|r| r.as_str()) {
            if !reasoning.is_empty() {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: String::new(),
                    summary: Vec::new(),
                    content: Some(vec![ReasoningItemContent::ReasoningText {
                        text: reasoning.to_string(),
                    }]),
                    encrypted_content: None,
                }));
            }
        }

        // 2. Parse tool calls (if present) with strict validation
        if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
            for (idx, tool_call) in tool_calls.iter().enumerate() {
                // Validate required fields
                let id = tool_call
                    .get("id")
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| {
                        CodexErr::Stream(format!("Missing 'id' field in tool_calls[{}]", idx), None)
                    })?;

                let function = tool_call.get("function").ok_or_else(|| {
                    CodexErr::Stream(
                        format!("Missing 'function' field in tool_calls[{}]", idx),
                        None,
                    )
                })?;

                let name = function
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| {
                        CodexErr::Stream(
                            format!("Missing 'name' in tool_calls[{}].function", idx),
                            None,
                        )
                    })?;

                // Arguments can be empty, so use default if missing
                let arguments = function
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");

                events.push(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    id: None,
                    name: name.to_string(),
                    call_id: id.to_string(),
                    arguments: arguments.to_string(),
                }));
            }
        }

        // 3. Parse content (if present and no tool calls)
        if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: content.to_string(),
                    }],
                }));
            }
        }

        // 4. Extract token usage
        let token_usage = data.get("usage").map(|u| {
            crate::protocol::TokenUsage {
                input_tokens: u.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                cached_input_tokens: 0, // Gemini may not report this
                output_tokens: u
                    .get("completion_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0),
                reasoning_output_tokens: 0, // Extract if Gemini reports separately
                total_tokens: u.get("total_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
            }
        });

        // 5. Completion event
        events.push(ResponseEvent::Completed {
            response_id: data
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string(),
            token_usage,
        });

        Ok(events)
    }

    /// Parse Gemini error response and classify into appropriate CodexErr
    fn parse_gemini_error(error: &JsonValue) -> Result<CodexErr> {
        let code = error.get("code").and_then(|c| c.as_str()).unwrap_or("");
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");

        Ok(match code {
            "context_length_exceeded" | "invalid_argument"
                if message.to_lowercase().contains("context") =>
            {
                CodexErr::ContextWindowExceeded
            }
            "resource_exhausted" | "insufficient_quota" => CodexErr::QuotaExceeded,
            "unauthenticated" | "permission_denied" => {
                CodexErr::Fatal(format!("Authentication error: {}", message))
            }
            _ => CodexErr::Stream(message.to_string(), None),
        })
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini_openapi"
    }

    fn supports_previous_response_id(&self) -> bool {
        // Gemini Chat API doesn't support conversation continuity via response_id
        false
    }

    fn validate_provider(&self, provider: &ModelProviderInfo) -> Result<()> {
        // Require Chat API (not Responses API)
        if provider.wire_api != crate::model_provider_info::WireApi::Chat {
            return Err(CodexErr::Fatal(format!(
                "GeminiAdapter requires wire_api = \"chat\". \
                 Current configuration uses wire_api = \"{:?}\". \
                 Please update your config to set wire_api = \"chat\" \
                 for provider '{}'.",
                provider.wire_api, provider.name
            )));
        }

        // Only support non-streaming for initial version
        if provider.ext.streaming {
            return Err(CodexErr::Fatal(
                "GeminiAdapter: streaming mode not yet supported. \
                 Set streaming = false in provider configuration."
                    .into(),
            ));
        }

        Ok(())
    }

    fn build_request_metadata(
        &self,
        _prompt: &Prompt,
        _provider: &ModelProviderInfo,
        _context: &RequestContext,
    ) -> Result<RequestMetadata> {
        // No special headers needed for Gemini Chat API
        Ok(RequestMetadata::default())
    }

    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue> {
        let model = provider.ext.model_name.as_ref().ok_or_else(|| {
            CodexErr::Fatal(
                "Provider must specify model_name when using gemini_openapi adapter".into(),
            )
        })?;

        // Transform messages with optional system instructions
        let messages = Self::transform_response_items_to_messages(
            &prompt.input,
            prompt.base_instructions_override.as_deref(),
        )?;

        // Build base request
        let mut request = json!({
            "model": model,
            "messages": messages,
            "stream": false
        });

        // Add tools if present
        let tools = crate::tools::spec::create_tools_json_for_chat_completions_api(&prompt.tools)?;
        if !tools.is_empty() {
            request["tools"] = json!(tools);
            request["tool_choice"] = json!("auto");
        }

        // Add thinking parameter (Gemini-specific)
        let params = &context.effective_parameters;
        if let Some(thinking) = Self::build_thinking_param(params)? {
            request["thinking"] = thinking;
        }

        // Add standard parameters
        if let Some(temp) = params.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = params.max_tokens {
            request["max_tokens"] = json!(max_tokens);
        }
        if let Some(top_p) = params.top_p {
            request["top_p"] = json!(top_p);
        }

        // Add output schema if present (response_format)
        if let Some(schema) = &prompt.output_schema {
            request["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "codex_output_schema",
                    "schema": schema,
                    "strict": true
                }
            });
        }

        Ok(request)
    }

    fn transform_response_chunk(
        &self,
        chunk: &str,
        _context: &mut AdapterContext,
        _provider: &ModelProviderInfo,
    ) -> Result<Vec<ResponseEvent>> {
        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Non-streaming mode: parse complete JSON
        Self::parse_complete_chat_json(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputPayload;

    #[test]
    fn test_adapter_name() {
        let adapter = GeminiAdapter::new();
        assert_eq!(adapter.name(), "gemini_openapi");
    }

    #[test]
    fn test_default_trait() {
        let adapter = GeminiAdapter::default();
        assert_eq!(adapter.name(), "gemini_openapi");
    }

    #[test]
    fn test_supports_previous_response_id() {
        let adapter = GeminiAdapter::new();
        assert!(!adapter.supports_previous_response_id());
    }

    #[test]
    fn test_validate_provider_requires_chat_api() {
        let adapter = GeminiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Chat;
        provider.name = "test_gemini".to_string();
        provider.ext.streaming = false; // Gemini adapter only supports non-streaming

        let result = adapter.validate_provider(&provider);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_provider_rejects_responses_api() {
        let adapter = GeminiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Responses;
        provider.name = "test_gemini".to_string();

        let result = adapter.validate_provider(&provider);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            CodexErr::Fatal(msg) => {
                assert!(msg.contains("requires wire_api = \"chat\""));
                assert!(msg.contains("test_gemini"));
            }
            _ => panic!("Expected CodexErr::Fatal"),
        }
    }

    #[test]
    fn test_validate_provider_rejects_streaming() {
        let adapter = GeminiAdapter::new();
        let mut provider = ModelProviderInfo::default();
        provider.wire_api = crate::model_provider_info::WireApi::Chat;
        provider.ext.streaming = true;

        let result = adapter.validate_provider(&provider);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            CodexErr::Fatal(msg) => {
                assert!(msg.contains("streaming mode not yet supported"));
            }
            _ => panic!("Expected CodexErr::Fatal"),
        }
    }

    #[test]
    fn test_thinking_param_none() {
        let params = ModelParameters::default();
        let result = GeminiAdapter::build_thinking_param(&params).unwrap();
        assert!(result.is_none()); // Use Gemini default
    }

    #[test]
    fn test_thinking_param_dynamic() {
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(-1);
        params.include_thoughts = Some(true);

        let result = GeminiAdapter::build_thinking_param(&params)
            .unwrap()
            .unwrap();
        assert_eq!(result["budget_tokens"], -1);
        assert_eq!(result["include_thoughts"], true);
    }

    #[test]
    fn test_thinking_param_disabled() {
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(0);

        let result = GeminiAdapter::build_thinking_param(&params)
            .unwrap()
            .unwrap();
        assert_eq!(result["budget_tokens"], 0);
        assert_eq!(result["include_thoughts"], true); // Default
    }

    #[test]
    fn test_thinking_param_fixed_budget() {
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(5000);
        params.include_thoughts = Some(false);

        let result = GeminiAdapter::build_thinking_param(&params)
            .unwrap()
            .unwrap();
        assert_eq!(result["budget_tokens"], 5000);
        assert_eq!(result["include_thoughts"], false);
    }

    #[test]
    fn test_thinking_param_invalid_negative() {
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(-2);

        let result = GeminiAdapter::build_thinking_param(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_thinking_param_invalid_too_high() {
        let mut params = ModelParameters::default();
        params.budget_tokens = Some(50000);

        let result = GeminiAdapter::build_thinking_param(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_transform_text_message() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_transform_image_message() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![
                ContentItem::InputText {
                    text: "What's in this image?".to_string(),
                },
                ContentItem::InputImage {
                    image_url: "data:image/jpeg;base64,abc123".to_string(),
                },
            ],
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"].as_array().unwrap().len(), 2);
        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[0]["content"][1]["type"], "image_url");
        assert_eq!(
            messages[0]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,abc123"
        );
    }

    #[test]
    fn test_transform_tool_call() {
        let items = vec![ResponseItem::FunctionCall {
            id: None,
            name: "get_weather".to_string(),
            call_id: "call_123".to_string(),
            arguments: r#"{"location":"Beijing"}"#.to_string(),
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], json!(null));
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_123");
        assert_eq!(
            messages[0]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
    }

    #[test]
    fn test_transform_tool_output() {
        let items = vec![ResponseItem::FunctionCallOutput {
            call_id: "call_123".to_string(),
            output: FunctionCallOutputPayload {
                content: "The weather is sunny".to_string(),
                content_items: None,
                success: None,
            },
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "call_123");
        assert_eq!(messages[0]["content"], "The weather is sunny");
    }

    #[test]
    fn test_parse_basic_response() {
        let json = r#"{
            "id": "chatcmpl-123",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello, how can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        assert_eq!(events.len(), 2); // Message + Completed

        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentItem::OutputText { text } => {
                        assert_eq!(text, "Hello, how can I help?");
                    }
                    _ => panic!("Expected OutputText"),
                }
            }
            _ => panic!("Expected Message"),
        }

        match &events[1] {
            ResponseEvent::Completed {
                response_id,
                token_usage,
            } => {
                assert_eq!(response_id, "chatcmpl-123");
                let usage = token_usage.as_ref().unwrap();
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 20);
                assert_eq!(usage.total_tokens, 30);
            }
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_parse_response_with_thinking() {
        let json = r#"{
            "id": "chatcmpl-456",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning": "Let me think about this problem...",
                    "content": "The answer is 42"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 25,
                "total_tokens": 40
            }
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        assert_eq!(events.len(), 3); // Reasoning + Message + Completed

        // First event should be Reasoning
        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::Reasoning { content, .. }) => {
                let content = content.as_ref().unwrap();
                match &content[0] {
                    ReasoningItemContent::ReasoningText { text }
                    | ReasoningItemContent::Text { text } => {
                        assert_eq!(text, "Let me think about this problem...");
                    }
                }
            }
            _ => panic!("Expected Reasoning item first"),
        }

        // Second event should be Message
        match &events[1] {
            ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                match &content[0] {
                    ContentItem::OutputText { text } => {
                        assert_eq!(text, "The answer is 42");
                    }
                    _ => panic!("Expected OutputText"),
                }
            }
            _ => panic!("Expected Message"),
        }
    }

    #[test]
    fn test_parse_response_with_tool_call() {
        let json = r#"{
            "id": "chatcmpl-789",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\":\"test\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 10,
                "total_tokens": 15
            }
        }"#;

        let events = GeminiAdapter::parse_complete_chat_json(json).unwrap();
        assert_eq!(events.len(), 2); // FunctionCall + Completed

        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                name,
                call_id,
                arguments,
                ..
            }) => {
                assert_eq!(name, "search");
                assert_eq!(call_id, "call_abc");
                assert_eq!(arguments, "{\"query\":\"test\"}");
            }
            _ => panic!("Expected FunctionCall"),
        }
    }

    #[test]
    fn test_parse_error_context_exceeded() {
        let json = r#"{
            "error": {
                "code": "context_length_exceeded",
                "message": "Maximum context length exceeded"
            }
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::ContextWindowExceeded => {}
            _ => panic!("Expected ContextWindowExceeded"),
        }
    }

    #[test]
    fn test_parse_error_quota() {
        let json = r#"{
            "error": {
                "code": "resource_exhausted",
                "message": "Quota exceeded"
            }
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::QuotaExceeded => {}
            _ => panic!("Expected QuotaExceeded"),
        }
    }

    #[test]
    fn test_parse_error_auth() {
        let json = r#"{
            "error": {
                "code": "unauthenticated",
                "message": "Invalid API key"
            }
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::Fatal(msg) => {
                assert!(msg.contains("Authentication error"));
                assert!(msg.contains("Invalid API key"));
            }
            _ => panic!("Expected Fatal with auth error"),
        }
    }

    #[test]
    fn test_transform_with_system_instructions() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(
            &items,
            Some("You are a helpful assistant"),
        )
        .unwrap();

        assert_eq!(messages.len(), 2); // system + user
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_transform_without_system_instructions() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];

        let messages = GeminiAdapter::transform_response_items_to_messages(&items, None).unwrap();

        assert_eq!(messages.len(), 1); // Only user message
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_system_instructions_first_position() {
        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "First user message".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Assistant response".to_string(),
                }],
            },
        ];

        let messages =
            GeminiAdapter::transform_response_items_to_messages(&items, Some("System prompt"))
                .unwrap();

        assert_eq!(messages.len(), 3); // system + user + assistant
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "System prompt");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
    }

    #[test]
    fn test_parse_missing_choices_array() {
        let json = r#"{
            "id": "test-123",
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::Stream(msg, _) => {
                assert!(msg.contains("choices"));
            }
            _ => panic!("Expected Stream error for missing choices"),
        }
    }

    #[test]
    fn test_parse_empty_choices_array() {
        let json = r#"{
            "id": "test-123",
            "choices": [],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::Stream(msg, _) => {
                assert!(msg.contains("Empty"));
                assert!(msg.contains("choices"));
            }
            _ => panic!("Expected Stream error for empty choices"),
        }
    }

    #[test]
    fn test_parse_tool_call_missing_id() {
        let json = r#"{
            "id": "test-123",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "type": "function",
                        "function": {
                            "name": "test_function",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }"#;

        let result = GeminiAdapter::parse_complete_chat_json(json);
        assert!(result.is_err());

        match result.unwrap_err() {
            CodexErr::Stream(msg, _) => {
                assert!(msg.contains("id"));
                assert!(msg.contains("tool_calls"));
            }
            _ => panic!("Expected Stream error for missing tool call id"),
        }
    }
}
