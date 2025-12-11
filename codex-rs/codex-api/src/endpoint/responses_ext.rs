//! Non-streaming client methods for Responses API.
//!
//! This module extends `ResponsesClient` with:
//! - `request()` - non-streaming request with raw body
//! - `request_prompt()` - non-streaming request with prompt and options

use crate::auth::add_auth_headers;
use crate::auth::AuthProvider;
use crate::common::Prompt as ApiPrompt;
use crate::common_ext::filter_incremental_input;
use crate::common_ext::parse_complete_response;
use crate::common_ext::NonStreamingResponse;
use crate::endpoint::responses::ResponsesClient;
use crate::endpoint::responses::ResponsesOptions;
use crate::error::ApiError;
use crate::requests::ResponsesRequestBuilder;
use crate::telemetry::run_with_request_telemetry;
use codex_client::HttpTransport;
use http::HeaderMap;
use http::Method;
use serde_json::Value;

impl<T: HttpTransport, A: AuthProvider> ResponsesClient<T, A> {
    /// Make a non-streaming request to the Responses API.
    ///
    /// This is the non-streaming counterpart to `stream()`.
    ///
    /// # Arguments
    ///
    /// * `body` - The JSON request body
    /// * `extra_headers` - Additional HTTP headers
    ///
    /// # Returns
    ///
    /// A `NonStreamingResponse` containing the parsed response events.
    pub async fn request(
        &self,
        body: Value,
        extra_headers: HeaderMap,
    ) -> Result<NonStreamingResponse, ApiError> {
        let path = self.path();

        // Build the request (no Accept: text/event-stream)
        let builder = || {
            let mut req = self.streaming.provider().build_request(Method::POST, path);
            req.headers.extend(extra_headers.clone());
            req.headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
            req.body = Some(body.clone());
            add_auth_headers(self.streaming.auth(), req)
        };

        // Execute the request with retry support
        let response = run_with_request_telemetry(
            self.streaming.provider().retry.to_policy(),
            self.streaming.request_telemetry(),
            builder,
            |req| self.streaming.transport().execute(req),
        )
        .await?;

        // Parse response body as JSON
        let body_str = String::from_utf8(response.body.to_vec())
            .map_err(|e| ApiError::Stream(format!("Invalid UTF-8 in response: {e}")))?;

        parse_complete_response(&body_str)
    }

    /// Make a non-streaming request to the Responses API with prompt.
    ///
    /// This is the non-streaming counterpart to `stream_prompt()`.
    ///
    /// # Arguments
    ///
    /// * `model` - The model to use
    /// * `prompt` - The prompt containing instructions, input, and optional previous_response_id
    /// * `options` - Request options (reasoning, text controls, etc.)
    ///
    /// # Incremental Input Filtering
    ///
    /// When `prompt.previous_response_id` is set:
    /// - Only items after the last LLM response are sent
    /// - The server uses stored history up to the previous response
    ///
    /// When `prompt.previous_response_id` is `None`:
    /// - All input items are sent (full history)
    ///
    /// # Returns
    ///
    /// A `NonStreamingResponse` containing the parsed response events.
    pub async fn request_prompt(
        &self,
        model: &str,
        prompt: &ApiPrompt,
        options: ResponsesOptions,
    ) -> Result<NonStreamingResponse, ApiError> {
        let ResponsesOptions {
            reasoning,
            include,
            prompt_cache_key,
            text,
            store_override,
            conversation_id,
            session_source,
        } = options;

        // Apply incremental filtering when previous_response_id exists
        let input = if prompt.previous_response_id.is_some() {
            match filter_incremental_input(&prompt.input) {
                None => {
                    // No LLM items found - first turn, use full input
                    &prompt.input[..]
                }
                Some(slice) if slice.is_empty() => {
                    // LLM item is last - no user input after, error state
                    return Err(ApiError::Stream(
                        "No user input after last LLM response".into(),
                    ));
                }
                Some(slice) => {
                    // Normal incremental mode - use filtered slice
                    slice
                }
            }
        } else {
            // No previous_response_id - use full input
            &prompt.input[..]
        };

        let request = ResponsesRequestBuilder::new(model, &prompt.instructions, input)
            .tools(&prompt.tools)
            .parallel_tool_calls(prompt.parallel_tool_calls)
            .reasoning(reasoning)
            .include(include)
            .prompt_cache_key(prompt_cache_key)
            .text(text)
            .conversation(conversation_id)
            .session_source(session_source)
            .store_override(store_override)
            .previous_response_id(prompt.previous_response_id.clone())
            .build_nonstream(self.streaming.provider())?;

        self.request(request.body, request.headers).await
    }
}
