//! Agent loop driver - the core 18-step conversation loop.

use std::sync::Arc;
use std::time::Instant;

use cocode_api::{ApiClient, CollectedResponse, QueryResultType, StreamOptions};
use cocode_context::ConversationContext;
use cocode_hooks::HookRegistry;
use cocode_message::{MessageHistory, TrackedMessage, Turn};
use cocode_prompt::SystemPromptBuilder;
use cocode_protocol::{
    AutoCompactTracking, CompactConfig, HookEventType, LoopConfig, LoopEvent, QueryTracking,
    TokenUsage, ToolResultContent,
};
use cocode_tools::{ExecutorConfig, StreamingToolExecutor, ToolExecutionResult, ToolRegistry};
use hyper_sdk::{
    ContentBlock, FinishReason, GenerateRequest, Message, Model, ToolCall, ToolDefinition,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::compaction::{
    CompactionConfig, SessionMemorySummary, TaskStatusRestoration, ThresholdStatus,
    build_compact_instructions, try_session_memory_compact, write_session_memory,
};
use crate::fallback::{FallbackConfig, FallbackState};
use crate::result::LoopResult;

/// Offset from the context window limit to determine the blocking threshold.
/// If estimated tokens >= context_window - BLOCKING_LIMIT_OFFSET, the loop
/// refuses to call the API.
///
/// Note: This constant is kept for backwards compatibility and test assertions.
/// The actual blocking limit is now calculated by `CompactConfig::blocking_limit()`.
#[allow(dead_code)]
const BLOCKING_LIMIT_OFFSET: i32 = 13_000;

/// Maximum number of retry attempts for output-token exhaustion recovery.
const MAX_OUTPUT_TOKEN_RECOVERY: i32 = 3;

/// The main agent loop that drives multi-turn conversations with LLM providers.
///
/// `AgentLoop` manages streaming API calls, concurrent tool execution,
/// context compaction, model fallback, and event emission.
pub struct AgentLoop {
    // Provider / model
    api_client: ApiClient,
    model: Arc<dyn Model>,

    // Tool system
    tool_registry: Arc<ToolRegistry>,

    // Conversation state
    message_history: MessageHistory,
    context: ConversationContext,

    // Config
    config: LoopConfig,
    fallback_config: FallbackConfig,
    compaction_config: CompactionConfig,
    /// Protocol-level compact configuration with all threshold constants.
    compact_config: CompactConfig,

    // Hooks
    hooks: Arc<HookRegistry>,

    // Event channel
    event_tx: mpsc::Sender<LoopEvent>,

    // State tracking
    turn_number: i32,
    cancel_token: CancellationToken,
    fallback_state: FallbackState,
    total_input_tokens: i32,
    total_output_tokens: i32,
}

/// Builder for constructing an [`AgentLoop`].
pub struct AgentLoopBuilder {
    api_client: Option<ApiClient>,
    model: Option<Arc<dyn Model>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    message_history: Option<MessageHistory>,
    context: Option<ConversationContext>,
    config: LoopConfig,
    fallback_config: FallbackConfig,
    compaction_config: CompactionConfig,
    compact_config: CompactConfig,
    hooks: Option<Arc<HookRegistry>>,
    event_tx: Option<mpsc::Sender<LoopEvent>>,
    cancel_token: CancellationToken,
}

impl AgentLoopBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            api_client: None,
            model: None,
            tool_registry: None,
            message_history: None,
            context: None,
            config: LoopConfig::default(),
            fallback_config: FallbackConfig::default(),
            compaction_config: CompactionConfig::default(),
            compact_config: CompactConfig::default(),
            hooks: None,
            event_tx: None,
            cancel_token: CancellationToken::new(),
        }
    }

    pub fn api_client(mut self, client: ApiClient) -> Self {
        self.api_client = Some(client);
        self
    }

    /// Set the model for API calls.
    pub fn model(mut self, model: Arc<dyn Model>) -> Self {
        self.model = Some(model);
        self
    }

    pub fn tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn message_history(mut self, history: MessageHistory) -> Self {
        self.message_history = Some(history);
        self
    }

    pub fn context(mut self, ctx: ConversationContext) -> Self {
        self.context = Some(ctx);
        self
    }

    pub fn config(mut self, config: LoopConfig) -> Self {
        self.config = config;
        self
    }

    pub fn fallback_config(mut self, config: FallbackConfig) -> Self {
        self.fallback_config = config;
        self
    }

    pub fn compaction_config(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = config;
        self
    }

    /// Set the protocol-level compact configuration.
    pub fn compact_config(mut self, config: CompactConfig) -> Self {
        self.compact_config = config;
        self
    }

    pub fn hooks(mut self, hooks: Arc<HookRegistry>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn event_tx(mut self, tx: mpsc::Sender<LoopEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Build the [`AgentLoop`].
    ///
    /// # Panics
    /// Panics if required fields (`api_client`, `model`, `tool_registry`,
    /// `context`, `event_tx`) have not been set.
    pub fn build(self) -> AgentLoop {
        let model_name = self
            .config
            .fallback_model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        AgentLoop {
            api_client: self.api_client.expect("api_client is required"),
            model: self.model.expect("model is required"),
            tool_registry: self.tool_registry.expect("tool_registry is required"),
            message_history: self.message_history.unwrap_or_default(),
            context: self.context.expect("context is required"),
            config: self.config,
            fallback_config: self.fallback_config,
            compaction_config: self.compaction_config,
            compact_config: self.compact_config,
            hooks: self.hooks.unwrap_or_else(|| Arc::new(HookRegistry::new())),
            event_tx: self.event_tx.expect("event_tx is required"),
            turn_number: 0,
            cancel_token: self.cancel_token,
            fallback_state: FallbackState::new(model_name),
            total_input_tokens: 0,
            total_output_tokens: 0,
        }
    }
}

impl Default for AgentLoopBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentLoop {
    /// Create a builder for constructing an agent loop.
    pub fn builder() -> AgentLoopBuilder {
        AgentLoopBuilder::new()
    }

    /// Run the agent loop to completion, starting with an initial user message.
    ///
    /// Returns a `LoopResult` describing how the loop terminated along with
    /// aggregate token usage and the final response text.
    pub async fn run(&mut self, initial_message: &str) -> anyhow::Result<LoopResult> {
        info!(
            max_turns = ?self.config.max_turns,
            "Starting agent loop"
        );

        // Add user message to history
        let turn_id = uuid::Uuid::new_v4().to_string();
        let user_msg = TrackedMessage::user(initial_message, &turn_id);
        let turn = Turn::new(1, user_msg);
        self.message_history.add_turn(turn);

        // Initialize tracking
        let mut query_tracking = QueryTracking::new_root(uuid::Uuid::new_v4().to_string());
        let mut auto_compact_tracking = AutoCompactTracking::new();

        self.core_message_loop(&mut query_tracking, &mut auto_compact_tracking)
            .await
    }

    /// The 18-step core message loop.
    ///
    /// This implements the algorithm from `docs/arch/core-loop.md`:
    ///
    /// SETUP (1-6): emit events, query tracking, normalize, micro-compact,
    ///   auto-compact, init state.
    /// EXECUTION (7-10): resolve model, check token limit, stream with tools
    ///   + retry, record telemetry.
    /// POST-PROCESSING (11-18): check tool calls, execute queue, abort handling,
    ///   hooks, tracking, queued commands, max turns, recurse.
    async fn core_message_loop(
        &mut self,
        query_tracking: &mut QueryTracking,
        auto_compact_tracking: &mut AutoCompactTracking,
    ) -> anyhow::Result<LoopResult> {
        // ── STEP 1: Signal stream_request_start ──
        self.emit(LoopEvent::StreamRequestStart).await;

        // ── STEP 2: Setup query tracking ──
        query_tracking.depth += 1;
        let turn_id = uuid::Uuid::new_v4().to_string();

        // ── STEP 3: Normalize messages ──
        // Messages are already normalized through MessageHistory::messages_for_api().

        // ── STEP 4: Micro-compaction (PRE-API) ──
        if self.config.enable_micro_compaction {
            let (removed, tokens_saved) = self.micro_compact();
            if removed > 0 {
                self.emit(LoopEvent::MicroCompactionApplied {
                    removed_results: removed,
                    tokens_saved,
                })
                .await;
            }
        }

        // ── STEP 5: Auto-compaction check ──
        // Use ThresholdStatus for accurate threshold calculations
        let estimated_tokens = self.message_history.estimate_tokens();
        let context_window = self.context.environment.context_window;

        // Apply safety margin to token estimate
        let estimated_with_margin = self
            .compact_config
            .estimate_tokens_with_margin(estimated_tokens);

        let status =
            ThresholdStatus::calculate(estimated_with_margin, context_window, &self.compact_config);

        debug!(
            estimated_tokens,
            estimated_with_margin,
            context_window,
            percent_left = %format!("{:.1}%", status.percent_left * 100.0),
            status = status.status_description(),
            "Context usage check"
        );

        // Emit warning event if above warning but below auto-compact
        if status.is_above_warning_threshold && !status.is_above_auto_compact_threshold {
            let target = self.compact_config.auto_compact_target(context_window);
            let warning_threshold = self.compact_config.warning_threshold(target);
            self.emit(LoopEvent::ContextUsageWarning {
                estimated_tokens: estimated_with_margin,
                warning_threshold,
                percent_left: status.percent_left,
            })
            .await;
        }

        // Trigger auto-compact if above threshold (and auto-compact is enabled)
        if status.is_above_auto_compact_threshold && self.compact_config.is_auto_compact_enabled() {
            // Tier 1: Try session memory first (zero API cost)
            if let Some(summary) =
                try_session_memory_compact(&self.compaction_config.session_memory)
            {
                self.apply_session_memory_summary(summary, &turn_id, auto_compact_tracking)
                    .await?;
            } else {
                // Tier 2: Fall back to LLM-based compaction
                self.compact(auto_compact_tracking, &turn_id).await?;
            }
        }

        // ── STEP 6: Initialize state ──
        self.turn_number += 1;
        self.emit(LoopEvent::TurnStarted {
            turn_id: turn_id.clone(),
            turn_number: self.turn_number,
        })
        .await;

        // ── STEP 7: Resolve model (permissions checked externally) ──
        // In this implementation, model selection is handled by ApiClient.

        // ── STEP 8: Check blocking token limit ──
        // Use CompactConfig for blocking limit calculation
        let blocking_limit = self.compact_config.blocking_limit(context_window);
        if status.is_at_blocking_limit {
            warn!(
                estimated_tokens = estimated_with_margin,
                blocking_limit, "Context window exceeded blocking limit"
            );
            return Ok(LoopResult::error(
                self.turn_number,
                self.total_input_tokens,
                self.total_output_tokens,
                format!(
                    "Context window exceeded: {estimated_with_margin} tokens >= {blocking_limit} limit"
                ),
            ));
        }

        // Create executor for this turn BEFORE streaming starts.
        // This enables tool execution to begin DURING streaming.
        let executor_config = ExecutorConfig {
            session_id: query_tracking.chain_id.clone(),
            permission_mode: self.config.permission_mode,
            ..ExecutorConfig::default()
        };
        let executor = StreamingToolExecutor::new(
            self.tool_registry.clone(),
            executor_config,
            Some(self.event_tx.clone()),
        )
        .with_cancel_token(self.cancel_token.clone())
        .with_hooks(self.hooks.clone());

        // ── STEP 9: Main API streaming loop with retry ──
        let mut output_recovery_attempts = 0;
        let collected = loop {
            if self.cancel_token.is_cancelled() {
                return Ok(LoopResult::interrupted(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }

            match self.stream_with_tools(&turn_id, &executor).await {
                Ok(collected) => break collected,
                Err(e) => {
                    // Check if retriable (output token exhaustion)
                    output_recovery_attempts += 1;
                    if output_recovery_attempts >= MAX_OUTPUT_TOKEN_RECOVERY {
                        return Err(e);
                    }
                    self.emit(LoopEvent::Retry {
                        attempt: output_recovery_attempts,
                        max_attempts: MAX_OUTPUT_TOKEN_RECOVERY,
                        delay_ms: 0,
                    })
                    .await;
                    continue;
                }
            }
        };

        // ── STEP 10: Record API call info ──
        if let Some(usage) = &collected.usage {
            self.total_input_tokens += usage.input_tokens as i32;
            self.total_output_tokens += usage.output_tokens as i32;
        }

        let usage = collected.usage.clone().unwrap_or_default();
        self.emit(LoopEvent::StreamRequestEnd {
            usage: usage.clone(),
        })
        .await;

        // Extract text from response
        let response_text: String = collected
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        // Check for tool calls
        let has_tool_calls = collected
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

        // Add assistant message to history
        if let Some(turn) = self.message_history.current_turn_mut() {
            let assistant_msg = TrackedMessage::assistant(&response_text, &turn_id, None);
            turn.set_assistant_message(assistant_msg);
            turn.update_usage(usage.clone());
        }

        // ── STEP 11: Check for tool calls ──
        // ── STEP 12: Execute tool queue ──
        // Tool execution already started DURING streaming for safe tools.
        // Now we execute pending unsafe tools and collect all results.
        if has_tool_calls {
            let tool_calls: Vec<_> = collected
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse {
                        id, name, input, ..
                    } => Some(hyper_sdk::ToolCall::new(id, name, input.clone())),
                    _ => None,
                })
                .collect();

            // Execute pending unsafe tools (safe tools already started during streaming)
            executor.execute_pending_unsafe().await;

            // Drain all results (both from streaming and unsafe execution)
            let results = executor.drain().await;

            // ── STEP 13: Handle abort after tool execution ──
            // Check if cancelled during tool execution
            if self.cancel_token.is_cancelled() {
                return Ok(LoopResult::interrupted(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }

            // Add tool results to history - use proper tool_result messages
            self.add_tool_results_to_history(&results, &tool_calls);
        }

        // ── STEP 14: Check for hook stop ──
        // Hook execution is deferred to a future session.

        // ── STEP 15: Update auto-compact tracking ──
        auto_compact_tracking.turn_counter += 1;

        // ── STEP 16: Process queued commands and attachments ──
        // Deferred to future sessions.

        // ── STEP 17: Check max turns limit ──
        if let Some(max) = self.config.max_turns {
            if self.turn_number >= max {
                self.emit(LoopEvent::MaxTurnsReached).await;
                return Ok(LoopResult::max_turns_reached(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }
        }

        // Emit turn completed
        self.emit(LoopEvent::TurnCompleted {
            turn_id: turn_id.clone(),
            usage,
        })
        .await;

        // ── STEP 18: Recurse or return ──
        match collected.finish_reason {
            FinishReason::Stop => Ok(LoopResult::completed(
                self.turn_number,
                self.total_input_tokens,
                self.total_output_tokens,
                response_text,
                collected.content,
            )),
            FinishReason::ToolCalls => {
                // Recursive call for next turn (boxed to avoid infinite future size)
                Box::pin(self.core_message_loop(query_tracking, auto_compact_tracking)).await
            }
            FinishReason::MaxTokens => {
                // Output token recovery already handled in step 9
                Ok(LoopResult::completed(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                    response_text,
                    collected.content,
                ))
            }
            other => {
                warn!(?other, "Unexpected finish reason");
                Ok(LoopResult::completed(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                    response_text,
                    collected.content,
                ))
            }
        }
    }

    /// Stream an API request and collect the response.
    ///
    /// Uses `ApiClient::stream_request()` with tool definitions from the
    /// registry. Includes stall detection based on `stall_detection` config.
    ///
    /// **Key feature**: Tool execution starts DURING streaming. When a ToolUse
    /// block is received, safe tools begin execution immediately via the
    /// executor. This enables concurrent tool execution while the LLM continues
    /// generating output.
    async fn stream_with_tools(
        &mut self,
        turn_id: &str,
        executor: &StreamingToolExecutor,
    ) -> anyhow::Result<CollectedResponse> {
        let request = self.build_request()?;

        debug!(turn_id, "Sending API request");

        let mut stream = self
            .api_client
            .stream_request(&*self.model, request, StreamOptions::streaming())
            .await
            .map_err(|e| anyhow::anyhow!("API stream error: {e}"))?;

        let mut all_content: Vec<ContentBlock> = Vec::new();
        let mut final_usage: Option<TokenUsage> = None;
        let mut final_finish_reason = FinishReason::Stop;

        // Stall detection configuration
        let stall_timeout = self.config.stall_detection.stall_timeout;
        let stall_enabled = self.config.stall_detection.enabled;
        let mut last_event_time = Instant::now();

        // Process streaming results with stall detection
        loop {
            let next_event = stream.next();

            // Use tokio::select! for stall detection if enabled
            let result = if stall_enabled {
                let timeout_at = last_event_time + stall_timeout;
                let remaining = timeout_at.saturating_duration_since(Instant::now());

                tokio::select! {
                    biased;
                    result = next_event => result,
                    _ = tokio::time::sleep(remaining) => {
                        // Stream stall detected
                        self.emit(LoopEvent::StreamStallDetected {
                            turn_id: turn_id.to_string(),
                            timeout: stall_timeout,
                        }).await;

                        // Handle based on recovery strategy
                        match self.config.stall_detection.recovery {
                            cocode_protocol::StallRecovery::Abort => {
                                return Err(anyhow::anyhow!(
                                    "Stream stalled for {:?}, aborting", stall_timeout
                                ));
                            }
                            cocode_protocol::StallRecovery::Retry => {
                                warn!(turn_id, timeout = ?stall_timeout, "Stream stalled, retrying");
                                return Err(anyhow::anyhow!(
                                    "Stream stalled for {:?}, retry requested", stall_timeout
                                ));
                            }
                            cocode_protocol::StallRecovery::Fallback => {
                                // Attempt model fallback
                                if self.fallback_state.should_fallback(&self.fallback_config) {
                                    if let Some(fallback_model) = self.fallback_state.next_model(&self.fallback_config) {
                                        self.emit(LoopEvent::ModelFallbackStarted {
                                            from: self.fallback_state.current_model.clone(),
                                            to: fallback_model.clone(),
                                            reason: format!("Stream stalled for {:?}", stall_timeout),
                                        }).await;
                                        self.fallback_state.record_fallback(
                                            fallback_model,
                                            format!("Stream stalled for {:?}", stall_timeout),
                                        );
                                    }
                                }
                                return Err(anyhow::anyhow!(
                                    "Stream stalled for {:?}, fallback triggered", stall_timeout
                                ));
                            }
                        }
                    }
                }
            } else {
                next_event.await
            };

            // Process the result
            let Some(result) = result else {
                break; // Stream ended
            };

            let result = result.map_err(|e| {
                // Check if this is an overload error for fallback handling
                let err_str = e.to_string();
                if err_str.contains("overload") || err_str.contains("rate_limit") {
                    if self.fallback_state.should_fallback(&self.fallback_config) {
                        if let Some(fallback_model) =
                            self.fallback_state.next_model(&self.fallback_config)
                        {
                            // Note: We can't emit async events here, but we record the fallback
                            self.fallback_state
                                .record_fallback(fallback_model, format!("API error: {}", err_str));
                        }
                    }
                }
                anyhow::anyhow!("Stream error: {e}")
            })?;

            // Update stall timer on any event
            last_event_time = Instant::now();

            match result.result_type {
                QueryResultType::Assistant => {
                    // Emit text deltas for UI and process tool uses DURING streaming
                    for block in &result.content {
                        match block {
                            ContentBlock::Text { text } if !text.is_empty() => {
                                self.emit(LoopEvent::TextDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: text.clone(),
                                })
                                .await;
                            }
                            ContentBlock::Thinking { content, .. } if !content.is_empty() => {
                                self.emit(LoopEvent::ThinkingDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: content.clone(),
                                })
                                .await;
                            }
                            ContentBlock::ToolUse {
                                id, name, input, ..
                            } => {
                                // Start tool execution DURING streaming!
                                // Safe tools begin immediately; unsafe tools are queued.
                                let tool_call = ToolCall::new(id, name, input.clone());
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

                    // Check for overload errors and handle fallback
                    if msg.contains("overload") || msg.contains("rate_limit") {
                        if self.fallback_state.should_fallback(&self.fallback_config) {
                            if let Some(fallback_model) =
                                self.fallback_state.next_model(&self.fallback_config)
                            {
                                self.emit(LoopEvent::ModelFallbackStarted {
                                    from: self.fallback_state.current_model.clone(),
                                    to: fallback_model.clone(),
                                    reason: msg.clone(),
                                })
                                .await;
                                self.fallback_state
                                    .record_fallback(fallback_model, msg.clone());
                            }
                        }
                    }

                    return Err(anyhow::anyhow!("Stream error: {msg}"));
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

    /// Build the API request from current context, history, and tools.
    fn build_request(&self) -> anyhow::Result<GenerateRequest> {
        // Build system prompt
        let system_prompt = SystemPromptBuilder::build(&self.context);

        // Get conversation messages
        let messages = self.message_history.messages_for_api();

        // Build request with system, messages, and tools
        let mut all_messages = vec![Message::system(&system_prompt)];
        all_messages.extend(messages);

        // Get tool definitions
        let tools: Vec<ToolDefinition> = self.tool_registry.all_definitions();

        let mut request = GenerateRequest::new(all_messages);

        if !tools.is_empty() {
            request.tools = Some(tools);
        }

        if let Some(max_tokens) = self.config.max_tokens {
            request.max_tokens = Some(max_tokens);
        }

        Ok(request)
    }

    /// Micro-compaction: remove old tool results to save tokens (no LLM call).
    ///
    /// Uses `ThresholdStatus` to determine if micro-compaction is needed based on
    /// current context usage relative to the warning threshold.
    ///
    /// Returns a tuple of (removed_count, tokens_saved).
    fn micro_compact(&mut self) -> (i32, i32) {
        // Check if micro-compact is enabled
        if !self.compact_config.is_micro_compact_enabled() {
            return (0, 0);
        }

        let tokens_before = self.message_history.estimate_tokens();
        let context_window = self.context.environment.context_window;

        // Use ThresholdStatus to check if we're above warning threshold
        let status =
            ThresholdStatus::calculate(tokens_before, context_window, &self.compact_config);

        if !status.is_above_warning_threshold {
            debug!(
                tokens_before,
                status = status.status_description(),
                "Below warning threshold, skipping micro-compact"
            );
            return (0, 0);
        }

        // Apply micro-compaction using configured recent_tool_results_to_keep
        let keep_count = self.compact_config.recent_tool_results_to_keep;
        let removed = self.message_history.micro_compact(keep_count);

        // Calculate tokens saved
        let tokens_after = self.message_history.estimate_tokens();
        let tokens_saved = tokens_before - tokens_after;

        debug!(
            removed,
            tokens_before, tokens_after, tokens_saved, "Micro-compaction complete"
        );

        (removed, tokens_saved)
    }

    /// Run auto-compaction (LLM-based summarization).
    ///
    /// Uses the 9-section compact instructions from `build_compact_instructions()`
    /// to generate a comprehensive conversation summary.
    ///
    /// Before compaction begins, PreCompact hooks are executed. If any hook
    /// returns `Reject`, compaction is skipped and the rejection is logged.
    async fn compact(
        &mut self,
        tracking: &mut AutoCompactTracking,
        turn_id: &str,
    ) -> anyhow::Result<()> {
        // Execute PreCompact hooks before starting compaction
        let hook_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::PreCompact,
            turn_id.to_string(),
            self.context.environment.cwd.clone(),
        );

        let outcomes = self.hooks.execute(&hook_ctx).await;

        // Check if any hook rejected compaction
        for outcome in &outcomes {
            // Emit HookExecuted event for each hook
            self.emit(LoopEvent::HookExecuted {
                hook_type: HookEventType::PreCompact,
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                info!(
                    hook_name = %outcome.hook_name,
                    reason = %reason,
                    "Compaction skipped by hook"
                );
                self.emit(LoopEvent::CompactionSkippedByHook {
                    hook_name: outcome.hook_name.clone(),
                    reason: reason.clone(),
                })
                .await;
                return Ok(());
            }
        }

        self.emit(LoopEvent::CompactionStarted).await;

        // Estimate tokens before compaction
        let tokens_before = self.message_history.estimate_tokens();

        // Build summarization prompt from conversation text
        let messages = self.message_history.messages_for_api();
        let conversation_text: String = messages
            .iter()
            .map(|m| format!("{:?}", m))
            .collect::<Vec<_>>()
            .join("\n");

        // Use the 9-section compact instructions
        let max_output_tokens = self.compact_config.max_compact_output_tokens;
        let system_prompt = build_compact_instructions(max_output_tokens);

        // Fallback to legacy prompt builder if available
        let (_, user_prompt) = SystemPromptBuilder::build_summarization(&conversation_text, None);

        // Use the API client to get a summary with retry mechanism
        let max_retries = self.compact_config.max_summary_retries;
        let mut attempt = 0;
        let mut last_error = String::new();

        let summary_text = loop {
            attempt += 1;

            // Build request for each attempt
            let summary_messages =
                vec![Message::system(&system_prompt), Message::user(&user_prompt)];
            let mut summary_request = GenerateRequest::new(summary_messages);
            summary_request.max_tokens = Some(max_output_tokens);

            match self
                .api_client
                .generate(&*self.model, summary_request)
                .await
            {
                Ok(response) => {
                    // Extract summary text
                    let text: String = response
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();

                    if text.is_empty() {
                        last_error = "Empty summary produced".to_string();
                        if attempt <= max_retries {
                            // Exponential backoff: 1s, 2s, 4s, ...
                            let delay_ms = 1000 * (1 << (attempt - 1));
                            self.emit(LoopEvent::CompactionRetry {
                                attempt,
                                max_attempts: max_retries + 1,
                                delay_ms,
                                reason: last_error.clone(),
                            })
                            .await;
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64))
                                .await;
                            continue;
                        }
                    } else {
                        break text;
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                    if attempt <= max_retries {
                        // Exponential backoff: 1s, 2s, 4s, ...
                        let delay_ms = 1000 * (1 << (attempt - 1));
                        warn!(
                            attempt,
                            max_retries,
                            error = %last_error,
                            delay_ms,
                            "Compaction API call failed, retrying"
                        );
                        self.emit(LoopEvent::CompactionRetry {
                            attempt,
                            max_attempts: max_retries + 1,
                            delay_ms,
                            reason: last_error.clone(),
                        })
                        .await;
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64))
                            .await;
                        continue;
                    }
                }
            }

            // All retries exhausted
            warn!(
                attempts = attempt,
                error = %last_error,
                "Compaction failed after all retries"
            );
            self.emit(LoopEvent::CompactionFailed {
                attempts: attempt,
                error: last_error,
            })
            .await;
            return Ok(());
        };

        // Extract task status from tool calls before compaction
        let tool_calls: Vec<(String, serde_json::Value)> = self
            .message_history
            .turns()
            .iter()
            .flat_map(|turn| {
                turn.tool_calls
                    .iter()
                    .map(|tc| (tc.name.clone(), tc.input.clone()))
            })
            .collect();

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);

        // Build final summary with task status
        let final_summary = if task_status.tasks.is_empty() {
            summary_text
        } else {
            let tasks_section = task_status
                .tasks
                .iter()
                .map(|t| {
                    let owner = t.owner.as_deref().unwrap_or("unassigned");
                    format!("- [{}] {}: {} ({})", t.status, t.id, t.subject, owner)
                })
                .collect::<Vec<_>>()
                .join("\n");

            format!("{summary_text}\n\n<task_status>\n{tasks_section}\n</task_status>")
        };

        // Apply compaction - keep recent turns
        let keep_turns = self.compaction_config.min_messages_to_keep;
        let tokens_after = self.message_history.estimate_tokens();
        let tokens_saved = (tokens_before - tokens_after).max(0);

        self.message_history.apply_compaction(
            final_summary.clone(),
            keep_turns,
            turn_id,
            tokens_saved,
        );

        // Update tracking
        tracking.mark_compacted(turn_id, self.turn_number);

        let estimated = tokens_after;
        self.emit(LoopEvent::CompactionCompleted {
            removed_messages: 0, // Tracked by MessageHistory
            summary_tokens: estimated,
        })
        .await;

        // Save to session memory for future Tier 1 compaction
        if self.compaction_config.session_memory.enabled {
            if let Some(ref path) = self.compaction_config.session_memory.summary_path {
                let summary_content = final_summary;
                let turn_id_owned = turn_id.to_string();
                let path_owned = path.clone();

                // Spawn background task to write session memory
                tokio::spawn(async move {
                    if let Err(e) =
                        write_session_memory(&path_owned, &summary_content, &turn_id_owned).await
                    {
                        tracing::warn!(
                            error = %e,
                            path = ?path_owned,
                            "Failed to write session memory"
                        );
                    } else {
                        tracing::debug!(
                            path = ?path_owned,
                            "Session memory saved for future Tier 1 compaction"
                        );
                    }
                });
            }
        }

        Ok(())
    }

    /// Apply a cached session memory summary (Tier 1 compaction).
    ///
    /// This is the zero-cost compaction path that uses a previously saved summary
    /// instead of making an LLM API call. The summary is stored in the session memory
    /// file and can be reused across conversation continuations.
    ///
    /// # Arguments
    /// * `summary` - The cached session memory summary
    /// * `turn_id` - ID of the current turn
    /// * `tracking` - Auto-compact tracking state
    async fn apply_session_memory_summary(
        &mut self,
        summary: SessionMemorySummary,
        turn_id: &str,
        tracking: &mut AutoCompactTracking,
    ) -> anyhow::Result<()> {
        let tokens_before = self.message_history.estimate_tokens();

        info!(
            summary_tokens = summary.token_estimate,
            last_id = ?summary.last_summarized_id,
            "Applying session memory summary (Tier 1)"
        );

        // Apply the cached summary
        let keep_turns = self.compaction_config.min_messages_to_keep;
        let tokens_saved = (tokens_before - summary.token_estimate).max(0);

        self.message_history.apply_compaction(
            summary.summary.clone(),
            keep_turns,
            turn_id,
            tokens_saved,
        );

        // Update tracking
        tracking.mark_compacted(turn_id, self.turn_number);

        // Emit event
        self.emit(LoopEvent::SessionMemoryCompactApplied {
            saved_tokens: tokens_saved,
            summary_tokens: summary.token_estimate,
        })
        .await;

        Ok(())
    }

    /// Add tool results to the message history.
    ///
    /// This creates proper tool_result messages that link back to the tool_use
    /// blocks via their call_id. The results are added to the current turn
    /// for tracking, and a new turn with tool result messages is created
    /// for the next API call.
    fn add_tool_results_to_history(
        &mut self,
        results: &[ToolExecutionResult],
        _tool_calls: &[ToolCall],
    ) {
        if results.is_empty() {
            return;
        }

        // Add tool results to current turn for tracking
        for result in results {
            let (output, is_error) = match &result.result {
                Ok(output) => (output.content.clone(), output.is_error),
                Err(e) => (ToolResultContent::Text(e.to_string()), true),
            };
            self.message_history
                .add_tool_result(&result.call_id, &result.name, output, is_error);
        }

        // Create a new turn with tool result messages for the next API call
        // Using TrackedMessage::tool_result for proper role assignment
        let next_turn_id = uuid::Uuid::new_v4().to_string();

        // Build tool result content blocks for the user message
        // (Some providers expect tool results as user messages with special content)
        let tool_results_text: String = results
            .iter()
            .map(|r| {
                let output_text = match &r.result {
                    Ok(output) => match &output.content {
                        ToolResultContent::Text(t) => t.clone(),
                        ToolResultContent::Structured(v) => v.to_string(),
                    },
                    Err(e) => format!("Tool error: {e}"),
                };
                format!(
                    "<tool_result tool_use_id=\"{}\" name=\"{}\">\n{}\n</tool_result>",
                    r.call_id, r.name, output_text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Create a user message containing the tool results
        // This will be normalized by MessageHistory::messages_for_api() to the correct format
        let user_msg = TrackedMessage::user(&tool_results_text, &next_turn_id);
        let turn = Turn::new(self.turn_number + 1, user_msg);
        self.message_history.add_turn(turn);
    }

    /// Emit a loop event to the event channel.
    async fn emit(&self, event: LoopEvent) {
        if let Err(e) = self.event_tx.send(event).await {
            debug!("Failed to send loop event: {e}");
        }
    }

    /// Returns the current turn number.
    pub fn turn_number(&self) -> i32 {
        self.turn_number
    }

    /// Returns the total input tokens consumed.
    pub fn total_input_tokens(&self) -> i32 {
        self.total_input_tokens
    }

    /// Returns the total output tokens generated.
    pub fn total_output_tokens(&self) -> i32 {
        self.total_output_tokens
    }

    /// Returns a reference to the message history.
    pub fn message_history(&self) -> &MessageHistory {
        &self.message_history
    }

    /// Returns a reference to the loop configuration.
    pub fn config(&self) -> &LoopConfig {
        &self.config
    }

    /// Returns the cancellation token.
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel_token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::StopReason;
    use cocode_context::EnvironmentInfo;

    fn test_env() -> EnvironmentInfo {
        EnvironmentInfo::builder()
            .cwd("/tmp/test")
            .model("test-model")
            .context_window(200000)
            .output_token_limit(16384)
            .build()
            .unwrap()
    }

    fn test_context() -> ConversationContext {
        ConversationContext::builder()
            .environment(test_env())
            .build()
            .unwrap()
    }

    #[test]
    fn test_default_config() {
        let config = LoopConfig::default();
        assert_eq!(config.max_turns, None);
        assert!((config.auto_compact_threshold - 0.8).abs() < f32::EPSILON);
        assert!(!config.enable_streaming_tools);
        assert!(!config.enable_micro_compaction);
    }

    #[test]
    fn test_builder_defaults() {
        let builder = AgentLoopBuilder::new();
        assert!(builder.api_client.is_none());
        assert!(builder.tool_registry.is_none());
        assert!(builder.context.is_none());
        assert!(builder.event_tx.is_none());
    }

    #[test]
    fn test_loop_result_constructors() {
        let completed = LoopResult::completed(5, 1000, 500, "text".to_string(), vec![]);
        assert_eq!(completed.turns_completed, 5);
        assert!(matches!(completed.stop_reason, StopReason::ModelStopSignal));

        let max = LoopResult::max_turns_reached(10, 2000, 1000);
        assert!(matches!(max.stop_reason, StopReason::MaxTurnsReached));

        let interrupted = LoopResult::interrupted(3, 500, 200);
        assert!(matches!(
            interrupted.stop_reason,
            StopReason::UserInterrupted
        ));

        let err = LoopResult::error(1, 100, 50, "boom".to_string());
        assert!(matches!(err.stop_reason, StopReason::Error { .. }));
    }

    #[test]
    fn test_constants() {
        assert_eq!(BLOCKING_LIMIT_OFFSET, 13_000);
        assert_eq!(MAX_OUTPUT_TOKEN_RECOVERY, 3);
    }

    #[test]
    fn test_micro_compact_empty_history() {
        // Cannot construct a full AgentLoop without a model, but we can test
        // the candidate finder directly.
        let messages: Vec<serde_json::Value> = vec![];
        let candidates = crate::compaction::micro_compact_candidates(&messages);
        assert!(candidates.is_empty());
    }
}
