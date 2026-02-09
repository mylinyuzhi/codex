//! Prompt caching helpers for reducing token costs.
//!
//! This module provides utilities for working with prompt caching features
//! available in some providers (like Anthropic's prompt caching).

use hyper_sdk::Message;
use serde::Deserialize;
use serde::Serialize;

/// Configuration for prompt caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCacheConfig {
    /// Enable prompt caching.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Minimum tokens for a message to be considered for caching.
    #[serde(default = "default_min_tokens")]
    pub min_tokens_for_cache: i32,
    /// Cache the system prompt.
    #[serde(default = "default_cache_system")]
    pub cache_system_prompt: bool,
    /// Cache tool definitions.
    #[serde(default = "default_cache_tools")]
    pub cache_tools: bool,
}

fn default_enabled() -> bool {
    true
}
fn default_min_tokens() -> i32 {
    1024
}
fn default_cache_system() -> bool {
    true
}
fn default_cache_tools() -> bool {
    true
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            min_tokens_for_cache: default_min_tokens(),
            cache_system_prompt: default_cache_system(),
            cache_tools: default_cache_tools(),
        }
    }
}

impl PromptCacheConfig {
    /// Create a disabled cache config.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Enable caching.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set minimum tokens for caching.
    pub fn with_min_tokens(mut self, min_tokens: i32) -> Self {
        self.min_tokens_for_cache = min_tokens;
        self
    }

    /// Enable/disable system prompt caching.
    pub fn with_cache_system(mut self, cache: bool) -> Self {
        self.cache_system_prompt = cache;
        self
    }

    /// Enable/disable tool definition caching.
    pub fn with_cache_tools(mut self, cache: bool) -> Self {
        self.cache_tools = cache;
        self
    }
}

/// Cache statistics from a response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    /// Tokens read from cache.
    pub cache_read_tokens: i32,
    /// Tokens created in cache.
    pub cache_creation_tokens: i32,
    /// Whether this was a cache hit.
    pub is_hit: bool,
    /// Estimated cost savings ratio (0.0-1.0).
    pub savings_ratio: f64,
}

impl CacheStats {
    /// Create stats from token usage.
    pub fn from_usage(cache_read: Option<i32>, cache_creation: Option<i32>) -> Self {
        let cache_read_tokens = cache_read.unwrap_or(0);
        let cache_creation_tokens = cache_creation.unwrap_or(0);
        let total_cached = cache_read_tokens + cache_creation_tokens;

        Self {
            cache_read_tokens,
            cache_creation_tokens,
            is_hit: cache_read_tokens > 0,
            // Rough estimate: cache hits cost 90% less
            savings_ratio: if total_cached > 0 {
                (cache_read_tokens as f64 / total_cached as f64) * 0.9
            } else {
                0.0
            },
        }
    }

    /// Check if any caching occurred.
    pub fn has_caching(&self) -> bool {
        self.cache_read_tokens > 0 || self.cache_creation_tokens > 0
    }
}

/// Marker trait for cacheable content.
pub trait Cacheable {
    /// Estimate the token count for this content.
    fn estimate_tokens(&self) -> i32;

    /// Check if this content should be cached based on config.
    fn should_cache(&self, config: &PromptCacheConfig) -> bool {
        config.enabled && self.estimate_tokens() >= config.min_tokens_for_cache
    }
}

impl Cacheable for String {
    fn estimate_tokens(&self) -> i32 {
        // Rough estimate: 4 characters per token
        (self.len() / 4) as i32
    }
}

impl Cacheable for &str {
    fn estimate_tokens(&self) -> i32 {
        (self.len() / 4) as i32
    }
}

impl Cacheable for Message {
    fn estimate_tokens(&self) -> i32 {
        use hyper_sdk::ContentBlock;

        self.content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => (text.len() / 4) as i32,
                ContentBlock::Thinking { content, .. } => (content.len() / 4) as i32,
                ContentBlock::Image { .. } => 1000, // Images are roughly 1000 tokens
                ContentBlock::ToolUse { input, .. } => (input.to_string().len() / 4) as i32,
                ContentBlock::ToolResult { content, .. } => {
                    use hyper_sdk::ToolResultContent;
                    match content {
                        ToolResultContent::Text(text) => (text.len() / 4) as i32,
                        ToolResultContent::Json(val) => (val.to_string().len() / 4) as i32,
                        ToolResultContent::Blocks(blocks) => {
                            blocks.len() as i32 * 100 // Rough estimate
                        }
                    }
                }
            })
            .sum()
    }
}

/// Helper to determine cache breakpoints in a conversation.
///
/// Returns indices of messages that should have cache_control markers.
pub fn find_cache_breakpoints(messages: &[Message], config: &PromptCacheConfig) -> Vec<usize> {
    if !config.enabled {
        return Vec::new();
    }

    let mut breakpoints = Vec::new();

    // Always consider caching the system prompt if it's substantial
    if let Some((idx, msg)) = messages
        .iter()
        .enumerate()
        .find(|(_, m)| m.role == hyper_sdk::Role::System)
    {
        if msg.should_cache(config) && config.cache_system_prompt {
            breakpoints.push(idx);
        }
    }

    // Consider caching at conversation turn boundaries for long contexts
    let mut accumulated_tokens = 0;
    for (idx, msg) in messages.iter().enumerate() {
        accumulated_tokens += msg.estimate_tokens();

        // Add breakpoint every ~4000 tokens (a reasonable cache boundary)
        if accumulated_tokens >= 4000 {
            breakpoints.push(idx);
            accumulated_tokens = 0;
        }
    }

    breakpoints.sort();
    breakpoints.dedup();
    breakpoints
}

#[cfg(test)]
#[path = "cache.test.rs"]
mod tests;
