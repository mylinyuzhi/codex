//! Google Generative AI language model implementation.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FilePart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::ProviderMetadata;
use vercel_ai_provider::ResponseFormat;
use vercel_ai_provider::ToolResultContent;
use vercel_ai_provider::ToolResultPart;
use vercel_ai_provider::content::SourcePart;
use vercel_ai_provider::language_model::LanguageModelV4Request;
use vercel_ai_provider::language_model::LanguageModelV4Response;
use vercel_ai_provider::language_model::v4::stream::File as StreamFile;
use vercel_ai_provider::response_metadata::ResponseMetadata;
use vercel_ai_provider::tool::ToolCall;

use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::combine_headers;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::post_stream_to_api_with_client;
use vercel_ai_provider_utils::without_trailing_slash;

use crate::convert_google_generative_ai_usage::GoogleUsageMetadata;
use crate::convert_google_generative_ai_usage::convert_usage;
use crate::convert_to_google_generative_ai_messages::ConvertOptions;
use crate::convert_to_google_generative_ai_messages::convert_to_google_generative_ai_messages;
use crate::get_model_path::get_model_path;
use crate::google_error::GoogleFailedResponseHandler;
use crate::google_generative_ai_options::GoogleLanguageModelOptions;
use crate::google_prepare_tools::prepare_tools;

/// Type alias for supported URL pattern function.
type SupportedUrlsFn = Arc<dyn Fn() -> HashMap<String, Vec<Regex>> + Send + Sync>;

/// Configuration for the Google Generative AI language model.
pub struct GoogleGenerativeAILanguageModelConfig {
    /// Provider identifier string.
    pub provider: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Function to generate request headers.
    pub headers: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Function to generate unique IDs.
    pub generate_id: Arc<dyn Fn() -> String + Send + Sync>,
    /// Supported URL patterns for file references.
    pub supported_urls: Option<SupportedUrlsFn>,
    /// Optional HTTP client for connection pooling.
    pub client: Option<Arc<reqwest::Client>>,
}

/// Google Generative AI language model.
pub struct GoogleGenerativeAILanguageModel {
    model_id: String,
    config: GoogleGenerativeAILanguageModelConfig,
}

impl GoogleGenerativeAILanguageModel {
    /// Create a new Google Generative AI language model.
    pub fn new(model_id: impl Into<String>, config: GoogleGenerativeAILanguageModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Parse provider options from call options.
    fn parse_provider_options(
        &self,
        options: &LanguageModelV4CallOptions,
    ) -> GoogleLanguageModelOptions {
        let Some(ref provider_options) = options.provider_options else {
            return GoogleLanguageModelOptions::default();
        };

        // Try "google" namespace first, then "vertex"
        let opts_map = provider_options
            .get("google")
            .or_else(|| provider_options.get("vertex"));

        let Some(opts_map) = opts_map else {
            return GoogleLanguageModelOptions::default();
        };

        let opts_value = serde_json::to_value(opts_map).unwrap_or(Value::Null);
        serde_json::from_value(opts_value).unwrap_or_default()
    }

    /// Build the request arguments for the Google API.
    fn get_args(&self, options: &LanguageModelV4CallOptions) -> (Value, HashMap<String, String>) {
        let provider_opts = self.parse_provider_options(options);

        // Check if model supports system instructions (Gemma models don't)
        let supports_system_instruction = !self.model_id.to_lowercase().contains("gemma");

        let convert_opts = ConvertOptions {
            supports_system_instruction,
        };

        let prompt = convert_to_google_generative_ai_messages(&options.prompt, &convert_opts);

        // Prepare tools
        let prepared_tools = prepare_tools(&options.tools, &options.tool_choice, &self.model_id);

        // Build generation config
        let mut generation_config = json!({});

        if let Some(max_tokens) = options.max_output_tokens {
            generation_config["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(temp) = options.temperature {
            generation_config["temperature"] = json!(temp);
        }
        if let Some(top_p) = options.top_p {
            generation_config["topP"] = json!(top_p);
        }
        if let Some(top_k) = options.top_k {
            generation_config["topK"] = json!(top_k);
        }
        if let Some(ref stop) = options.stop_sequences {
            generation_config["stopSequences"] = json!(stop);
        }
        if let Some(freq) = options.frequency_penalty {
            generation_config["frequencyPenalty"] = json!(freq);
        }
        if let Some(pres) = options.presence_penalty {
            generation_config["presencePenalty"] = json!(pres);
        }
        if let Some(seed) = options.seed {
            generation_config["seed"] = json!(seed);
        }

        // Response format
        if let Some(ref format) = options.response_format {
            match format {
                ResponseFormat::Json {
                    schema,
                    name: _,
                    description: _,
                } => {
                    generation_config["responseMimeType"] = json!("application/json");
                    if let Some(schema_val) = schema {
                        let schema_json = serde_json::to_value(schema_val)
                            .unwrap_or(Value::Object(Default::default()));
                        if let Some(openapi) =
                            crate::convert_json_schema_to_openapi_schema::convert_json_schema_to_openapi_schema(&schema_json)
                        {
                            generation_config["responseSchema"] = openapi;
                        }
                    }
                }
                ResponseFormat::Text => {
                    generation_config["responseMimeType"] = json!("text/plain");
                }
            }
        }

        // Provider-specific options
        if let Some(ref modalities) = provider_opts.response_modalities {
            generation_config["responseModalities"] =
                serde_json::to_value(modalities).unwrap_or(Value::Null);
        }
        if let Some(ref media_res) = provider_opts.media_resolution {
            generation_config["mediaResolution"] =
                serde_json::to_value(media_res).unwrap_or(Value::Null);
        }
        if let Some(audio_ts) = provider_opts.audio_timestamp {
            generation_config["audioTimestamp"] = json!(audio_ts);
        }
        if let Some(ref image_config) = provider_opts.image_config
            && let Ok(val) = serde_json::to_value(image_config)
        {
            generation_config["imageGenerationConfig"] = val;
        }

        // Build request body
        let mut body = json!({
            "generationConfig": generation_config,
            "contents": prompt.contents,
        });

        if let Some(si) = prompt.system_instruction {
            body["systemInstruction"] = serde_json::to_value(si).unwrap_or(Value::Null);
        }

        // Tools
        let mut tools_array: Vec<Value> = Vec::new();
        if let Some(func_decls) = prepared_tools.function_declarations {
            tools_array.push(json!({ "functionDeclarations": func_decls }));
        }
        for entry in prepared_tools.tool_entries {
            tools_array.push(entry);
        }
        if !tools_array.is_empty() {
            body["tools"] = json!(tools_array);
        }
        if let Some(tool_config) = prepared_tools.tool_config {
            body["toolConfig"] = tool_config;
        }

        // Safety settings
        if let Some(ref safety) = provider_opts.safety_settings {
            if let Ok(val) = serde_json::to_value(safety) {
                body["safetySettings"] = val;
            }
        } else if let Some(ref threshold) = provider_opts.threshold {
            let categories = [
                "HARM_CATEGORY_HATE_SPEECH",
                "HARM_CATEGORY_DANGEROUS_CONTENT",
                "HARM_CATEGORY_SEXUALLY_EXPLICIT",
                "HARM_CATEGORY_HARASSMENT",
                "HARM_CATEGORY_CIVIC_INTEGRITY",
            ];
            let settings: Vec<Value> = categories
                .iter()
                .map(|cat| {
                    json!({
                        "category": cat,
                        "threshold": serde_json::to_value(threshold).unwrap_or(Value::Null),
                    })
                })
                .collect();
            body["safetySettings"] = json!(settings);
        }

        // Thinking config
        if let Some(ref thinking) = provider_opts.thinking_config
            && let Ok(val) = serde_json::to_value(thinking)
        {
            body["generationConfig"]["thinkingConfig"] = val;
        }

        // Cached content
        if let Some(ref cached) = provider_opts.cached_content {
            body["cachedContent"] = json!(cached);
        }

        // Labels
        if let Some(ref labels) = provider_opts.labels {
            body["labels"] = serde_json::to_value(labels).unwrap_or(Value::Null);
        }

        // Build headers
        let headers = combine_headers(vec![Some((self.config.headers)()), options.headers.clone()]);

        (body, headers)
    }
}

/// Google API response for generateContent.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<GoogleCandidate>,
    #[serde(default)]
    pub usage_metadata: Option<GoogleUsageMetadata>,
    #[serde(default)]
    pub model_version: Option<String>,
}

/// A candidate response from Google.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCandidate {
    #[serde(default)]
    pub content: Option<GoogleCandidateContent>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub safety_ratings: Option<Value>,
    #[serde(default)]
    pub grounding_metadata: Option<GroundingMetadata>,
    #[serde(default)]
    pub url_context_metadata: Option<UrlContextMetadata>,
}

/// Content in a candidate response.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleCandidateContent {
    #[serde(default)]
    pub parts: Vec<GoogleResponsePart>,
}

/// A part in a Google response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleResponsePart {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub thought: Option<bool>,
    #[serde(default)]
    pub function_call: Option<GoogleFunctionCall>,
    #[serde(default)]
    pub inline_data: Option<GoogleInlineData>,
    #[serde(default)]
    pub executable_code: Option<GoogleExecutableCode>,
    #[serde(default)]
    pub code_execution_result: Option<GoogleCodeExecutionResult>,
}

/// A function call in a Google response.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleFunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Inline data in a Google response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleInlineData {
    pub mime_type: String,
    pub data: String,
}

/// Executable code in a Google response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleExecutableCode {
    #[serde(default)]
    pub language: Option<String>,
    pub code: String,
}

/// Code execution result in a Google response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCodeExecutionResult {
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
}

/// Grounding metadata from Google Search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingMetadata {
    #[serde(default)]
    pub grounding_chunks: Option<Vec<GroundingChunk>>,
    #[serde(default)]
    pub web_search_queries: Option<Vec<String>>,
}

/// A grounding chunk (source).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingChunk {
    #[serde(default)]
    pub web: Option<GroundingWeb>,
}

/// Web grounding info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingWeb {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

/// URL context metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlContextMetadata {
    #[serde(default)]
    pub url_metadata: Option<Vec<UrlMetadataEntry>>,
}

/// A URL metadata entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlMetadataEntry {
    #[serde(default)]
    pub retrieved_url: Option<String>,
    #[serde(default)]
    pub url_retrieval_status: Option<String>,
}

/// Extract sources from grounding and URL context metadata.
fn extract_sources(
    grounding: &Option<GroundingMetadata>,
    url_context: &Option<UrlContextMetadata>,
    id_gen: &dyn Fn() -> String,
) -> Vec<SourcePart> {
    let mut sources = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

    // Extract from grounding metadata
    if let Some(gm) = grounding
        && let Some(ref chunks) = gm.grounding_chunks
    {
        for chunk in chunks {
            if let Some(ref web) = chunk.web
                && let Some(ref uri) = web.uri
                && seen_urls.insert(uri.clone())
            {
                let mut source = SourcePart::url(id_gen(), uri);
                if let Some(ref title) = web.title {
                    source.title = Some(title.clone());
                }
                sources.push(source);
            }
        }
    }

    // Extract from URL context metadata
    if let Some(ucm) = url_context
        && let Some(ref entries) = ucm.url_metadata
    {
        for entry in entries {
            if let Some(ref url) = entry.retrieved_url
                && seen_urls.insert(url.clone())
            {
                sources.push(SourcePart::url(id_gen(), url));
            }
        }
    }

    sources
}

/// Convert Google response parts to assistant content parts.
fn convert_response_parts(
    parts: &[GoogleResponsePart],
    id_gen: &dyn Fn() -> String,
) -> Vec<AssistantContentPart> {
    let mut result = Vec::new();

    for part in parts {
        if let Some(ref text) = part.text {
            if part.thought == Some(true) {
                result.push(AssistantContentPart::reasoning(text));
            } else {
                result.push(AssistantContentPart::text(text));
            }
        }

        if let Some(ref fc) = part.function_call {
            result.push(AssistantContentPart::tool_call(
                id_gen(),
                &fc.name,
                fc.args.clone(),
            ));
        }

        if let Some(ref inline) = part.inline_data {
            result.push(AssistantContentPart::File(FilePart::image_base64(
                &inline.data,
                &inline.mime_type,
            )));
        }

        if let Some(ref exec_code) = part.executable_code {
            result.push(AssistantContentPart::tool_call(
                id_gen(),
                "code_execution",
                json!({
                    "language": exec_code.language,
                    "code": exec_code.code,
                }),
            ));
        }

        if let Some(ref exec_result) = part.code_execution_result {
            result.push(AssistantContentPart::ToolResult(ToolResultPart::new(
                id_gen(),
                "code_execution",
                ToolResultContent::json(json!({
                    "outcome": exec_result.outcome,
                    "output": exec_result.output,
                })),
            )));
        }
    }

    result
}

#[async_trait]
impl LanguageModelV4 for GoogleGenerativeAILanguageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        if let Some(ref f) = self.config.supported_urls {
            f()
        } else {
            HashMap::new()
        }
    }

    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4GenerateResult, AISdkError> {
        let (body, headers) = self.get_args(&options);

        let model_path = get_model_path(&self.model_id);
        let url = format!(
            "{}/v1beta/{}:generateContent",
            without_trailing_slash(&self.config.base_url),
            model_path
        );

        let response: GoogleGenerateContentResponse = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            GoogleFailedResponseHandler,
            options.abort_signal.clone(),
            self.config.client.clone(),
        )
        .await?;

        let candidate = response.candidates.first();
        let id_gen = &*self.config.generate_id;

        // Extract content parts
        let content = if let Some(candidate) = candidate {
            if let Some(ref content) = candidate.content {
                convert_response_parts(&content.parts, id_gen)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Extract sources from grounding metadata
        let mut all_content = content;
        if let Some(candidate) = candidate {
            let sources = extract_sources(
                &candidate.grounding_metadata,
                &candidate.url_context_metadata,
                id_gen,
            );
            for source in sources {
                all_content.push(AssistantContentPart::Source(source));
            }
        }

        // Check for tool calls in content
        let has_tool_calls = all_content
            .iter()
            .any(|p| matches!(p, AssistantContentPart::ToolCall(_)));

        let finish_reason = crate::map_google_generative_ai_finish_reason::map_finish_reason(
            candidate.and_then(|c| c.finish_reason.as_deref()),
            has_tool_calls,
        );

        let usage = convert_usage(response.usage_metadata.as_ref());

        // Build provider metadata
        let mut provider_meta = HashMap::new();
        if let Some(candidate) = candidate {
            if let Some(ref sr) = candidate.safety_ratings {
                provider_meta.insert("safetyRatings".to_string(), sr.clone());
            }
            if let Some(ref gm) = candidate.grounding_metadata
                && let Ok(val) = serde_json::to_value(gm)
            {
                provider_meta.insert("groundingMetadata".to_string(), val);
            }
            if let Some(ref ucm) = candidate.url_context_metadata
                && let Ok(val) = serde_json::to_value(ucm)
            {
                provider_meta.insert("urlContextMetadata".to_string(), val);
            }
        }

        let provider_metadata = if provider_meta.is_empty() {
            None
        } else {
            Some(ProviderMetadata::from_map(provider_meta))
        };

        let mut result = LanguageModelV4GenerateResult::new(all_content, usage, finish_reason);
        result.request = Some(LanguageModelV4Request::new().with_body(body));
        if let Some(model_version) = response.model_version {
            result.response = Some(LanguageModelV4Response::new().with_model_id(model_version));
        }
        result.provider_metadata = provider_metadata;

        Ok(result)
    }

    async fn do_stream(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> Result<LanguageModelV4StreamResult, AISdkError> {
        let (body, headers) = self.get_args(&options);
        let include_raw = options.include_raw_chunks.unwrap_or(false);

        let model_path = get_model_path(&self.model_id);
        let url = format!(
            "{}/v1beta/{}:streamGenerateContent?alt=sse",
            without_trailing_slash(&self.config.base_url),
            model_path
        );

        let byte_stream = post_stream_to_api_with_client(
            &url,
            Some(headers),
            &body,
            options.abort_signal.clone(),
            self.config.client.clone(),
        )
        .await?;

        let id_gen = self.config.generate_id.clone();

        let stream = create_google_stream(byte_stream, id_gen, include_raw);

        let mut result = LanguageModelV4StreamResult::new(stream);
        result.request = Some(LanguageModelV4Request::new().with_body(body));

        Ok(result)
    }
}

struct StreamState {
    byte_stream: vercel_ai_provider_utils::ByteStream,
    buffer: String,
    id_gen: Arc<dyn Fn() -> String + Send + Sync>,
    include_raw: bool,
    text_id: Option<String>,
    reasoning_id: Option<String>,
    tool_call_ids: HashMap<String, String>,
    seen_source_urls: std::collections::HashSet<String>,
    pending_parts: Vec<LanguageModelV4StreamPart>,
    done: bool,
    started: bool,
}

/// Create a stream of LanguageModelV4StreamPart from a byte stream.
fn create_google_stream(
    byte_stream: vercel_ai_provider_utils::ByteStream,
    id_gen: Arc<dyn Fn() -> String + Send + Sync>,
    include_raw: bool,
) -> Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>> {
    use std::collections::HashSet;

    let stream = futures::stream::unfold(
        StreamState {
            byte_stream,
            buffer: String::new(),
            id_gen,
            include_raw,
            text_id: None,
            reasoning_id: None,
            tool_call_ids: HashMap::new(),
            seen_source_urls: HashSet::new(),
            pending_parts: Vec::new(),
            done: false,
            started: false,
        },
        |mut state| async move {
            // Return pending parts first
            if let Some(part) = state.pending_parts.pop() {
                return Some((Ok(part), state));
            }

            if state.done {
                return None;
            }

            // Emit stream start
            if !state.started {
                state.started = true;
                let part = LanguageModelV4StreamPart::StreamStart {
                    warnings: Vec::new(),
                };
                return Some((Ok(part), state));
            }

            use futures::TryStreamExt;
            loop {
                match state.byte_stream.try_next().await {
                    Ok(Some(bytes)) => {
                        let chunk = String::from_utf8_lossy(&bytes);
                        state.buffer.push_str(&chunk);

                        // Process SSE lines
                        while let Some(pos) = state.buffer.find('\n') {
                            let line = state.buffer[..pos].trim().to_string();
                            state.buffer = state.buffer[pos + 1..].to_string();

                            if line.is_empty() || line == "data: [DONE]" {
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data: ")
                                && let Ok(response) =
                                    serde_json::from_str::<GoogleGenerateContentResponse>(data)
                            {
                                let parts = process_stream_chunk(
                                    &response,
                                    &state.id_gen,
                                    &mut state.text_id,
                                    &mut state.reasoning_id,
                                    &mut state.tool_call_ids,
                                    &mut state.seen_source_urls,
                                    state.include_raw,
                                    data,
                                );
                                if !parts.is_empty() {
                                    let mut parts = parts;
                                    let first = parts.remove(0);
                                    parts.reverse();
                                    state.pending_parts = parts;
                                    return Some((Ok(first), state));
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // Stream ended - close any open text/reasoning blocks
                        let mut final_parts = Vec::new();
                        if let Some(ref tid) = state.text_id {
                            final_parts.push(LanguageModelV4StreamPart::TextEnd {
                                id: tid.clone(),
                                provider_metadata: None,
                            });
                        }
                        if let Some(ref rid) = state.reasoning_id {
                            final_parts.push(LanguageModelV4StreamPart::ReasoningEnd {
                                id: rid.clone(),
                                provider_metadata: None,
                            });
                        }

                        state.done = true;

                        if let Some(part) = final_parts.pop() {
                            final_parts.reverse();
                            state.pending_parts = final_parts;
                            return Some((Ok(part), state));
                        }

                        return None;
                    }
                    Err(e) => {
                        state.done = true;
                        return Some((Err(AISdkError::new(format!("Stream error: {e}"))), state));
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

/// Process a single SSE chunk and return stream parts.
#[allow(clippy::too_many_arguments)]
fn process_stream_chunk(
    response: &GoogleGenerateContentResponse,
    id_gen: &Arc<dyn Fn() -> String + Send + Sync>,
    text_id: &mut Option<String>,
    reasoning_id: &mut Option<String>,
    tool_call_ids: &mut HashMap<String, String>,
    seen_source_urls: &mut std::collections::HashSet<String>,
    include_raw: bool,
    raw_data: &str,
) -> Vec<LanguageModelV4StreamPart> {
    let mut parts = Vec::new();

    // Emit raw chunk if requested
    if include_raw && let Ok(raw_val) = serde_json::from_str::<Value>(raw_data) {
        parts.push(LanguageModelV4StreamPart::Raw { raw_value: raw_val });
    }

    let candidate = match response.candidates.first() {
        Some(c) => c,
        None => {
            // Emit usage if present even without candidate
            if let Some(ref usage_meta) = response.usage_metadata {
                let usage = convert_usage(Some(usage_meta));
                parts.push(LanguageModelV4StreamPart::Finish {
                    usage,
                    finish_reason: FinishReason::other(),
                    provider_metadata: None,
                });
            }
            return parts;
        }
    };

    if let Some(ref content) = candidate.content {
        for part in &content.parts {
            // Handle text parts
            if let Some(ref text) = part.text {
                if part.thought == Some(true) {
                    // Reasoning text
                    if reasoning_id.is_none() {
                        let id = id_gen();
                        *reasoning_id = Some(id.clone());
                        parts.push(LanguageModelV4StreamPart::ReasoningStart {
                            id,
                            provider_metadata: None,
                        });
                    }
                    parts.push(LanguageModelV4StreamPart::ReasoningDelta {
                        id: reasoning_id.clone().unwrap_or_default(),
                        delta: text.clone(),
                        provider_metadata: None,
                    });
                } else {
                    // Close reasoning block if transitioning
                    if let Some(rid) = reasoning_id.take() {
                        parts.push(LanguageModelV4StreamPart::ReasoningEnd {
                            id: rid,
                            provider_metadata: None,
                        });
                    }

                    // Regular text
                    if text_id.is_none() {
                        let id = id_gen();
                        *text_id = Some(id.clone());
                        parts.push(LanguageModelV4StreamPart::TextStart {
                            id,
                            provider_metadata: None,
                        });
                    }
                    parts.push(LanguageModelV4StreamPart::TextDelta {
                        id: text_id.clone().unwrap_or_default(),
                        delta: text.clone(),
                        provider_metadata: None,
                    });
                }
            }

            // Handle function calls
            if let Some(ref fc) = part.function_call {
                // Close text block if open
                if let Some(tid) = text_id.take() {
                    parts.push(LanguageModelV4StreamPart::TextEnd {
                        id: tid,
                        provider_metadata: None,
                    });
                }

                let call_id = id_gen();
                let args_str = serde_json::to_string(&fc.args).unwrap_or_default();

                parts.push(LanguageModelV4StreamPart::ToolInputStart {
                    id: call_id.clone(),
                    tool_name: fc.name.clone(),
                    provider_executed: None,
                    dynamic: None,
                    title: None,
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolInputDelta {
                    id: call_id.clone(),
                    delta: args_str,
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolInputEnd {
                    id: call_id.clone(),
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolCall(ToolCall::new(
                    &call_id,
                    &fc.name,
                    fc.args.clone(),
                )));

                tool_call_ids.insert(fc.name.clone(), call_id);
            }

            // Handle inline data (images)
            if let Some(ref inline) = part.inline_data {
                parts.push(LanguageModelV4StreamPart::File(StreamFile {
                    data: inline.data.clone(),
                    media_type: inline.mime_type.clone(),
                    provider_metadata: None,
                }));
            }

            // Handle executable code
            if let Some(ref exec_code) = part.executable_code {
                let call_id = id_gen();
                let args = json!({
                    "language": exec_code.language,
                    "code": exec_code.code,
                });
                let args_str = serde_json::to_string(&args).unwrap_or_default();

                parts.push(LanguageModelV4StreamPart::ToolInputStart {
                    id: call_id.clone(),
                    tool_name: "code_execution".to_string(),
                    provider_executed: Some(true),
                    dynamic: None,
                    title: None,
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolInputDelta {
                    id: call_id.clone(),
                    delta: args_str,
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolInputEnd {
                    id: call_id.clone(),
                    provider_metadata: None,
                });
                parts.push(LanguageModelV4StreamPart::ToolCall(
                    ToolCall::new(&call_id, "code_execution", args).with_provider_executed(true),
                ));
            }
        }
    }

    // Extract sources from grounding metadata
    if let Some(ref gm) = candidate.grounding_metadata
        && let Some(ref chunks) = gm.grounding_chunks
    {
        for chunk in chunks {
            if let Some(ref web) = chunk.web
                && let Some(ref uri) = web.uri
                && seen_source_urls.insert(uri.clone())
            {
                let mut source = SourcePart::url(id_gen(), uri);
                if let Some(ref title) = web.title {
                    source.title = Some(title.clone());
                }
                parts.push(LanguageModelV4StreamPart::Source(source));
            }
        }
    }

    // Extract from URL context metadata
    if let Some(ref ucm) = candidate.url_context_metadata
        && let Some(ref entries) = ucm.url_metadata
    {
        for entry in entries {
            if let Some(ref url) = entry.retrieved_url
                && seen_source_urls.insert(url.clone())
            {
                parts.push(LanguageModelV4StreamPart::Source(SourcePart::url(
                    id_gen(),
                    url,
                )));
            }
        }
    }

    // Handle finish
    if let Some(ref finish_str) = candidate.finish_reason {
        let has_tool_calls = !tool_call_ids.is_empty();
        let finish_reason = crate::map_google_generative_ai_finish_reason::map_finish_reason(
            Some(finish_str),
            has_tool_calls,
        );

        let usage = convert_usage(response.usage_metadata.as_ref());

        // Build provider metadata
        let mut pm = HashMap::new();
        if let Some(ref sr) = candidate.safety_ratings {
            pm.insert("safetyRatings".to_string(), sr.clone());
        }
        if let Some(ref gm) = candidate.grounding_metadata
            && let Ok(val) = serde_json::to_value(gm)
        {
            pm.insert("groundingMetadata".to_string(), val);
        }

        let provider_metadata = if pm.is_empty() {
            None
        } else {
            Some(ProviderMetadata::from_map(pm))
        };

        // Close open blocks before finish
        if let Some(tid) = text_id.take() {
            parts.push(LanguageModelV4StreamPart::TextEnd {
                id: tid,
                provider_metadata: None,
            });
        }
        if let Some(rid) = reasoning_id.take() {
            parts.push(LanguageModelV4StreamPart::ReasoningEnd {
                id: rid,
                provider_metadata: None,
            });
        }

        // Emit model version as response metadata
        if let Some(ref model_version) = response.model_version {
            parts.push(LanguageModelV4StreamPart::ResponseMetadata(
                ResponseMetadata::new().with_model(model_version),
            ));
        }

        parts.push(LanguageModelV4StreamPart::Finish {
            usage,
            finish_reason,
            provider_metadata,
        });
    }

    parts
}

#[cfg(test)]
#[path = "google_generative_ai_language_model.test.rs"]
mod tests;
