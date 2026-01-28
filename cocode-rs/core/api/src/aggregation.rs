//! Stream event aggregation for accumulating deltas into complete blocks.
//!
//! This module provides [`AggregationState`] which aggregates streaming deltas
//! into complete content blocks. It handles text, thinking, and tool call events.

use crate::unified_stream::saturating_i64_to_i32;
use cocode_protocol::TokenUsage as ProtocolUsage;
use hyper_sdk::{ContentBlock, FinishReason, StreamEvent, TokenUsage, ToolCall};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Partial content block being accumulated.
#[derive(Debug, Clone)]
pub enum PartialBlock {
    /// Text content being accumulated.
    Text {
        /// Accumulated text buffer.
        buffer: String,
    },
    /// Thinking content being accumulated.
    Thinking {
        /// Accumulated thinking buffer.
        buffer: String,
        /// Signature for verification (set on ThinkingDone).
        signature: Option<String>,
    },
    /// Tool call being accumulated.
    ToolCall {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Accumulated arguments JSON buffer.
        arguments_buffer: String,
    },
}

impl PartialBlock {
    /// Get the content block index type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            PartialBlock::Text { .. } => "text",
            PartialBlock::Thinking { .. } => "thinking",
            PartialBlock::ToolCall { .. } => "tool_call",
        }
    }
}

/// Telemetry information collected during streaming.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamTelemetry {
    /// Time to first chunk (text or thinking delta).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_chunk: Option<Duration>,
    /// Number of stalls detected.
    pub stall_count: i32,
    /// Total chunks received.
    pub chunk_count: i32,
    /// Time of last event.
    #[serde(skip)]
    pub last_event_time: Option<Instant>,
}

/// State machine that aggregates streaming deltas into complete blocks.
///
/// This is similar to Claude Code's `AggregationState` - it tracks pending
/// content blocks being built up from streaming deltas and emits complete
/// blocks when they are finished.
#[derive(Debug, Clone)]
pub struct AggregationState {
    /// Pending blocks being accumulated, keyed by index.
    pending_blocks: HashMap<i64, PartialBlock>,
    /// Completed content blocks.
    completed_blocks: Vec<ContentBlock>,
    /// Response ID from the API.
    response_id: Option<String>,
    /// Model name.
    model: Option<String>,
    /// Token usage information.
    usage: Option<TokenUsage>,
    /// Finish reason.
    finish_reason: Option<FinishReason>,
    /// Stream start time for telemetry.
    start_time: Instant,
    /// Telemetry information.
    telemetry: StreamTelemetry,
    /// Whether the stream is complete.
    is_complete: bool,
}

impl Default for AggregationState {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregationState {
    /// Create a new aggregation state.
    pub fn new() -> Self {
        Self {
            pending_blocks: HashMap::new(),
            completed_blocks: Vec::new(),
            response_id: None,
            model: None,
            usage: None,
            finish_reason: None,
            start_time: Instant::now(),
            telemetry: StreamTelemetry::default(),
            is_complete: false,
        }
    }

    /// Process a stream event and return any newly completed blocks.
    pub fn process_event(&mut self, event: &StreamEvent) -> Vec<ContentBlock> {
        self.telemetry.last_event_time = Some(Instant::now());
        self.telemetry.chunk_count += 1;

        let mut completed = Vec::new();

        match event {
            StreamEvent::Ignored => {}

            StreamEvent::TextDelta { index, delta } => {
                self.record_first_chunk();
                let block =
                    self.pending_blocks
                        .entry(*index)
                        .or_insert_with(|| PartialBlock::Text {
                            buffer: String::new(),
                        });
                if let PartialBlock::Text { buffer } = block {
                    buffer.push_str(delta);
                }
            }

            StreamEvent::TextDone { index, text } => {
                // Complete the text block
                let block = self.pending_blocks.remove(index);
                let final_text = match block {
                    Some(PartialBlock::Text { buffer }) => {
                        // Prefer accumulated deltas, fallback to final text
                        if buffer.is_empty() {
                            text.clone()
                        } else {
                            buffer
                        }
                    }
                    _ => text.clone(),
                };
                if !final_text.is_empty() {
                    let content_block = ContentBlock::text(&final_text);
                    completed.push(content_block.clone());
                    self.completed_blocks.push(content_block);
                }
            }

            StreamEvent::ThinkingDelta { index, delta } => {
                self.record_first_chunk();
                let block =
                    self.pending_blocks
                        .entry(*index)
                        .or_insert_with(|| PartialBlock::Thinking {
                            buffer: String::new(),
                            signature: None,
                        });
                if let PartialBlock::Thinking { buffer, .. } = block {
                    buffer.push_str(delta);
                }
            }

            StreamEvent::ThinkingDone {
                index,
                content,
                signature,
            } => {
                let block = self.pending_blocks.remove(index);
                let (final_content, final_signature) = match block {
                    Some(PartialBlock::Thinking { buffer, .. }) => {
                        // Prefer accumulated deltas, fallback to final content
                        let text = if buffer.is_empty() {
                            content.clone()
                        } else {
                            buffer
                        };
                        (text, signature.clone())
                    }
                    _ => (content.clone(), signature.clone()),
                };
                if !final_content.is_empty() {
                    let content_block = ContentBlock::Thinking {
                        content: final_content,
                        signature: final_signature,
                    };
                    completed.push(content_block.clone());
                    self.completed_blocks.push(content_block);
                }
            }

            StreamEvent::ToolCallStart { index, id, name } => {
                self.pending_blocks.insert(
                    *index,
                    PartialBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments_buffer: String::new(),
                    },
                );
            }

            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
                ..
            } => {
                if let Some(PartialBlock::ToolCall {
                    arguments_buffer, ..
                }) = self.pending_blocks.get_mut(index)
                {
                    arguments_buffer.push_str(arguments_delta);
                }
            }

            StreamEvent::ToolCallDone { index, tool_call } => {
                self.pending_blocks.remove(index);
                let content_block = ContentBlock::tool_use(
                    &tool_call.id,
                    &tool_call.name,
                    tool_call.arguments.clone(),
                );
                completed.push(content_block.clone());
                self.completed_blocks.push(content_block);
            }

            StreamEvent::ResponseCreated { id } => {
                self.response_id = Some(id.clone());
            }

            StreamEvent::ResponseDone {
                id: _,
                model,
                usage,
                finish_reason,
            } => {
                self.model = Some(model.clone());
                self.usage = usage.clone();
                self.finish_reason = Some(*finish_reason);
                self.is_complete = true;
            }

            StreamEvent::Error(err) => {
                tracing::error!(code = %err.code, message = %err.message, "Stream error");
            }
        }

        completed
    }

    /// Record first chunk time for telemetry.
    fn record_first_chunk(&mut self) {
        if self.telemetry.time_to_first_chunk.is_none() {
            self.telemetry.time_to_first_chunk = Some(self.start_time.elapsed());
        }
    }

    /// Take all completed blocks, clearing the internal list.
    pub fn take_completed(&mut self) -> Vec<ContentBlock> {
        std::mem::take(&mut self.completed_blocks)
    }

    /// Get a reference to completed blocks.
    pub fn completed_blocks(&self) -> &[ContentBlock] {
        &self.completed_blocks
    }

    /// Get accumulated text from completed blocks.
    pub fn text(&self) -> String {
        self.completed_blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Get accumulated thinking from completed blocks.
    pub fn thinking(&self) -> Option<String> {
        self.completed_blocks.iter().find_map(|b| match b {
            ContentBlock::Thinking { content, .. } => Some(content.clone()),
            _ => None,
        })
    }

    /// Get tool calls from completed blocks.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.completed_blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some(ToolCall::new(id, name, input.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Get the response ID.
    pub fn response_id(&self) -> Option<&str> {
        self.response_id.as_deref()
    }

    /// Get the model name.
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Get token usage.
    pub fn usage(&self) -> Option<&TokenUsage> {
        self.usage.as_ref()
    }

    /// Get token usage in protocol format.
    pub fn protocol_usage(&self) -> Option<ProtocolUsage> {
        self.usage.as_ref().map(|u| ProtocolUsage {
            input_tokens: saturating_i64_to_i32(u.prompt_tokens, "prompt_tokens"),
            output_tokens: saturating_i64_to_i32(u.completion_tokens, "completion_tokens"),
            cache_read_tokens: u
                .cache_read_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_read_tokens")),
            cache_creation_tokens: u
                .cache_creation_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_creation_tokens")),
        })
    }

    /// Get the finish reason.
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }

    /// Check if the stream is complete.
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Get telemetry information.
    pub fn telemetry(&self) -> &StreamTelemetry {
        &self.telemetry
    }

    /// Record a stall detection.
    pub fn record_stall(&mut self) {
        self.telemetry.stall_count += 1;
    }

    /// Check if stream appears stalled (no events for given duration).
    pub fn is_stalled(&self, threshold: Duration) -> bool {
        self.telemetry
            .last_event_time
            .map(|t| t.elapsed() > threshold)
            .unwrap_or(false)
    }

    /// Get the number of pending blocks.
    pub fn pending_count(&self) -> usize {
        self.pending_blocks.len()
    }

    /// Check if there are any pending tool calls.
    pub fn has_pending_tool_calls(&self) -> bool {
        self.pending_blocks
            .values()
            .any(|b| matches!(b, PartialBlock::ToolCall { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_accumulation() {
        let mut state = AggregationState::new();

        // Process text deltas
        let completed = state.process_event(&StreamEvent::text_delta(0, "Hello "));
        assert!(completed.is_empty()); // Not complete yet

        let completed = state.process_event(&StreamEvent::text_delta(0, "world"));
        assert!(completed.is_empty());

        // Complete the text block
        let completed = state.process_event(&StreamEvent::text_done(0, "Hello world"));
        assert_eq!(completed.len(), 1);

        assert_eq!(state.text(), "Hello world");
    }

    #[test]
    fn test_thinking_accumulation() {
        let mut state = AggregationState::new();

        state.process_event(&StreamEvent::thinking_delta(0, "Let me "));
        state.process_event(&StreamEvent::thinking_delta(0, "think..."));

        let completed = state.process_event(&StreamEvent::ThinkingDone {
            index: 0,
            content: "Let me think...".to_string(),
            signature: Some("sig123".to_string()),
        });

        assert_eq!(completed.len(), 1);
        assert_eq!(state.thinking(), Some("Let me think...".to_string()));
    }

    #[test]
    fn test_tool_call_accumulation() {
        let mut state = AggregationState::new();

        state.process_event(&StreamEvent::tool_call_start(0, "call_1", "get_weather"));
        state.process_event(&StreamEvent::ToolCallDelta {
            index: 0,
            id: "call_1".to_string(),
            arguments_delta: "{\"city\":".to_string(),
        });
        state.process_event(&StreamEvent::ToolCallDelta {
            index: 0,
            id: "call_1".to_string(),
            arguments_delta: "\"NYC\"}".to_string(),
        });

        let completed = state.process_event(&StreamEvent::tool_call_done(
            0,
            ToolCall::new("call_1", "get_weather", serde_json::json!({"city": "NYC"})),
        ));

        assert_eq!(completed.len(), 1);
        let tool_calls = state.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "get_weather");
    }

    #[test]
    fn test_response_done() {
        let mut state = AggregationState::new();

        state.process_event(&StreamEvent::response_created("resp_1"));
        assert_eq!(state.response_id(), Some("resp_1"));

        state.process_event(&StreamEvent::response_done_full(
            "resp_1",
            "test-model",
            Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            }),
            FinishReason::Stop,
        ));

        assert!(state.is_complete());
        assert_eq!(state.model(), Some("test-model"));
        assert_eq!(state.finish_reason(), Some(FinishReason::Stop));
    }

    #[test]
    fn test_telemetry() {
        let mut state = AggregationState::new();

        // Process some events
        state.process_event(&StreamEvent::text_delta(0, "Hello"));
        assert!(state.telemetry().time_to_first_chunk.is_some());
        assert_eq!(state.telemetry().chunk_count, 1);

        state.process_event(&StreamEvent::text_delta(0, " world"));
        assert_eq!(state.telemetry().chunk_count, 2);

        // Stall detection
        state.record_stall();
        assert_eq!(state.telemetry().stall_count, 1);
    }

    #[test]
    fn test_take_completed() {
        let mut state = AggregationState::new();

        state.process_event(&StreamEvent::text_delta(0, "Hello"));
        state.process_event(&StreamEvent::text_done(0, "Hello"));

        let completed = state.take_completed();
        assert_eq!(completed.len(), 1);

        // Should be empty now
        let completed = state.take_completed();
        assert!(completed.is_empty());
    }
}
