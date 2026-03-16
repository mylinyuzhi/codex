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
use vercel_ai_provider::ReasoningPart;
use vercel_ai_provider::ResponseFormat;
use vercel_ai_provider::ResponseMetadata;
use vercel_ai_provider::SourceType;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::Warning;
use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::post_stream_to_api_with_client;

use crate::anthropic_config::AnthropicConfig;
use crate::anthropic_error::AnthropicFailedResponseHandler;

use super::anthropic_messages_api::AnthropicCitation;
use super::anthropic_messages_api::AnthropicMessagesResponse;
use super::anthropic_messages_api::AnthropicResponseContentBlock;
use super::anthropic_messages_api::ContentBlockDelta;
use super::anthropic_messages_api::ContentBlockStart;
use super::anthropic_messages_api::ContentBlockStartEvent;
use super::anthropic_messages_api::ContentBlockStopEvent;
use super::anthropic_messages_api::MessageDeltaEvent;
use super::anthropic_messages_api::MessageStartEvent;
use super::anthropic_messages_api::StreamErrorEvent;
use super::anthropic_messages_options::Speed;
use super::anthropic_messages_options::StructuredOutputMode;
use super::anthropic_messages_options::ThinkingConfig;
use super::anthropic_messages_options::extract_anthropic_options;
use super::convert_anthropic_usage::convert_anthropic_usage;
use super::convert_to_anthropic_messages::convert_to_anthropic_messages_full;
use super::map_anthropic_stop_reason::map_anthropic_stop_reason;
use super::prepare_tools::prepare_anthropic_tools;

/// Model capabilities for a given model family.
struct ModelCapabilities {
    max_output_tokens: u64,
    supports_structured_output: bool,
}

/// Get model capabilities based on model ID, matching the TS `getModelCapabilities`.
fn get_model_capabilities(model_id: &str) -> ModelCapabilities {
    // claude-sonnet-4-6 / claude-opus-4-6
    if model_id.starts_with("claude-sonnet-4-6") || model_id.starts_with("claude-opus-4-6") {
        return ModelCapabilities {
            max_output_tokens: 128_000,
            supports_structured_output: true,
        };
    }
    // claude-sonnet-4-5 / claude-opus-4-5 / claude-haiku-4-5
    if model_id.starts_with("claude-sonnet-4-5")
        || model_id.starts_with("claude-opus-4-5")
        || model_id.starts_with("claude-haiku-4-5")
    {
        return ModelCapabilities {
            max_output_tokens: 64_000,
            supports_structured_output: true,
        };
    }
    // claude-opus-4-1
    if model_id.starts_with("claude-opus-4-1") {
        return ModelCapabilities {
            max_output_tokens: 32_000,
            supports_structured_output: true,
        };
    }
    // claude-sonnet-4-*
    if model_id.starts_with("claude-sonnet-4") {
        return ModelCapabilities {
            max_output_tokens: 64_000,
            supports_structured_output: false,
        };
    }
    // claude-opus-4-*
    if model_id.starts_with("claude-opus-4") {
        return ModelCapabilities {
            max_output_tokens: 32_000,
            supports_structured_output: false,
        };
    }
    // claude-3-haiku and default
    ModelCapabilities {
        max_output_tokens: 4096,
        supports_structured_output: false,
    }
}

/// Build a mapping from provider tool API names to SDK tool IDs.
fn build_tool_name_mapping(
    tools: &Option<Vec<vercel_ai_provider::LanguageModelV4Tool>>,
) -> HashMap<String, String> {
    let mut mapping = HashMap::new();
    let Some(tools) = tools else {
        return mapping;
    };
    for tool in tools {
        if let vercel_ai_provider::LanguageModelV4Tool::Provider(pt) = tool {
            let api_name = match pt.id.as_str() {
                "anthropic.code_execution_20250522"
                | "anthropic.code_execution_20250825"
                | "anthropic.code_execution_20260120" => Some("code_execution"),
                "anthropic.web_search_20250305" | "anthropic.web_search_20260209" => {
                    Some("web_search")
                }
                "anthropic.web_fetch_20250910" | "anthropic.web_fetch_20260209" => {
                    Some("web_fetch")
                }
                "anthropic.computer_20241022"
                | "anthropic.computer_20250124"
                | "anthropic.computer_20251124" => Some("computer"),
                "anthropic.text_editor_20241022" | "anthropic.text_editor_20250124" => {
                    Some("str_replace_editor")
                }
                "anthropic.text_editor_20250429" | "anthropic.text_editor_20250728" => {
                    Some("str_replace_based_edit_tool")
                }
                "anthropic.bash_20241022" | "anthropic.bash_20250124" => Some("bash"),
                "anthropic.memory_20250818" => Some("memory"),
                "anthropic.tool_search_regex_20251119" => Some("tool_search_tool_regex"),
                "anthropic.tool_search_bm25_20251119" => Some("tool_search_tool_bm25"),
                _ => None,
            };
            if let Some(api_name) = api_name {
                mapping.insert(api_name.to_string(), pt.id.clone());
            }
        }
    }
    mapping
}

/// Check if web tools 20260209 are present without code execution tools.
fn has_web_tool_20260209_without_code_execution(
    tools: &Option<Vec<vercel_ai_provider::LanguageModelV4Tool>>,
) -> bool {
    let Some(tools) = tools else {
        return false;
    };
    let mut has_web_20260209 = false;
    let mut has_code_execution = false;
    for tool in tools {
        if let vercel_ai_provider::LanguageModelV4Tool::Provider(pt) = tool {
            match pt.id.as_str() {
                "anthropic.web_search_20260209" | "anthropic.web_fetch_20260209" => {
                    has_web_20260209 = true;
                }
                s if s.starts_with("anthropic.code_execution_") => {
                    has_code_execution = true;
                }
                _ => {}
            }
        }
    }
    has_web_20260209 && !has_code_execution
}

/// Anthropic Messages language model implementing `LanguageModelV4`.
pub struct AnthropicMessagesLanguageModel {
    model_id: String,
    config: Arc<AnthropicConfig>,
}

impl AnthropicMessagesLanguageModel {
    /// Create a new Anthropic Messages language model.
    pub fn new(model_id: impl Into<String>, config: Arc<AnthropicConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Build request body, headers, and collect warnings.
    fn get_args(
        &self,
        options: &LanguageModelV4CallOptions,
        stream: bool,
    ) -> Result<(Value, HashMap<String, String>, Vec<Warning>), AISdkError> {
        let mut warnings = Vec::new();
        let anthropic_options = extract_anthropic_options(&options.provider_options);

        // Unsupported standard parameters
        if options.frequency_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "frequencyPenalty".into(),
                details: None,
            });
        }
        if options.presence_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "presencePenalty".into(),
                details: None,
            });
        }
        if options.seed.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "seed".into(),
                details: None,
            });
        }

        // Temperature clamping
        let mut temperature = options.temperature;
        if let Some(t) = temperature {
            if t > 1.0 {
                warnings.push(Warning::Unsupported {
                    feature: "temperature".into(),
                    details: Some(format!(
                        "{t} exceeds anthropic maximum of 1.0. clamped to 1.0"
                    )),
                });
                temperature = Some(1.0);
            } else if t < 0.0 {
                warnings.push(Warning::Unsupported {
                    feature: "temperature".into(),
                    details: Some(format!("{t} is below anthropic minimum of 0. clamped to 0")),
                });
                temperature = Some(0.0);
            }
        }

        // Structured output mode
        let structured_output_mode = anthropic_options
            .structured_output_mode
            .unwrap_or(StructuredOutputMode::Auto);
        let use_structured_output = matches!(
            structured_output_mode,
            StructuredOutputMode::OutputFormat | StructuredOutputMode::Auto
        );

        // JSON response format handling
        let json_response_tool = if let Some(ResponseFormat::Json {
            schema: Some(ref schema),
            ..
        }) = options.response_format
        {
            if !use_structured_output {
                // Use json tool fallback
                Some(schema.clone())
            } else {
                None
            }
        } else {
            None
        };

        let uses_json_response_tool = json_response_tool.is_some();

        // Convert prompt
        let send_reasoning = anthropic_options.send_reasoning.unwrap_or(true);
        let converted = convert_to_anthropic_messages_full(&options.prompt, send_reasoning);
        let system = converted.system;
        let messages = converted.messages;
        warnings.extend(converted.warnings);
        let betas_from_messages = converted.betas;

        // Model capabilities
        let capabilities = get_model_capabilities(&self.model_id);

        // Prepare tools (possibly injecting the JSON response tool)
        let prepared = if uses_json_response_tool {
            let json_schema = json_response_tool.as_ref().unwrap_or(&Value::Null);
            // Build a synthetic function tool list with the JSON tool appended
            let mut tools_with_json = options.tools.as_ref().cloned().unwrap_or_default();
            tools_with_json.push(make_json_response_tool(json_schema));
            prepare_anthropic_tools(
                &Some(tools_with_json),
                &Some(vercel_ai_provider::LanguageModelV4ToolChoice::Required),
                Some(true),
                false,
            )
        } else {
            prepare_anthropic_tools(
                &options.tools,
                &options.tool_choice,
                anthropic_options.disable_parallel_tool_use,
                capabilities.supports_structured_output,
            )
        };
        warnings.extend(prepared.warnings);
        let mut betas = prepared.betas;
        betas.extend(betas_from_messages);

        // Thinking configuration
        let thinking_type = anthropic_options.thinking.as_ref();
        let is_thinking = matches!(
            thinking_type,
            Some(ThinkingConfig::Enabled { .. }) | Some(ThinkingConfig::Adaptive)
        );

        let mut thinking_budget: Option<u64> = match thinking_type {
            Some(ThinkingConfig::Enabled { budget_tokens }) => *budget_tokens,
            _ => None,
        };

        let max_tokens = options
            .max_output_tokens
            .unwrap_or(capabilities.max_output_tokens);

        // Build base body
        let mut body = json!({
            "model": self.model_id,
            "max_tokens": max_tokens,
        });

        // System messages
        if let Some(system) = system {
            body["system"] = Value::Array(system);
        }
        body["messages"] = Value::Array(messages);

        // Tools
        if let Some(tools) = prepared.tools {
            body["tools"] = Value::Array(tools);
        }
        if let Some(tc) = prepared.tool_choice {
            body["tool_choice"] = tc;
        }

        // Thinking
        if is_thinking {
            match thinking_type {
                Some(ThinkingConfig::Enabled { budget_tokens }) => {
                    let budget = budget_tokens.unwrap_or_else(|| {
                        warnings.push(Warning::Other {
                            message: "thinking budget is required when thinking is enabled. using default budget of 1024 tokens.".into(),
                        });
                        thinking_budget = Some(1024);
                        1024
                    });
                    body["thinking"] = json!({
                        "type": "enabled",
                        "budget_tokens": budget,
                    });
                }
                Some(ThinkingConfig::Adaptive) => {
                    body["thinking"] = json!({"type": "adaptive"});
                }
                _ => {}
            }

            // When thinking is enabled, disable temperature/topK/topP
            if temperature.is_some() {
                temperature = None;
                warnings.push(Warning::Unsupported {
                    feature: "temperature".into(),
                    details: Some("temperature is not supported when thinking is enabled".into()),
                });
            }
            if options.top_k.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "topK".into(),
                    details: Some("topK is not supported when thinking is enabled".into()),
                });
            }
            if options.top_p.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "topP".into(),
                    details: Some("topP is not supported when thinking is enabled".into()),
                });
            }

            // Adjust max_tokens to account for thinking budget
            body["max_tokens"] = json!(max_tokens + thinking_budget.unwrap_or(0));
        } else {
            // Only check temperature/topP mutual exclusivity when thinking is not enabled
            if options.top_p.is_some() && temperature.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "topP".into(),
                    details: Some(
                        "topP is not supported when temperature is set. topP is ignored.".into(),
                    ),
                });
            } else if let Some(top_p) = options.top_p {
                body["top_p"] = json!(top_p);
            }
        }

        // Standard parameters
        if let Some(t) = temperature {
            body["temperature"] = json!(t);
        }
        if let Some(top_k) = options.top_k {
            if !is_thinking {
                body["top_k"] = json!(top_k);
            }
        }
        if let Some(ref stop) = options.stop_sequences
            && !stop.is_empty()
        {
            body["stop_sequences"] = json!(stop);
        }

        // Effort
        if let Some(ref effort) = anthropic_options.effort {
            body["output_config"] = json!({"effort": effort.as_str()});
            betas.insert("effort-2025-11-24".into());
        }

        // Structured output via output_config
        if use_structured_output {
            if let Some(ResponseFormat::Json {
                schema: Some(ref schema),
                ..
            }) = options.response_format
            {
                let output_config = body
                    .as_object_mut()
                    .and_then(|m| m.get_mut("output_config"))
                    .and_then(|v| v.as_object_mut());
                if let Some(oc) = output_config {
                    oc.insert(
                        "format".into(),
                        json!({"type": "json_schema", "schema": schema}),
                    );
                } else {
                    body["output_config"] = json!({
                        "format": {"type": "json_schema", "schema": schema},
                    });
                }
            }
        }

        // Speed
        if let Some(ref speed) = anthropic_options.speed {
            body["speed"] = Value::String(speed.as_str().into());
            if matches!(speed, Speed::Fast) {
                betas.insert("fast-mode-2026-02-01".into());
            }
        }

        // MCP servers
        if let Some(ref mcp_servers) = anthropic_options.mcp_servers
            && !mcp_servers.is_empty()
        {
            let servers: Vec<Value> = mcp_servers
                .iter()
                .map(|s| {
                    let mut sv = json!({
                        "type": s.server_type.as_deref().unwrap_or("url"),
                        "name": s.name,
                        "url": s.url,
                    });
                    if let Some(ref token) = s.authorization_token {
                        sv["authorization_token"] = Value::String(token.clone());
                    }
                    if let Some(ref tc) = s.tool_configuration {
                        let mut config = json!({});
                        if let Some(enabled) = tc.enabled {
                            config["enabled"] = Value::Bool(enabled);
                        }
                        if let Some(ref allowed) = tc.allowed_tools {
                            config["allowed_tools"] = json!(allowed);
                        }
                        sv["tool_configuration"] = config;
                    }
                    sv
                })
                .collect();
            body["mcp_servers"] = Value::Array(servers);
            betas.insert("mcp-client-2025-04-04".into());
        }

        // Container
        if let Some(ref container) = anthropic_options.container {
            if let Some(ref skills) = container.skills
                && !skills.is_empty()
            {
                let skill_values: Vec<Value> = skills
                    .iter()
                    .map(|s| {
                        let mut sv = json!({
                            "type": s.skill_type,
                            "skill_id": s.skill_id,
                        });
                        if let Some(ref v) = s.version {
                            sv["version"] = Value::String(v.clone());
                        }
                        sv
                    })
                    .collect();
                let mut container_val = json!({"skills": skill_values});
                if let Some(ref id) = container.id {
                    container_val["id"] = Value::String(id.clone());
                }
                body["container"] = container_val;
                betas.insert("code-execution-2025-08-25".into());
                betas.insert("skills-2025-10-02".into());
                betas.insert("files-api-2025-04-14".into());
            } else if let Some(ref id) = container.id {
                body["container"] = Value::String(id.clone());
            }
        }

        // Context management
        if let Some(ref ctx_mgmt) = anthropic_options.context_management {
            body["context_management"] = ctx_mgmt.clone();
            betas.insert("context-management-2025-06-27".into());
            // Check for compact edit
            if let Some(edits) = ctx_mgmt.get("edits").and_then(|e| e.as_array()) {
                if edits
                    .iter()
                    .any(|e| e.get("type").and_then(|t| t.as_str()) == Some("compact_20260112"))
                {
                    betas.insert("compact-2026-01-12".into());
                }
            }
        }

        // Streaming
        if stream {
            body["stream"] = Value::Bool(true);
            // Enable fine-grained tool streaming
            if anthropic_options.tool_streaming.unwrap_or(true) {
                betas.insert("fine-grained-tool-streaming-2025-05-14".into());
            }
        }

        // Add user-supplied beta flags
        if let Some(ref extra_betas) = anthropic_options.anthropic_beta {
            for b in extra_betas {
                betas.insert(b.clone());
            }
        }

        // Build merged headers
        let mut headers = self.config.get_headers();
        if !betas.is_empty() {
            let beta_str: Vec<&str> = betas.iter().map(String::as_str).collect();
            headers.insert("anthropic-beta".into(), beta_str.join(","));
        }
        // Merge per-request headers
        if let Some(ref extra) = options.headers {
            for (k, v) in extra {
                headers.insert(k.clone(), v.clone());
            }
        }

        Ok((body, headers, warnings))
    }
}

/// Create a synthetic JSON response function tool.
fn make_json_response_tool(schema: &Value) -> vercel_ai_provider::LanguageModelV4Tool {
    use vercel_ai_provider::language_model::v4::function_tool::LanguageModelV4FunctionTool;
    vercel_ai_provider::LanguageModelV4Tool::Function(LanguageModelV4FunctionTool {
        name: "json".into(),
        description: Some("Respond with a JSON object.".into()),
        input_schema: schema.clone(),
        input_examples: None,
        strict: None,
        provider_options: None,
    })
}

#[async_trait]
impl LanguageModelV4 for AnthropicMessagesLanguageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> HashMap<String, Vec<Regex>> {
        let mut map = HashMap::new();
        // Anthropic supports image URLs and PDF URLs
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
        let (body, headers, warnings) = self.get_args(&options, false)?;
        let url = self.config.url("/messages");

        let response: AnthropicMessagesResponse = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            AnthropicFailedResponseHandler,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let mut content: Vec<AssistantContentPart> = Vec::new();
        let mut is_json_response_from_tool = false;

        // Build tool name mapping (provider API name → SDK tool ID)
        let tool_name_mapping = build_tool_name_mapping(&options.tools);
        let dynamic_code_execution = has_web_tool_20260209_without_code_execution(&options.tools);

        // Determine if we're using JSON response tool
        let uses_json_response_tool = body
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|tools| {
                tools
                    .iter()
                    .any(|t| t.get("name").and_then(|n| n.as_str()) == Some("json"))
            })
            .unwrap_or(false)
            && body
                .get("tool_choice")
                .and_then(|tc| tc.get("type"))
                .and_then(|t| t.as_str())
                == Some("any");

        for block in &response.content {
            match block {
                AnthropicResponseContentBlock::Text { text, citations } => {
                    if !uses_json_response_tool {
                        content.push(AssistantContentPart::Text(TextPart {
                            text: text.clone(),
                            provider_metadata: None,
                        }));

                        // Process citations
                        if let Some(citations) = citations {
                            for citation in citations {
                                if let Some(source) = citation_to_source(citation) {
                                    content.push(source);
                                }
                            }
                        }
                    }
                }
                AnthropicResponseContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    let mut meta = HashMap::new();
                    meta.insert("anthropic".into(), json!({"signature": signature}));
                    content.push(AssistantContentPart::Reasoning(ReasoningPart {
                        text: thinking.clone(),
                        provider_metadata: Some(ProviderMetadata(meta)),
                    }));
                }
                AnthropicResponseContentBlock::RedactedThinking { data } => {
                    let mut meta = HashMap::new();
                    meta.insert("anthropic".into(), json!({"redactedData": data}));
                    content.push(AssistantContentPart::Reasoning(ReasoningPart {
                        text: String::new(),
                        provider_metadata: Some(ProviderMetadata(meta)),
                    }));
                }
                AnthropicResponseContentBlock::Compaction { content: text } => {
                    let mut meta = HashMap::new();
                    meta.insert("anthropic".into(), json!({"type": "compaction"}));
                    content.push(AssistantContentPart::Text(TextPart {
                        text: text.clone(),
                        provider_metadata: Some(ProviderMetadata(meta)),
                    }));
                }
                AnthropicResponseContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    caller,
                } => {
                    let is_json_tool = uses_json_response_tool && name == "json";
                    if is_json_tool {
                        is_json_response_from_tool = true;
                        content.push(AssistantContentPart::Text(TextPart {
                            text: serde_json::to_string(input).unwrap_or_default(),
                            provider_metadata: None,
                        }));
                    } else {
                        // Forward caller as provider metadata
                        let provider_metadata = caller.as_ref().map(|c| {
                            let mut meta = HashMap::new();
                            meta.insert("anthropic".into(), json!({"caller": c}));
                            ProviderMetadata(meta)
                        });
                        content.push(AssistantContentPart::ToolCall(ToolCallPart {
                            tool_call_id: id.clone(),
                            tool_name: name.clone(),
                            input: input.clone(),
                            provider_executed: None,
                            provider_metadata,
                        }));
                    }
                }
                AnthropicResponseContentBlock::ServerToolUse { id, name, input } => {
                    // Map provider tool name to SDK tool ID
                    let tool_name = tool_name_mapping
                        .get(name.as_str())
                        .cloned()
                        .unwrap_or_else(|| name.clone());

                    // Inject type for code_execution server tool use sub-types
                    let mut mapped_input = input.clone().unwrap_or(Value::Null);
                    match name.as_str() {
                        "text_editor_code_execution" | "bash_code_execution" => {
                            // Map to code_execution tool with injected type
                            if let Some(obj) = mapped_input.as_object_mut() {
                                obj.insert("type".to_string(), Value::String(name.clone()));
                            }
                        }
                        "code_execution" => {
                            // Inject programmatic tool call type if needed
                            if let Some(obj) = mapped_input.as_object_mut()
                                && !obj.contains_key("type")
                            {
                                obj.insert(
                                    "type".to_string(),
                                    Value::String("code_execution".to_string()),
                                );
                            }
                        }
                        _ => {}
                    }

                    // Mark dynamically created code_execution calls when web tools
                    // 20260209 are present without explicit code execution tools
                    let is_dynamic_code_exec = dynamic_code_execution
                        && matches!(
                            name.as_str(),
                            "code_execution" | "text_editor_code_execution" | "bash_code_execution"
                        );

                    let mut meta = HashMap::new();
                    if is_dynamic_code_exec {
                        meta.insert("anthropic".into(), json!({"dynamic": true}));
                    }

                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone(),
                        tool_name,
                        input: mapped_input,
                        provider_executed: Some(true),
                        provider_metadata: if meta.is_empty() {
                            None
                        } else {
                            Some(ProviderMetadata(meta))
                        },
                    }));
                }
                AnthropicResponseContentBlock::McpToolUse {
                    id,
                    name,
                    server_name,
                    input,
                } => {
                    let mut meta = HashMap::new();
                    meta.insert(
                        "anthropic".into(),
                        json!({"type": "mcp-tool-use", "serverName": server_name}),
                    );
                    content.push(AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id: id.clone(),
                        tool_name: name.clone(),
                        input: input.clone(),
                        provider_executed: Some(true),
                        provider_metadata: Some(ProviderMetadata(meta)),
                    }));
                }
                AnthropicResponseContentBlock::McpToolResult {
                    tool_use_id,
                    content: result_content,
                    ..
                } => {
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: String::new(),
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::WebSearchToolResult {
                    tool_use_id,
                    content: result_content,
                } => {
                    // Extract sources from web search results
                    if let Some(results) = result_content.as_array() {
                        for result in results {
                            if let (Some(url), Some(title)) = (
                                result.get("url").and_then(|v| v.as_str()),
                                result.get("title").and_then(|v| v.as_str()),
                            ) {
                                content.push(AssistantContentPart::Source(
                                    vercel_ai_provider::content::SourcePart {
                                        source_type: SourceType::Url,
                                        id: vercel_ai_provider_utils::generate_id("src"),
                                        url: Some(url.to_string()),
                                        title: Some(title.to_string()),
                                        media_type: None,
                                        filename: None,
                                        provider_metadata: None,
                                    },
                                ));
                            }
                        }
                    }
                    let result_tool_name = tool_name_mapping
                        .get("web_search")
                        .cloned()
                        .unwrap_or_default();
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: result_tool_name,
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::WebFetchToolResult {
                    tool_use_id,
                    content: result_content,
                } => {
                    let result_tool_name = tool_name_mapping
                        .get("web_fetch")
                        .cloned()
                        .unwrap_or_default();
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: result_tool_name,
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::CodeExecutionToolResult {
                    tool_use_id,
                    content: result_content,
                } => {
                    let result_tool_name = tool_name_mapping
                        .get("code_execution")
                        .cloned()
                        .unwrap_or_default();
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: result_tool_name,
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::BashCodeExecutionToolResult {
                    tool_use_id,
                    content: result_content,
                }
                | AnthropicResponseContentBlock::TextEditorCodeExecutionToolResult {
                    tool_use_id,
                    content: result_content,
                } => {
                    // These map back to code_execution tool
                    let result_tool_name = tool_name_mapping
                        .get("code_execution")
                        .cloned()
                        .unwrap_or_default();
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: result_tool_name,
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::ToolSearchToolResult {
                    tool_use_id,
                    content: result_content,
                } => {
                    content.push(AssistantContentPart::ToolResult(
                        vercel_ai_provider::content::ToolResultPart {
                            tool_call_id: tool_use_id.clone(),
                            tool_name: String::new(),
                            output: vercel_ai_provider::ToolResultContent::Json {
                                value: result_content.clone(),
                                provider_options: None,
                            },
                            is_error: false,
                            provider_metadata: None,
                        },
                    ));
                }
                AnthropicResponseContentBlock::Unknown => {
                    // Unknown content block type — silently skip
                }
            }
        }

        let finish_reason =
            map_anthropic_stop_reason(response.stop_reason.as_deref(), is_json_response_from_tool);
        let usage = convert_anthropic_usage(response.usage.as_ref());

        // Provider metadata
        let mut provider_meta: HashMap<String, Value> = HashMap::new();
        if let Some(ref u) = response.usage {
            if let Ok(v) = serde_json::to_value(u) {
                provider_meta.insert("usage".into(), v);
            }
            if let Some(cc) = u.cache_creation_input_tokens {
                provider_meta.insert("cacheCreationInputTokens".into(), Value::Number(cc.into()));
            }
            if let Some(ref iterations) = u.iterations {
                if let Ok(v) = serde_json::to_value(iterations) {
                    provider_meta.insert("iterations".into(), v);
                }
            }
        }
        if let Some(ref ss) = response.stop_sequence {
            provider_meta.insert("stopSequence".into(), Value::String(ss.clone()));
        }
        if let Some(ref container) = response.container {
            if let Ok(v) = serde_json::to_value(container) {
                provider_meta.insert("container".into(), v);
            }
        }
        if let Some(ref ctx_mgmt) = response.context_management {
            provider_meta.insert("contextManagement".into(), ctx_mgmt.clone());
        }

        let provider_metadata = if provider_meta.is_empty() {
            None
        } else {
            let mut outer = HashMap::new();
            outer.insert(
                "anthropic".into(),
                serde_json::to_value(&provider_meta).unwrap_or_default(),
            );
            Some(ProviderMetadata(outer))
        };

        Ok(LanguageModelV4GenerateResult {
            content,
            usage,
            finish_reason,
            warnings,
            provider_metadata,
            request: Some(LanguageModelV4Request { body: Some(body) }),
            response: Some(LanguageModelV4Response {
                timestamp: None,
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
        let (body, headers, warnings) = self.get_args(&options, true)?;
        let url = self.config.url("/messages");
        let include_raw = options.include_raw_chunks.unwrap_or(false);

        let byte_stream = post_stream_to_api_with_client(
            &url,
            Some(headers),
            &body,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let request_body = body.clone();

        // Detect if using JSON response tool
        let uses_json_response_tool = body
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|tools| {
                tools
                    .iter()
                    .any(|t| t.get("name").and_then(|n| n.as_str()) == Some("json"))
            })
            .unwrap_or(false)
            && body
                .get("tool_choice")
                .and_then(|tc| tc.get("type"))
                .and_then(|t| t.as_str())
                == Some("any");

        let stream =
            create_anthropic_stream(byte_stream, warnings, include_raw, uses_json_response_tool);

        Ok(LanguageModelV4StreamResult {
            stream,
            request: Some(LanguageModelV4Request {
                body: Some(request_body),
            }),
            response: Some(LanguageModelV4StreamResponse::new()),
        })
    }
}

/// Convert an Anthropic citation to a source content part.
fn citation_to_source(citation: &AnthropicCitation) -> Option<AssistantContentPart> {
    match citation {
        AnthropicCitation::WebSearchResultLocation {
            url,
            title,
            cited_text,
            encrypted_index,
        } => {
            let mut meta = HashMap::new();
            meta.insert(
                "anthropic".into(),
                json!({"citedText": cited_text, "encryptedIndex": encrypted_index}),
            );
            Some(AssistantContentPart::Source(
                vercel_ai_provider::content::SourcePart {
                    source_type: SourceType::Url,
                    id: vercel_ai_provider_utils::generate_id("src"),
                    url: Some(url.clone()),
                    title: Some(title.clone()),
                    media_type: None,
                    filename: None,
                    provider_metadata: Some(ProviderMetadata(meta)),
                },
            ))
        }
        AnthropicCitation::PageLocation { .. }
        | AnthropicCitation::CharLocation { .. }
        | AnthropicCitation::Unknown => {
            // Document citations or unknown types — skip
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming implementation
// ---------------------------------------------------------------------------

/// In-progress content block accumulator.
enum InProgressBlock {
    Text {
        id: String,
        started: bool,
    },
    Thinking {
        id: String,
        started: bool,
        signature: String,
    },
    ToolUse {
        id: String,
        tool_name: String,
        input_json: String,
        started: bool,
        is_json_tool: bool,
        provider_executed: Option<bool>,
    },
    ServerToolResult,
    Other,
}

struct AnthropicStreamState {
    byte_stream: vercel_ai_provider_utils::ByteStream,
    buffer: String,
    pending: std::collections::VecDeque<LanguageModelV4StreamPart>,
    blocks: Vec<InProgressBlock>,
    current_event_type: Option<String>,
    current_data_lines: Vec<String>,
    usage: Option<super::anthropic_messages_api::AnthropicUsage>,
    stop_reason: Option<String>,
    finish_emitted: bool,
    done: bool,
    metadata_emitted: bool,
    include_raw: bool,
    uses_json_response_tool: bool,
    is_json_response_from_tool: bool,
}

impl AnthropicStreamState {
    fn new(
        byte_stream: vercel_ai_provider_utils::ByteStream,
        warnings: Vec<Warning>,
        include_raw: bool,
        uses_json_response_tool: bool,
    ) -> Self {
        let mut pending = std::collections::VecDeque::new();
        pending.push_back(LanguageModelV4StreamPart::StreamStart { warnings });

        Self {
            byte_stream,
            buffer: String::new(),
            pending,
            blocks: Vec::new(),
            current_event_type: None,
            current_data_lines: Vec::new(),
            usage: None,
            stop_reason: None,
            finish_emitted: false,
            done: false,
            metadata_emitted: false,
            include_raw,
            uses_json_response_tool,
            is_json_response_from_tool: false,
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
            None => {
                // Flush any remaining buffered data lines
                if !self.current_data_lines.is_empty() {
                    let data = self.current_data_lines.join("\n");
                    self.current_data_lines.clear();
                    let event_type = self.current_event_type.take();
                    self.process_sse_event(event_type.as_deref(), &data);
                }
                Ok(false)
            }
        }
    }

    /// Parse SSE lines. Supports multi-line `data:` fields per SSE spec.
    fn process_buffer(&mut self) {
        while let Some(line_end) = self.buffer.find('\n') {
            let line = self.buffer[..line_end].trim_end_matches('\r').to_string();
            self.buffer = self.buffer[line_end + 1..].to_string();

            if line.is_empty() {
                // Empty line = event dispatch per SSE spec
                if !self.current_data_lines.is_empty() {
                    let data = self.current_data_lines.join("\n");
                    self.current_data_lines.clear();
                    let event_type = self.current_event_type.take();
                    self.process_sse_event(event_type.as_deref(), &data);
                }
                continue;
            }

            if let Some(event_type) = line.strip_prefix("event: ") {
                self.current_event_type = Some(event_type.to_string());
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                self.current_data_lines.push(data.to_string());
            }
        }
    }

    fn process_sse_event(&mut self, event_type: Option<&str>, data: &str) {
        // Emit raw chunk if requested
        if self.include_raw {
            if let Ok(raw) = serde_json::from_str::<Value>(data) {
                self.pending
                    .push_back(LanguageModelV4StreamPart::Raw { raw_value: raw });
            }
        }

        match event_type {
            Some("message_start") => {
                if let Ok(event) = serde_json::from_str::<MessageStartEvent>(data) {
                    if !self.metadata_emitted {
                        self.metadata_emitted = true;
                        let mut meta = ResponseMetadata::new();
                        if let Some(ref id) = event.message.id {
                            meta = meta.with_id(id.clone());
                        }
                        if let Some(ref model) = event.message.model {
                            meta = meta.with_model(model.clone());
                        }
                        self.pending
                            .push_back(LanguageModelV4StreamPart::ResponseMetadata(meta));
                    }

                    // Process pre-populated content (deferred tool calls)
                    if let Some(ref content) = event.message.content {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                if let (Some(id), Some(name)) = (
                                    block.get("id").and_then(|v| v.as_str()),
                                    block.get("name").and_then(|v| v.as_str()),
                                ) {
                                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                                    self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                                        vercel_ai_provider::tool::ToolCall::new(
                                            id.to_string(),
                                            name.to_string(),
                                            input,
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Some("content_block_start") => {
                if let Ok(event) = serde_json::from_str::<ContentBlockStartEvent>(data) {
                    let idx = event.index as usize;
                    // Ensure blocks vec is large enough
                    while self.blocks.len() <= idx {
                        self.blocks.push(InProgressBlock::Other);
                    }

                    match event.content_block {
                        ContentBlockStart::Text { .. } => {
                            let text_id = vercel_ai_provider_utils::generate_id("txt");
                            self.blocks[idx] = InProgressBlock::Text {
                                id: text_id,
                                started: false,
                            };
                        }
                        ContentBlockStart::Thinking { .. } => {
                            let think_id = vercel_ai_provider_utils::generate_id("rea");
                            self.blocks[idx] = InProgressBlock::Thinking {
                                id: think_id,
                                started: false,
                                signature: String::new(),
                            };
                        }
                        ContentBlockStart::RedactedThinking { data: ref d } => {
                            let think_id = vercel_ai_provider_utils::generate_id("rea");
                            let mut meta = HashMap::new();
                            meta.insert(
                                "anthropic".into(),
                                json!({"redactedData": d.as_deref().unwrap_or("")}),
                            );
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ReasoningStart {
                                    id: think_id.clone(),
                                    provider_metadata: Some(ProviderMetadata(meta)),
                                });
                            self.pending
                                .push_back(LanguageModelV4StreamPart::ReasoningEnd {
                                    id: think_id,
                                    provider_metadata: None,
                                });
                            self.blocks[idx] = InProgressBlock::Other;
                        }
                        ContentBlockStart::ToolUse { id, name, .. } => {
                            let is_json_tool = self.uses_json_response_tool && name == "json";
                            if is_json_tool {
                                self.is_json_response_from_tool = true;
                                // For JSON response tool, treat as text
                                let text_id = vercel_ai_provider_utils::generate_id("txt");
                                self.blocks[idx] = InProgressBlock::ToolUse {
                                    id: text_id,
                                    tool_name: name,
                                    input_json: String::new(),
                                    started: false,
                                    is_json_tool: true,
                                    provider_executed: None,
                                };
                            } else {
                                self.blocks[idx] = InProgressBlock::ToolUse {
                                    id,
                                    tool_name: name,
                                    input_json: String::new(),
                                    started: false,
                                    is_json_tool: false,
                                    provider_executed: None,
                                };
                            }
                        }
                        ContentBlockStart::ServerToolUse { id, name, .. } => {
                            self.blocks[idx] = InProgressBlock::ToolUse {
                                id,
                                tool_name: name,
                                input_json: String::new(),
                                started: false,
                                is_json_tool: false,
                                provider_executed: Some(true),
                            };
                        }
                        ContentBlockStart::McpToolUse {
                            id,
                            name,
                            server_name: _,
                            ..
                        } => {
                            self.blocks[idx] = InProgressBlock::ToolUse {
                                id,
                                tool_name: name,
                                input_json: String::new(),
                                started: false,
                                is_json_tool: false,
                                provider_executed: Some(true),
                            };
                        }
                        ContentBlockStart::Compaction { content } => {
                            // Emit compaction as text with metadata
                            let text_id = vercel_ai_provider_utils::generate_id("txt");
                            let mut meta = HashMap::new();
                            meta.insert("anthropic".into(), json!({"type": "compaction"}));
                            self.pending
                                .push_back(LanguageModelV4StreamPart::TextStart {
                                    id: text_id.clone(),
                                    provider_metadata: Some(ProviderMetadata(meta)),
                                });
                            if let Some(text) = content {
                                self.pending
                                    .push_back(LanguageModelV4StreamPart::TextDelta {
                                        id: text_id.clone(),
                                        delta: text,
                                        provider_metadata: None,
                                    });
                            }
                            self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                                id: text_id,
                                provider_metadata: None,
                            });
                            self.blocks[idx] = InProgressBlock::Other;
                        }
                        // Server tool results
                        ContentBlockStart::WebSearchToolResult { .. }
                        | ContentBlockStart::WebFetchToolResult { .. }
                        | ContentBlockStart::CodeExecutionToolResult { .. }
                        | ContentBlockStart::BashCodeExecutionToolResult { .. }
                        | ContentBlockStart::TextEditorCodeExecutionToolResult { .. }
                        | ContentBlockStart::McpToolResult { .. }
                        | ContentBlockStart::ToolSearchToolResult { .. } => {
                            self.blocks[idx] = InProgressBlock::ServerToolResult;
                        }
                        ContentBlockStart::Unknown => {
                            self.blocks[idx] = InProgressBlock::Other;
                        }
                    }
                }
            }
            Some("content_block_delta") => {
                if let Ok(event) = serde_json::from_str::<
                    super::anthropic_messages_api::ContentBlockDeltaEvent,
                >(data)
                {
                    let idx = event.index as usize;
                    if idx < self.blocks.len() {
                        match (&mut self.blocks[idx], &event.delta) {
                            (
                                InProgressBlock::Text { id, started },
                                ContentBlockDelta::TextDelta { text },
                            ) => {
                                if !*started {
                                    *started = true;
                                    self.pending
                                        .push_back(LanguageModelV4StreamPart::TextStart {
                                            id: id.clone(),
                                            provider_metadata: None,
                                        });
                                }
                                self.pending
                                    .push_back(LanguageModelV4StreamPart::TextDelta {
                                        id: id.clone(),
                                        delta: text.clone(),
                                        provider_metadata: None,
                                    });
                            }
                            (
                                InProgressBlock::Thinking { id, started, .. },
                                ContentBlockDelta::ThinkingDelta { thinking },
                            ) => {
                                if !*started {
                                    *started = true;
                                    self.pending.push_back(
                                        LanguageModelV4StreamPart::ReasoningStart {
                                            id: id.clone(),
                                            provider_metadata: None,
                                        },
                                    );
                                }
                                self.pending
                                    .push_back(LanguageModelV4StreamPart::ReasoningDelta {
                                        id: id.clone(),
                                        delta: thinking.clone(),
                                        provider_metadata: None,
                                    });
                            }
                            (
                                InProgressBlock::Thinking { signature, .. },
                                ContentBlockDelta::SignatureDelta {
                                    signature: sig_delta,
                                },
                            ) => {
                                signature.push_str(sig_delta);
                            }
                            (
                                InProgressBlock::ToolUse {
                                    id,
                                    tool_name,
                                    input_json,
                                    started,
                                    is_json_tool,
                                    provider_executed,
                                },
                                ContentBlockDelta::InputJsonDelta { partial_json },
                            ) => {
                                input_json.push_str(partial_json);

                                if *is_json_tool {
                                    // Emit as text for JSON response tool
                                    if !*started {
                                        *started = true;
                                        self.pending.push_back(
                                            LanguageModelV4StreamPart::TextStart {
                                                id: id.clone(),
                                                provider_metadata: None,
                                            },
                                        );
                                    }
                                    self.pending
                                        .push_back(LanguageModelV4StreamPart::TextDelta {
                                            id: id.clone(),
                                            delta: partial_json.clone(),
                                            provider_metadata: None,
                                        });
                                } else {
                                    if !*started {
                                        *started = true;
                                        self.pending.push_back(
                                            LanguageModelV4StreamPart::ToolInputStart {
                                                id: id.clone(),
                                                tool_name: tool_name.clone(),
                                                provider_executed: *provider_executed,
                                                dynamic: None,
                                                title: None,
                                                provider_metadata: None,
                                            },
                                        );
                                    }
                                    self.pending.push_back(
                                        LanguageModelV4StreamPart::ToolInputDelta {
                                            id: id.clone(),
                                            delta: partial_json.clone(),
                                            provider_metadata: None,
                                        },
                                    );
                                }
                            }
                            (
                                InProgressBlock::Text { id: _, .. },
                                ContentBlockDelta::CitationsDelta { citation },
                            ) => {
                                if let Some(source) = citation_to_source(citation) {
                                    if let AssistantContentPart::Source(sp) = source {
                                        self.pending
                                            .push_back(LanguageModelV4StreamPart::Source(sp));
                                    }
                                }
                            }
                            _ => {
                                // Unhandled delta/block combination — ignore
                            }
                        }
                    }
                }
            }
            Some("content_block_stop") => {
                if let Ok(event) = serde_json::from_str::<ContentBlockStopEvent>(data) {
                    let idx = event.index as usize;
                    if idx < self.blocks.len() {
                        match &self.blocks[idx] {
                            InProgressBlock::Text { id, started } => {
                                if *started {
                                    self.pending.push_back(LanguageModelV4StreamPart::TextEnd {
                                        id: id.clone(),
                                        provider_metadata: None,
                                    });
                                }
                            }
                            InProgressBlock::Thinking {
                                id,
                                started,
                                signature,
                            } => {
                                if *started {
                                    let mut meta = HashMap::new();
                                    meta.insert(
                                        "anthropic".into(),
                                        json!({"signature": signature}),
                                    );
                                    self.pending.push_back(
                                        LanguageModelV4StreamPart::ReasoningEnd {
                                            id: id.clone(),
                                            provider_metadata: Some(ProviderMetadata(meta)),
                                        },
                                    );
                                }
                            }
                            InProgressBlock::ToolUse {
                                id,
                                tool_name,
                                input_json,
                                started,
                                is_json_tool,
                                ..
                            } => {
                                if *is_json_tool {
                                    if *started {
                                        self.pending.push_back(
                                            LanguageModelV4StreamPart::TextEnd {
                                                id: id.clone(),
                                                provider_metadata: None,
                                            },
                                        );
                                    }
                                } else {
                                    if *started {
                                        self.pending.push_back(
                                            LanguageModelV4StreamPart::ToolInputEnd {
                                                id: id.clone(),
                                                provider_metadata: None,
                                            },
                                        );
                                    }
                                    let input: Value =
                                        serde_json::from_str(input_json).unwrap_or(Value::Null);
                                    self.pending.push_back(LanguageModelV4StreamPart::ToolCall(
                                        vercel_ai_provider::tool::ToolCall::new(
                                            id.clone(),
                                            tool_name.clone(),
                                            input,
                                        ),
                                    ));
                                }
                            }
                            InProgressBlock::ServerToolResult => {
                                // Server tool results completed via content_block_stop
                                // The full result is in the content_block field
                                if let Some(ref block_val) = event.content_block {
                                    if let Some(tool_use_id) =
                                        block_val.get("tool_use_id").and_then(|v| v.as_str())
                                    {
                                        self.pending.push_back(
                                            LanguageModelV4StreamPart::ToolResult(
                                                vercel_ai_provider::tool::ToolResult::new(
                                                    tool_use_id.to_string(),
                                                    String::new(),
                                                    block_val.clone(),
                                                ),
                                            ),
                                        );
                                    }
                                }
                            }
                            InProgressBlock::Other => {}
                        }
                    }
                }
            }
            Some("message_delta") => {
                if let Ok(event) = serde_json::from_str::<MessageDeltaEvent>(data) {
                    if let Some(ref sr) = event.delta.stop_reason {
                        self.stop_reason = Some(sr.clone());
                    }
                    if let Some(ref du) = event.usage {
                        // Merge output tokens into usage
                        if let Some(ref mut u) = self.usage {
                            if let Some(ot) = du.output_tokens {
                                u.output_tokens = ot;
                            }
                        }
                    }
                }
            }
            Some("message_stop") => {
                // Message complete — finish will be emitted by the unfold
            }
            Some("ping") => {
                // Ignore
            }
            Some("error") => {
                if let Ok(event) = serde_json::from_str::<StreamErrorEvent>(data) {
                    let msg = event
                        .error
                        .and_then(|e| e.message)
                        .unwrap_or_else(|| "Unknown stream error".into());
                    self.pending.push_back(LanguageModelV4StreamPart::Error {
                        error: vercel_ai_provider::StreamError {
                            message: msg,
                            code: None,
                            is_retryable: false,
                        },
                    });
                }
            }
            _ => {
                // Unknown event type — ignore
            }
        }
    }
}

fn create_anthropic_stream(
    byte_stream: vercel_ai_provider_utils::ByteStream,
    warnings: Vec<Warning>,
    include_raw: bool,
    uses_json_response_tool: bool,
) -> Pin<Box<dyn Stream<Item = Result<LanguageModelV4StreamPart, AISdkError>> + Send>> {
    let stream = futures::stream::unfold(
        AnthropicStreamState::new(byte_stream, warnings, include_raw, uses_json_response_tool),
        |mut state| async move {
            loop {
                // Drain pending events first
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), state));
                }

                if state.done {
                    return None;
                }

                match state.next_events().await {
                    Ok(true) => {
                        // More events pending, loop to drain
                    }
                    Ok(false) => {
                        // Stream ended
                        state.done = true;
                        if !state.finish_emitted {
                            state.finish_emitted = true;
                            let finish = LanguageModelV4StreamPart::Finish {
                                usage: convert_anthropic_usage(state.usage.as_ref()),
                                finish_reason: map_anthropic_stop_reason(
                                    state.stop_reason.as_deref(),
                                    state.is_json_response_from_tool,
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

#[cfg(test)]
#[path = "anthropic_messages_language_model.test.rs"]
mod tests;
