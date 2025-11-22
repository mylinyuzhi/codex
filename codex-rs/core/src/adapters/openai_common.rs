//! OpenAI-compatible format parsers for adapter system
//!
//! This module provides parsers for standard OpenAI API formats, extracted from
//! the built-in implementations to enable code reuse across adapters.
//! (To be extracted from `client.rs:process_sse()`)

use crate::client_common::ResponseEvent;
use crate::error::Result;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// Parser state for OpenAI Chat Completions streaming format
///
/// This state machine accumulates data across multiple SSE chunks to handle:
/// - Assistant messages that arrive as multiple delta events
/// - Reasoning content that arrives incrementally
/// - Tool calls whose arguments arrive in fragments
///
/// # Memory Management
///
/// This parser is **serialized into `AdapterContext.state`** and persists for
/// the duration of a single streaming request.
///
/// ## Memory Accumulation Pattern
///
/// Fields grow as chunks arrive:
/// - `assistant_text`: Grows with each `delta.content` chunk
/// - `reasoning_text`: Grows with each `delta.reasoning` chunk
/// - `tool_call_arguments`: Grows as JSON fragments arrive
///
/// ## Typical Memory Usage (per request)
///
/// - **Short conversation**: 1-5 KB (few hundred characters)
/// - **Long response**: 50-200 KB (documentation, code generation)
/// - **Very long response**: Up to 10 MB (large documents, but rare)
///
/// ## Lifecycle
///
/// ```text
/// Request Start:  ChatCompletionsParserState::new() → empty strings
///   Chunk 1:      assistant_text = "Hello"
///   Chunk 2:      assistant_text = "Hello world"
///   Chunk N:      assistant_text = "Hello world! How can I help?"
/// Request End:    Parser dropped → all accumulated text freed
/// ```
///
/// **Important**: Memory is automatically freed when the request completes
/// (AdapterContext drops). No manual cleanup needed.
///
/// # Example
///
/// ```rust
/// use codex_core::adapters::openai_common::ChatCompletionsParserState;
///
/// let mut state = ChatCompletionsParserState::new();
///
/// // Process first chunk - assistant_text accumulates
/// let events1 = state.parse_chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#)?;
/// // events1: [OutputTextDelta("Hello")]
/// // Internal state: assistant_text = "Hello"
///
/// // Process second chunk - assistant_text continues to grow
/// let events2 = state.parse_chunk(r#"{"choices":[{"delta":{"content":" world"}}]}"#)?;
/// // events2: [OutputTextDelta(" world")]
/// // Internal state: assistant_text = "Hello world"
///
/// // Process completion - emits full accumulated message
/// let events3 = state.parse_chunk(r#"{"choices":[{"finish_reason":"stop"}]}"#)?;
/// // events3: [OutputItemDone(Message { text: "Hello world" }), Completed]
/// // Internal state: assistant_text cleared (moved into Message)
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionsParserState {
    /// Accumulated assistant message content
    ///
    /// This string grows as we receive `choices[0].delta.content` chunks.
    /// Typical size: 100 bytes - 10 MB (depending on response length).
    /// Freed when: finish_reason="stop" triggers OutputItemDone (text moved)
    /// or when request completes (parser dropped).
    assistant_text: String,

    /// Accumulated reasoning content
    ///
    /// This string grows as we receive `choices[0].delta.reasoning` chunks.
    /// Typical size: 100 bytes - 5 KB (reasoning is usually shorter than output).
    /// Freed when: finish_reason triggers OutputItemDone or request completes.
    reasoning_text: String,

    /// Tool call accumulation state
    ///
    /// These fields track tool call details across multiple chunks:
    /// - tool_call_name: Function name (e.g., "search_code")
    /// - tool_call_arguments: JSON string built from fragments
    /// - tool_call_id: Call identifier (e.g., "call_abc123")
    ///
    /// Typical size: 100 bytes - 5 KB (JSON arguments)
    /// Freed when: finish_reason="tool_calls" emits FunctionCall or request completes.
    #[serde(default)]
    tool_call_name: Option<String>,
    #[serde(default)]
    tool_call_arguments: String,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    tool_call_active: bool,
}

impl Default for ChatCompletionsParserState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatCompletionsParserState {
    /// Create a new parser state
    pub fn new() -> Self {
        Self {
            assistant_text: String::new(),
            reasoning_text: String::new(),
            tool_call_name: None,
            tool_call_arguments: String::new(),
            tool_call_id: None,
            tool_call_active: false,
        }
    }

    /// Parse a single Chat Completions SSE chunk
    ///
    /// This is the core logic extracted from `chat_completions.rs:process_chat_sse()`.
    ///
    /// # Arguments
    ///
    /// * `chunk` - Raw SSE data (without "data: " prefix)
    ///
    /// # Returns
    ///
    /// Vector of ResponseEvent. Can be:
    /// - Empty if chunk contains no actionable data
    /// - One or more events if chunk contains deltas or completion signals
    pub fn parse_chunk(&mut self, chunk: &str) -> Result<Vec<ResponseEvent>> {
        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Handle [DONE] signal
        if chunk.trim() == "[DONE]" {
            return self.handle_done();
        }

        // Parse JSON
        let data: JsonValue = serde_json::from_str(chunk)?;

        // Extract choices[0]
        let Some(choice) = data
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
        else {
            return Ok(vec![]);
        };

        let mut events = Vec::new();

        // Handle delta.content (assistant text)
        if let Some(content) = choice
            .get("delta")
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            && !content.is_empty()
        {
            self.assistant_text.push_str(content);
            events.push(ResponseEvent::OutputTextDelta(content.to_string()));
        }

        // Handle delta.reasoning
        if let Some(reasoning_val) = choice.get("delta").and_then(|d| d.get("reasoning")) {
            let reasoning_text = Self::extract_reasoning_text(reasoning_val);
            if !reasoning_text.is_empty() {
                self.reasoning_text.push_str(&reasoning_text);
                events.push(ResponseEvent::ReasoningContentDelta {
                    delta: reasoning_text,
                    content_index: 0,
                });
            }
        }

        // Handle message.reasoning (some providers only include it at the end)
        if let Some(message_reasoning) = choice.get("message").and_then(|m| m.get("reasoning")) {
            let reasoning_text = Self::extract_reasoning_text(message_reasoning);
            if !reasoning_text.is_empty() {
                self.reasoning_text.push_str(&reasoning_text);
                events.push(ResponseEvent::ReasoningContentDelta {
                    delta: reasoning_text,
                    content_index: 0,
                });
            }
        }

        // Handle delta.tool_calls
        if let Some(tool_calls) = choice
            .get("delta")
            .and_then(|d| d.get("tool_calls"))
            .and_then(|tc| tc.as_array())
            && let Some(tool_call) = tool_calls.first()
        {
            self.tool_call_active = true;

            // Extract call_id
            if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                self.tool_call_id.get_or_insert_with(|| id.to_string());
            }

            // Extract function details
            if let Some(function) = tool_call.get("function") {
                if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                    self.tool_call_name.get_or_insert_with(|| name.to_string());
                }

                if let Some(args_fragment) = function.get("arguments").and_then(|a| a.as_str()) {
                    self.tool_call_arguments.push_str(args_fragment);
                }
            }
        }

        // Handle finish_reason
        if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            match finish_reason {
                "tool_calls" if self.tool_call_active => {
                    // Emit reasoning first if accumulated
                    if !self.reasoning_text.is_empty() {
                        let item = ResponseItem::Reasoning {
                            id: String::new(),
                            summary: Vec::new(),
                            content: Some(vec![ReasoningItemContent::ReasoningText {
                                text: std::mem::take(&mut self.reasoning_text),
                            }]),
                            encrypted_content: None,
                        };
                        events.push(ResponseEvent::OutputItemDone(item));
                    }

                    // Emit tool call
                    let item = ResponseItem::FunctionCall {
                        id: None,
                        name: self.tool_call_name.take().unwrap_or_default(),
                        arguments: std::mem::take(&mut self.tool_call_arguments),
                        call_id: self.tool_call_id.take().unwrap_or_default(),
                    };
                    events.push(ResponseEvent::OutputItemDone(item));
                    events.push(ResponseEvent::Completed {
                        response_id: String::new(),
                        token_usage: None,
                    });
                    self.tool_call_active = false;
                }
                "stop" => {
                    // Emit reasoning first if accumulated
                    if !self.reasoning_text.is_empty() {
                        let item = ResponseItem::Reasoning {
                            id: String::new(),
                            summary: Vec::new(),
                            content: Some(vec![ReasoningItemContent::ReasoningText {
                                text: std::mem::take(&mut self.reasoning_text),
                            }]),
                            encrypted_content: None,
                        };
                        events.push(ResponseEvent::OutputItemDone(item));
                    }

                    // Emit assistant message if we have text
                    if !self.assistant_text.is_empty() {
                        let item = ResponseItem::Message {
                            id: None,
                            role: "assistant".to_string(),
                            content: vec![ContentItem::OutputText {
                                text: std::mem::take(&mut self.assistant_text),
                            }],
                        };
                        events.push(ResponseEvent::OutputItemDone(item));
                    }

                    events.push(ResponseEvent::Completed {
                        response_id: String::new(),
                        token_usage: None,
                    });
                }
                _ => {}
            }
        }

        Ok(events)
    }

    /// Handle [DONE] signal
    fn handle_done(&mut self) -> Result<Vec<ResponseEvent>> {
        let mut events = Vec::new();

        // Emit any remaining reasoning
        if !self.reasoning_text.is_empty() {
            let item = ResponseItem::Reasoning {
                id: String::new(),
                summary: Vec::new(),
                content: Some(vec![ReasoningItemContent::ReasoningText {
                    text: std::mem::take(&mut self.reasoning_text),
                }]),
                encrypted_content: None,
            };
            events.push(ResponseEvent::OutputItemDone(item));
        }

        // Emit any remaining assistant text
        if !self.assistant_text.is_empty() {
            let item = ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: std::mem::take(&mut self.assistant_text),
                }],
            };
            events.push(ResponseEvent::OutputItemDone(item));
        }

        events.push(ResponseEvent::Completed {
            response_id: String::new(),
            token_usage: None,
        });

        Ok(events)
    }

    /// Extract reasoning text from various formats
    ///
    /// Some providers send reasoning as plain string, others as object with text/content field
    fn extract_reasoning_text(reasoning_val: &JsonValue) -> String {
        // Try as plain string first
        if let Some(s) = reasoning_val.as_str() {
            return s.to_string();
        }

        // Try as object with "text" field
        if let Some(obj) = reasoning_val.as_object() {
            if let Some(s) = obj.get("text").and_then(|v| v.as_str()) {
                return s.to_string();
            }
            // Try "content" field
            if let Some(s) = obj.get("content").and_then(|v| v.as_str()) {
                return s.to_string();
            }
        }

        String::new()
    }
}

/// Parser state for OpenAI Responses API streaming format
///
/// This state machine handles the Responses API SSE format which uses
/// structured events like `response.output_text.delta` instead of the
/// Chat Completions format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesApiParserState {
    /// Current response ID
    response_id: Option<String>,

    /// Accumulated assistant message text
    accumulated_text: String,

    /// Accumulated reasoning summary parts
    reasoning_summary: Vec<String>,

    /// Accumulated reasoning content
    reasoning_content: Option<String>,

    /// Current item being built
    current_item: Option<ResponseItem>,

    /// Encrypted content for reasoning (if available)
    encrypted_content: Option<String>,
}

impl Default for ResponsesApiParserState {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponsesApiParserState {
    /// Create a new Responses API parser state
    pub fn new() -> Self {
        Self {
            response_id: None,
            accumulated_text: String::new(),
            reasoning_summary: Vec::new(),
            reasoning_content: None,
            current_item: None,
            encrypted_content: None,
        }
    }

    /// Parse a single Responses API SSE chunk
    pub fn parse_chunk(&mut self, chunk: &str) -> Result<Vec<ResponseEvent>> {
        if chunk.trim().is_empty() {
            return Ok(vec![]);
        }

        // Parse JSON
        let data: JsonValue = serde_json::from_str(chunk)?;

        // Extract event type
        let event_type = data.get("event").and_then(|e| e.as_str()).unwrap_or("");

        match event_type {
            "response.created" => {
                // Extract response ID
                if let Some(id) = data
                    .get("response")
                    .and_then(|r| r.get("id"))
                    .and_then(|i| i.as_str())
                {
                    self.response_id = Some(id.to_string());
                }
                Ok(vec![ResponseEvent::Created])
            }

            "response.output_item.added" => {
                // New item started - store it
                if let Some(item_data) = data.get("item") {
                    let item_type = item_data.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    match item_type {
                        "message" => {
                            self.current_item = Some(ResponseItem::Message {
                                id: item_data
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .map(std::string::ToString::to_string),
                                role: "assistant".to_string(),
                                content: vec![],
                            });
                        }
                        "reasoning" => {
                            self.current_item = Some(ResponseItem::Reasoning {
                                id: item_data
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                summary: vec![],
                                content: None,
                                encrypted_content: None,
                            });
                        }
                        "function_call" => {
                            self.current_item = Some(ResponseItem::FunctionCall {
                                id: item_data
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .map(std::string::ToString::to_string),
                                name: item_data
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                arguments: String::new(),
                                call_id: item_data
                                    .get("call_id")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            });
                        }
                        _ => {}
                    }
                }
                Ok(vec![ResponseEvent::OutputItemAdded(
                    self.current_item.clone().unwrap_or(ResponseItem::Other),
                )])
            }

            "response.output_text.delta" => {
                let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
                if !delta.is_empty() {
                    self.accumulated_text.push_str(delta);
                    Ok(vec![ResponseEvent::OutputTextDelta(delta.to_string())])
                } else {
                    Ok(vec![])
                }
            }

            "response.reasoning_summary.delta" => {
                let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
                if !delta.is_empty() {
                    self.reasoning_summary.push(delta.to_string());
                    Ok(vec![ResponseEvent::ReasoningSummaryDelta {
                        delta: delta.to_string(),
                        summary_index: 0,
                    }])
                } else {
                    Ok(vec![])
                }
            }

            "response.reasoning_content.delta" => {
                let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
                if !delta.is_empty() {
                    let content = self.reasoning_content.get_or_insert_with(String::new);
                    content.push_str(delta);
                    Ok(vec![ResponseEvent::ReasoningContentDelta {
                        delta: delta.to_string(),
                        content_index: 0,
                    }])
                } else {
                    Ok(vec![])
                }
            }

            "response.output_item.done" => {
                // Item completed - emit it
                if let Some(mut item) = self.current_item.take() {
                    // Finalize item with accumulated data
                    match &mut item {
                        ResponseItem::Message { content, .. } => {
                            if !self.accumulated_text.is_empty() {
                                content.push(ContentItem::OutputText {
                                    text: std::mem::take(&mut self.accumulated_text),
                                });
                            }
                        }
                        ResponseItem::Reasoning {
                            summary,
                            content,
                            encrypted_content,
                            ..
                        } => {
                            *summary = self.reasoning_summary.drain(..).map(|text| {
                                codex_protocol::models::ReasoningItemReasoningSummary::SummaryText { text }
                            }).collect();

                            if let Some(reasoning_text) = self.reasoning_content.take() {
                                *content = Some(vec![ReasoningItemContent::ReasoningText {
                                    text: reasoning_text,
                                }]);
                            }

                            *encrypted_content = self.encrypted_content.take();
                        }
                        _ => {}
                    }

                    Ok(vec![ResponseEvent::OutputItemDone(item)])
                } else {
                    Ok(vec![])
                }
            }

            "response.done" => {
                // Extract token usage
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

                Ok(vec![ResponseEvent::Completed {
                    response_id: self.response_id.clone().unwrap_or_default(),
                    token_usage,
                }])
            }

            _ => Ok(vec![]),
        }
    }
}

/// Get type names for a slice of ResponseItems
///
/// Useful for debug logging to show input composition without dumping full data.
/// Returns a vector of string slices representing each item's type.
pub fn get_response_item_types(items: &[ResponseItem]) -> Vec<&str> {
    items
        .iter()
        .map(|item| match item {
            ResponseItem::Message { .. } => "message",
            ResponseItem::Reasoning { .. } => "reasoning",
            ResponseItem::FunctionCall { .. } => "function_call",
            ResponseItem::FunctionCallOutput { .. } => "function_call_output",
            ResponseItem::LocalShellCall { .. } => "local_shell_call",
            ResponseItem::CustomToolCall { .. } => "custom_tool_call",
            ResponseItem::CustomToolCallOutput { .. } => "custom_tool_call_output",
            ResponseItem::WebSearchCall { .. } => "web_search_call",
            ResponseItem::GhostSnapshot { .. } => "ghost_snapshot",
            ResponseItem::CompactionSummary { .. } => "compaction_summary",
            ResponseItem::Other => "other",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_chunk() {
        let mut state = ChatCompletionsParserState::new();
        let events = state.parse_chunk("").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_whitespace_chunk() {
        let mut state = ChatCompletionsParserState::new();
        let events = state.parse_chunk("   \n  ").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_content_delta() {
        let mut state = ChatCompletionsParserState::new();
        let chunk = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        let events = state.parse_chunk(chunk).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::OutputTextDelta(text) => assert_eq!(text, "Hello"),
            _ => panic!("Expected OutputTextDelta"),
        }
    }

    #[test]
    fn test_multiple_deltas() {
        let mut state = ChatCompletionsParserState::new();

        let events1 = state
            .parse_chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#)
            .unwrap();
        assert_eq!(events1.len(), 1);

        let events2 = state
            .parse_chunk(r#"{"choices":[{"delta":{"content":" world"}}]}"#)
            .unwrap();
        assert_eq!(events2.len(), 1);

        // State should accumulate
        assert_eq!(state.assistant_text, "Hello world");
    }

    #[test]
    fn test_finish_reason_stop() {
        let mut state = ChatCompletionsParserState::new();

        // Add some content first
        state
            .parse_chunk(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#)
            .unwrap();

        // Then finish
        let events = state
            .parse_chunk(r#"{"choices":[{"finish_reason":"stop"}]}"#)
            .unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentItem::OutputText { text } => assert_eq!(text, "Hello"),
                    _ => panic!("Expected OutputText"),
                }
            }
            _ => panic!("Expected OutputItemDone"),
        }
        match &events[1] {
            ResponseEvent::Completed { .. } => {}
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_done_signal() {
        let mut state = ChatCompletionsParserState::new();

        // Add content
        state
            .parse_chunk(r#"{"choices":[{"delta":{"content":"Test"}}]}"#)
            .unwrap();

        // Send [DONE]
        let events = state.parse_chunk("[DONE]").unwrap();

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
    fn test_reasoning_delta() {
        let mut state = ChatCompletionsParserState::new();
        let chunk = r#"{"choices":[{"delta":{"reasoning":"Thinking..."}}]}"#;
        let events = state.parse_chunk(chunk).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            ResponseEvent::ReasoningContentDelta { delta, .. } => assert_eq!(delta, "Thinking..."),
            _ => panic!("Expected ReasoningContentDelta"),
        }
    }

    #[test]
    fn test_no_choices() {
        let mut state = ChatCompletionsParserState::new();
        let chunk = r#"{"id":"test"}"#;
        let events = state.parse_chunk(chunk).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_invalid_json() {
        let mut state = ChatCompletionsParserState::new();
        let result = state.parse_chunk(r#"{"invalid"#);
        assert!(result.is_err());
    }
}
