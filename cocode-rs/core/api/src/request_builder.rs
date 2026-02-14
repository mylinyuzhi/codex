//! Unified request builder for LLM inference.
//!
//! `RequestBuilder` provides a unified way to construct `GenerateRequest` from
//! an `InferenceContext`. It handles:
//!
//! - Applying model-specific parameters (temperature, top_p, max_tokens)
//! - Converting thinking configuration to provider-specific options
//! - Message sanitization for target provider
//!
//! # Example
//!
//! ```ignore
//! use cocode_api::RequestBuilder;
//! use cocode_protocol::execution::InferenceContext;
//!
//! // Context from ModelHub (selections passed as parameter)
//! let (ctx, model) = hub.prepare_main_with_selections(&selections, "session", 1)?;
//!
//! // Build request with messages and tools
//! let request = RequestBuilder::new(ctx)
//!     .messages(messages)
//!     .tools(tools)
//!     .build();
//!
//! // Use with model
//! model.stream(request).await?;
//! ```

use cocode_protocol::execution::InferenceContext;
use hyper_sdk::GenerateRequest;
use hyper_sdk::Message;
use hyper_sdk::ToolChoice;
use hyper_sdk::ToolDefinition;

use crate::request_options_merge;
use crate::thinking_convert;

/// Builder for constructing `GenerateRequest` from `InferenceContext`.
///
/// This centralizes all the parameter assembly that was previously scattered
/// across different parts of the codebase.
pub struct RequestBuilder {
    context: InferenceContext,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDefinition>>,
    tool_choice: Option<ToolChoice>,

    // Optional overrides (take precedence over context values)
    temperature_override: Option<f64>,
    max_tokens_override: Option<i32>,
    top_p_override: Option<f64>,
}

impl RequestBuilder {
    /// Create a new request builder with the given inference context.
    pub fn new(context: InferenceContext) -> Self {
        Self {
            context,
            messages: Vec::new(),
            tools: None,
            tool_choice: None,
            temperature_override: None,
            max_tokens_override: None,
            top_p_override: None,
        }
    }

    /// Set the messages for the request.
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the tools for the request.
    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice for the request.
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Override the temperature from context.
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature_override = Some(temp);
        self
    }

    /// Override the max tokens from context.
    pub fn max_tokens(mut self, tokens: i32) -> Self {
        self.max_tokens_override = Some(tokens);
        self
    }

    /// Override the top_p from context.
    pub fn top_p(mut self, p: f64) -> Self {
        self.top_p_override = Some(p);
        self
    }

    /// Build the final `GenerateRequest`.
    ///
    /// This method:
    /// 1. Sets sampling parameters from context (temperature, top_p, max_tokens)
    /// 2. Converts thinking level to provider-specific options
    /// 3. Applies any overrides
    pub fn build(self) -> GenerateRequest {
        let mut request = GenerateRequest::new(self.messages);

        // Apply temperature (override > context > default None)
        request.temperature = self
            .temperature_override
            .or_else(|| self.context.temperature().map(|t| t as f64));

        // Apply max_tokens (override > context > default None)
        request.max_tokens = self
            .max_tokens_override
            .or_else(|| self.context.max_output_tokens().map(|t| t as i32));

        // Apply top_p (override > context > default None)
        request.top_p = self
            .top_p_override
            .or_else(|| self.context.top_p().map(|p| p as f64));

        // Apply tools and tool choice
        request.tools = self.tools;
        request.tool_choice = self.tool_choice;

        // Step 1: Build provider options from thinking config
        let mut provider_options =
            if let Some(thinking_level) = self.context.effective_thinking_level() {
                thinking_convert::to_provider_options(
                    thinking_level,
                    &self.context.model_info,
                    self.context.model_spec.provider_type,
                )
            } else {
                None
            };

        // Step 2: Merge request_options into provider_options
        if let Some(req_opts) = &self.context.request_options
            && !req_opts.is_empty()
        {
            provider_options = Some(request_options_merge::merge_into_provider_options(
                provider_options,
                req_opts,
                self.context.model_spec.provider_type,
            ));
        }

        request.provider_options = provider_options;

        request
    }

    /// Get a reference to the inference context.
    pub fn context(&self) -> &InferenceContext {
        &self.context
    }
}

/// Convenience function to build a request directly from context and messages.
pub fn build_request(
    context: InferenceContext,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDefinition>>,
) -> GenerateRequest {
    let mut builder = RequestBuilder::new(context).messages(messages);
    if let Some(t) = tools {
        builder = builder.tools(t);
    }
    builder.build()
}

#[cfg(test)]
#[path = "request_builder.test.rs"]
mod tests;
