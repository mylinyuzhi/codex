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
use vercel_ai_provider::content::ToolApprovalRequestPart;
use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::post_stream_to_api_with_client;

use crate::openai_capabilities::SystemMessageMode;
use crate::openai_capabilities::get_capabilities;
use crate::openai_config::OpenAIConfig;
use crate::openai_error::OpenAIFailedResponseHandler;

use super::convert_responses_usage::convert_openai_responses_usage;
use super::convert_to_responses_input::ProviderToolFlags;
use super::convert_to_responses_input::convert_to_openai_responses_input_with_flags;
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
        let tool_flags = ProviderToolFlags::from_tools(&options.tools);
        let (input, input_warnings) = convert_to_openai_responses_input_with_flags(
            &options.prompt,
            system_message_mode,
            &tool_flags,
        );
        warnings.extend(input_warnings);

        // Prepare tools
        let prepared = prepare_responses_tools(&options.tools, &options.tool_choice);
        warnings.extend(prepared.warnings);

        // Unsupported parameter warnings for Responses API
        if options.top_k.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "topK".into(),
                details: Some("topK is not supported by the OpenAI Responses API".into()),
            });
        }
        if options.seed.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "seed".into(),
                details: Some("seed is not supported by the OpenAI Responses API".into()),
            });
        }
        if options.presence_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "presencePenalty".into(),
                details: Some(
                    "presencePenalty is not supported by the OpenAI Responses API".into(),
                ),
            });
        }
        if options.frequency_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "frequencyPenalty".into(),
                details: Some(
                    "frequencyPenalty is not supported by the OpenAI Responses API".into(),
                ),
            });
        }
        if options
            .stop_sequences
            .as_ref()
            .is_some_and(|s| !s.is_empty())
        {
            warnings.push(Warning::Unsupported {
                feature: "stopSequences".into(),
                details: Some("stopSequences is not supported by the OpenAI Responses API".into()),
            });
        }

        // Warn about conversation + previousResponseId conflict
        if openai_options.conversation.is_some() && openai_options.previous_response_id.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "conversation + previousResponseId".into(),
                details: Some(
                    "conversation and previousResponseId should not be used together".into(),
                ),
            });
        }

        // Warn about reasoning model param conflicts
        if is_reasoning_model && (options.temperature.is_some() || options.top_p.is_some()) {
            let is_no_effort = openai_options.reasoning_effort
                == Some(crate::chat::openai_chat_options::ReasoningEffort::None);
            if !(is_no_effort && caps.supports_non_reasoning_params_with_no_effort) {
                warnings.push(Warning::Unsupported {
                    feature: "temperature/topP with reasoning model".into(),
                    details: Some(
                        "temperature and topP are not supported with reasoning models unless \
                         reasoning_effort is 'none' and the model supports it"
                            .into(),
                    ),
                });
            }
        }

        // Warn about reasoning effort on non-reasoning models
        if !is_reasoning_model && openai_options.reasoning_effort.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "reasoningEffort on non-reasoning model".into(),
                details: Some(
                    "reasoningEffort is only supported on reasoning models (o1, o3, o4-mini, gpt-5)"
                        .into(),
                ),
            });
        }

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
        const TOP_LOGPROBS_MAX: u64 = 20;
        if let Some(ref logprobs) = openai_options.logprobs {
            match logprobs {
                Value::Bool(true) => {
                    body["top_logprobs"] = json!(TOP_LOGPROBS_MAX);
                    ensure_include_entry(&mut body, "message.output_text.logprobs");
                }
                Value::Number(n) => {
                    body["top_logprobs"] = Value::Number(n.clone());
                    ensure_include_entry(&mut body, "message.output_text.logprobs");
                }
                _ => {}
            }
        }

        // Auto-include: add sources and outputs for provider tools present in tools
        if let Some(ref tools) = options.tools {
            let has_web_search = tools.iter().any(|t| match t {
                vercel_ai_provider::LanguageModelV4Tool::Provider(pt) => {
                    pt.name == "web_search" || pt.name == "web_search_preview"
                }
                _ => false,
            });
            let has_code_interpreter = tools.iter().any(|t| match t {
                vercel_ai_provider::LanguageModelV4Tool::Provider(pt) => {
                    pt.name == "code_interpreter"
                }
                _ => false,
            });

            if has_web_search {
                ensure_include_entry(&mut body, "web_search_call.action.sources");
            }
            if has_code_interpreter {
                ensure_include_entry(&mut body, "code_interpreter_call.outputs");
            }
        }

        // Auto-include reasoning encrypted_content when store=false and reasoning model
        if is_reasoning_model && openai_options.store == Some(false) {
            ensure_include_entry(&mut body, "reasoning.encrypted_content");
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
            map.insert("image/*".into(), vec![re.clone()]);
            map.insert("application/pdf".into(), vec![re]);
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
                        match part {
                            ResponseMessageContent::OutputText {
                                text,
                                annotations,
                                logprobs,
                            } => {
                                let text_meta = logprobs
                                    .as_ref()
                                    .filter(|lp| !lp.is_empty())
                                    .and_then(|lp| serde_json::to_value(lp).ok())
                                    .map(|v| {
                                        ProviderMetadata(HashMap::from([("logprobs".into(), v)]))
                                    });
                                if let Some(text) = text {
                                    content.push(AssistantContentPart::Text(TextPart {
                                        text: text.clone(),
                                        provider_metadata: text_meta,
                                    }));
                                }
                                if let Some(anns) = annotations {
                                    emit_annotations(anns, &mut content);
                                }
                            }
                            ResponseMessageContent::Refusal {
                                refusal: Some(text),
                            } => {
                                content.push(AssistantContentPart::Text(TextPart {
                                    text: text.clone(),
                                    provider_metadata: None,
                                }));
                            }
                            _ => {}
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
                ResponseOutputItem::CustomToolCall {
                    id, name, input, ..
                } => {
                    has_function_call = true;
                    let parsed_input: Value = input
                        .as_deref()
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or(Value::Null);
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone().unwrap_or_default(),
                        tool_name: name.clone().unwrap_or_default(),
                        input: parsed_input,
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                }
                ResponseOutputItem::Reasoning {
                    summary,
                    encrypted_content,
                    ..
                } => {
                    if let Some(summaries) = summary {
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
                    // Include encrypted_content in provider metadata if present
                    if let Some(ec) = encrypted_content {
                        let meta = ProviderMetadata(HashMap::from([(
                            "encrypted_content".into(),
                            ec.clone(),
                        )]));
                        content.push(AssistantContentPart::Reasoning(
                            vercel_ai_provider::ReasoningPart {
                                text: String::new(),
                                provider_metadata: Some(meta),
                            },
                        ));
                    }
                }
                // Provider-executed tools — emit as ToolCall with provider_executed flag
                ResponseOutputItem::WebSearchCall { id, .. } => {
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone().unwrap_or_default(),
                        tool_name: "web_search".into(),
                        input: json!({ "type": "web_search" }),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                }
                ResponseOutputItem::FileSearchCall { id, results, .. } => {
                    let mut meta = None;
                    if let Some(r) = results
                        && let Ok(v) = serde_json::to_value(r)
                    {
                        meta = Some(ProviderMetadata(HashMap::from([("results".into(), v)])));
                    }
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone().unwrap_or_default(),
                        tool_name: "file_search".into(),
                        input: json!({ "type": "file_search" }),
                        provider_executed: Some(true),
                        provider_metadata: meta,
                    }));
                }
                ResponseOutputItem::CodeInterpreterCall {
                    id, code, outputs, ..
                } => {
                    let call_id = id.clone().unwrap_or_default();
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone(),
                        tool_name: "code_interpreter".into(),
                        input: json!({ "type": "code_interpreter", "code": code }),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                    // Emit tool result if outputs are present
                    if let Some(outs) = outputs {
                        content.push(AssistantContentPart::ToolResult(
                            vercel_ai_provider::ToolResultPart::new(
                                call_id,
                                "code_interpreter",
                                vercel_ai_provider::ToolResultContent::json(json!(outs)),
                            ),
                        ));
                    }
                }
                ResponseOutputItem::ImageGenerationCall { id, result, .. } => {
                    let call_id = id.clone().unwrap_or_default();
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone(),
                        tool_name: "image_generation".into(),
                        input: json!({ "type": "image_generation" }),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                    if let Some(res) = result {
                        content.push(AssistantContentPart::ToolResult(
                            vercel_ai_provider::ToolResultPart::new(
                                call_id,
                                "image_generation",
                                vercel_ai_provider::ToolResultContent::json(res.clone()),
                            ),
                        ));
                    }
                }
                ResponseOutputItem::McpCall {
                    id,
                    name,
                    arguments,
                    server_label,
                    output,
                    error,
                } => {
                    let call_id = id.clone().unwrap_or_default();
                    let parsed_args: Value = arguments
                        .as_deref()
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or(Value::Null);
                    let mut meta_map = HashMap::new();
                    if let Some(label) = server_label {
                        meta_map.insert("serverLabel".into(), Value::String(label.clone()));
                    }
                    let meta = if meta_map.is_empty() {
                        None
                    } else {
                        Some(ProviderMetadata(meta_map))
                    };
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone(),
                        tool_name: name.clone().unwrap_or_default(),
                        input: parsed_args,
                        provider_executed: Some(true),
                        provider_metadata: meta,
                    }));
                    // Emit result or error
                    let tool_name_str = name.clone().unwrap_or_default();
                    if let Some(err) = error {
                        content.push(AssistantContentPart::ToolResult(
                            vercel_ai_provider::ToolResultPart::new(
                                call_id,
                                &tool_name_str,
                                vercel_ai_provider::ToolResultContent::json(err.clone()),
                            )
                            .with_error(),
                        ));
                    } else if let Some(out) = output {
                        content.push(AssistantContentPart::ToolResult(
                            vercel_ai_provider::ToolResultPart::new(
                                call_id,
                                &tool_name_str,
                                vercel_ai_provider::ToolResultContent::json(out.clone()),
                            ),
                        ));
                    }
                }
                ResponseOutputItem::McpApprovalRequest { id, rest } => {
                    let approval_id = id.clone().unwrap_or_default();
                    let mut part = ToolApprovalRequestPart::new(approval_id.clone(), approval_id);
                    if let Some(name) = rest.get("name").and_then(|v| v.as_str()) {
                        part = part.with_tool_name(name);
                    }
                    if let Some(label) = rest.get("server_label").and_then(|v| v.as_str()) {
                        part = part.with_context(label);
                    }
                    content.push(AssistantContentPart::ToolApprovalRequest(part));
                }
                ResponseOutputItem::LocalShellCall {
                    id,
                    call_id,
                    action,
                    ..
                } => {
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone().or_else(|| id.clone()).unwrap_or_default(),
                        tool_name: "local_shell".into(),
                        input: action.clone().unwrap_or(Value::Null),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                }
                ResponseOutputItem::ShellCall {
                    id,
                    call_id,
                    action,
                    output,
                    ..
                } => {
                    let tc_id = call_id.clone().or_else(|| id.clone()).unwrap_or_default();
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: tc_id.clone(),
                        tool_name: "shell".into(),
                        input: action.clone().unwrap_or(Value::Null),
                        provider_executed: Some(true),
                        provider_metadata: None,
                    }));
                    if let Some(outs) = output {
                        content.push(AssistantContentPart::ToolResult(
                            vercel_ai_provider::ToolResultPart::new(
                                tc_id,
                                "shell",
                                vercel_ai_provider::ToolResultContent::json(json!(outs)),
                            ),
                        ));
                    }
                }
                ResponseOutputItem::ApplyPatchCall {
                    id,
                    call_id,
                    operation,
                    ..
                } => {
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: call_id.clone().or_else(|| id.clone()).unwrap_or_default(),
                        tool_name: "apply_patch".into(),
                        input: operation.clone().unwrap_or(Value::Null),
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

        Ok(LanguageModelV4GenerateResult {
            content,
            usage,
            finish_reason,
            warnings,
            provider_metadata,
            request: Some(LanguageModelV4Request { body: Some(body) }),
            response: Some(LanguageModelV4Response {
                timestamp: response.created_at,
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

struct ActiveCustomToolCall {
    id: String,
    name: String,
    input: String,
    started: bool,
}

struct ActiveReasoning {
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
                if state.done && state.pending.is_empty() {
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
                                provider_metadata: build_responses_provider_metadata(
                                    state.response_id.as_deref(),
                                    state.service_tier.as_deref(),
                                ),
                            };
                            state.pending.push_back(finish);
                        }
                        // Fall through to loop — drain pending
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
    active_custom_calls: HashMap<String, ActiveCustomToolCall>,
    active_reasoning: HashMap<String, ActiveReasoning>,
    usage: Option<super::convert_responses_usage::OpenAIResponsesUsage>,
    status: Option<String>,
    has_function_call: bool,
    finish_emitted: bool,
    done: bool,
    metadata_emitted: bool,
    include_raw: bool,
    response_id: Option<String>,
    service_tier: Option<String>,
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
            active_custom_calls: HashMap::new(),
            active_reasoning: HashMap::new(),
            usage: None,
            status: None,
            has_function_call: false,
            finish_emitted: false,
            done: false,
            metadata_emitted: false,
            include_raw,
            response_id: None,
            service_tier: None,
        }
    }

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

    fn process_buffer(&mut self) {
        while let Some(line_end) = self.buffer.find('\n') {
            let line_len = line_end + 1;
            let line = self.buffer[..line_end].trim_end_matches('\r');
            if let Some(data) = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
                && !data.is_empty()
                && data != "[DONE]"
            {
                let data = data.to_string();
                self.buffer.drain(..line_len);
                self.process_event(&data);
                continue;
            }
            self.buffer.drain(..line_len);
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
                        let mut openai_obj = serde_json::Map::new();
                        openai_obj.insert("serviceTier".into(), Value::String(tier.clone()));
                        let mut pm = ProviderMetadata::default();
                        pm.0.insert("openai".into(), Value::Object(openai_obj));
                        meta = meta.with_provider_metadata(pm);
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ResponseMetadata(meta));
                }
                // Track response_id and service_tier for Finish metadata
                self.response_id = resp.id;
                self.service_tier = resp.service_tier;
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
                ResponseOutputItem::CustomToolCall { id, name, .. } => {
                    self.has_function_call = true;
                    let item_id = id.clone().unwrap_or_default();
                    self.active_custom_calls.insert(
                        item_id.clone(),
                        ActiveCustomToolCall {
                            id: item_id,
                            name: name.clone().unwrap_or_default(),
                            input: String::new(),
                            started: false,
                        },
                    );
                }
                ResponseOutputItem::Message { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.active_texts
                        .insert(item_id, ActiveTextItem { started: false });
                }
                ResponseOutputItem::Reasoning { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.active_reasoning
                        .insert(item_id, ActiveReasoning { started: false });
                }
                // Provider-executed tool starts — emit ToolInputStart
                ResponseOutputItem::WebSearchCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "web_search".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::FileSearchCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "file_search".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::CodeInterpreterCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "code_interpreter".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::ImageGenerationCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "image_generation".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::ShellCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "shell".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::LocalShellCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "local_shell".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::ApplyPatchCall { id, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: "apply_patch".into(),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
                }
                ResponseOutputItem::McpCall { id, name, .. } => {
                    let item_id = id.clone().unwrap_or_default();
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputStart {
                            id: item_id,
                            tool_name: name.clone().unwrap_or_else(|| "mcp".into()),
                            provider_executed: Some(true),
                            dynamic: None,
                            title: None,
                            provider_metadata: None,
                        });
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

            // Custom tool call streaming
            ResponsesStreamEvent::CustomToolCallInputDelta { item_id, delta } => {
                if let (Some(item_id), Some(delta)) = (item_id, delta)
                    && let Some(ct) = self.active_custom_calls.get_mut(&item_id)
                {
                    if !ct.started {
                        ct.started = true;
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputStart {
                                id: ct.id.clone(),
                                tool_name: ct.name.clone(),
                                provider_executed: Some(true),
                                dynamic: None,
                                title: None,
                                provider_metadata: None,
                            });
                    }
                    ct.input.push_str(&delta);
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                            id: item_id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::CustomToolCallInputDone {
                item_id: Some(item_id),
                ..
            } => {
                if let Some(ct) = self.active_custom_calls.remove(&item_id) {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                            id: ct.id.clone(),
                            provider_metadata: None,
                        });
                    let input: Value = serde_json::from_str(&ct.input).unwrap_or(Value::Null);
                    self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                        vercel_ai_provider::tool::ToolCall::new(ct.id, ct.name, input),
                    ));
                }
            }

            // Reasoning lifecycle
            ResponsesStreamEvent::ReasoningSummaryDelta { item_id, delta } => {
                if let (Some(id), Some(delta)) = (item_id, delta) {
                    // Emit ReasoningStart if this is the first delta for this item
                    if let Some(r) = self.active_reasoning.get_mut(&id) {
                        if !r.started {
                            r.started = true;
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ReasoningStart {
                                    id: id.clone(),
                                    provider_metadata: None,
                                });
                        }
                    } else {
                        self.active_reasoning
                            .insert(id.clone(), ActiveReasoning { started: true });
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ReasoningStart {
                                id: id.clone(),
                                provider_metadata: None,
                            });
                    }
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ReasoningDelta {
                            id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::ReasoningSummaryDone {
                item_id: Some(id), ..
            } => {
                self.active_reasoning.remove(&id);
                self.pending
                    .push_back(LanguageModelV4StreamPart::ReasoningEnd {
                        id,
                        provider_metadata: None,
                    });
            }

            // Code interpreter streaming
            ResponsesStreamEvent::CodeInterpreterCodeDelta { item_id, delta } => {
                if let (Some(id), Some(delta)) = (item_id, delta) {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                            id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::CodeInterpreterCodeDone { .. } => {
                // Code completion handled by OutputItemDone
            }

            // Apply patch streaming
            ResponsesStreamEvent::ApplyPatchDiffDelta { item_id, delta } => {
                if let (Some(id), Some(delta)) = (item_id, delta) {
                    self.pending
                        .push_back(LanguageModelV4StreamPart::ToolInputDelta {
                            id,
                            delta,
                            provider_metadata: None,
                        });
                }
            }

            ResponsesStreamEvent::ApplyPatchDiffDone { .. } => {
                // Completion handled by OutputItemDone
            }

            // OutputItemDone — emit ToolCall + ToolResult for provider-executed tools
            ResponsesStreamEvent::OutputItemDone { item: Some(item) } => {
                match &item {
                    ResponseOutputItem::WebSearchCall { id, .. } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                            vercel_ai_provider::tool::ToolCall::new(
                                &item_id,
                                "web_search",
                                json!({ "type": "web_search" }),
                            )
                            .with_provider_executed(true),
                        ));
                    }
                    ResponseOutputItem::FileSearchCall { id, results, .. } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "file_search",
                            json!({ "type": "file_search" }),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                        // Emit results as ToolResult
                        if let Some(r) = results {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::new(
                                        &item_id,
                                        "file_search",
                                        json!(r),
                                    ),
                                ));
                        }
                    }
                    ResponseOutputItem::CodeInterpreterCall {
                        id, code, outputs, ..
                    } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "code_interpreter",
                            json!({ "type": "code_interpreter", "code": code }),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                        if let Some(outs) = outputs {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::new(
                                        &item_id,
                                        "code_interpreter",
                                        json!(outs),
                                    ),
                                ));
                        }
                    }
                    ResponseOutputItem::ImageGenerationCall { id, result, .. } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "image_generation",
                            json!({ "type": "image_generation" }),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                        if let Some(res) = result {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::new(
                                        &item_id,
                                        "image_generation",
                                        res.clone(),
                                    ),
                                ));
                        }
                    }
                    ResponseOutputItem::ShellCall {
                        id, action, output, ..
                    } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "shell",
                            action.clone().unwrap_or(Value::Null),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                        if let Some(outs) = output {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::new(
                                        &item_id,
                                        "shell",
                                        json!(outs),
                                    ),
                                ));
                        }
                    }
                    ResponseOutputItem::LocalShellCall { id, action, .. } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "local_shell",
                            action.clone().unwrap_or(Value::Null),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                    }
                    ResponseOutputItem::ApplyPatchCall { id, operation, .. } => {
                        let item_id = id.clone().unwrap_or_default();
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            "apply_patch",
                            operation.clone().unwrap_or(Value::Null),
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                    }
                    ResponseOutputItem::McpCall {
                        id,
                        name,
                        arguments,
                        output,
                        error,
                        ..
                    } => {
                        let item_id = id.clone().unwrap_or_default();
                        let tool_name = name.clone().unwrap_or_else(|| "mcp".into());
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolInputEnd {
                                id: item_id.clone(),
                                provider_metadata: None,
                            });
                        let parsed_args: Value = arguments
                            .as_deref()
                            .and_then(|a| serde_json::from_str(a).ok())
                            .unwrap_or(Value::Null);
                        let tc = vercel_ai_provider::tool::ToolCall::new(
                            &item_id,
                            &tool_name,
                            parsed_args,
                        )
                        .with_provider_executed(true);
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolCall(tc));
                        if let Some(err) = error {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::error(
                                        &item_id,
                                        &tool_name,
                                        err.clone(),
                                    ),
                                ));
                        } else if let Some(out) = output {
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ToolResult(
                                    vercel_ai_provider::tool::ToolResult::new(
                                        &item_id,
                                        &tool_name,
                                        out.clone(),
                                    ),
                                ));
                        }
                    }
                    ResponseOutputItem::McpApprovalRequest { id, rest } => {
                        let approval_id = id.clone().unwrap_or_default();
                        let req = vercel_ai_provider::language_model::v4::LanguageModelV4ToolApprovalRequest::new(
                            approval_id.clone(),
                            approval_id,
                        );
                        let _ = rest;
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ToolApprovalRequest(req));
                    }
                    _ => {}
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

            _ => {}
        }
    }
}

/// Emit Source parts for annotations (url_citation, file_citation, file_path, container_file_citation).
fn emit_annotations(anns: &[ResponseAnnotation], content: &mut Vec<AssistantContentPart>) {
    for ann in anns {
        match ann {
            ResponseAnnotation::UrlCitation { url, title, .. } => {
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
            ResponseAnnotation::FileCitation { file_id, .. } => {
                content.push(AssistantContentPart::Source(
                    vercel_ai_provider::content::SourcePart {
                        source_type: SourceType::Document,
                        id: vercel_ai_provider_utils::generate_id("src"),
                        url: file_id.clone(),
                        title: None,
                        media_type: None,
                        filename: None,
                        provider_metadata: None,
                    },
                ));
            }
            ResponseAnnotation::FilePath { file_id, .. } => {
                content.push(AssistantContentPart::Source(
                    vercel_ai_provider::content::SourcePart {
                        source_type: SourceType::Document,
                        id: vercel_ai_provider_utils::generate_id("src"),
                        url: file_id.clone(),
                        title: None,
                        media_type: None,
                        filename: None,
                        provider_metadata: None,
                    },
                ));
            }
            ResponseAnnotation::ContainerFileCitation {
                file_id,
                container_id,
            } => {
                let mut meta_map = HashMap::new();
                if let Some(cid) = container_id {
                    meta_map.insert("containerId".into(), Value::String(cid.clone()));
                }
                let meta = if meta_map.is_empty() {
                    None
                } else {
                    Some(ProviderMetadata(meta_map))
                };
                content.push(AssistantContentPart::Source(
                    vercel_ai_provider::content::SourcePart {
                        source_type: SourceType::Document,
                        id: vercel_ai_provider_utils::generate_id("src"),
                        url: file_id.clone(),
                        title: None,
                        media_type: None,
                        filename: None,
                        provider_metadata: meta,
                    },
                ));
            }
            _ => {}
        }
    }
}

/// Ensure an entry exists in the `include` array, creating it if needed.
fn ensure_include_entry(body: &mut Value, entry: &str) {
    if body.get("include").is_none() {
        body["include"] = json!([]);
    }
    if let Some(arr) = body["include"].as_array_mut() {
        let val = Value::String(entry.into());
        if !arr.contains(&val) {
            arr.push(val);
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
