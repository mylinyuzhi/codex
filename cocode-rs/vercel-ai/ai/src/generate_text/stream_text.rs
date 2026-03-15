//! Stream text from a prompt.
//!
//! This module provides the `stream_text` function for streaming text
//! generation from a language model.

use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::LanguageModelV4ToolChoice;
use vercel_ai_provider::Usage;

use crate::error::AIError;
use crate::model::LanguageModel;
use crate::model::resolve_language_model;
use crate::prompt::CallSettings;
use crate::types::ProviderOptions;
use crate::types::ToolExecutionOptions;
use crate::types::ToolRegistry;
use crate::util::retry::RetryConfig;

use super::build_call_options;
use super::callback::FinishEventMetadata;
use super::callback::OnChunkEvent;
use super::callback::OnFinishEvent;
use super::callback::OnStartEvent;
use super::callback::OnStepFinishEvent;
use super::callback::OnStepStartEvent;
use super::callback::OnToolCallFinishEvent;
use super::callback::OnToolCallStartEvent;
use super::callback::StreamTextCallbacks;
use super::content_utils;
use super::generate_text_result::GenerateTextResult;
use super::generate_text_result::ToolCall;
use super::generate_text_result::ToolResult;
use super::output::Output;
use super::response_message::build_tool_result_message;
use super::step_result::StepResult;

/// Options for `stream_text`.
#[derive(Default)]
pub struct StreamTextOptions {
    /// The model to use.
    pub model: LanguageModel,
    /// The prompt to send to the model.
    pub prompt: crate::prompt::Prompt,
    /// Tools available to the model.
    pub tools: Option<Arc<ToolRegistry>>,
    /// Tool choice configuration.
    pub tool_choice: Option<LanguageModelV4ToolChoice>,
    /// Maximum number of steps for tool calling.
    pub max_steps: Option<u32>,
    /// Call settings.
    pub settings: CallSettings,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Callbacks for lifecycle events.
    pub callbacks: StreamTextCallbacks,
    /// Retry configuration for transient failures.
    pub retry_config: Option<RetryConfig>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Output configuration for structured output.
    pub output: Option<Output>,
    /// Filter which tools are available per step.
    pub active_tools: Option<Vec<String>>,
}

impl StreamTextOptions {
    /// Create new options with a model and prompt.
    pub fn new(model: impl Into<LanguageModel>, prompt: impl Into<crate::prompt::Prompt>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    /// Set the tools registry.
    pub fn with_tools(mut self, tools: Arc<ToolRegistry>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice.
    pub fn with_tool_choice(mut self, choice: LanguageModelV4ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set the maximum steps.
    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = Some(max_steps);
        self
    }

    /// Set the call settings.
    pub fn with_settings(mut self, settings: CallSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }

    /// Set the callbacks.
    pub fn with_callbacks(mut self, callbacks: StreamTextCallbacks) -> Self {
        self.callbacks = callbacks;
        self
    }

    /// Set the retry configuration.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Set provider-specific options.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set the output configuration for structured output.
    pub fn with_output(mut self, output: Output) -> Self {
        self.output = Some(output);
        self
    }

    /// Set the active tools filter.
    pub fn with_active_tools(mut self, tools: Vec<String>) -> Self {
        self.active_tools = Some(tools);
        self
    }
}

/// A part of the text stream.
#[derive(Debug)]
pub enum TextStreamPart {
    /// Text delta.
    TextDelta {
        /// The text delta.
        delta: String,
    },
    /// Reasoning delta.
    ReasoningDelta {
        /// The reasoning delta.
        delta: String,
    },
    /// Tool call started.
    ToolCallStart {
        /// The tool call ID.
        tool_call_id: String,
        /// The tool name.
        tool_name: String,
    },
    /// Tool call delta (partial arguments).
    ToolCallDelta {
        /// The tool call ID.
        tool_call_id: String,
        /// The arguments delta.
        delta: String,
    },
    /// Tool call complete.
    ToolCall {
        /// The tool call.
        tool_call: ToolCall,
    },
    /// Tool result from executed tool.
    ToolResult {
        /// The tool result.
        result: ToolResult,
    },
    /// Step finished.
    StepFinish {
        /// The step result.
        step: Box<StepResult>,
    },
    /// Finish event.
    Finish {
        /// The finish reason.
        finish_reason: FinishReason,
        /// Token usage.
        usage: Usage,
    },
    /// Error occurred.
    Error {
        /// The error.
        error: AIError,
    },
}

/// Result of `stream_text`.
///
/// This struct provides both streaming access and async methods to get
/// the final result.
pub struct StreamTextResult {
    /// The full stream of text parts (text, reasoning, tool calls, steps, etc.).
    pub stream: Pin<Box<dyn Stream<Item = TextStreamPart> + Send>>,
    /// The model ID.
    pub model_id: String,
}

impl StreamTextResult {
    /// Create a new stream text result.
    pub fn new(
        stream: Pin<Box<dyn Stream<Item = TextStreamPart> + Send>>,
        model_id: String,
    ) -> Self {
        Self { stream, model_id }
    }

    /// Collect the stream into a final text string.
    pub async fn into_text(mut self) -> Result<String, AIError> {
        let mut text = String::new();
        while let Some(part) = self.stream.next().await {
            match part {
                TextStreamPart::TextDelta { delta } => {
                    text.push_str(&delta);
                }
                TextStreamPart::Error { error } => {
                    return Err(error);
                }
                _ => {}
            }
        }
        Ok(text)
    }

    /// Consume the stream and collect into a full `GenerateTextResult`.
    ///
    /// This processes all stream parts and builds the complete result
    /// with text, reasoning, tool calls, steps, and usage information.
    pub async fn into_result(mut self) -> Result<GenerateTextResult, AIError> {
        let mut text = String::new();
        let mut steps: Vec<StepResult> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut tool_results: Vec<ToolResult> = Vec::new();
        let mut total_usage = Usage::default();
        let mut finish_reason = FinishReason::stop();

        while let Some(part) = self.stream.next().await {
            match part {
                TextStreamPart::TextDelta { delta } => {
                    text.push_str(&delta);
                }
                TextStreamPart::ToolCall { tool_call } => {
                    tool_calls.push(tool_call);
                }
                TextStreamPart::ToolResult { result } => {
                    tool_results.push(result);
                }
                TextStreamPart::StepFinish { step } => {
                    steps.push(*step);
                }
                TextStreamPart::Finish {
                    finish_reason: fr,
                    usage,
                } => {
                    finish_reason = fr;
                    total_usage = usage;
                }
                TextStreamPart::Error { error } => {
                    return Err(error);
                }
                _ => {}
            }
        }

        let mut result = GenerateTextResult::new(text, total_usage.clone(), finish_reason);
        result.model_id = Some(self.model_id);
        result.total_usage = total_usage;
        result.tool_calls = tool_calls;
        result.tool_results = tool_results;
        result.steps = steps;
        Ok(result)
    }

    /// Create a text-only stream that filters to only text deltas.
    ///
    /// Returns a stream of `String` values, one per text delta.
    pub fn text_stream(self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.stream.filter_map(|part| async move {
            match part {
                TextStreamPart::TextDelta { delta } => Some(delta),
                _ => None,
            }
        }))
    }
}

/// Stream text from a prompt.
///
/// This function streams text generation from a language model.
/// It supports tool calling with automatic tool execution.
///
/// # Arguments
///
/// * `options` - The streaming options including model, prompt, and settings.
///
/// # Returns
///
/// A `StreamTextResult` containing the stream of text parts.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{stream_text, StreamTextOptions, LanguageModel, Prompt};
///
/// let result = stream_text(StreamTextOptions {
///     model: "claude-3-sonnet".into(),
///     prompt: Prompt::user("Hello, world!"),
///     ..Default::default()
/// });
///
/// // Consume the stream
/// while let Some(part) = result.stream.next().await {
///     match part {
///         TextStreamPart::TextDelta { delta } => print!("{}", delta),
///         _ => {}
///     }
/// }
/// ```
pub fn stream_text(options: StreamTextOptions) -> StreamTextResult {
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    // Spawn the streaming task
    tokio::spawn(async move {
        if let Err(e) = stream_text_inner(options, tx.clone()).await {
            let _ = tx.send(TextStreamPart::Error { error: e }).await;
        }
    });

    let stream = ReceiverStream::new(rx);
    StreamTextResult::new(Box::pin(stream), String::new())
}

async fn stream_text_inner(
    options: StreamTextOptions,
    tx: tokio::sync::mpsc::Sender<TextStreamPart>,
) -> Result<(), AIError> {
    // Resolve the model
    let model = resolve_language_model(options.model)?;
    let model_id = model.model_id().to_string();

    // Call on_start callback
    if let Some(ref callback) = options.callbacks.on_start {
        callback(OnStartEvent::new(&model_id));
    }

    // Build the initial prompt
    let mut messages = options.prompt.to_model_prompt();

    // Track steps
    let max_steps = options.max_steps.unwrap_or(1);
    let mut total_usage = Usage::default();

    // Get tools if available
    let tools = options.tools.as_ref();
    let tool_definitions: Option<Vec<LanguageModelV4Tool>> = tools.map(|t| {
        t.definitions()
            .into_iter()
            .map(|d| LanguageModelV4Tool::function(d.clone()))
            .collect()
    });

    // Build retry config
    let retry_config = options
        .retry_config
        .clone()
        .or_else(|| {
            options
                .settings
                .max_retries
                .map(|max_retries| RetryConfig::new().with_max_retries(max_retries))
        })
        .unwrap_or_default();

    // Multi-step loop
    for step in 0..max_steps {
        // Check for cancellation
        if let Some(ref signal) = options.abort_signal
            && signal.is_cancelled()
        {
            break;
        }

        // Filter active tools
        let effective_tools =
            build_call_options::filter_active_tools(&tool_definitions, &options.active_tools);

        // Build call options using shared builder
        let call_options = build_call_options::build_call_options(
            &options.settings,
            &options.tool_choice,
            &options.abort_signal,
            &options.provider_options,
            &options.output,
            messages.clone(),
            &effective_tools,
        );

        // Execute with retry for stream initialization
        let stream_result = execute_stream_with_retry(
            &model,
            call_options,
            retry_config.clone(),
            options.abort_signal.clone(),
        )
        .await?;

        // Process the stream
        let mut current_text = String::new();
        let mut current_reasoning = String::new();
        let mut current_tool_id: Option<String> = None;
        let mut current_tool_input = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage = Usage::default();
        let mut finish_reason = FinishReason::stop();
        let mut content: Vec<AssistantContentPart> = Vec::new();

        let mut stream = stream_result.stream;

        while let Some(part_result) = stream.next().await {
            match part_result {
                Ok(part) => match part {
                    LanguageModelV4StreamPart::TextStart { .. } => {
                        current_text.clear();
                    }
                    LanguageModelV4StreamPart::TextDelta { delta, .. } => {
                        current_text.push_str(&delta);
                        // Call on_chunk callback
                        if let Some(ref cb) = options.callbacks.on_chunk {
                            cb(OnChunkEvent::text_delta(&delta));
                        }
                        let _ = tx.send(TextStreamPart::TextDelta { delta }).await;
                    }
                    LanguageModelV4StreamPart::TextEnd { .. } => {
                        if !current_text.is_empty() {
                            content.push(AssistantContentPart::text(&current_text));
                        }
                    }
                    LanguageModelV4StreamPart::ReasoningStart { .. } => {
                        current_reasoning.clear();
                    }
                    LanguageModelV4StreamPart::ReasoningDelta { delta, .. } => {
                        current_reasoning.push_str(&delta);
                        // Call on_chunk callback
                        if let Some(ref cb) = options.callbacks.on_chunk {
                            cb(OnChunkEvent::reasoning_delta(&delta));
                        }
                        let _ = tx.send(TextStreamPart::ReasoningDelta { delta }).await;
                    }
                    LanguageModelV4StreamPart::ReasoningEnd { .. } => {
                        if !current_reasoning.is_empty() {
                            content.push(AssistantContentPart::reasoning(&current_reasoning));
                        }
                    }
                    LanguageModelV4StreamPart::ToolInputStart { id, tool_name, .. } => {
                        current_tool_id = Some(id.clone());
                        current_tool_input.clear();

                        // Call on_chunk and on_tool_call_start callbacks
                        if let Some(ref cb) = options.callbacks.on_chunk {
                            cb(OnChunkEvent::tool_call_start(&id, &tool_name));
                        }
                        if let Some(ref cb) = options.callbacks.on_tool_call_start {
                            cb(OnToolCallStartEvent::new(
                                step,
                                ToolCall::new(&id, &tool_name, serde_json::Value::Null),
                            ));
                        }

                        let _ = tx
                            .send(TextStreamPart::ToolCallStart {
                                tool_call_id: id,
                                tool_name,
                            })
                            .await;
                    }
                    LanguageModelV4StreamPart::ToolInputDelta { delta, .. } => {
                        current_tool_input.push_str(&delta);
                        if let Some(id) = current_tool_id.as_ref() {
                            // Call on_chunk callback
                            if let Some(ref cb) = options.callbacks.on_chunk {
                                cb(OnChunkEvent::tool_call_delta(id, &delta));
                            }
                            let _ = tx
                                .send(TextStreamPart::ToolCallDelta {
                                    tool_call_id: id.clone(),
                                    delta,
                                })
                                .await;
                        }
                    }
                    LanguageModelV4StreamPart::ToolInputEnd { .. } => {
                        // Tool input complete, wait for ToolCall event
                    }
                    LanguageModelV4StreamPart::ToolCall(tc) => {
                        let tool_call = ToolCall::new(
                            tc.tool_call_id.clone(),
                            tc.tool_name.clone(),
                            tc.input.clone(),
                        );
                        tool_calls.push(tool_call.clone());

                        // Send the tool call event
                        let _ = tx.send(TextStreamPart::ToolCall { tool_call }).await;

                        current_tool_id = None;
                    }
                    LanguageModelV4StreamPart::Finish {
                        finish_reason: fr,
                        usage: u,
                        ..
                    } => {
                        finish_reason = fr;
                        usage = u.clone();
                        total_usage.add(&u);
                    }
                    _ => {}
                },
                Err(e) => {
                    let _ = tx
                        .send(TextStreamPart::Error {
                            error: AIError::ProviderError(e),
                        })
                        .await;
                    return Err(AIError::ProviderError(vercel_ai_provider::AISdkError::new(
                        "Stream error",
                    )));
                }
            }
        }

        // Check if we need to execute tools
        if !tool_calls.is_empty()
            && let Some(tools_reg) = tools
        {
            let mut tool_results = Vec::new();

            for tc in &tool_calls {
                // Call step start callback
                if let Some(ref callback) = options.callbacks.on_step_start {
                    callback(OnStepStartEvent::new(step).with_tool_call(tc.clone()));
                }

                // Execute the tool
                let exec_options =
                    ToolExecutionOptions::new(&tc.tool_call_id).with_messages(messages.clone());

                let result = tools_reg
                    .execute(&tc.tool_name, tc.args.clone(), exec_options)
                    .await;

                let tool_result = match result {
                    Ok(output) => ToolResult::new(&tc.tool_call_id, &tc.tool_name, output),
                    Err(e) => ToolResult::error(&tc.tool_call_id, &tc.tool_name, e.to_string()),
                };

                // Call on_tool_call_finish callback
                if let Some(ref cb) = options.callbacks.on_tool_call_finish {
                    cb(OnToolCallFinishEvent::new(
                        step,
                        tc.clone(),
                        tool_result.result.clone(),
                        tool_result.is_error,
                    ));
                }

                // Send tool result event
                let _ = tx
                    .send(TextStreamPart::ToolResult {
                        result: tool_result.clone(),
                    })
                    .await;

                tool_results.push(tool_result);
            }

            // Create step result
            let step_result = StepResult::new(
                step,
                content_utils::extract_text(&content),
                usage.clone(),
                finish_reason.clone(),
            )
            .with_content(content.clone())
            .with_tool_calls(tool_calls.clone())
            .with_tool_results(tool_results.clone());

            // Call step finish callback
            if let Some(ref callback) = options.callbacks.on_step_finish {
                callback(OnStepFinishEvent::new(step, step_result.clone()));
            }

            // Send step finish event
            let _ = tx
                .send(TextStreamPart::StepFinish {
                    step: Box::new(step_result),
                })
                .await;

            // Add assistant message and tool results to conversation
            messages.push(vercel_ai_provider::LanguageModelV4Message::assistant(
                content,
            ));

            // Add tool results as tool messages using shared utility
            let tool_result_msg = build_tool_result_message(&tool_results);
            messages.push(tool_result_msg);

            // Continue to next step
            continue;
        }

        // Call on_finish callback
        if let Some(ref callback) = options.callbacks.on_finish {
            let metadata = FinishEventMetadata::new().with_model_id(&model_id);
            callback(
                OnFinishEvent::new(finish_reason.clone(), total_usage.clone(), String::new())
                    .with_metadata(metadata),
            );
        }

        // Send finish event
        let _ = tx
            .send(TextStreamPart::Finish {
                finish_reason,
                usage: total_usage,
            })
            .await;

        break;
    }

    Ok(())
}

/// Execute a streaming request with retry logic.
async fn execute_stream_with_retry(
    model: &Arc<dyn vercel_ai_provider::LanguageModelV4>,
    call_options: LanguageModelV4CallOptions,
    retry_config: RetryConfig,
    abort_signal: Option<CancellationToken>,
) -> Result<vercel_ai_provider::LanguageModelV4StreamResult, AIError> {
    use crate::util::retry::with_retry;

    let model = model.clone();

    with_retry(retry_config, abort_signal, || {
        let model = model.clone();
        let call_options = call_options.clone();
        async move { model.do_stream(call_options).await.map_err(AIError::from) }
    })
    .await
}
