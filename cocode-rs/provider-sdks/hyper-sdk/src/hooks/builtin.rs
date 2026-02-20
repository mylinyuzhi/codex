//! Built-in hooks for common functionality.

use super::HookContext;
use super::RequestHook;
use super::ResponseHook;
use super::StreamHook;
use crate::error::HyperError;
use crate::options::OpenAIOptions;
use crate::options::VolcengineOptions;
use crate::options::downcast_options;
use crate::request::GenerateRequest;
use crate::response::GenerateResponse;
use crate::response::TokenUsage;
use crate::stream::StreamEvent;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;

/// Hook that injects `previous_response_id` for conversation continuity.
///
/// This hook reads `previous_response_id` from the context and injects it
/// into provider-specific options for providers that support conversation
/// continuity (e.g., OpenAI Responses API).
///
/// # Priority
///
/// This hook has priority 10 (early execution) to ensure the response ID
/// is set before other hooks process the request.
#[derive(Debug, Default)]
pub struct ResponseIdHook;

impl ResponseIdHook {
    /// Create a new ResponseIdHook.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RequestHook for ResponseIdHook {
    async fn on_request(
        &self,
        request: &mut GenerateRequest,
        context: &mut HookContext,
    ) -> Result<(), HyperError> {
        if let Some(ref prev_id) = context.previous_response_id {
            // For OpenAI provider, inject into OpenAI options
            if context.provider == "openai" {
                let mut options = request
                    .provider_options
                    .as_ref()
                    .and_then(|opts| downcast_options::<OpenAIOptions>(opts))
                    .cloned()
                    .unwrap_or_default();

                if options.previous_response_id.is_none() {
                    options.previous_response_id = Some(prev_id.clone());
                }

                request.provider_options = Some(Box::new(options));
            }
            // For Volcengine provider, inject into Volcengine options
            else if context.provider == "volcengine" {
                let mut options = request
                    .provider_options
                    .as_ref()
                    .and_then(|opts| downcast_options::<VolcengineOptions>(opts))
                    .cloned()
                    .unwrap_or_default();

                if options.previous_response_id.is_none() {
                    options.previous_response_id = Some(prev_id.clone());
                }

                request.provider_options = Some(Box::new(options));
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        10
    }

    fn name(&self) -> &str {
        "response_id"
    }
}

/// Hook that logs requests and responses.
///
/// # Log Levels
///
/// - `Debug`: Log all requests and responses with full details
/// - `Info`: Log request/response summaries
/// - `Warn`: Log only errors
#[derive(Debug)]
pub struct LoggingHook {
    level: LogLevel,
}

#[derive(Debug, Clone, Copy)]
enum LogLevel {
    Debug,
    Info,
    Warn,
}

impl LoggingHook {
    /// Create a logging hook with debug level.
    pub fn debug() -> Self {
        Self {
            level: LogLevel::Debug,
        }
    }

    /// Create a logging hook with info level.
    pub fn info() -> Self {
        Self {
            level: LogLevel::Info,
        }
    }

    /// Create a logging hook with warn level.
    pub fn warn() -> Self {
        Self {
            level: LogLevel::Warn,
        }
    }
}

impl Default for LoggingHook {
    fn default() -> Self {
        Self::info()
    }
}

#[async_trait]
impl RequestHook for LoggingHook {
    async fn on_request(
        &self,
        request: &mut GenerateRequest,
        context: &mut HookContext,
    ) -> Result<(), HyperError> {
        match self.level {
            LogLevel::Debug => {
                tracing::debug!(
                    provider = %context.provider,
                    model = %context.model_id,
                    messages = request.messages.len(),
                    temperature = ?request.temperature,
                    max_tokens = ?request.max_tokens,
                    has_tools = request.has_tools(),
                    "Sending request"
                );
            }
            LogLevel::Info => {
                tracing::info!(
                    provider = %context.provider,
                    model = %context.model_id,
                    messages = request.messages.len(),
                    "Sending request"
                );
            }
            LogLevel::Warn => {
                // No logging at warn level for normal requests
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        0
    }

    fn name(&self) -> &str {
        "logging"
    }
}

#[async_trait]
impl ResponseHook for LoggingHook {
    async fn on_response(
        &self,
        response: &mut GenerateResponse,
        context: &HookContext,
    ) -> Result<(), HyperError> {
        match self.level {
            LogLevel::Debug => {
                tracing::debug!(
                    provider = %context.provider,
                    model = %response.model,
                    response_id = %response.id,
                    finish_reason = ?response.finish_reason,
                    has_tool_calls = response.has_tool_calls(),
                    usage = ?response.usage,
                    "Received response"
                );
            }
            LogLevel::Info => {
                tracing::info!(
                    provider = %context.provider,
                    response_id = %response.id,
                    finish_reason = ?response.finish_reason,
                    "Received response"
                );
            }
            LogLevel::Warn => {
                // No logging at warn level for normal responses
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        0
    }

    fn name(&self) -> &str {
        "logging"
    }
}

#[async_trait]
impl StreamHook for LoggingHook {
    async fn on_event(&self, event: &StreamEvent, context: &HookContext) -> Result<(), HyperError> {
        if matches!(self.level, LogLevel::Debug) {
            match event {
                StreamEvent::ResponseCreated { id } => {
                    tracing::debug!(provider = %context.provider, response_id = %id, "Stream started");
                }
                StreamEvent::ResponseDone {
                    id, finish_reason, ..
                } => {
                    tracing::debug!(
                        provider = %context.provider,
                        response_id = %id,
                        finish_reason = ?finish_reason,
                        "Stream completed"
                    );
                }
                StreamEvent::Error(err) => {
                    tracing::warn!(
                        provider = %context.provider,
                        error = %err.message,
                        "Stream error"
                    );
                }
                _ => {
                    // Don't log every delta
                }
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "logging"
    }
}

/// Hook that tracks cumulative token usage across requests.
///
/// This hook accumulates token usage from each response, useful for
/// monitoring total token consumption in a conversation or session.
#[derive(Debug)]
pub struct UsageTrackingHook {
    usage: Arc<Mutex<TokenUsage>>,
}

impl UsageTrackingHook {
    /// Create a new usage tracking hook with its own counter.
    pub fn new() -> Self {
        Self {
            usage: Arc::new(Mutex::new(TokenUsage::default())),
        }
    }

    /// Create a usage tracking hook with a shared counter.
    pub fn with_shared_usage(usage: Arc<Mutex<TokenUsage>>) -> Self {
        Self { usage }
    }

    /// Get the current accumulated usage.
    #[allow(clippy::unwrap_used)]
    pub fn get_usage(&self) -> TokenUsage {
        self.usage.lock().unwrap().clone()
    }

    /// Reset the usage counter.
    #[allow(clippy::unwrap_used)]
    pub fn reset(&self) {
        let mut usage = self.usage.lock().unwrap();
        *usage = TokenUsage::default();
    }

    /// Get a reference to the shared usage counter.
    pub fn usage_ref(&self) -> Arc<Mutex<TokenUsage>> {
        self.usage.clone()
    }
}

impl Default for UsageTrackingHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResponseHook for UsageTrackingHook {
    async fn on_response(
        &self,
        response: &mut GenerateResponse,
        _context: &HookContext,
    ) -> Result<(), HyperError> {
        if let Some(ref response_usage) = response.usage {
            #[allow(clippy::unwrap_used)]
            let mut total = self.usage.lock().unwrap();
            total.prompt_tokens += response_usage.prompt_tokens;
            total.completion_tokens += response_usage.completion_tokens;
            total.total_tokens += response_usage.total_tokens;
            if let Some(cached) = response_usage.cache_read_tokens {
                *total.cache_read_tokens.get_or_insert(0) += cached;
            }
            if let Some(cache_creation) = response_usage.cache_creation_tokens {
                *total.cache_creation_tokens.get_or_insert(0) += cache_creation;
            }
            if let Some(reasoning) = response_usage.reasoning_tokens {
                *total.reasoning_tokens.get_or_insert(0) += reasoning;
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        200 // Run late to capture final usage
    }

    fn name(&self) -> &str {
        "usage_tracking"
    }
}

/// Hook that sanitizes history messages when switching providers.
///
/// This hook automatically converts messages from other providers to be
/// compatible with the target provider. It:
/// - Strips thinking signatures from other providers
/// - Clears provider-specific options
/// - Preserves source tracking in metadata for debugging
#[derive(Debug, Default)]
pub struct CrossProviderSanitizationHook;

impl CrossProviderSanitizationHook {
    /// Create a new CrossProviderSanitizationHook.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RequestHook for CrossProviderSanitizationHook {
    async fn on_request(
        &self,
        request: &mut GenerateRequest,
        context: &mut HookContext,
    ) -> Result<(), HyperError> {
        // Sanitize all messages for target provider
        for msg in &mut request.messages {
            // Only convert if message came from different provider
            if !msg.metadata.is_from_provider(&context.provider) {
                // Has source info and it's different from target
                if msg.metadata.source_provider.is_some() {
                    msg.convert_for_provider(&context.provider, &context.model_id);
                }
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        5 // Run very early, before ResponseIdHook (priority 10)
    }

    fn name(&self) -> &str {
        "cross_provider_sanitization"
    }
}

#[cfg(test)]
#[path = "builtin.test.rs"]
mod tests;
