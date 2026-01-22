//! Adapter for hyper-sdk providers.
//!
//! This adapter wraps hyper-sdk providers and implements the ProviderAdapter trait,
//! enabling multi-provider support through the unified hyper-sdk interface.

use super::AdapterConfig;
use super::GenerateResult;
use super::ProviderAdapter;
use crate::common::Prompt;
use crate::common::ResponseEvent;
use crate::error::ApiError;
use async_trait::async_trait;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use hyper_sdk::ContentBlock;
use hyper_sdk::GenerateRequest;
use hyper_sdk::GenerateResponse;
use hyper_sdk::HyperError;
use hyper_sdk::Provider;
use hyper_sdk::ThinkingConfig;
use hyper_sdk::ToolChoice;
use hyper_sdk::ToolDefinition;
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps a hyper-sdk provider.
///
/// This adapter enables codex-api to use any hyper-sdk provider (OpenAI, Anthropic,
/// Gemini, etc.) through the unified ProviderAdapter interface.
#[derive(Debug)]
pub struct HyperSdkAdapter {
    /// The hyper-sdk provider instance.
    provider: Arc<dyn Provider>,
    /// Adapter name (provider name).
    name: String,
}

impl HyperSdkAdapter {
    /// Create a new HyperSdkAdapter wrapping a provider.
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        let name = provider.name().to_string();
        Self { provider, name }
    }

    /// Create adapter for a specific provider by name.
    pub fn from_provider_name(name: &str) -> Result<Self, ApiError> {
        let provider = hyper_sdk::get_provider(name).ok_or_else(|| ApiError::Api {
            status: http::StatusCode::NOT_FOUND,
            message: format!("Provider '{}' not found in hyper-sdk registry", name),
        })?;
        Ok(Self::new(provider))
    }
}

#[async_trait]
impl ProviderAdapter for HyperSdkAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    async fn generate(
        &self,
        prompt: &Prompt,
        config: &AdapterConfig,
    ) -> Result<GenerateResult, ApiError> {
        // Get the model from provider
        let model = self
            .provider
            .model(&config.model)
            .map_err(map_hyper_error)?;

        // Convert Prompt to GenerateRequest
        let request = convert_prompt_to_request(prompt, config)?;

        // Generate response
        let response = model.generate(request).await.map_err(map_hyper_error)?;

        // Convert response to events
        let events = convert_response_to_events(&response);
        let usage = response.usage.as_ref().map(convert_token_usage);

        Ok(GenerateResult {
            events,
            usage,
            response_id: Some(response.id),
        })
    }

    fn supports_response_id(&self) -> bool {
        // OpenAI supports response ID, others may not
        self.name == "openai"
    }
}

/// Convert a codex-api Prompt to a hyper-sdk GenerateRequest.
fn convert_prompt_to_request(
    prompt: &Prompt,
    config: &AdapterConfig,
) -> Result<GenerateRequest, ApiError> {
    // Convert messages from prompt.input
    let messages = convert_input_to_messages(&prompt.input)?;

    // Build base request
    let mut request = GenerateRequest::new(messages);

    // Add system message if instructions present
    if !prompt.instructions.is_empty() {
        // Prepend system message
        request
            .messages
            .insert(0, hyper_sdk::Message::system(&prompt.instructions));
    }

    // Convert tools
    if !prompt.tools.is_empty() {
        let tools = prompt
            .tools
            .iter()
            .filter_map(|t| convert_tool_definition(t))
            .collect();
        request = request.tools(tools);

        // Set tool choice based on parallel_tool_calls
        if !prompt.parallel_tool_calls {
            request = request.tool_choice(ToolChoice::Auto);
        }
    }

    // Apply ultrathink config if present
    if let Some(ultrathink) = &config.ultrathink_config {
        if ultrathink.budget_tokens > 0 {
            request =
                request.thinking_config(ThinkingConfig::with_budget(ultrathink.budget_tokens));
        }
    }

    // Apply model parameters from config.extra if present
    if let Some(extra) = &config.extra {
        if let Some(temp) = extra.get("temperature").and_then(|v| v.as_f64()) {
            request = request.temperature(temp);
        }
        if let Some(max_tokens) = extra.get("max_tokens").and_then(|v| v.as_i64()) {
            request = request.max_tokens(max_tokens as i32);
        }
        if let Some(top_p) = extra.get("top_p").and_then(|v| v.as_f64()) {
            request = request.top_p(top_p);
        }
        if let Some(top_k) = extra.get("top_k").and_then(|v| v.as_i64()) {
            request = request.top_k(top_k as i32);
        }
        if let Some(presence_penalty) = extra.get("presence_penalty").and_then(|v| v.as_f64()) {
            request = request.presence_penalty(presence_penalty);
        }
        if let Some(frequency_penalty) = extra.get("frequency_penalty").and_then(|v| v.as_f64()) {
            request = request.frequency_penalty(frequency_penalty);
        }
    }

    Ok(request)
}

/// Convert codex-api input items to hyper-sdk messages.
fn convert_input_to_messages(input: &[ResponseItem]) -> Result<Vec<hyper_sdk::Message>, ApiError> {
    let mut messages = Vec::new();

    for item in input {
        match item {
            ResponseItem::Message { role, content, .. } => {
                let hyper_msg = match role.as_str() {
                    "user" => {
                        let text = content
                            .iter()
                            .filter_map(|c| {
                                if let ContentItem::InputText { text } = c {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        hyper_sdk::Message::user(&text)
                    }
                    "assistant" => {
                        let text = content
                            .iter()
                            .filter_map(|c| {
                                if let ContentItem::OutputText { text } = c {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        hyper_sdk::Message::assistant(&text)
                    }
                    "system" | "developer" => hyper_sdk::Message::system(
                        &content
                            .iter()
                            .filter_map(|c| {
                                if let ContentItem::InputText { text } = c {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(""),
                    ),
                    _ => continue,
                };
                messages.push(hyper_msg);
            }
            ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                // Parse arguments string to JSON
                let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);

                // Function calls become assistant messages with tool use
                let tool_use = ContentBlock::tool_use(call_id, name, args);
                messages.push(hyper_sdk::Message {
                    role: hyper_sdk::Role::Assistant,
                    content: vec![tool_use],
                    provider_options: None,
                    metadata: hyper_sdk::ProviderMetadata::new(),
                });
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                // Function outputs become tool result messages
                messages.push(hyper_sdk::Message::tool_result(
                    call_id,
                    hyper_sdk::ToolResultContent::text(&output.content),
                ));
            }
            ResponseItem::Reasoning { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::GhostSnapshot { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::Other => {
                // Skip these items
            }
        }
    }

    Ok(messages)
}

/// Convert a JSON tool definition to hyper-sdk ToolDefinition.
fn convert_tool_definition(tool: &Value) -> Option<ToolDefinition> {
    let name = tool.get("name")?.as_str()?;
    let description = tool.get("description").and_then(|d| d.as_str());
    let parameters = tool.get("parameters").cloned().unwrap_or(Value::Null);

    Some(if let Some(desc) = description {
        ToolDefinition::full(name, desc, parameters)
    } else {
        ToolDefinition::new(name, parameters)
    })
}

/// Convert hyper-sdk GenerateResponse to codex-api ResponseEvents.
fn convert_response_to_events(response: &GenerateResponse) -> Vec<ResponseEvent> {
    let mut events = Vec::new();

    // Created event
    events.push(ResponseEvent::Created);

    // Process content blocks
    for block in &response.content {
        match block {
            ContentBlock::Text { text } => {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Message {
                    id: Some(response.id.clone()),
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText { text: text.clone() }],
                }));
            }
            ContentBlock::ToolUse { id, name, input } => {
                // Serialize arguments to string as expected by codex-protocol
                let arguments = serde_json::to_string(&input).unwrap_or_default();
                events.push(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    id: Some(format!("fc_{}", id)),
                    call_id: id.clone(),
                    name: name.clone(),
                    arguments,
                }));
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let output_text = match content {
                    hyper_sdk::ToolResultContent::Text(s) => s.clone(),
                    hyper_sdk::ToolResultContent::Json(v) => {
                        serde_json::to_string(&v).unwrap_or_default()
                    }
                    hyper_sdk::ToolResultContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            hyper_sdk::tools::ToolResultBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                };
                events.push(ResponseEvent::OutputItemDone(
                    ResponseItem::FunctionCallOutput {
                        call_id: tool_use_id.clone(),
                        output: FunctionCallOutputPayload {
                            content: output_text,
                            content_items: None,
                            success: Some(!is_error),
                        },
                    },
                ));
            }
            ContentBlock::Thinking { content, .. } => {
                events.push(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
                    id: format!("think_{}", response.id),
                    summary: vec![
                        codex_protocol::models::ReasoningItemReasoningSummary::SummaryText {
                            text: content.clone(),
                        },
                    ],
                    content: None,
                    encrypted_content: None,
                }));
            }
            ContentBlock::Image { .. } => {
                // Skip image blocks for now
            }
        }
    }

    // Completed event
    events.push(ResponseEvent::Completed {
        response_id: response.id.clone(),
        token_usage: response.usage.as_ref().map(convert_token_usage),
    });

    events
}

/// Convert hyper-sdk TokenUsage to codex-protocol TokenUsage.
fn convert_token_usage(usage: &hyper_sdk::TokenUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cached_input_tokens: usage.cache_read_tokens.unwrap_or(0),
        reasoning_output_tokens: usage.reasoning_tokens.unwrap_or(0),
    }
}

/// Map hyper-sdk errors to codex-api ApiError.
fn map_hyper_error(error: HyperError) -> ApiError {
    match error {
        HyperError::RateLimitExceeded(msg) => ApiError::RateLimit(msg),
        HyperError::ContextWindowExceeded(_) => ApiError::ContextWindowExceeded,
        HyperError::UnsupportedCapability(cap) => ApiError::Api {
            status: http::StatusCode::BAD_REQUEST,
            message: format!("Unsupported capability: {:?}", cap),
        },
        HyperError::ProviderError { code, message } => ApiError::Api {
            status: http::StatusCode::BAD_REQUEST,
            message: format!("{}: {}", code, message),
        },
        HyperError::ConfigError(msg) => ApiError::Api {
            status: http::StatusCode::BAD_REQUEST,
            message: format!("Configuration error: {}", msg),
        },
        HyperError::AuthenticationFailed(msg) => ApiError::Api {
            status: http::StatusCode::UNAUTHORIZED,
            message: format!("Authentication failed: {}", msg),
        },
        HyperError::ModelNotFound(model) => ApiError::Api {
            status: http::StatusCode::NOT_FOUND,
            message: format!("Model not found: {}", model),
        },
        HyperError::Internal(msg) => ApiError::Api {
            status: http::StatusCode::INTERNAL_SERVER_ERROR,
            message: msg,
        },
        HyperError::NetworkError(msg) => {
            ApiError::Transport(codex_client::TransportError::Network(msg))
        }
        HyperError::ParseError(msg) => ApiError::Stream(msg),
        HyperError::StreamError(msg) => ApiError::Stream(msg),
        HyperError::InvalidRequest(msg) => ApiError::Api {
            status: http::StatusCode::BAD_REQUEST,
            message: msg,
        },
        HyperError::ProviderNotFound(name) => ApiError::Api {
            status: http::StatusCode::NOT_FOUND,
            message: format!("Provider not found: {}", name),
        },
        HyperError::Retryable { message, delay } => ApiError::Retryable { message, delay },
        HyperError::PreviousResponseNotFound(_) => ApiError::PreviousResponseNotFound,
        HyperError::QuotaExceeded(_) => ApiError::QuotaExceeded,
        HyperError::StreamIdleTimeout(duration) => {
            ApiError::Stream(format!("Stream idle timeout after {:?}", duration))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_tool_definition() {
        let tool = serde_json::json!({
            "name": "get_weather",
            "description": "Get the weather for a location",
            "parameters": {
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }
        });

        let def = convert_tool_definition(&tool).unwrap();
        assert_eq!(def.name, "get_weather");
    }

    #[test]
    fn test_convert_token_usage() {
        let hyper_usage = hyper_sdk::TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cache_read_tokens: Some(20),
            cache_creation_tokens: Some(10),
            reasoning_tokens: Some(30),
        };

        let codex_usage = convert_token_usage(&hyper_usage);
        assert_eq!(codex_usage.input_tokens, 100);
        assert_eq!(codex_usage.output_tokens, 50);
        assert_eq!(codex_usage.total_tokens, 150);
        assert_eq!(codex_usage.cached_input_tokens, 20);
        assert_eq!(codex_usage.reasoning_output_tokens, 30);
    }
}
