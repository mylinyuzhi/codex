//! Unified request builder for LLM inference.
//!
//! Builds `LanguageModelCallOptions` from an `InferenceContext`.

use crate::LanguageModelCallOptions;
use crate::LanguageModelMessage;
use crate::LanguageModelPrompt;
use crate::LanguageModelTool;
use crate::LanguageModelToolChoice;
use cocode_config::interceptors;
use cocode_protocol::execution::InferenceContext;

use crate::message_normalize;
use crate::prompt_cache;
use crate::request_options_merge;
use crate::schema_sanitize;
use crate::thinking_convert;

/// Builder for constructing `LanguageModelCallOptions` from `InferenceContext`.
pub struct RequestBuilder {
    context: InferenceContext,
    prompt: LanguageModelPrompt,
    tools: Option<Vec<LanguageModelTool>>,
    tool_choice: Option<LanguageModelToolChoice>,

    // Optional overrides
    temperature_override: Option<f32>,
    max_tokens_override: Option<u64>,
    top_p_override: Option<f32>,
}

impl RequestBuilder {
    /// Create a new request builder with the given inference context.
    pub fn new(context: InferenceContext) -> Self {
        Self {
            context,
            prompt: Vec::new(),
            tools: None,
            tool_choice: None,
            temperature_override: None,
            max_tokens_override: None,
            top_p_override: None,
        }
    }

    /// Set the messages (prompt) for the request.
    pub fn messages(mut self, messages: Vec<LanguageModelMessage>) -> Self {
        self.prompt = messages;
        self
    }

    /// Set the tools for the request.
    pub fn tools(mut self, tools: Vec<LanguageModelTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice for the request.
    pub fn tool_choice(mut self, choice: LanguageModelToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Override the temperature from context.
    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature_override = Some(temp);
        self
    }

    /// Override the max tokens from context.
    pub fn max_tokens(mut self, tokens: u64) -> Self {
        self.max_tokens_override = Some(tokens);
        self
    }

    /// Override the top_p from context.
    pub fn top_p(mut self, p: f32) -> Self {
        self.top_p_override = Some(p);
        self
    }

    /// Build the final `LanguageModelCallOptions`.
    ///
    /// Pipeline (in execution order):
    ///   Step 1: message normalization (mutates prompt in place)
    ///   Step 2: provider base options (store:false, thinkingConfig, etc.)
    ///   Step 3: reasoning level + thinking config → provider options
    ///   Step 4: request_options merge (user config overlay)
    ///   Step 5: HTTP interceptors → extra headers
    ///
    /// Each step overrides the previous, so user config always wins.
    pub fn build(mut self) -> LanguageModelCallOptions {
        let api = self.context.model_spec.api;

        // Step 1: Per-provider message normalization (empty content, tool ID sanitization)
        message_normalize::normalize_prompt(&mut self.prompt, api);

        // Step 1.5: Apply prompt cache breakpoints (Anthropic only)
        if let Some(ref cache_config) = self.context.prompt_cache_config {
            prompt_cache::apply_message_breakpoints(
                &mut self.prompt,
                cache_config,
                api,
                &self.context.model_spec.slug,
            );
        }

        let mut opts = LanguageModelCallOptions::new(self.prompt);

        // Apply temperature (override > context > default None)
        opts.temperature = self
            .temperature_override
            .or_else(|| self.context.temperature());

        // Apply max_tokens (override > context > default None)
        opts.max_output_tokens = self
            .max_tokens_override
            .or_else(|| self.context.max_output_tokens().map(|t| t as u64));

        // Apply top_p (override > context > default None)
        opts.top_p = self.top_p_override.or_else(|| self.context.top_p());

        // Apply top_k from model info
        opts.top_k = self.context.model_info.top_k.map(|k| k as u64);

        // Apply tools (with provider-specific schema sanitization) and tool choice
        if let Some(ref mut tools) = self.tools {
            schema_sanitize::sanitize_tool_schemas(tools, api);
        }
        opts.tools = self.tools;
        opts.tool_choice = self.tool_choice;

        // Step 2: Inject provider-specific base options (store:false, thinkingConfig, etc.)
        let mut provider_options = request_options_merge::provider_base_options(api);

        // Step 3: Reasoning level + provider options from thinking config
        if let Some(thinking_level) = self.context.effective_thinking_level() {
            opts.reasoning = thinking_convert::effort_to_reasoning_level(thinking_level.effort);
            let thinking_opts = thinking_convert::to_provider_options(
                thinking_level,
                &self.context.model_info,
                api,
            );
            provider_options =
                request_options_merge::merge_provider_options(provider_options, thinking_opts);
        }

        // Step 4: Merge request_options into provider_options (overrides thinking)
        if let Some(req_opts) = &self.context.request_options
            && !req_opts.is_empty()
        {
            provider_options = Some(request_options_merge::merge_into_provider_options(
                provider_options,
                req_opts,
                api,
            ));
        }

        opts.provider_options = provider_options;

        // Step 5: Apply HTTP interceptors as extra headers
        if !self.context.interceptor_names.is_empty() {
            let mut chain = interceptors::resolve_chain(&self.context.interceptor_names);
            if !chain.is_empty() {
                let mut http_request =
                    interceptors::HttpRequest::post(&self.context.model_spec.provider);
                let ctx = interceptors::HttpInterceptorContext::with_provider(
                    &self.context.model_spec.provider,
                    &self.context.model_spec.slug,
                )
                .conversation_id(&self.context.session_id)
                .request_id(&self.context.call_id);
                chain.apply(&mut http_request, &ctx);

                // Copy interceptor-injected headers to call options
                if !http_request.headers.is_empty() {
                    let mut headers = opts.headers.take().unwrap_or_default();
                    for (name, value) in &http_request.headers {
                        if let Ok(v) = value.to_str() {
                            headers.insert(name.to_string(), v.to_string());
                        }
                    }
                    opts.headers = Some(headers);
                }
            }
        }

        opts
    }

    /// Get a reference to the inference context.
    pub fn context(&self) -> &InferenceContext {
        &self.context
    }
}

/// Convenience function to build a request directly from context and messages.
pub fn build_request(
    context: InferenceContext,
    messages: Vec<LanguageModelMessage>,
    tools: Option<Vec<LanguageModelTool>>,
) -> LanguageModelCallOptions {
    let mut builder = RequestBuilder::new(context).messages(messages);
    if let Some(t) = tools {
        builder = builder.tools(t);
    }
    builder.build()
}

#[cfg(test)]
#[path = "request_builder.test.rs"]
mod tests;
