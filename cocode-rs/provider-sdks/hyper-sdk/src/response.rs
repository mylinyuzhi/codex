//! Response types from generation.

use crate::messages::ContentBlock;
use crate::tools::ToolCall;
use serde::Deserialize;
use serde::Serialize;

/// Reason why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FinishReason {
    /// Natural end of generation.
    #[default]
    Stop,
    /// Hit max tokens limit.
    MaxTokens,
    /// Model wants to use a tool.
    ToolCalls,
    /// Content was filtered.
    ContentFilter,
    /// Generation is still in progress.
    InProgress,
    /// Unknown or other reason.
    Other,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of tokens in the prompt.
    #[serde(default)]
    pub prompt_tokens: i64,
    /// Number of tokens in the completion.
    #[serde(default)]
    pub completion_tokens: i64,
    /// Total tokens used.
    #[serde(default)]
    pub total_tokens: i64,
    /// Tokens read from cache (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// Tokens used to create cache (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
    /// Tokens used for reasoning (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
}

impl TokenUsage {
    /// Create usage from prompt and completion token counts.
    pub fn new(prompt_tokens: i64, completion_tokens: i64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }
    }

    /// Set cache read tokens.
    pub fn with_cache_read_tokens(mut self, tokens: i64) -> Self {
        self.cache_read_tokens = Some(tokens);
        self
    }

    /// Set cache creation tokens.
    pub fn with_cache_creation_tokens(mut self, tokens: i64) -> Self {
        self.cache_creation_tokens = Some(tokens);
        self
    }

    /// Set reasoning tokens.
    pub fn with_reasoning_tokens(mut self, tokens: i64) -> Self {
        self.reasoning_tokens = Some(tokens);
        self
    }
}

/// Response from text generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateResponse {
    /// Unique response ID.
    pub id: String,
    /// Content blocks in the response.
    pub content: Vec<ContentBlock>,
    /// Reason generation stopped.
    pub finish_reason: FinishReason,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// Model that generated the response.
    pub model: String,
}

impl GenerateResponse {
    /// Create a new response.
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: vec![],
            finish_reason: FinishReason::Stop,
            usage: None,
            model: model.into(),
        }
    }

    /// Add content to the response.
    pub fn with_content(mut self, content: Vec<ContentBlock>) -> Self {
        self.content = content;
        self
    }

    /// Set the finish reason.
    pub fn with_finish_reason(mut self, reason: FinishReason) -> Self {
        self.finish_reason = reason;
        self
    }

    /// Set token usage.
    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Get all text content concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all tool calls from the response.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Get thinking content if present.
    pub fn thinking(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            ContentBlock::Thinking { content, .. } => Some(content.as_str()),
            _ => None,
        })
    }

    /// Check if the response contains tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(super::messages::ContentBlock::is_tool_use)
    }

    /// Check if the response contains thinking.
    pub fn has_thinking(&self) -> bool {
        self.content
            .iter()
            .any(super::messages::ContentBlock::is_thinking)
    }

    /// Check if generation stopped due to tool calls.
    pub fn stopped_for_tool_calls(&self) -> bool {
        self.finish_reason == FinishReason::ToolCalls
    }
}

#[cfg(test)]
#[path = "response.test.rs"]
mod tests;
