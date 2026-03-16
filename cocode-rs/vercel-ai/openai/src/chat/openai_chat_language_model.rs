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
use vercel_ai_provider::ResponseFormat;
use vercel_ai_provider::ResponseMetadata;
use vercel_ai_provider::SourceType;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::Warning;
use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::post_stream_to_api_with_client;

use crate::openai_capabilities::SystemMessageMode;
use crate::openai_capabilities::get_capabilities;
use crate::openai_config::OpenAIConfig;
use crate::openai_error::OpenAIFailedResponseHandler;

use super::convert_chat_usage::convert_openai_chat_usage;
use super::convert_to_chat_messages::convert_to_openai_chat_messages;
use super::map_finish_reason::map_openai_chat_finish_reason;
use super::openai_chat_api::OpenAIChatChunk;
use super::openai_chat_api::OpenAIChatResponse;
use super::openai_chat_options::OpenAIChatProviderOptions;
use super::openai_chat_options::ReasoningEffort;
use super::openai_chat_options::extract_openai_options;
use super::prepare_tools::prepare_chat_tools;

/// OpenAI Chat Completions language model.
pub struct OpenAIChatLanguageModel {
    model_id: String,
    config: Arc<OpenAIConfig>,
}

impl OpenAIChatLanguageModel {
    /// Create a new chat language model.
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAIConfig>) -> Self {
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
        let openai_options = extract_openai_options(&options.provider_options);
        let caps = get_capabilities(&self.model_id);

        let force_reasoning = openai_options.force_reasoning.unwrap_or(false);
        let is_reasoning_model = force_reasoning || caps.is_reasoning_model;

        let system_message_mode =
            openai_options
                .system_message_mode
                .unwrap_or(if is_reasoning_model {
                    SystemMessageMode::Developer
                } else {
                    caps.system_message_mode
                });

        // Convert prompt to messages
        let (messages, msg_warnings) =
            convert_to_openai_chat_messages(&options.prompt, system_message_mode);
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

        // Reasoning effort
        let reasoning_effort = openai_options.reasoning_effort;
        let is_no_effort = reasoning_effort == Some(ReasoningEffort::None);
        let can_use_non_reasoning_params =
            is_no_effort && caps.supports_non_reasoning_params_with_no_effort;

        // Reasoning model parameter restrictions
        if is_reasoning_model {
            if let Some(effort) = reasoning_effort {
                body["reasoning_effort"] = Value::String(effort.as_str().into());
            }

            // Use max_completion_tokens instead of max_tokens
            if let Some(max) = openai_options
                .max_completion_tokens
                .or(options.max_output_tokens)
            {
                body["max_completion_tokens"] = json!(max);
            }

            // Only include these if effort is none and model supports it
            if can_use_non_reasoning_params {
                set_optional_f32(&mut body, "temperature", options.temperature);
                set_optional_f32(&mut body, "top_p", options.top_p);
                set_logprobs(&mut body, &openai_options);
            }
            // Always omit: frequency_penalty, presence_penalty, logit_bias for reasoning
        } else {
            // Non-reasoning model: include all parameters
            set_optional_f32(&mut body, "temperature", options.temperature);
            set_optional_f32(&mut body, "top_p", options.top_p);
            set_optional_f32(&mut body, "frequency_penalty", options.frequency_penalty);
            set_optional_f32(&mut body, "presence_penalty", options.presence_penalty);

            if let Some(max) = options.max_output_tokens {
                body["max_tokens"] = json!(max);
            }

            set_logprobs(&mut body, &openai_options);

            if let Some(ref bias) = openai_options.logit_bias {
                body["logit_bias"] = serde_json::to_value(bias).unwrap_or_default();
            }
        }

        // Search model handling
        if self.model_id.starts_with("gpt-4o-search-preview") {
            body.as_object_mut().map(|o| o.remove("temperature"));
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

        // Provider-specific fields
        if let Some(ref user) = openai_options.user {
            body["user"] = Value::String(user.clone());
        }
        if let Some(parallel) = openai_options.parallel_tool_calls {
            body["parallel_tool_calls"] = Value::Bool(parallel);
        }
        if let Some(store) = openai_options.store {
            body["store"] = Value::Bool(store);
        }
        if let Some(ref metadata) = openai_options.metadata {
            body["metadata"] = serde_json::to_value(metadata).unwrap_or_default();
        }
        if let Some(ref prediction) = openai_options.prediction {
            body["prediction"] = prediction.clone();
        }
        if let Some(ref verbosity) = openai_options.text_verbosity {
            body["verbosity"] = Value::String(verbosity.as_str().into());
        }
        if let Some(ref cache_key) = openai_options.prompt_cache_key {
            body["prompt_cache_key"] = Value::String(cache_key.clone());
        }
        if let Some(ref retention) = openai_options.prompt_cache_retention {
            body["prompt_cache_retention"] = Value::String(retention.as_str().into());
        }
        if let Some(ref safety) = openai_options.safety_identifier {
            body["safety_identifier"] = Value::String(safety.clone());
        }

        // Service tier
        if let Some(ref tier) = openai_options.service_tier {
            body["service_tier"] = Value::String(tier.as_str().into());
        }

        // Response format
        if let Some(ref format) = options.response_format {
            match format {
                ResponseFormat::Text => {
                    body["response_format"] = json!({"type": "text"});
                }
                ResponseFormat::Json {
                    schema,
                    name,
                    description,
                } => {
                    if let Some(schema) = schema {
                        let strict = openai_options.strict_json_schema.unwrap_or(true);
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
                        body["response_format"] = json!({"type": "json_object"});
                    }
                }
            }
        }

        Ok((body, warnings))
    }
}

#[async_trait]
impl LanguageModelV4 for OpenAIChatLanguageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        let mut map = HashMap::new();
        // OpenAI chat supports image URLs
        if let Ok(re) = Regex::new(r"^https?://.*$") {
            map.insert("image/*".into(), vec![re]);
        }
        map
    }

    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let (body, warnings) = self.get_args(&options)?;
        let url = self.config.url("/chat/completions");
        let headers = self.config.get_headers();

        let response: OpenAIChatResponse = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            OpenAIFailedResponseHandler,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        // Extract content from first choice
        let choice = response
            .choices
            .first()
            .ok_or_else(|| AISdkError::new("No choices in response"))?;

        let mut content: Vec<AssistantContentPart> = Vec::new();

        // Text content
        if let Some(ref text) = choice.message.content
            && !text.is_empty()
        {
            content.push(AssistantContentPart::Text(TextPart {
                text: text.clone(),
                provider_metadata: None,
            }));
        }

        // Tool calls
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                content.push(AssistantContentPart::ToolCall(ToolCallPart {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.function.name.clone(),
                    input,
                    provider_executed: None,
                    provider_metadata: None,
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

        let finish_reason = map_openai_chat_finish_reason(choice.finish_reason.as_deref());
        let usage = convert_openai_chat_usage(response.usage.as_ref());

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
        let provider_metadata = if provider_meta.is_empty() {
            None
        } else {
            Some(ProviderMetadata(provider_meta))
        };

        // Response metadata
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
                headers: None,
                body: None,
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
        body["stream_options"] = json!({"include_usage": true});

        let include_raw = options.include_raw_chunks.unwrap_or(false);

        let url = self.config.url("/chat/completions");
        let headers = self.config.get_headers();

        let byte_stream = post_stream_to_api_with_client(
            &url,
            Some(headers),
            &body,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let request_body = body.clone();

        let stream = create_chat_stream(byte_stream, warnings, include_raw);

        Ok(LanguageModelV4StreamResult {
            stream,
            request: Some(LanguageModelV4Request {
                body: Some(request_body),
            }),
            response: Some(LanguageModelV4StreamResponse::new()),
        })
    }
}

/// In-progress tool call accumulator.
struct InProgressToolCall {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

/// Create a stream of `LanguageModelV4StreamPart` from a raw SSE byte stream.
fn create_chat_stream(
    byte_stream: vercel_ai_provider_utils::ByteStream,
    warnings: Vec<Warning>,
    include_raw: bool,
) -> Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>> {
    let stream = futures::stream::unfold(
        ChatStreamState::new(byte_stream, warnings, include_raw),
        |mut state| async move {
            loop {
                // If we have queued events, yield them first
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), state));
                }

                // If done, yield nothing
                if state.done {
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
                        // Emit finish if we haven't already
                        if !state.finish_emitted {
                            state.finish_emitted = true;
                            let finish = LanguageModelV4StreamPart::Finish {
                                usage: convert_openai_chat_usage(state.usage.as_ref()),
                                finish_reason: map_openai_chat_finish_reason(
                                    state.finish_reason.as_deref(),
                                ),
                                provider_metadata: None,
                            };
                            return Some((Ok(finish), state));
                        }
                        return None;
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
    usage: Option<super::convert_chat_usage::OpenAIChatUsage>,
    finish_reason: Option<String>,
    finish_emitted: bool,
    done: bool,
    metadata_emitted: bool,
    include_raw: bool,
}

impl ChatStreamState {
    fn new(
        byte_stream: vercel_ai_provider_utils::ByteStream,
        warnings: Vec<Warning>,
        include_raw: bool,
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
            usage: None,
            finish_reason: None,
            finish_emitted: false,
            done: false,
            metadata_emitted: false,
            include_raw,
        }
    }

    /// Read from byte_stream, parse SSE lines, produce events.
    /// Returns Ok(true) if there are pending events, Ok(false) if stream ended.
    async fn next_events(&mut self) -> Result<bool, AISdkError> {
        use futures::StreamExt;

        match self.byte_stream.next().await {
            Some(Ok(bytes)) => {
                let text = String::from_utf8_lossy(&bytes);
                self.buffer.push_str(&text);
                self.process_buffer();
                Ok(!self.pending.is_empty())
            }
            Some(Err(e)) => Err(AISdkError::new(format!("Stream read error: {e}"))),
            None => Ok(false),
        }
    }

    /// Process accumulated buffer, extracting complete SSE data lines.
    fn process_buffer(&mut self) {
        while let Some(line_end) = self.buffer.find('\n') {
            let line_len = line_end + 1;
            let line = self.buffer[..line_end].trim_end_matches('\r');
            if !line.is_empty()
                && let Some(data) = line.strip_prefix("data: ")
                && data != "[DONE]"
            {
                let data = data.to_string();
                self.buffer.drain(..line_len);
                self.process_data_line(&data);
                continue;
            }
            self.buffer.drain(..line_len);
        }
    }

    /// Process a single SSE data JSON line.
    fn process_data_line(&mut self, data: &str) {
        let chunk: OpenAIChatChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Emit raw chunk if requested
        if self.include_raw
            && let Ok(raw) = serde_json::from_str::<Value>(data)
        {
            self.pending
                .push_back(LanguageModelV4StreamPart::Raw { raw_value: raw });
        }

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

                // Text delta
                if let Some(ref content) = choice.delta.content
                    && !content.is_empty()
                {
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

                // Tool call deltas
                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc_delta in tool_calls {
                        let idx = tc_delta.index as usize;

                        // Ensure tool_calls vec is big enough
                        while self.tool_calls.len() <= idx {
                            self.tool_calls.push(InProgressToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                                started: false,
                            });
                        }

                        let tc = &mut self.tool_calls[idx];

                        // First chunk for this tool call
                        if let Some(ref id) = tc_delta.id {
                            tc.id = id.clone();
                        }
                        if let Some(ref func) = tc_delta.function {
                            if let Some(ref name) = func.name {
                                tc.name = name.clone();
                            }
                            if let Some(ref args) = func.arguments {
                                tc.arguments.push_str(args);
                            }
                        }

                        // Emit ToolInputStart on first delta
                        if !tc.started && !tc.name.is_empty() {
                            tc.started = true;
                            // Close text if open
                            if self.text_started {
                                self.text_started = false;
                                self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                                    id: self.text_id.clone(),
                                    provider_metadata: None,
                                });
                            }
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolInputStart {
                                    id: tc.id.clone(),
                                    tool_name: tc.name.clone(),
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
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                                    id: self.tool_calls[idx].id.clone(),
                                    delta: args.clone(),
                                    provider_metadata: None,
                                });
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

        // When stream is ending, close open text and finalize tool calls
        if self.finish_reason.is_some() {
            if self.text_started {
                self.text_started = false;
                self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                    id: self.text_id.clone(),
                    provider_metadata: None,
                });
            }

            // Finalize tool calls
            let tool_calls = std::mem::take(&mut self.tool_calls);
            for tc in tool_calls {
                if tc.started {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                            id: tc.id.clone(),
                            provider_metadata: None,
                        });

                    let input: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                        vercel_ai_provider::tool::ToolCall::new(tc.id, tc.name, input),
                    ));
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

fn set_logprobs(body: &mut Value, options: &OpenAIChatProviderOptions) {
    if let Some(ref logprobs) = options.logprobs {
        match logprobs {
            Value::Bool(true) => {
                body["logprobs"] = Value::Bool(true);
            }
            Value::Number(n) => {
                body["logprobs"] = Value::Bool(true);
                body["top_logprobs"] = Value::Number(n.clone());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "openai_chat_language_model.test.rs"]
mod tests;
