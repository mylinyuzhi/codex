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

use super::convert_responses_usage::convert_openai_responses_usage;
use super::convert_to_responses_input::convert_to_openai_responses_input;
use super::map_finish_reason::map_openai_responses_finish_reason;
use super::openai_responses_api::OpenAIResponsesResponse;
use super::openai_responses_api::ResponseAnnotation;
use super::openai_responses_api::ResponseMessageContent;
use super::openai_responses_api::ResponseOutputItem;
use super::openai_responses_api::ResponsesStreamEvent;
use super::openai_responses_options::extract_responses_options;
use super::prepare_tools::prepare_responses_tools;
use super::provider_metadata::build_responses_provider_metadata;

/// OpenAI Responses API language model.
pub struct OpenAIResponsesLanguageModel {
    model_id: String,
    config: Arc<OpenAIConfig>,
}

impl OpenAIResponsesLanguageModel {
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAIConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    fn get_args(
        &self,
        options: &LanguageModelV4CallOptions,
    ) -> Result<(Value, Vec<Warning>), AISdkError> {
        let mut warnings = Vec::new();
        let openai_options = extract_responses_options(&options.provider_options);
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

        // Convert prompt to input items
        let (input, input_warnings) =
            convert_to_openai_responses_input(&options.prompt, system_message_mode);
        warnings.extend(input_warnings);

        // Prepare tools
        let prepared = prepare_responses_tools(&options.tools, &options.tool_choice);
        warnings.extend(prepared.warnings);

        let mut body = json!({
            "model": self.model_id,
            "input": input,
        });

        // Tools
        if let Some(tools) = prepared.tools {
            body["tools"] = Value::Array(tools);
        }
        if let Some(tc) = prepared.tool_choice {
            body["tool_choice"] = tc;
        }

        // Reasoning model handling
        let reasoning_effort = openai_options.reasoning_effort;
        let is_no_effort =
            reasoning_effort == Some(crate::chat::openai_chat_options::ReasoningEffort::None);
        let can_use_non_reasoning_params =
            is_no_effort && caps.supports_non_reasoning_params_with_no_effort;

        if is_reasoning_model {
            let mut reasoning = serde_json::Map::new();
            if let Some(effort) = reasoning_effort {
                reasoning.insert("effort".into(), Value::String(effort.as_str().into()));
            }
            if let Some(ref summary) = openai_options.reasoning_summary {
                reasoning.insert("summary".into(), Value::String(summary.clone()));
            }
            if !reasoning.is_empty() {
                body["reasoning"] = Value::Object(reasoning);
            }

            if let Some(max) = options.max_output_tokens {
                body["max_output_tokens"] = json!(max);
            }

            if can_use_non_reasoning_params {
                set_optional_f32(&mut body, "temperature", options.temperature);
                set_optional_f32(&mut body, "top_p", options.top_p);
            }
        } else {
            set_optional_f32(&mut body, "temperature", options.temperature);
            set_optional_f32(&mut body, "top_p", options.top_p);
            if let Some(max) = options.max_output_tokens {
                body["max_output_tokens"] = json!(max);
            }
        }

        // Response format: handled via `text` field
        if let Some(ref format) = options.response_format {
            match format {
                ResponseFormat::Text => {}
                ResponseFormat::Json {
                    schema,
                    name,
                    description,
                } => {
                    let strict = openai_options.strict_json_schema.unwrap_or(true);
                    if let Some(schema) = schema {
                        let schema_name = name.as_deref().unwrap_or("response");
                        let mut json_schema = json!({
                            "schema": schema,
                            "strict": strict,
                            "name": schema_name,
                        });
                        if let Some(desc) = description {
                            json_schema["description"] = Value::String(desc.clone());
                        }
                        body["text"] = json!({ "format": { "type": "json_schema", "json_schema": json_schema } });
                    } else {
                        body["text"] = json!({ "format": { "type": "json_object" } });
                    }
                }
            }
        }

        // Text verbosity
        if let Some(ref verbosity) = openai_options.text_verbosity {
            if body.get("text").is_none() {
                body["text"] = json!({});
            }
            body["text"]["verbosity"] = Value::String(verbosity.as_str().into());
        }

        // Provider options
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
            body["metadata"] = metadata.clone();
        }
        if let Some(ref instructions) = openai_options.instructions {
            body["instructions"] = Value::String(instructions.clone());
        }
        if let Some(ref conversation) = openai_options.conversation {
            body["conversation"] = Value::String(conversation.clone());
        }
        if let Some(ref prev_id) = openai_options.previous_response_id {
            body["previous_response_id"] = Value::String(prev_id.clone());
        }
        if let Some(max_tc) = openai_options.max_tool_calls {
            body["max_tool_calls"] = json!(max_tc);
        }
        if let Some(ref include) = openai_options.include {
            body["include"] = json!(include);
        }
        if let Some(ref truncation) = openai_options.truncation {
            body["truncation"] = Value::String(truncation.clone());
        }
        if let Some(ref tier) = openai_options.service_tier {
            body["service_tier"] = Value::String(tier.as_str().into());
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

        // Logprobs
        if let Some(ref logprobs) = openai_options.logprobs {
            match logprobs {
                Value::Bool(true) => {
                    // Include logprobs in text output
                    if body.get("include").is_none() {
                        body["include"] = json!([]);
                    }
                    if let Some(arr) = body["include"].as_array_mut() {
                        arr.push(Value::String("message.output_text.logprobs".into()));
                    }
                }
                Value::Number(n) => {
                    body["top_logprobs"] = Value::Number(n.clone());
                    if body.get("include").is_none() {
                        body["include"] = json!([]);
                    }
                    if let Some(arr) = body["include"].as_array_mut() {
                        arr.push(Value::String("message.output_text.logprobs".into()));
                    }
                }
                _ => {}
            }
        }

        Ok((body, warnings))
    }
}

#[async_trait]
impl LanguageModelV4 for OpenAIResponsesLanguageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        let mut map = HashMap::new();
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
        let url = self.config.url("/responses");
        let headers = self.config.get_headers();

        let response: OpenAIResponsesResponse = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            OpenAIFailedResponseHandler,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let mut content: Vec<AssistantContentPart> = Vec::new();
        let mut has_function_call = false;

        for item in &response.output {
            match item {
                ResponseOutputItem::Message {
                    content: msg_content,
                    ..
                } => {
                    for part in msg_content {
                        if let ResponseMessageContent::OutputText {
                            text, annotations, ..
                        } = part
                        {
                            if let Some(text) = text {
                                content.push(AssistantContentPart::Text(TextPart {
                                    text: text.clone(),
                                    provider_metadata: None,
                                }));
                            }
                            if let Some(anns) = annotations {
                                for ann in anns {
                                    if let ResponseAnnotation::UrlCitation { url, title, .. } = ann
                                    {
                                        content.push(AssistantContentPart::Source(
                                            vercel_ai_provider::content::SourcePart {
                                                source_type: SourceType::Url,
                                                id: vercel_ai_provider_utils::generate_id("src"),
                                                url: url.clone(),
                                                title: title.clone(),
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
                ResponseOutputItem::FunctionCall {
                    call_id,
                    name,
                    arguments,
                    ..
                } => {
                    has_function_call = true;
                    let input: Value = arguments
                        .as_deref()
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or(Value::Null);
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone().unwrap_or_default(),
                        tool_name: name.clone().unwrap_or_default(),
                        input,
                        provider_executed: None,
                        provider_metadata: None,
                    }));
                }
                ResponseOutputItem::Reasoning {
                    summary: Some(summaries),
                    ..
                } => {
                    for s in summaries {
                        if let Some(text) = &s.text {
                            content.push(AssistantContentPart::Reasoning(
                                vercel_ai_provider::ReasoningPart {
                                    text: text.clone(),
                                    provider_metadata: None,
                                },
                            ));
                        }
                    }
                }
                // Provider-executed tools — emit as ToolCall with provider_executed flag
                ResponseOutputItem::WebSearchCall { id, .. }
                | ResponseOutputItem::FileSearchCall { id, .. }
                | ResponseOutputItem::CodeInterpreterCall { id, .. } => {
                    let (tool_name, tool_type) = match item {
                        ResponseOutputItem::WebSearchCall { .. } => ("web_search", "web_search"),
                        ResponseOutputItem::FileSearchCall { .. } => ("file_search", "file_search"),
                        ResponseOutputItem::CodeInterpreterCall { .. } => {
                            ("code_interpreter", "code_interpreter")
                        }
                        _ => continue,
                    };
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone().unwrap_or_default(),
                        tool_name: tool_name.into(),
                        input: json!({ "type": tool_type }),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                }
                _ => {}
            }
        }

        let finish_reason =
            map_openai_responses_finish_reason(response.status.as_deref(), has_function_call);
        let usage = convert_openai_responses_usage(response.usage.as_ref());
        let provider_metadata = build_responses_provider_metadata(
            response.id.as_deref(),
            response.service_tier.as_deref(),
        );

        let timestamp = response
            .created_at
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
        body["stream"] = Value::Bool(true);

        let include_raw = options.include_raw_chunks.unwrap_or(false);
        let url = self.config.url("/responses");
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
        let stream = create_responses_stream(byte_stream, warnings, include_raw);

        Ok(LanguageModelV4StreamResult {
            stream,
            request: Some(LanguageModelV4Request {
                body: Some(request_body),
            }),
            response: Some(LanguageModelV4StreamResponse::new()),
        })
    }
}

// --- Streaming state machine ---

struct ActiveTextItem {
    started: bool,
}

struct ActiveFnCall {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

fn create_responses_stream(
    byte_stream: vercel_ai_provider_utils::ByteStream,
    warnings: Vec<Warning>,
    include_raw: bool,
) -> Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>> {
    let stream = futures::stream::unfold(
        ResponsesStreamState::new(byte_stream, warnings, include_raw),
        |mut state| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), state));
                }
                if state.done {
                    return None;
                }
                match state.next_events().await {
                    Ok(true) => {}
                    Ok(false) => {
                        state.done = true;
                        if !state.finish_emitted {
                            state.finish_emitted = true;
                            let finish = LanguageModelV4StreamPart::Finish {
                                usage: convert_openai_responses_usage(state.usage.as_ref()),
                                finish_reason: map_openai_responses_finish_reason(
                                    state.status.as_deref(),
                                    state.has_function_call,
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

struct ResponsesStreamState {
    byte_stream: vercel_ai_provider_utils::ByteStream,
    buffer: String,
    pending: std::collections::VecDeque<LanguageModelV4StreamPart>,
    active_texts: HashMap<String, ActiveTextItem>,
    active_fn_calls: HashMap<String, ActiveFnCall>,
    usage: Option<super::convert_responses_usage::OpenAIResponsesUsage>,
    status: Option<String>,
    has_function_call: bool,
    finish_emitted: bool,
    done: bool,
    metadata_emitted: bool,
    include_raw: bool,
}

impl ResponsesStreamState {
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
            active_texts: HashMap::new(),
            active_fn_calls: HashMap::new(),
            usage: None,
            status: None,
            has_function_call: false,
            finish_emitted: false,
            done: false,
            metadata_emitted: false,
            include_raw,
        }
    }

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

    fn process_buffer(&mut self) {
        while let Some(line_end) = self.buffer.find('\n') {
            let line = self.buffer[..line_end].trim_end_matches('\r').to_string();
            self.buffer = self.buffer[line_end + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                self.process_event(data);
            }
        }
    }

    fn process_event(&mut self, data: &str) {
        // Emit raw
        if self.include_raw
            && let Ok(raw) = serde_json::from_str::<Value>(data)
        {
            self.pending
                .push_back(LanguageModelV4StreamPart::Raw { raw_value: raw });
        }

        let event: ResponsesStreamEvent = match serde_json::from_str(data) {
            Ok(e) => e,
            Err(_) => return,
        };

        match event {
            ResponsesStreamEvent::ResponseCreated {
                response: Some(resp),
            } => {
                if !self.metadata_emitted {
                    self.metadata_emitted = true;
                    let mut meta = ResponseMetadata::new();
                    if let Some(ref id) = resp.id {
                        meta = meta.with_id(id.clone());
                    }
                    if let Some(ref model) = resp.model {
                        meta = meta.with_model(model.clone());
                    }
                    if let Some(ref tier) = resp.service_tier {
                        let pm = ProviderMetadata(HashMap::from([(
                            "serviceTier".to_string(),
                            Value::String(tier.clone()),
                        )]));
                        meta = meta.with_provider_metadata(pm);
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ResponseMetadata(meta));
                }
            }

            ResponsesStreamEvent::ResponseCompleted { response }
            | ResponsesStreamEvent::ResponseIncomplete { response } => {
                if let Some(resp) = response {
                    self.usage = resp.usage;
                    self.status = resp.status;
                }
            }

            ResponsesStreamEvent::OutputItemAdded { item: Some(item) } => match &item {
                ResponseOutputItem::FunctionCall { id, name, .. } => {
                    self.has_function_call = true;
                    let item_id = id.clone().unwrap_or_default();
                    self.active_fn_calls.insert(
                        item_id.clone(),
                        ActiveFnCall {
                            id: item_id,
                            name: name.clone().unwrap_or_default(),
                            arguments: String::new(),
                            started: false,
                        },
                    );
                }
                ResponseOutputItem::Message { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.active_texts
                        .insert(item_id, ActiveTextItem { started: false });
                }
                _ => {}
            },

            ResponsesStreamEvent::OutputTextDelta { item_id, delta, .. } => {
                if let (Some(item_id), Some(delta)) = (item_id, delta) {
                    // Emit TextStart if not yet
                    if let Some(text_item) = self.active_texts.get_mut(&item_id) {
                        if !text_item.started {
                            text_item.started = true;
                            self.pending
                                .push_back(LanguageModelV4StreamPart::TextStart {
                                    id: item_id.clone(),
                                    provider_metadata: None,
                                });
                        }
                    } else {
                        // Auto-create if not tracked
                        self.active_texts
                            .insert(item_id.clone(), ActiveTextItem { started: true });
                        self.pending
                            .push_back(LanguageModelV4StreamPart::TextStart {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::TextDelta {
                            id: item_id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::OutputTextDone {
                item_id: Some(item_id),
                ..
            } => {
                self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                    id: item_id.clone(),
                    provider_metadata: None,
                });
                self.active_texts.remove(&item_id);
            }

            ResponsesStreamEvent::FnCallArgsDelta { item_id, delta } => {
                if let (Some(item_id), Some(delta)) = (item_id, delta)
                    && let Some(fc) = self.active_fn_calls.get_mut(&item_id)
                {
                    if !fc.started {
                        fc.started = true;
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputStart {
                                id: fc.id.clone(),
                                tool_name: fc.name.clone(),
                                provider_executed: None,
                                dynamic: None,
                                title: None,
                                provider_metadata: None,
                            });
                    }
                    fc.arguments.push_str(&delta);
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                            id: item_id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::FnCallArgsDone {
                item_id: Some(item_id),
                ..
            } => {
                if let Some(fc) = self.active_fn_calls.remove(&item_id) {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                            id: fc.id.clone(),
                            provider_metadata: None,
                        });
                    let input: Value = serde_json::from_str(&fc.arguments).unwrap_or(Value::Null);
                    self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                        vercel_ai_provider::tool::ToolCall::new(fc.id, fc.name, input),
                    ));
                }
            }

            ResponsesStreamEvent::OutputTextAnnotationAdded {
                annotation: Some(ResponseAnnotation::UrlCitation { url, title, .. }),
                ..
            } => {
                self.pending.push_back(LanguageModelV4StreamPart::Source(
                    vercel_ai_provider::content::SourcePart {
                        source_type: SourceType::Url,
                        id: vercel_ai_provider_utils::generate_id("src"),
                        url,
                        title,
                        media_type: None,
                        filename: None,
                        provider_metadata: None,
                    },
                ));
            }

            ResponsesStreamEvent::ReasoningSummaryDelta { item_id, delta } => {
                if let (Some(id), Some(delta)) = (item_id, delta) {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ReasoningDelta {
                            id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::Error { message, code } => {
                self.pending.push_back(LanguageModelV4StreamPart::Error {
                    error: vercel_ai_provider::StreamError {
                        message: message.unwrap_or_else(|| "Unknown error".into()),
                        code,
                        is_retryable: false,
                    },
                });
            }

            _ => {
                // Custom tool calls, code interpreter, image generation, etc.
                // These would need more specific handling for each type.
                // For now, unknown events are ignored.
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
#[path = "openai_responses_language_model.test.rs"]
mod tests;
