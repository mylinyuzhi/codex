//! Generate text from a prompt.
//!
//! This module provides the `generate_text` function for generating text
//! from a language model without streaming.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::LanguageModelV4ToolChoice;

use crate::error::AIError;
use crate::model::LanguageModel;
use crate::model::resolve_language_model;
use crate::prompt::CallSettings;
use crate::prompt::TimeoutConfiguration;
use crate::types::ProviderOptions;
use crate::types::ToolExecutionOptions;
use crate::types::ToolRegistry;
use crate::util::retry::RetryConfig;
use crate::util::retry::with_retry;

use super::build_call_options;
use super::callback::FinishEventMetadata;
use super::callback::GenerateTextCallbacks;
use super::callback::OnFinishEvent;
use super::callback::OnStartEvent;
use super::callback::OnStepFinishEvent;
use super::callback::OnStepStartEvent;
use super::collect_tool_approvals::ToolApprovalCollector;
use super::collect_tool_approvals::ToolApprovalRequest;
use super::collect_tool_approvals::apply_approvals;
use super::content_utils;
use super::generate_text_result::GenerateTextResult;
use super::generate_text_result::ToolResult;
use super::output::Output;
use super::response_message::build_tool_result_message;
use super::step_result::StepResult;
use super::stop_condition::StopCondition;
use super::tool_call_repair::ToolCallRepairFunction;
use super::tool_call_repair::validate_tool_call_for_repair;

/// A function that can override step configuration.
///
/// Called before each step to allow per-step model/tool/message overrides.
pub type PrepareStepFn =
    Arc<dyn Fn(&PrepareStepContext) -> Option<PrepareStepOverrides> + Send + Sync>;

/// Context provided to the `prepare_step` callback.
#[derive(Debug)]
pub struct PrepareStepContext {
    /// The current step number.
    pub step: u32,
    /// Steps completed so far.
    pub steps: Vec<StepResult>,
    /// The current model ID.
    pub model_id: String,
}

/// Overrides returned from `prepare_step`.
#[derive(Debug, Default)]
pub struct PrepareStepOverrides {
    /// Override the tool choice for this step.
    pub tool_choice: Option<LanguageModelV4ToolChoice>,
    /// Override the active tools for this step.
    pub active_tools: Option<Vec<String>>,
}

/// Options for `generate_text`.
#[derive(Default)]
pub struct GenerateTextOptions {
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
    pub callbacks: GenerateTextCallbacks,
    /// Retry configuration for transient failures.
    pub retry_config: Option<RetryConfig>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Output configuration for structured output.
    pub output: Option<Output>,
    /// Stop conditions for multi-step generation.
    pub stop_when: Vec<StopCondition>,
    /// Filter which tools are available per step.
    pub active_tools: Option<Vec<String>>,
    /// Per-step overrides callback.
    pub prepare_step: Option<PrepareStepFn>,
    /// Tool call repair function for malformed tool calls.
    pub repair_tool_call: Option<Arc<dyn ToolCallRepairFunction>>,
    /// Tool approval collector.
    pub tool_call_approval: Option<Arc<dyn ToolApprovalCollector>>,
}

impl GenerateTextOptions {
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
    pub fn with_callbacks(mut self, callbacks: GenerateTextCallbacks) -> Self {
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

    /// Add a stop condition.
    pub fn with_stop_when(mut self, condition: StopCondition) -> Self {
        self.stop_when.push(condition);
        self
    }

    /// Set the active tools filter.
    pub fn with_active_tools(mut self, tools: Vec<String>) -> Self {
        self.active_tools = Some(tools);
        self
    }

    /// Set the prepare_step callback.
    pub fn with_prepare_step(mut self, prepare: PrepareStepFn) -> Self {
        self.prepare_step = Some(prepare);
        self
    }

    /// Set the tool call repair function.
    pub fn with_repair_tool_call(mut self, repair: Arc<dyn ToolCallRepairFunction>) -> Self {
        self.repair_tool_call = Some(repair);
        self
    }

    /// Set the tool approval collector.
    pub fn with_tool_call_approval(mut self, approval: Arc<dyn ToolApprovalCollector>) -> Self {
        self.tool_call_approval = Some(approval);
        self
    }
}

/// Generate text from a prompt.
///
/// This function sends a prompt to a language model and returns the generated text.
/// It supports tool calling with automatic tool execution.
///
/// # Arguments
///
/// * `options` - The generation options including model, prompt, and settings.
///
/// # Returns
///
/// A `GenerateTextResult` containing the generated text, tool calls, and metadata.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{generate_text, GenerateTextOptions, LanguageModel, Prompt};
///
/// let result = generate_text(GenerateTextOptions {
///     model: "claude-3-sonnet".into(),
///     prompt: Prompt::user("Hello, world!"),
///     ..Default::default()
/// }).await?;
///
/// println!("Response: {}", result.text);
/// ```
pub async fn generate_text(options: GenerateTextOptions) -> Result<GenerateTextResult, AIError> {
    // Resolve the model (this moves options.model, so do it first)
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
    let mut steps: Vec<StepResult> = Vec::new();
    let mut total_usage = vercel_ai_provider::Usage::default();

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

    // Extract options needed for the loop
    let callbacks = &options.callbacks;
    let settings = &options.settings;
    let tool_choice = &options.tool_choice;
    let abort_signal = &options.abort_signal;
    let provider_options = &options.provider_options;
    let output = &options.output;

    // Multi-step loop
    for step in 0..max_steps {
        // Check for cancellation
        if let Some(signal) = abort_signal
            && signal.is_cancelled()
        {
            break;
        }

        // Check stop conditions
        if !options.stop_when.is_empty() {
            let should_stop =
                super::stop_condition::is_stop_condition_met(&options.stop_when, steps.clone())
                    .await;
            if should_stop {
                break;
            }
        }

        // Check timeout
        if let Some(ref timeout) = settings.timeout {
            check_timeout(timeout, step)?;
        }

        // Apply prepare_step overrides
        let step_tool_choice;
        let step_active_tools;
        if let Some(ref prepare) = options.prepare_step {
            let ctx = PrepareStepContext {
                step,
                steps: steps.clone(),
                model_id: model_id.clone(),
            };
            let overrides = prepare(&ctx);
            step_tool_choice = overrides
                .as_ref()
                .and_then(|o| o.tool_choice.clone())
                .or_else(|| tool_choice.clone());
            step_active_tools = overrides
                .as_ref()
                .and_then(|o| o.active_tools.clone())
                .or_else(|| options.active_tools.clone());
        } else {
            step_tool_choice = tool_choice.clone();
            step_active_tools = options.active_tools.clone();
        }

        // Filter active tools
        let effective_tools =
            build_call_options::filter_active_tools(&tool_definitions, &step_active_tools);

        // Build call options using shared builder
        let call_options = build_call_options::build_call_options(
            settings,
            &step_tool_choice,
            abort_signal,
            provider_options,
            output,
            messages.clone(),
            &effective_tools,
        );

        // Execute with retry, optionally with timeout
        let result = if let Some(ref timeout) = settings.timeout
            && let Some(step_ms) = timeout.step_ms
        {
            let duration = std::time::Duration::from_millis(step_ms);
            match tokio::time::timeout(
                duration,
                execute_with_retry(
                    &model,
                    call_options,
                    retry_config.clone(),
                    abort_signal.clone(),
                ),
            )
            .await
            {
                Ok(r) => r?,
                Err(_) => {
                    return Err(AIError::Timeout(format!(
                        "Step {step} timed out after {step_ms}ms"
                    )));
                }
            }
        } else {
            execute_with_retry(
                &model,
                call_options,
                retry_config.clone(),
                abort_signal.clone(),
            )
            .await?
        };

        // Update total usage
        total_usage.add(&result.usage);

        // Extract text and tool calls using shared utilities
        let text = content_utils::extract_text(&result.content);
        let mut tool_calls = content_utils::extract_tool_calls(&result.content);

        // Create step result
        let step_result = StepResult::new(
            step,
            text.clone(),
            result.usage.clone(),
            result.finish_reason.clone(),
        )
        .with_content(result.content.clone())
        .with_warnings(result.warnings.clone());

        // Check if we need to execute tools
        if !tool_calls.is_empty()
            && let Some(tools_reg) = tools
        {
            // Attempt tool call repair if configured
            if let Some(ref repair_fn) = options.repair_tool_call {
                let mut repaired = Vec::new();
                for tc in &tool_calls {
                    match validate_tool_call_for_repair(tc, tools_reg) {
                        Ok(()) => repaired.push(tc.clone()),
                        Err(error) => {
                            if let Some(fixed) = repair_fn.repair(tc, &error).await {
                                repaired.push(fixed);
                            } else {
                                repaired.push(tc.clone());
                            }
                        }
                    }
                }
                tool_calls = repaired;
            }

            // Collect tool approvals if configured
            if let Some(ref approval_collector) = options.tool_call_approval {
                let requests: Vec<ToolApprovalRequest> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tools_reg.get(&tc.tool_name).map(|tool| {
                            let desc = tool.definition().description.clone();
                            ToolApprovalRequest::new(tc.clone())
                                .with_description(desc.unwrap_or_default())
                        })
                    })
                    .collect();

                if !requests.is_empty()
                    && let Ok(approvals) = approval_collector.collect_approvals(requests).await
                {
                    tool_calls = apply_approvals(tool_calls, &approvals);
                }
            }

            let mut tool_results = Vec::new();

            for tc in &tool_calls {
                // Call step start callback
                if let Some(ref callback) = callbacks.on_step_start {
                    callback(OnStepStartEvent::new(step).with_tool_call(tc.clone()));
                }

                // Execute the tool
                let exec_options =
                    ToolExecutionOptions::new(&tc.tool_call_id).with_messages(messages.clone());

                let tool_result = tools_reg
                    .execute(&tc.tool_name, tc.args.clone(), exec_options)
                    .await;

                let result = match tool_result {
                    Ok(output) => ToolResult::new(&tc.tool_call_id, &tc.tool_name, output),
                    Err(e) => ToolResult::error(&tc.tool_call_id, &tc.tool_name, e.to_string()),
                };

                tool_results.push(result);
            }

            // Build step result with tool results
            let step_result = step_result
                .with_tool_calls(tool_calls.clone())
                .with_tool_results(tool_results.clone());

            // Call step finish callback
            if let Some(ref callback) = callbacks.on_step_finish {
                callback(OnStepFinishEvent::new(step, step_result.clone()));
            }

            steps.push(step_result);

            // Add assistant message and tool results to conversation
            messages.push(vercel_ai_provider::LanguageModelV4Message::assistant(
                result.content.clone(),
            ));

            // Add tool results as tool messages using shared utility
            let tool_result_msg = build_tool_result_message(&tool_results);
            messages.push(tool_result_msg);

            // Continue to next step if we haven't reached max_steps
            continue;
        }

        // No tool calls or no tools available - finish
        steps.push(step_result);

        // Build final result
        let mut final_result =
            GenerateTextResult::from_generate_result(result, &model_id).with_steps(steps.clone());
        final_result.total_usage = total_usage.clone();

        // Call on_finish callback
        if let Some(ref callback) = callbacks.on_finish {
            let metadata = FinishEventMetadata::new()
                .with_model_id(&model_id)
                .with_timestamp(final_result.timestamp.clone().unwrap_or_default())
                .with_headers(final_result.response_headers.clone().unwrap_or_default());

            callback(
                OnFinishEvent::new(
                    final_result.finish_reason.clone(),
                    total_usage,
                    final_result.text.clone(),
                )
                .with_metadata(metadata)
                .with_steps(steps),
            );
        }

        return Ok(final_result);
    }

    // Reached max steps
    // Return the last step's result
    let last_step = steps.last().cloned().unwrap_or_else(|| {
        StepResult::new(
            max_steps - 1,
            String::new(),
            vercel_ai_provider::Usage::default(),
            vercel_ai_provider::FinishReason::stop(),
        )
    });

    let mut final_result =
        GenerateTextResult::new(last_step.text, total_usage.clone(), last_step.finish_reason)
            .with_steps(steps.clone());
    final_result.total_usage = total_usage.clone();

    // Call on_finish callback
    if let Some(ref callback) = callbacks.on_finish {
        let metadata = FinishEventMetadata::new().with_model_id(&model_id);
        callback(
            OnFinishEvent::new(
                final_result.finish_reason.clone(),
                total_usage,
                final_result.text.clone(),
            )
            .with_metadata(metadata)
            .with_steps(steps),
        );
    }

    Ok(final_result)
}

/// Execute a model call with retry logic.
async fn execute_with_retry(
    model: &Arc<dyn vercel_ai_provider::LanguageModelV4>,
    call_options: LanguageModelV4CallOptions,
    retry_config: RetryConfig,
    abort_signal: Option<CancellationToken>,
) -> Result<vercel_ai_provider::LanguageModelV4GenerateResult, AIError> {
    let model = model.clone();

    with_retry(retry_config, abort_signal, || {
        let model = model.clone();
        let call_options = call_options.clone();
        async move { model.do_generate(call_options).await.map_err(AIError::from) }
    })
    .await
}

/// Check if a timeout has been exceeded.
fn check_timeout(timeout: &TimeoutConfiguration, _step: u32) -> Result<(), AIError> {
    // Validate the timeout configuration.
    // Actual per-step timeout enforcement is done via tokio::time::timeout in the loop.
    if let Some(total_ms) = timeout.total_ms
        && total_ms == 0
    {
        return Err(AIError::InvalidConfig(
            "total_ms timeout cannot be 0".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "generate_text.test.rs"]
mod tests;
