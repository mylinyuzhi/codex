use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use regex::Regex;
use serde_json::Value;
use serde_json::json;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4Request;
use vercel_ai_provider::LanguageModelV4Response;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4StreamResponse;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::ProviderMetadata;
use vercel_ai_provider::ReasoningLevel;
use vercel_ai_provider::ReasoningPart;
use vercel_ai_provider::ResponseMetadata;
use vercel_ai_provider::SourceType;
use vercel_ai_provider::StreamError;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::Warning;
use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::is_custom_reasoning;
use vercel_ai_provider_utils::post_json_to_api_with_client_and_headers;
use vercel_ai_provider_utils::post_stream_to_api_with_client_and_headers;

use crate::metadata_extractor::StreamMetadataExtractor;
use crate::openai_compatible_config::OpenAICompatibleConfig;

use super::convert_chat_usage::convert_openai_compatible_chat_usage;
use super::convert_to_chat_messages::convert_to_openai_compatible_chat_messages;
use super::map_finish_reason::map_openai_compatible_chat_finish_reason;
use super::openai_compatible_chat_api::OpenAICompatibleChatChunk;
use super::openai_compatible_chat_api::OpenAICompatibleChatResponse;
use super::openai_compatible_chat_options::extract_compatible_options;
use super::prepare_tools::prepare_chat_tools;

/// OpenAI-compatible Chat Completions language model.
pub struct OpenAICompatibleChatLanguageModel {
    model_id: String,
    config: Arc<OpenAICompatibleConfig>,
}

impl OpenAICompatibleChatLanguageModel {
    /// Create a new chat language model.
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAICompatibleConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Build request body and collect warnings.
    fn get_args(
        &self,
        options: &LanguageModelV4CallOptions,
    ) -> Result<(Value, Vec<Warning>), AISdkError> {
        let mut warnings = Vec::new();
        let provider_name = self.config.provider_options_name();
        let (compat_options, passthrough) =
            extract_compatible_options(&options.provider_options, provider_name);

        // Warn: topK is not supported by OpenAI-compatible providers
        if options.top_k.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "topK".into(),
                details: Some("This model does not support topK. topK is ignored.".into()),
            });
        }

        // Convert prompt to messages (always uses "system" role, no Developer mode)
        let (messages, msg_warnings) = convert_to_openai_compatible_chat_messages(&options.prompt)?;
        warnings.extend(msg_warnings);

        // Prepare tools
        let prepared = prepare_chat_tools(&options.tools, &options.tool_choice);
        warnings.extend(prepared.warnings);

        // Build base body
        let mut body = json!({
            "model": self.model_id,
            "messages": messages,
        });

        // Add tools
        if let Some(tools) = prepared.tools {
            body["tools"] = Value::Array(tools);
        }
        if let Some(tc) = prepared.tool_choice {
            body["tool_choice"] = tc;
        }

        // Standard parameters
        set_optional_f32(&mut body, "temperature", options.temperature);
        set_optional_f32(&mut body, "top_p", options.top_p);
        set_optional_f32(&mut body, "frequency_penalty", options.frequency_penalty);
        set_optional_f32(&mut body, "presence_penalty", options.presence_penalty);

        if let Some(max) = options.max_output_tokens {
            body["max_tokens"] = json!(max);
        }

        // Reasoning effort: provider option takes precedence, then top-level reasoning
        let resolved_reasoning_effort =
            compat_options
                .reasoning_effort
                .clone()
                .or_else(|| match options.reasoning {
                    Some(level)
                        if is_custom_reasoning(Some(level)) && level != ReasoningLevel::None =>
                    {
                        Some(level.as_str().to_string())
                    }
                    _ => None,
                });
        if let Some(ref effort) = resolved_reasoning_effort {
            body["reasoning_effort"] = Value::String(effort.clone());
        }

        // Text verbosity (as string, generic)
        if let Some(ref verbosity) = compat_options.text_verbosity {
            body["verbosity"] = Value::String(verbosity.clone());
        }

        // Common fields
        if let Some(ref stop) = options.stop_sequences
            && !stop.is_empty()
        {
            body["stop"] = json!(stop);
        }
        if let Some(seed) = options.seed {
            body["seed"] = json!(seed);
        }

        // Known schema field: user
        if let Some(ref user) = compat_options.user {
            body["user"] = Value::String(user.clone());
        }

        // Response format
        if let Some(ref format) = options.response_format {
            match format {
                vercel_ai_provider::ResponseFormat::Text => {
                    // Omit response_format for text (some providers reject explicit {"type":"text"})
                }
                vercel_ai_provider::ResponseFormat::Json {
                    schema,
                    name,
                    description,
                } => {
                    if let Some(schema) = schema
                        && self.config.supports_structured_outputs
                    {
                        let strict = compat_options.strict_json_schema.unwrap_or(true);
                        let schema_name = name.as_deref().unwrap_or("response");
                        let mut json_schema = json!({
                            "type": "json_schema",
                            "json_schema": {
                                "schema": schema,
                                "strict": strict,
                                "name": schema_name,
                            }
                        });
                        if let Some(desc) = description {
                            json_schema["json_schema"]["description"] = Value::String(desc.clone());
                        }
                        body["response_format"] = json_schema;
                    } else {
                        // Warn if schema was provided but structured outputs not supported
                        if schema.is_some() {
                            warnings.push(Warning::Unsupported {
                                feature: "responseFormat.schema".into(),
                                details: Some(
                                    "JSON schema is only supported with structuredOutputs. \
                                     Falling back to json_object format."
                                        .into(),
                                ),
                            });
                        }
                        body["response_format"] = json!({"type": "json_object"});
                    }
                }
            }
        }

        // Passthrough: spread remaining provider-specific keys into body
        if let Some(obj) = body.as_object_mut() {
            for (k, v) in &passthrough {
                obj.insert(k.clone(), v.clone());
            }
        }

        // Apply request body transform
        let body = self.config.transform_body(body);

        Ok((body, warnings))
    }
}

#[async_trait]
impl LanguageModelV4 for OpenAICompatibleChatLanguageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        self.config
            .supported_urls
            .as_ref()
            .map(|f| f())
            .unwrap_or_default()
    }

    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let (body, warnings) = self.get_args(&options)?;
        let url = self.config.url("/chat/completions");
        let headers = self.config.get_headers();
        let provider_name = self.config.provider_options_name();

        let api_response =
            post_json_to_api_with_client_and_headers::<OpenAICompatibleChatResponse>(
                &url,
                Some(headers),
                &body,
                JsonResponseHandler::new(),
                self.config.error_handler.clone(),
                options.abort_signal,
                self.config.client.clone(),
            )
            .await?;

        let response = api_response.value;
        let response_headers = api_response.headers;

        // Extract content from first choice
        let choice = response
            .choices
            .first()
            .ok_or_else(|| AISdkError::new("No choices in response"))?;

        let mut content: Vec<AssistantContentPart> = Vec::new();

        // Text content (before reasoning, matching TS order)
        if let Some(ref text) = choice.message.content
            && !text.is_empty()
        {
            content.push(AssistantContentPart::Text(TextPart {
                text: text.clone(),
                provider_metadata: None,
            }));
        }

        // Reasoning content (check both fields)
        let reasoning_text = choice
            .message
            .reasoning_content
            .as_ref()
            .or(choice.message.reasoning.as_ref());
        if let Some(reasoning) = reasoning_text
            && !reasoning.is_empty()
        {
            content.push(AssistantContentPart::Reasoning(ReasoningPart::new(
                reasoning.clone(),
            )));
        }

        // Tool calls
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);

                // Extract thought_signature from extra_content.google.thought_signature
                let tc_provider_metadata = tc
                    .extra_content
                    .as_ref()
                    .and_then(|ec| ec.get("google"))
                    .and_then(|g| g.get("thought_signature"))
                    .and_then(|ts| ts.as_str())
                    .map(|ts| {
                        let mut inner = HashMap::new();
                        inner.insert(
                            "thoughtSignature".to_string(),
                            Value::String(ts.to_string()),
                        );
                        let mut meta = HashMap::new();
                        meta.insert(
                            provider_name.to_string(),
                            Value::Object(inner.into_iter().collect()),
                        );
                        ProviderMetadata(meta)
                    });

                content.push(AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: tc
                        .id
                        .clone()
                        .unwrap_or_else(|| vercel_ai_provider_utils::generate_id("call")),
                    tool_name: tc.function.name.clone(),
                    input,
                    provider_executed: None,
                    provider_metadata: tc_provider_metadata,
                }));
            }
        }

        // Sources from annotations
        if let Some(ref annotations) = choice.message.annotations {
            for ann in annotations {
                if ann.annotation_type.as_deref() == Some("url_citation")
                    && let Some(ref citation) = ann.url_citation
                {
                    content.push(AssistantContentPart::Source(
                        vercel_ai_provider::content::SourcePart {
                            source_type: SourceType::Url,
                            id: vercel_ai_provider_utils::generate_id("src"),
                            url: citation.url.clone(),
                            title: citation.title.clone(),
                            media_type: None,
                            filename: None,
                            provider_metadata: None,
                        },
                    ));
                }
            }
        }

        let finish_reason =
            map_openai_compatible_chat_finish_reason(choice.finish_reason.as_deref());
        let usage = convert_openai_compatible_chat_usage(response.usage.as_ref());

        // Provider metadata
        let mut provider_meta = HashMap::new();
        if let Some(ref logprobs) = choice.logprobs
            && let Ok(v) = serde_json::to_value(logprobs)
        {
            provider_meta.insert("logprobs".into(), v);
        }
        if let Some(ref tier) = response.service_tier {
            provider_meta.insert("serviceTier".into(), Value::String(tier.clone()));
        }

        // Prediction tokens in provider metadata
        if let Some(ref usage_data) = response.usage
            && let Some(ref details) = usage_data.completion_tokens_details
        {
            let mut prediction_meta = serde_json::Map::new();
            if let Some(accepted) = details.accepted_prediction_tokens {
                prediction_meta.insert(
                    "acceptedPredictionTokens".into(),
                    Value::Number(accepted.into()),
                );
            }
            if let Some(rejected) = details.rejected_prediction_tokens {
                prediction_meta.insert(
                    "rejectedPredictionTokens".into(),
                    Value::Number(rejected.into()),
                );
            }
            if !prediction_meta.is_empty() {
                provider_meta.insert(provider_name.to_string(), Value::Object(prediction_meta));
            }
        }

        // Use MetadataExtractor if available
        if let Some(ref extractor) = self.config.metadata_extractor
            && let Ok(resp_value) = serde_json::to_value(&response)
            && let Some(extracted) = extractor.extract_metadata(&resp_value)
        {
            for (k, v) in extracted.0 {
                provider_meta.insert(k, v);
            }
        }

        let provider_metadata = if provider_meta.is_empty() {
            None
        } else {
            Some(ProviderMetadata(provider_meta))
        };

        // Response metadata
        let response_body = serde_json::to_value(&response).ok();
        let timestamp = response
            .created
            .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
            .map(|dt| dt.to_rfc3339());

        Ok(LanguageModelV4GenerateResult {
            content,
            usage,
            finish_reason,
            warnings,
            provider_metadata,
            request: Some(LanguageModelV4Request { body: Some(body) }),
            response: Some(LanguageModelV4Response {
                timestamp,
                model_id: response.model,
                headers: Some(response_headers),
                body: response_body,
            }),
        })
    }

    async fn do_stream(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        let (mut body, warnings) = self.get_args(&options)?;

        // Enable streaming
        body["stream"] = Value::Bool(true);

        // Conditionally include stream_options based on include_usage setting
        if self.config.include_usage {
            body["stream_options"] = json!({"include_usage": true});
        }

        let include_raw = options.include_raw_chunks.unwrap_or(false);

        let url = self.config.url("/chat/completions");
        let headers = self.config.get_headers();
        let provider_name = self.config.provider_options_name();

        let (byte_stream, response_headers) = post_stream_to_api_with_client_and_headers(
            &url,
            Some(headers),
            &body,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let request_body = body.clone();

        // Create stream metadata extractor if available
        let stream_extractor = self
            .config
            .metadata_extractor
            .as_ref()
            .and_then(|e| e.create_stream_extractor());

        let stream = create_chat_stream(
            byte_stream,
            warnings,
            include_raw,
            provider_name.to_string(),
            stream_extractor,
        );

        Ok(LanguageModelV4StreamResult {
            stream,
            request: Some(LanguageModelV4Request {
                body: Some(request_body),
            }),
            response: Some(LanguageModelV4StreamResponse {
                headers: Some(response_headers),
            }),
        })
    }
}

/// In-progress tool call accumulator.
struct InProgressToolCall {
    id: String,
    name: String,
    arguments: String,
    has_finished: bool,
    started: bool,
    thought_signature: Option<String>,
}

/// Create a stream of `LanguageModelV4StreamPart` from a raw SSE byte stream.
fn create_chat_stream(
    byte_stream: vercel_ai_provider_utils::ByteStream,
    warnings: Vec<Warning>,
    include_raw: bool,
    provider_name: String,
    stream_extractor: Option<Box<dyn StreamMetadataExtractor>>,
) -> Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>> {
    let stream = futures::stream::unfold(
        ChatStreamState::new(
            byte_stream,
            warnings,
            include_raw,
            provider_name,
            stream_extractor,
        ),
        |mut state| async move {
            loop {
                // If we have queued events, yield them first
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), state));
                }

                // If done and all pending events drained, yield nothing
                if state.done && state.pending.is_empty() {
                    return None;
                }

                // Read more bytes and parse SSE lines
                match state.next_events().await {
                    Ok(true) => {
                        // More events pending, loop back to drain
                    }
                    Ok(false) => {
                        // Stream ended
                        state.done = true;

                        // Flush open segments (idempotent — safe if already closed)
                        state.close_reasoning();
                        state.close_text();

                        // Finalize unfinished tool calls
                        let tool_calls = std::mem::take(&mut state.tool_calls);
                        for tc in tool_calls {
                            if tc.started && !tc.has_finished {
                                state
                                    .pending
                                    .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                        id: tc.id.clone(),
                                        provider_metadata: None,
                                    });

                                let input: Value =
                                    serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                                let pm = state.tool_call_provider_metadata(&tc.thought_signature);
                                let mut tool_call =
                                    vercel_ai_provider::tool::ToolCall::new(tc.id, tc.name, input);
                                tool_call.provider_metadata = pm;
                                state
                                    .pending
                                    .push_back(LanguageModelV4StreamPart::ToolCall(tool_call));
                            }
                        }

                        // Emit finish if we haven't already
                        if !state.finish_emitted {
                            state.finish_emitted = true;

                            // Build provider metadata: merge extractor + prediction tokens
                            let mut finish_meta = state
                                .stream_extractor
                                .as_ref()
                                .and_then(|e| e.build_metadata())
                                .map(|pm| pm.0)
                                .unwrap_or_default();

                            if let Some(ref usage_data) = state.usage
                                && let Some(ref details) = usage_data.completion_tokens_details
                            {
                                let mut prediction = serde_json::Map::new();
                                if let Some(accepted) = details.accepted_prediction_tokens {
                                    prediction.insert(
                                        "acceptedPredictionTokens".into(),
                                        Value::Number(accepted.into()),
                                    );
                                }
                                if let Some(rejected) = details.rejected_prediction_tokens {
                                    prediction.insert(
                                        "rejectedPredictionTokens".into(),
                                        Value::Number(rejected.into()),
                                    );
                                }
                                if !prediction.is_empty() {
                                    finish_meta.insert(
                                        state.provider_name.clone(),
                                        Value::Object(prediction),
                                    );
                                }
                            }

                            let provider_metadata = if finish_meta.is_empty() {
                                None
                            } else {
                                Some(ProviderMetadata(finish_meta))
                            };

                            let finish = LanguageModelV4StreamPart::Finish {
                                usage: convert_openai_compatible_chat_usage(state.usage.as_ref()),
                                finish_reason: map_openai_compatible_chat_finish_reason(
                                    state.finish_reason.as_deref(),
                                ),
                                provider_metadata,
                            };
                            state.pending.push_back(finish);
                        }
                        // Fall through to loop — drain pending (TextEnd, Finish, etc.)
                    }
                    Err(e) => {
                        state.done = true;
                        return Some((Err(e), state));
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

struct ChatStreamState {
    byte_stream: vercel_ai_provider_utils::ByteStream,
    buffer: String,
    pending: std::collections::VecDeque<LanguageModelV4StreamPart>,
    tool_calls: Vec<InProgressToolCall>,
    text_started: bool,
    text_id: String,
    reasoning_started: bool,
    reasoning_id: String,
    usage: Option<super::convert_chat_usage::OpenAICompatibleChatUsage>,
    finish_reason: Option<String>,
    finish_emitted: bool,
    done: bool,
    metadata_emitted: bool,
    include_raw: bool,
    provider_name: String,
    stream_extractor: Option<Box<dyn StreamMetadataExtractor>>,
}

impl ChatStreamState {
    fn new(
        byte_stream: vercel_ai_provider_utils::ByteStream,
        warnings: Vec<Warning>,
        include_raw: bool,
        provider_name: String,
        stream_extractor: Option<Box<dyn StreamMetadataExtractor>>,
    ) -> Self {
        let mut pending = std::collections::VecDeque::new();
        pending.push_back(LanguageModelV4StreamPart::StreamStart { warnings });

        Self {
            byte_stream,
            buffer: String::new(),
            pending,
            tool_calls: Vec::new(),
            text_started: false,
            text_id: vercel_ai_provider_utils::generate_id("txt"),
            reasoning_started: false,
            reasoning_id: vercel_ai_provider_utils::generate_id("reason"),
            usage: None,
            finish_reason: None,
            finish_emitted: false,
            done: false,
            metadata_emitted: false,
            include_raw,
            provider_name,
            stream_extractor,
        }
    }

    /// Read from byte_stream, parse SSE lines, produce events.
    /// Returns Ok(true) if the stream is still open, Ok(false) if the stream ended.
    async fn next_events(&mut self) -> Result<bool, AISdkError> {
        use futures::StreamExt;

        match self.byte_stream.next().await {
            Some(Ok(bytes)) => {
                let text = String::from_utf8_lossy(&bytes);
                self.buffer.push_str(&text);
                self.process_buffer();
                Ok(true)
            }
            Some(Err(e)) => Err(AISdkError::new(format!("Stream read error: {e}"))),
            None => Ok(false),
        }
    }

    /// Process accumulated buffer, extracting complete SSE data lines.
    fn process_buffer(&mut self) {
        while let Some(line_end) = self.buffer.find('\n') {
            let line = self.buffer[..line_end].trim_end_matches('\r').to_string();
            self.buffer = self.buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
            {
                if data == "[DONE]" {
                    continue;
                }
                self.process_data_line(data);
            }
        }
    }

    /// Build provider metadata for a tool call (includes thought_signature if present).
    fn tool_call_provider_metadata(
        &self,
        thought_signature: &Option<String>,
    ) -> Option<ProviderMetadata> {
        thought_signature.as_ref().map(|ts| {
            let mut inner = serde_json::Map::new();
            inner.insert("thoughtSignature".into(), Value::String(ts.clone()));
            let mut meta = HashMap::new();
            meta.insert(self.provider_name.clone(), Value::Object(inner));
            ProviderMetadata(meta)
        })
    }

    /// Close reasoning if it's currently active.
    fn close_reasoning(&mut self) {
        if self.reasoning_started {
            self.reasoning_started = false;
            self.pending
                .push_back(LanguageModelV4StreamPart::ReasoningEnd {
                    id: self.reasoning_id.clone(),
                    provider_metadata: None,
                });
        }
    }

    /// Close text if it's currently active.
    fn close_text(&mut self) {
        if self.text_started {
            self.text_started = false;
            self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                id: self.text_id.clone(),
                provider_metadata: None,
            });
        }
    }

    /// Process a single SSE data JSON line.
    fn process_data_line(&mut self, data: &str) {
        // Parse once as Value for reuse
        let raw: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                self.finish_reason = Some("error".to_string());
                self.pending.push_back(LanguageModelV4StreamPart::Error {
                    error: StreamError::new(format!("Failed to parse chat chunk: {e}")),
                });
                return;
            }
        };

        // 1. Emit raw chunk BEFORE any validation (matches TS)
        if self.include_raw {
            self.pending.push_back(LanguageModelV4StreamPart::Raw {
                raw_value: raw.clone(),
            });
        }

        // 2. Feed metadata extractor BEFORE error-key check (matches TS)
        if let Some(ref mut extractor) = self.stream_extractor {
            extractor.process_chunk(&raw);
        }

        // 3. Detect error chunks from the API (e.g. {"error": {"message": "..."}})
        if let Some(error) = raw.get("error") {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            self.finish_reason = Some("error".to_string());
            self.pending.push_back(LanguageModelV4StreamPart::Error {
                error: StreamError::new(message),
            });
            return;
        }

        // 4. Typed deserialization
        let chunk: OpenAICompatibleChatChunk = match serde_json::from_value(raw) {
            Ok(c) => c,
            Err(e) => {
                self.finish_reason = Some("error".to_string());
                self.pending.push_back(LanguageModelV4StreamPart::Error {
                    error: StreamError::new(format!("Invalid chat chunk structure: {e}")),
                });
                return;
            }
        };

        // Emit response metadata once
        if !self.metadata_emitted {
            self.metadata_emitted = true;
            let timestamp = chunk
                .created
                .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
                .map(|dt| dt.to_rfc3339());

            let mut meta = ResponseMetadata::new();
            if let Some(ref id) = chunk.id {
                meta = meta.with_id(id.clone());
            }
            if let Some(ref model) = chunk.model {
                meta = meta.with_model(model.clone());
            }
            if let Some(ts) = timestamp {
                meta = meta.with_timestamp(ts);
            }
            if let Some(ref tier) = chunk.service_tier {
                let pm = ProviderMetadata(HashMap::from([(
                    "serviceTier".to_string(),
                    Value::String(tier.clone()),
                )]));
                meta = meta.with_provider_metadata(pm);
            }
            self.pending
                .push_back(LanguageModelV4StreamPart::ResponseMetadata(meta));
        }

        // Usage (comes in the final chunk)
        if let Some(ref u) = chunk.usage {
            self.usage = Some(u.clone());
        }

        // Process choices
        if let Some(ref choices) = chunk.choices {
            for choice in choices {
                // Track finish reason
                if let Some(ref fr) = choice.finish_reason {
                    self.finish_reason = Some(fr.clone());
                }

                // Reasoning content delta
                let reasoning_delta = choice
                    .delta
                    .reasoning_content
                    .as_ref()
                    .or(choice.delta.reasoning.as_ref());
                if let Some(reasoning) = reasoning_delta
                    && !reasoning.is_empty()
                {
                    if !self.reasoning_started {
                        self.reasoning_started = true;
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ReasoningStart {
                                id: self.reasoning_id.clone(),
                                provider_metadata: None,
                            });
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ReasoningDelta {
                            id: self.reasoning_id.clone(),
                            delta: reasoning.clone(),
                            provider_metadata: None,
                        });
                }

                // Text delta
                if let Some(ref content) = choice.delta.content
                    && !content.is_empty()
                {
                    // End reasoning if it was open before text starts
                    self.close_reasoning();

                    if !self.text_started {
                        self.text_started = true;
                        self.pending
                            .push_back(LanguageModelV4StreamPart::TextStart {
                                id: self.text_id.clone(),
                                provider_metadata: None,
                            });
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::TextDelta {
                            id: self.text_id.clone(),
                            delta: content.clone(),
                            provider_metadata: None,
                        });
                }

                // Tool call deltas (#6: close reasoning before tool calls)
                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    // Close reasoning when tool calls start
                    self.close_reasoning();

                    for tc_delta in tool_calls {
                        let idx = tc_delta.index.unwrap_or(self.tool_calls.len() as u32) as usize;

                        // Track if this is a new tool call entry
                        let is_new = self.tool_calls.len() <= idx;

                        // Ensure tool_calls vec is big enough
                        while self.tool_calls.len() <= idx {
                            self.tool_calls.push(InProgressToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                                has_finished: false,
                                started: false,
                                thought_signature: None,
                            });
                        }

                        // Skip if already finished (early completion)
                        if self.tool_calls[idx].has_finished {
                            continue;
                        }

                        // Update tool call state from delta
                        if let Some(ref id) = tc_delta.id {
                            self.tool_calls[idx].id = id.clone();
                        }

                        // Validate first delta for a new tool call has id and function.name
                        if is_new {
                            let has_id = tc_delta.id.is_some();
                            let has_name = tc_delta
                                .function
                                .as_ref()
                                .and_then(|f| f.name.as_ref())
                                .is_some();
                            if !has_id || !has_name {
                                self.pending.push_back(LanguageModelV4StreamPart::Error {
                                    error: StreamError::new(format!(
                                        "Invalid response data: expected tool call \
                                             delta to have id and function.name in first chunk, \
                                             got id={:?} function.name={:?}",
                                        tc_delta.id,
                                        tc_delta.function.as_ref().and_then(|f| f.name.as_ref()),
                                    )),
                                });
                                continue;
                            }
                        }

                        // Capture thought_signature from extra_content
                        if let Some(ref ec) = tc_delta.extra_content
                            && let Some(ts) = ec
                                .get("google")
                                .and_then(|g| g.get("thought_signature"))
                                .and_then(|v| v.as_str())
                        {
                            self.tool_calls[idx].thought_signature = Some(ts.to_string());
                        }

                        if let Some(ref func) = tc_delta.function {
                            if let Some(ref name) = func.name {
                                self.tool_calls[idx].name = name.clone();
                            }
                            if let Some(ref args) = func.arguments {
                                self.tool_calls[idx].arguments.push_str(args);
                            }
                        }

                        // Emit ToolInputStart on first delta
                        if !self.tool_calls[idx].started && !self.tool_calls[idx].name.is_empty() {
                            self.tool_calls[idx].started = true;

                            let tc_id = self.tool_calls[idx].id.clone();
                            let tc_name = self.tool_calls[idx].name.clone();
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolInputStart {
                                    id: tc_id,
                                    tool_name: tc_name,
                                    provider_executed: None,
                                    dynamic: None,
                                    title: None,
                                    provider_metadata: None,
                                });
                        }

                        // Emit argument delta
                        if let Some(ref func) = tc_delta.function
                            && let Some(ref args) = func.arguments
                            && !args.is_empty()
                        {
                            let tc_id = self.tool_calls[idx].id.clone();
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                                    id: tc_id,
                                    delta: args.clone(),
                                    provider_metadata: None,
                                });
                        }

                        // #7: Early tool call completion via JSON parse detection
                        if self.tool_calls[idx].started
                            && !self.tool_calls[idx].has_finished
                            && !self.tool_calls[idx].arguments.is_empty()
                            && serde_json::from_str::<Value>(&self.tool_calls[idx].arguments)
                                .is_ok()
                        {
                            self.tool_calls[idx].has_finished = true;

                            let thought_sig = self.tool_calls[idx].thought_signature.clone();
                            let pm = self.tool_call_provider_metadata(&thought_sig);
                            let tc_id = self.tool_calls[idx].id.clone();
                            let tc_name = self.tool_calls[idx].name.clone();
                            let tc_args = self.tool_calls[idx].arguments.clone();

                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                    id: tc_id.clone(),
                                    provider_metadata: None,
                                });

                            let input: Value =
                                serde_json::from_str(&tc_args).unwrap_or(Value::Null);
                            let mut tool_call =
                                vercel_ai_provider::tool::ToolCall::new(tc_id, tc_name, input);
                            tool_call.provider_metadata = pm;
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolCall(tool_call));
                        }
                    }
                }

                // Annotations (sources)
                if let Some(ref annotations) = choice.delta.annotations {
                    for ann in annotations {
                        if ann.annotation_type.as_deref() == Some("url_citation")
                            && let Some(ref citation) = ann.url_citation
                        {
                            self.pending.push_back(LanguageModelV4StreamPart::Source(
                                vercel_ai_provider::content::SourcePart {
                                    source_type: SourceType::Url,
                                    id: vercel_ai_provider_utils::generate_id("src"),
                                    url: citation.url.clone(),
                                    title: citation.title.clone(),
                                    media_type: None,
                                    filename: None,
                                    provider_metadata: None,
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn set_optional_f32(body: &mut Value, key: &str, value: Option<f32>) {
    if let Some(v) = value {
        body[key] = json!(v);
    }
}

#[cfg(test)]
#[path = "openai_compatible_chat_language_model.test.rs"]
mod tests;
