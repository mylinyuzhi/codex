//! API streaming and tool resolution methods for the agent loop.

use std::time::Instant;

use cocode_api::AssistantContentPart;
use cocode_api::CollectedResponse;
use cocode_api::FinishReason;
use cocode_api::LanguageModelMessage;
use cocode_api::LanguageModelTool;
use cocode_api::QueryResultType;
use cocode_api::RequestBuilder;
use cocode_api::StreamOptions;
use cocode_api::TextPart;
use cocode_api::ToolCall;
use cocode_api::ToolCallPart;
use cocode_error::ErrorExt;
use cocode_prompt::SystemPromptBuilder;
use cocode_protocol::LoopEvent;
use cocode_protocol::QueryTracking;
use cocode_protocol::TokenUsage;
use cocode_system_reminder::InjectedBlock;
use cocode_system_reminder::InjectedMessage;
use cocode_tools::StreamingToolExecutor;
use cocode_tools::ToolDefinition;
use snafu::ResultExt;
use tracing::debug;
use tracing::error;
use tracing::warn;

use super::AgentLoop;
use crate::error::agent_loop_error;

impl AgentLoop {
    pub(super) async fn stream_with_tools(
        &mut self,
        turn_id: &str,
        executor: &StreamingToolExecutor,
        injected_messages: &[InjectedMessage],
        query_tracking: &QueryTracking,
    ) -> crate::error::Result<CollectedResponse> {
        debug!(turn_id, "Sending API request");

        // Get model and build request using ModelHub
        // Use the real session_id from query_tracking instead of extracting from turn_id
        let session_id = &query_tracking.chain_id;

        // Apply skill model override if set, then clear it (one-shot).
        let override_role =
            self.model_override
                .take()
                .and_then(|m| match m.to_lowercase().as_str() {
                    "sonnet" | "fast" => Some(cocode_protocol::model::ModelRole::Fast),
                    "opus" | "main" => Some(cocode_protocol::model::ModelRole::Main),
                    "haiku" => Some(cocode_protocol::model::ModelRole::Fast),
                    "inherit" => None,
                    _ => {
                        warn!(model = %m, "Unknown skill model override, ignoring");
                        None
                    }
                });

        let effective_selections = if let Some(role) = override_role {
            // Temporarily swap main selection with the role's selection
            let mut sel = self.selections.clone();
            if let Some(role_sel) = self.selections.get(role).cloned() {
                sel.set(cocode_protocol::ModelRole::Main, role_sel);
            }
            sel
        } else {
            self.selections.clone()
        };

        let (ctx, model) = self
            .model_hub
            .prepare_main_with_selections(&effective_selections, session_id, self.turn_number)
            .context(agent_loop_error::PrepareMainModelSnafu)?;

        // Build messages and tools using existing logic (model-aware filtering)
        let (messages, tools) = self.build_messages_and_tools(injected_messages, &ctx.model_info);

        // Tell the executor which tool names the model was actually given.
        // Any tool call outside this set is rejected as NotFound, preventing
        // hallucinated calls to apply_patch (when type=None/Shell) or tools
        // outside experimental_supported_tools.
        executor.set_allowed_tool_names(tools.iter().map(|d| d.name().to_string()).collect());

        // Use RequestBuilder to assemble the final request with context parameters
        let mut builder = RequestBuilder::new(ctx).messages(messages);
        if !tools.is_empty() {
            builder = builder.tools(tools);
        }
        if let Some(max_tokens) = self.config.max_tokens {
            builder = builder.max_tokens(max_tokens as u64);
        }

        let request = builder.build();

        let api_request_start = Instant::now();
        let stream_result = self
            .api_client
            .stream_request(&*model, request, StreamOptions::streaming())
            .await;
        let api_connect_duration = api_request_start.elapsed();

        // Record API request connection event
        if let Some(otel) = &self.otel_manager {
            let (status, error) = match &stream_result {
                Ok(_) => (Some(200u16), None),
                Err(e) => (None, Some(e.to_string())),
            };
            otel.record_api_request(1, status, error.as_deref(), api_connect_duration);
        }

        let mut stream = stream_result.context(agent_loop_error::ApiStreamSnafu)?;

        let mut all_content: Vec<AssistantContentPart> = Vec::new();
        let mut final_usage: Option<TokenUsage> = None;
        let mut final_finish_reason = FinishReason::stop();

        // Stall detection and two-tier watchdog configuration.
        //
        // Tier 1 (warning): Emit StreamWatchdogWarning after warning_timeout.
        // Tier 2 (abort): Kill stream after abort_timeout or stall_timeout.
        let stall_enabled = self.config.stall_detection.enabled;
        let watchdog = &self.config.stall_detection.watchdog;
        let abort_timeout = if watchdog.enabled {
            watchdog.abort_timeout
        } else {
            self.config.stall_detection.stall_timeout
        };
        let warning_timeout = watchdog.warning_timeout;
        let watchdog_enabled = watchdog.enabled && stall_enabled;
        let mut last_event_time = Instant::now();
        let mut warning_emitted = false;

        // Process streaming results with stall detection
        loop {
            let next_event = stream.next();

            // Use tokio::select! for stall detection and cancellation
            let result = if stall_enabled {
                // Choose deadline: warning (if not yet emitted) or abort
                let effective_timeout = if watchdog_enabled && !warning_emitted {
                    warning_timeout
                } else {
                    abort_timeout
                };
                let timeout_at = last_event_time + effective_timeout;
                let remaining = timeout_at.saturating_duration_since(Instant::now());

                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        // Cancelled during streaming — break out
                        break;
                    }
                    result = next_event => result,
                    _ = tokio::time::sleep(remaining) => {
                        // Tier 1: Warning phase — emit event and continue waiting
                        if watchdog_enabled && !warning_emitted {
                            let elapsed = last_event_time.elapsed().as_secs() as i64;
                            warn!(
                                turn_id,
                                elapsed_secs = elapsed,
                                "Stream watchdog warning: no event received"
                            );
                            self.emit(LoopEvent::StreamWatchdogWarning {
                                elapsed_secs: elapsed,
                            }).await;
                            warning_emitted = true;
                            // Continue the loop — the next iteration will use
                            // abort_timeout as the deadline.
                            continue;
                        }

                        // Tier 2: Abort phase — stream is stalled beyond recovery
                        let stall_timeout = abort_timeout;
                        self.emit(LoopEvent::StreamStallDetected {
                            turn_id: turn_id.to_string(),
                            timeout: stall_timeout,
                        }).await;

                        // Handle based on recovery strategy
                        let strategy = self.config.stall_detection.recovery;
                        match strategy {
                            cocode_protocol::StallRecovery::Abort => {}
                            cocode_protocol::StallRecovery::Retry => {
                                warn!(turn_id, timeout = ?stall_timeout, "Stream stalled, retrying");
                            }
                            cocode_protocol::StallRecovery::Fallback => {
                                self.try_model_fallback(&format!("Stream stalled for {stall_timeout:?}")).await;
                            }
                        }

                        return agent_loop_error::StreamStallSnafu {
                            timeout: format!("{stall_timeout:?}"),
                            strategy,
                        }.fail();
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        break;
                    }
                    result = next_event => result,
                }
            };

            // Process the result — consolidated match handles None (stream end)
            // and Err (provider error with fallback) in one place.
            let result = match result {
                None => break, // Stream ended
                Some(Err(e)) => {
                    let err_str = e.to_string();
                    // Only attempt model fallback for overload/rate-limit errors.
                    // Transient network errors should NOT trigger a model switch.
                    let classified = cocode_api::error::classify_by_message(&err_str);
                    if classified.is_overload_or_rate_limit() {
                        self.try_model_fallback(&err_str).await;
                    }
                    error!("Stream error from provider: {err_str}");
                    return agent_loop_error::StreamSnafu { message: err_str }.fail();
                }
                Some(Ok(r)) => r,
            };

            // Update stall timer on any event and reset watchdog warning
            last_event_time = Instant::now();
            warning_emitted = false;

            match result.result_type {
                QueryResultType::Assistant => {
                    // Emit text deltas for UI and process tool uses DURING streaming
                    for block in &result.content {
                        match block {
                            AssistantContentPart::Text(TextPart { text, .. })
                                if !text.is_empty() =>
                            {
                                self.emit(LoopEvent::TextDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: text.clone(),
                                })
                                .await;
                            }
                            AssistantContentPart::Reasoning(rp) if !rp.text.is_empty() => {
                                self.emit(LoopEvent::ThinkingDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: rp.text.clone(),
                                })
                                .await;
                            }
                            AssistantContentPart::ToolCall(ToolCallPart {
                                tool_call_id,
                                tool_name,
                                input,
                                ..
                            }) => {
                                // Start tool execution DURING streaming!
                                // Safe tools begin immediately; unsafe tools are queued.
                                let tool_call =
                                    ToolCall::new(tool_call_id, tool_name, input.clone());
                                executor.on_tool_complete(tool_call).await;
                            }
                            _ => {}
                        }
                    }
                    all_content.extend(result.content);

                    // Capture usage from non-streaming responses
                    if result.usage.is_some() {
                        final_usage = result.usage;
                    }
                    if let Some(fr) = result.finish_reason {
                        final_finish_reason = fr;
                    }
                }
                QueryResultType::Done => {
                    final_usage = result.usage;
                    if let Some(fr) = result.finish_reason {
                        final_finish_reason = fr;
                    }
                    break;
                }
                QueryResultType::Error => {
                    let msg = result.error.unwrap_or_else(|| "Unknown error".to_string());

                    // P26: Use structured error classification instead of raw string matching.
                    // The provider's is_retryable hint (from StreamError) is used as a fast
                    // path; otherwise fall back to heuristic message classification.
                    let classified = cocode_api::error::classify_by_message(&msg);
                    let is_retryable = result
                        .is_retryable
                        .unwrap_or_else(|| classified.is_retryable());

                    // Only attempt model fallback for overload/rate-limit errors.
                    // Transient network errors should NOT trigger a model switch.
                    if is_retryable && classified.is_overload_or_rate_limit() {
                        self.try_model_fallback(&msg).await;
                    }

                    error!("Stream error from provider: {msg}");
                    return agent_loop_error::StreamSnafu { message: msg }.fail();
                }
                QueryResultType::Retry | QueryResultType::Event => {
                    // Continue
                }
            }
        }

        Ok(CollectedResponse {
            content: all_content,
            usage: final_usage,
            finish_reason: final_finish_reason,
        })
    }

    /// Attempt model fallback for a retryable error.
    ///
    /// Checks whether fallback is configured and available. If so, emits
    /// `ModelFallbackStarted`, fires the notification hook, records the
    /// fallback in state, and increments the telemetry counter.
    ///
    /// Returns `true` if a fallback was initiated.
    async fn try_model_fallback(&mut self, reason: &str) -> bool {
        if !self.fallback_state.should_fallback(&self.fallback_config) {
            return false;
        }
        let Some(fallback_model) = self.fallback_state.next_model(&self.fallback_config) else {
            return false;
        };

        let from_model = self.fallback_state.current_model.clone();
        self.emit(LoopEvent::ModelFallbackStarted {
            from: from_model.clone(),
            to: fallback_model.clone(),
            reason: reason.to_string(),
        })
        .await;
        self.fire_notification_hook(
            "model_fallback",
            "Model fallback",
            &format!("Falling back from {from_model} to {fallback_model}"),
        )
        .await;
        self.fallback_state
            .record_fallback(fallback_model, reason.to_string());
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.model.fallback", 1, &[]);
        }
        true
    }

    /// Build messages and tool definitions for the API request.
    ///
    /// This extracts the message/tool building logic for use with `RequestBuilder`.
    /// Tool definitions are filtered per-model based on `ModelInfo` capabilities.
    ///
    /// # Arguments
    ///
    /// * `injected_messages` - Injected messages from system reminders
    /// * `model_info` - Model information for tool filtering
    pub(super) fn build_messages_and_tools(
        &self,
        injected_messages: &[InjectedMessage],
        model_info: &cocode_protocol::ModelInfo,
    ) -> (Vec<LanguageModelMessage>, Vec<LanguageModelTool>) {
        // Build system prompt (use custom prompt if set, otherwise generate from builder)
        let system_prompt = if let Some(ref custom) = self.custom_system_prompt {
            custom.clone()
        } else {
            let mut prompt = SystemPromptBuilder::build(&self.context);
            // Append system prompt suffix (critical_reminder) for highest authority
            if let Some(ref suffix) = self.system_prompt_suffix {
                prompt.push_str("\n\n");
                prompt.push_str(suffix);
            }
            prompt
        };

        // Get conversation messages
        let messages = self.message_history.messages_for_api();

        // Build messages with system, reminders, and conversation
        let mut all_messages = vec![LanguageModelMessage::system(&system_prompt)];

        // Inject system reminders as individual messages before the conversation
        // This supports both text reminders and multi-message tool_use/tool_result pairs
        for msg in injected_messages {
            all_messages.push(self.convert_injected_message(msg));
        }

        all_messages.extend(messages);

        // Get tool definitions with model-aware filtering
        let tools = self.select_tools_for_model(model_info);

        (all_messages, tools)
    }

    pub(super) fn select_tools_for_model(
        &self,
        model_info: &cocode_protocol::ModelInfo,
    ) -> Vec<LanguageModelTool> {
        select_tools_for_model(
            self.tool_registry.definitions_filtered(&self.features),
            model_info,
        )
    }

    /// Convert an injected message to an API message.
    pub(super) fn convert_injected_message(&self, msg: &InjectedMessage) -> LanguageModelMessage {
        match msg {
            InjectedMessage::UserText { content, .. } => {
                // Text reminders become simple user messages
                LanguageModelMessage::user_text(content.as_str())
            }
            InjectedMessage::AssistantBlocks { blocks, .. } => {
                // Assistant blocks (typically tool_use) become assistant messages
                let content_parts: Vec<AssistantContentPart> = blocks
                    .iter()
                    .map(Self::convert_injected_block_to_assistant)
                    .collect();
                LanguageModelMessage::assistant(content_parts)
            }
            InjectedMessage::UserBlocks { blocks, .. } => {
                // User blocks (typically tool_result) become user messages
                let content_parts: Vec<cocode_api::UserContentPart> = blocks
                    .iter()
                    .map(|block| match block {
                        InjectedBlock::Text(text) => {
                            cocode_api::UserContentPart::text(text.as_str())
                        }
                        InjectedBlock::ToolUse { .. } | InjectedBlock::ToolResult { .. } => {
                            // Tool-related blocks in user messages are serialized as text
                            cocode_api::UserContentPart::text(format!("{block:?}"))
                        }
                    })
                    .collect();
                LanguageModelMessage::user(content_parts)
            }
        }
    }

    /// Convert an injected block to an AssistantContentPart.
    pub(super) fn convert_injected_block_to_assistant(
        block: &InjectedBlock,
    ) -> AssistantContentPart {
        match block {
            InjectedBlock::Text(text) => AssistantContentPart::text(text.as_str()),
            InjectedBlock::ToolUse { id, name, input } => {
                AssistantContentPart::tool_call(id.as_str(), name.as_str(), input.clone())
            }
            InjectedBlock::ToolResult {
                tool_use_id,
                content,
            } => AssistantContentPart::ToolResult(cocode_api::ToolResultPart::new(
                tool_use_id.as_str(),
                "",
                cocode_api::ToolResultContent::text(content.as_str()),
            )),
        }
    }
}

/// Filter tool definitions based on model capabilities.
///
/// Applies shell_type, apply_patch variant, excluded_tools, and
/// experimental_supported_tools filters from `ModelInfo`.
pub(super) fn select_tools_for_model(
    mut defs: Vec<ToolDefinition>,
    model_info: &cocode_protocol::ModelInfo,
) -> Vec<LanguageModelTool> {
    use cocode_protocol::ApplyPatchToolType;
    use cocode_protocol::ConfigShellToolType;
    use cocode_tools::builtin::ApplyPatchTool;

    // 1. Handle shell_type
    match model_info.shell_type {
        Some(ConfigShellToolType::Disabled) => {
            use cocode_protocol::ToolName;
            defs.retain(|d| {
                let name = d.name.as_str();
                name != ToolName::Bash.as_str()
                    && name != ToolName::Shell.as_str()
                    && name != ToolName::TaskOutput.as_str()
                    && name != ToolName::TaskStop.as_str()
            });
        }
        Some(ConfigShellToolType::Shell) => {
            // Shell mode: remove Bash, keep shell tool
            defs.retain(|d| d.name != cocode_protocol::ToolName::Bash.as_str());
        }
        Some(ConfigShellToolType::ShellCommand) | None => {
            // ShellCommand (default): remove shell tool, keep Bash
            defs.retain(|d| d.name != cocode_protocol::ToolName::Shell.as_str());
        }
    }

    // 2. Handle apply_patch: remove registry default, add model-specific variant
    defs.retain(|d| d.name != cocode_protocol::ToolName::ApplyPatch.as_str());
    match model_info.apply_patch_tool_type {
        Some(ApplyPatchToolType::Function) => {
            defs.push(ApplyPatchTool::function_definition());
        }
        Some(ApplyPatchToolType::Freeform) => {
            defs.push(ApplyPatchTool::freeform_definition());
        }
        Some(ApplyPatchToolType::Shell) | None => {
            // Shell: prompt handles it; None: no apply_patch at all
        }
    }

    // 3. Handle excluded_tools (blacklist filter)
    if let Some(ref excluded) = model_info.excluded_tools
        && !excluded.is_empty()
    {
        defs.retain(|d| !excluded.contains(&d.name));
    }

    // 4. Handle experimental_supported_tools (whitelist filter)
    if let Some(ref supported) = model_info.experimental_supported_tools
        && !supported.is_empty()
    {
        defs.retain(|d| supported.contains(&d.name));
    }

    // Wrap ToolDefinition (LanguageModelFunctionTool) into LanguageModelTool::Function
    defs.into_iter().map(LanguageModelTool::function).collect()
}

/// Format a `LanguageModelMessage` as `[role]: text` for conversation summaries.
pub(super) fn format_language_model_message(m: &LanguageModelMessage) -> String {
    match m {
        LanguageModelMessage::System { content, .. } => format!("[system]: {content}"),
        LanguageModelMessage::User { content, .. } => {
            let text: String = content
                .iter()
                .filter_map(|part| match part {
                    cocode_api::UserContentPart::Text(tp) => Some(tp.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("[user]: {text}")
        }
        LanguageModelMessage::Assistant { content, .. } => {
            let text: String = content
                .iter()
                .filter_map(|part| match part {
                    AssistantContentPart::Text(tp) => Some(tp.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("[assistant]: {text}")
        }
        LanguageModelMessage::Tool { content, .. } => {
            let text: String = content
                .iter()
                .filter_map(|part| match part {
                    cocode_api::ToolContentPart::ToolResult(r) => Some(r.tool_name.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("[tool]: {text}")
        }
    }
}

#[cfg(test)]
#[path = "streaming.test.rs"]
mod tests;
