//! Agent executor for running subagent conversations.

mod complete_task;
mod context;
mod model_bridge;
mod model_client_bridge;
mod subagent_tools;

pub use complete_task::create_complete_task_tool;
pub use context::SubagentContext;
pub use model_bridge::SharedModelBridge;
pub use model_bridge::StubModelBridge;
pub use model_bridge::SubagentModelBridge;
pub use model_bridge::TurnEventReceiver;
pub use model_client_bridge::ModelClientBridge;
pub use subagent_tools::execute_tool;
pub use subagent_tools::get_all_subagent_tool_specs;
pub use subagent_tools::get_tool_spec_by_name;

use super::SubagentErr;
use super::definition::AgentDefinition;
use super::events::SubagentActivityEvent;
use super::events::SubagentEventType;
use super::events_bridge::SubagentEventBridge;
use super::transcript::MessageRole;
use super::transcript::TranscriptMessage;
use super::transcript::TranscriptStore;
use super::transcript::TranscriptToolCall;
use super::transcript::TranscriptToolResult;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Status of a completed subagent execution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    /// Successfully completed (complete_task called).
    Goal,
    /// Execution timed out.
    Timeout,
    /// Maximum turns exceeded.
    MaxTurns,
    /// Execution was cancelled.
    Aborted,
    /// Execution error occurred.
    Error,
}

/// Result of a subagent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    /// Final status.
    pub status: SubagentStatus,
    /// Result content (from complete_task or error message).
    pub result: String,
    /// Number of conversation turns used.
    pub turns_used: i32,
    /// Total execution duration.
    pub duration: Duration,
    /// Agent instance ID.
    pub agent_id: String,
    /// Total number of tool calls made.
    pub total_tool_use_count: i32,
    /// Total execution time in milliseconds.
    pub total_duration_ms: i64,
    /// Total tokens used (input + output).
    pub total_tokens: i32,
    /// Detailed token usage.
    pub usage: Option<TokenUsage>,
}

/// Token usage details.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<i32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<i32>,
}

/// Metrics from a single turn execution.
#[derive(Debug, Default)]
struct TurnMetrics {
    tool_use_count: i32,
    input_tokens: i32,
    output_tokens: i32,
}

/// Result of a single turn.
enum TurnResult {
    /// Continue execution.
    Continue,
    /// Agent called complete_task with output.
    Completed(String),
}

/// Executor for running a subagent conversation.
pub struct AgentExecutor {
    /// Execution context.
    pub context: SubagentContext,
    /// Event bridge for sending activity events.
    event_bridge: Option<SubagentEventBridge>,
    /// Model bridge for LLM calls.
    model_bridge: Option<SharedModelBridge>,
}

impl AgentExecutor {
    /// Create a new executor.
    pub fn new(context: SubagentContext) -> Self {
        Self {
            context,
            event_bridge: None,
            model_bridge: None,
        }
    }

    /// Set the event bridge for activity events.
    pub fn with_event_bridge(mut self, bridge: SubagentEventBridge) -> Self {
        self.event_bridge = Some(bridge);
        self
    }

    /// Set the model bridge for LLM calls.
    pub fn with_model_bridge(mut self, bridge: SharedModelBridge) -> Self {
        self.model_bridge = Some(bridge);
        self
    }

    /// Run the agent with optional resume support.
    pub async fn run_with_resume(
        &self,
        prompt: String,
        resume_agent_id: Option<&str>,
        transcript_store: &TranscriptStore,
    ) -> Result<SubagentResult, SubagentErr> {
        let start = Instant::now();
        let mut turns = 0;
        let mut total_tool_use_count = 0;
        let mut total_input_tokens = 0;
        let mut total_output_tokens = 0;

        // Initialize or resume transcript
        // When fork_context is true, start fresh without inheriting previous context
        let initial_messages = if self.context.definition.fork_context {
            // Fork context: start fresh, don't inherit parent history
            tracing::info!(
                "Agent {} has fork_context=true, starting fresh without previous messages",
                self.context.agent_id
            );
            Vec::new()
        } else if let Some(prev_id) = resume_agent_id {
            // Load previous transcript for resume
            let prev_messages = transcript_store
                .load_transcript(prev_id)
                .ok_or_else(|| SubagentErr::TranscriptNotFound(prev_id.to_string()))?;

            tracing::info!(
                "Resuming agent from {} with {} previous messages",
                prev_id,
                prev_messages.len()
            );

            prev_messages
        } else {
            Vec::new()
        };

        // Initialize transcript for this execution (new transcript for current agent)
        transcript_store.init_transcript(
            self.context.agent_id.clone(),
            self.context.definition.agent_type.clone(),
        );

        // If resuming, copy previous messages to new transcript for context continuity
        if !initial_messages.is_empty() {
            tracing::debug!(
                "Copying {} previous messages to transcript {}",
                initial_messages.len(),
                self.context.agent_id
            );
            for msg in &initial_messages {
                transcript_store.record_message(&self.context.agent_id, msg.clone());
            }
        }

        // Build initial conversation history
        let mut conversation_history: Vec<codex_protocol::models::ResponseItem> = Vec::new();

        // Add system prompt if configured
        if let Some(system_prompt) = &self.context.definition.prompt_config.system_prompt {
            conversation_history.push(codex_protocol::models::ResponseItem::Message {
                id: None,
                role: "system".to_string(),
                content: vec![codex_protocol::models::ContentItem::InputText {
                    text: system_prompt.clone(),
                }],
            });
        }

        // Add critical system reminder as user message (extra safety enforcement)
        if let Some(reminder) = &self.context.definition.critical_system_reminder {
            conversation_history.push(codex_protocol::models::ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![codex_protocol::models::ContentItem::InputText {
                    text: reminder.clone(),
                }],
            });
        }

        // Add resumed messages if any (convert from TranscriptMessage to ResponseItem)
        for msg in &initial_messages {
            let role = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool", // Tool results
            };
            conversation_history.push(codex_protocol::models::ResponseItem::Message {
                id: None,
                role: role.to_string(),
                content: vec![codex_protocol::models::ContentItem::InputText {
                    text: msg.content.clone(),
                }],
            });
        }

        // Add the user prompt
        conversation_history.push(codex_protocol::models::ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![codex_protocol::models::ContentItem::InputText {
                text: prompt.clone(),
            }],
        });

        // Send started event
        self.send_event(SubagentActivityEvent::started(
            &self.context.agent_id,
            &self.context.definition.agent_type,
            &prompt,
        ))
        .await;

        let max_turns = self.context.definition.run_config.max_turns;
        let max_time =
            Duration::from_secs(self.context.definition.run_config.max_time_seconds as u64);

        // Main execution loop
        loop {
            // Check cancellation
            if self.context.cancellation_token.is_cancelled() {
                self.send_event(SubagentActivityEvent::error(
                    &self.context.agent_id,
                    &self.context.definition.agent_type,
                    "Execution cancelled",
                ))
                .await;
                return Ok(self.build_result(
                    SubagentStatus::Aborted,
                    "Cancelled".to_string(),
                    turns,
                    start,
                    total_tool_use_count,
                    total_input_tokens,
                    total_output_tokens,
                ));
            }

            // Check turn limit
            if turns >= max_turns {
                // Try grace period with full conversation history
                if let Some(result) = self
                    .execute_grace_period(SubagentStatus::MaxTurns, &conversation_history)
                    .await
                {
                    return Ok(self.build_result(
                        SubagentStatus::Goal,
                        result,
                        turns,
                        start,
                        total_tool_use_count,
                        total_input_tokens,
                        total_output_tokens,
                    ));
                }
                return Ok(self.build_result(
                    SubagentStatus::MaxTurns,
                    format!("Max turns ({max_turns}) exceeded"),
                    turns,
                    start,
                    total_tool_use_count,
                    total_input_tokens,
                    total_output_tokens,
                ));
            }

            // Check time limit
            if start.elapsed() > max_time {
                // Try grace period with full conversation history
                if let Some(result) = self
                    .execute_grace_period(SubagentStatus::Timeout, &conversation_history)
                    .await
                {
                    return Ok(self.build_result(
                        SubagentStatus::Goal,
                        result,
                        turns,
                        start,
                        total_tool_use_count,
                        total_input_tokens,
                        total_output_tokens,
                    ));
                }
                return Ok(self.build_result(
                    SubagentStatus::Timeout,
                    format!("Timeout after {} seconds", max_time.as_secs()),
                    turns,
                    start,
                    total_tool_use_count,
                    total_input_tokens,
                    total_output_tokens,
                ));
            }

            // Execute turn
            turns += 1;
            self.send_event(
                SubagentActivityEvent::new(
                    &self.context.agent_id,
                    &self.context.definition.agent_type,
                    SubagentEventType::TurnStart,
                )
                .with_data("turn_number", turns),
            )
            .await;

            // Execute turn with model bridge (falls back to stub if not configured)
            let (turn_result, metrics, turn_items) =
                self.execute_turn(&conversation_history).await?;

            // Accumulate metrics
            total_tool_use_count += metrics.tool_use_count;
            total_input_tokens += metrics.input_tokens;
            total_output_tokens += metrics.output_tokens;

            // Accumulate assistant responses to conversation history for next turn
            for item in &turn_items {
                conversation_history.push(item.clone());
            }

            // Record to transcript with actual content, tool calls, and results
            let content = turn_items
                .iter()
                .filter_map(|item| {
                    if let codex_protocol::models::ResponseItem::Message { content, .. } = item {
                        Some(
                            content
                                .iter()
                                .filter_map(|c| {
                                    if let codex_protocol::models::ContentItem::OutputText {
                                        text,
                                    } = c
                                    {
                                        Some(text.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(""),
                        )
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Extract tool calls from turn items
            let tool_calls: Vec<TranscriptToolCall> = turn_items
                .iter()
                .filter_map(|item| {
                    if let codex_protocol::models::ResponseItem::FunctionCall {
                        call_id,
                        name,
                        arguments,
                        ..
                    } = item
                    {
                        // Parse arguments string as JSON, fallback to string value
                        let args_value = serde_json::from_str(arguments)
                            .unwrap_or_else(|_| serde_json::Value::String(arguments.clone()));
                        Some(TranscriptToolCall {
                            id: call_id.clone(),
                            name: name.clone(),
                            arguments: args_value,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            // Extract tool results from turn items
            let tool_results: Vec<TranscriptToolResult> = turn_items
                .iter()
                .filter_map(|item| {
                    if let codex_protocol::models::ResponseItem::FunctionCallOutput {
                        call_id,
                        output,
                        ..
                    } = item
                    {
                        Some(TranscriptToolResult {
                            tool_call_id: call_id.clone(),
                            content: output.content.clone(),
                            success: output.success.unwrap_or(true),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            transcript_store.record_message(
                &self.context.agent_id,
                TranscriptMessage {
                    role: MessageRole::Assistant,
                    content: if content.is_empty() {
                        format!("Turn {turns} completed")
                    } else {
                        content
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_results: if tool_results.is_empty() {
                        None
                    } else {
                        Some(tool_results)
                    },
                    timestamp: chrono::Utc::now().timestamp(),
                },
            );

            self.send_event(
                SubagentActivityEvent::new(
                    &self.context.agent_id,
                    &self.context.definition.agent_type,
                    SubagentEventType::TurnComplete,
                )
                .with_data("turn_number", turns),
            )
            .await;

            match turn_result {
                TurnResult::Completed(output) => {
                    self.send_event(SubagentActivityEvent::completed(
                        &self.context.agent_id,
                        &self.context.definition.agent_type,
                        turns,
                        start.elapsed().as_secs_f32(),
                    ))
                    .await;
                    return Ok(self.build_result(
                        SubagentStatus::Goal,
                        output,
                        turns,
                        start,
                        total_tool_use_count,
                        total_input_tokens,
                        total_output_tokens,
                    ));
                }
                TurnResult::Continue => {
                    // Continue to next turn
                }
            }
        }
    }

    /// Build a SubagentResult with all metrics.
    fn build_result(
        &self,
        status: SubagentStatus,
        result: String,
        turns: i32,
        start: Instant,
        tool_use_count: i32,
        input_tokens: i32,
        output_tokens: i32,
    ) -> SubagentResult {
        SubagentResult {
            status,
            result,
            turns_used: turns,
            duration: start.elapsed(),
            agent_id: self.context.agent_id.clone(),
            total_tool_use_count: tool_use_count,
            total_duration_ms: start.elapsed().as_millis() as i64,
            total_tokens: input_tokens + output_tokens,
            usage: Some(TokenUsage {
                input_tokens,
                output_tokens,
                ..Default::default()
            }),
        }
    }

    /// Run the agent (convenience method without resume).
    pub async fn run(&self, prompt: String) -> Result<SubagentResult, SubagentErr> {
        // Create a temporary transcript store for non-resume execution
        let temp_store = TranscriptStore::new();
        self.run_with_resume(prompt, None, &temp_store).await
    }

    /// Execute grace period when time/turn limit is reached.
    ///
    /// Sends a warning to the model and gives it one final chance to
    /// call `complete_task` with its best answer.
    ///
    /// Unlike before, this now passes the full conversation history so the model
    /// has context to generate a meaningful response.
    async fn execute_grace_period(
        &self,
        reason: SubagentStatus,
        conversation_history: &[codex_protocol::models::ResponseItem],
    ) -> Option<String> {
        let grace_seconds = self.context.definition.run_config.grace_period_seconds;
        if grace_seconds <= 0 {
            return None;
        }

        // If no model bridge, can't execute grace period
        if self.model_bridge.is_none() {
            return None;
        }

        self.send_event(
            SubagentActivityEvent::new(
                &self.context.agent_id,
                &self.context.definition.agent_type,
                SubagentEventType::GracePeriodStart,
            )
            .with_data("grace_seconds", grace_seconds),
        )
        .await;

        // Create warning message based on reason
        let warning = match reason {
            SubagentStatus::Timeout => "You have EXCEEDED the time limit.",
            SubagentStatus::MaxTurns => "You have reached the MAXIMUM number of turns.",
            _ => "Resource limit reached.",
        };

        let warning_message = format!(
            "{warning} You have ONE FINAL CHANCE to complete the task.\n\
             You MUST call `complete_task` immediately with your best answer.\n\
             Do NOT call any other tools."
        );

        // Build prompt with warning appended to full conversation history
        let warning_item = codex_protocol::models::ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![codex_protocol::models::ContentItem::InputText {
                text: warning_message,
            }],
        };

        // Build grace period history: full conversation + warning
        let mut grace_history: Vec<codex_protocol::models::ResponseItem> =
            conversation_history.to_vec();
        grace_history.push(warning_item);

        // Execute with timeout - now with full conversation context
        let grace_timeout = Duration::from_secs(grace_seconds as u64);

        let result = tokio::time::timeout(grace_timeout, async {
            self.execute_turn(&grace_history).await
        })
        .await;

        let recovered = match result {
            Ok(Ok((TurnResult::Completed(output), _, _))) => {
                self.send_event(
                    SubagentActivityEvent::new(
                        &self.context.agent_id,
                        &self.context.definition.agent_type,
                        SubagentEventType::GracePeriodEnd,
                    )
                    .with_data("recovered", true),
                )
                .await;
                Some(output)
            }
            _ => {
                self.send_event(
                    SubagentActivityEvent::new(
                        &self.context.agent_id,
                        &self.context.definition.agent_type,
                        SubagentEventType::GracePeriodEnd,
                    )
                    .with_data("recovered", false),
                )
                .await;
                None
            }
        };

        recovered
    }

    /// Execute a single turn, using model bridge if available.
    ///
    /// If no model bridge is configured, falls back to stub behavior.
    /// Returns (TurnResult, TurnMetrics, Vec<ResponseItem>) - the items from this turn.
    async fn execute_turn(
        &self,
        messages: &[codex_protocol::models::ResponseItem],
    ) -> Result<
        (
            TurnResult,
            TurnMetrics,
            Vec<codex_protocol::models::ResponseItem>,
        ),
        SubagentErr,
    > {
        // If no model bridge, use stub
        let Some(bridge) = &self.model_bridge else {
            let (result, metrics) = self.execute_turn_stub().await?;
            return Ok((result, metrics, vec![]));
        };

        // Build tools list: filtered tools from definition + complete_task
        let mut tools = self.get_filtered_tools();
        let complete_task =
            create_complete_task_tool(self.context.definition.output_config.as_ref());
        tools.push(complete_task);

        // Build prompt from messages
        let prompt = crate::client_common::Prompt {
            input: messages.to_vec(),
            tools,
            parallel_tool_calls: true,
            base_instructions_override: self.context.definition.prompt_config.system_prompt.clone(),
            output_schema: None,
            previous_response_id: None,
        };

        // Execute via bridge
        let mut receiver = bridge
            .execute_turn(prompt)
            .await
            .map_err(|e| SubagentErr::ModelError(e.to_string()))?;

        let mut metrics = TurnMetrics::default();
        let mut turn_items = Vec::new();

        // Process events
        while let Some(event_result) = receiver.recv().await {
            match event_result {
                Ok(crate::client_common::ResponseEvent::OutputItemDone(item)) => {
                    turn_items.push(item);
                }
                Ok(crate::client_common::ResponseEvent::Completed { token_usage, .. }) => {
                    if let Some(usage) = token_usage {
                        metrics.input_tokens = usage.input_tokens as i32;
                        metrics.output_tokens = usage.output_tokens as i32;
                    }
                    break;
                }
                Err(e) => return Err(SubagentErr::ModelError(e.to_string())),
                _ => {}
            }
        }

        // Process tool calls: check for complete_task and execute other tools
        let mut tool_results: Vec<codex_protocol::models::ResponseItem> = Vec::new();

        for item in &turn_items {
            if let codex_protocol::models::ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } = item
            {
                if name == "complete_task" {
                    // Extract and validate output from arguments
                    let output = self.extract_complete_task_output(arguments)?;
                    return Ok((TurnResult::Completed(output), metrics, turn_items));
                }

                // Execute other tools and collect results
                if self.context.is_tool_allowed(name) {
                    let (success, output) =
                        subagent_tools::execute_tool(name, arguments, &self.context.cwd);

                    tool_results.push(codex_protocol::models::ResponseItem::FunctionCallOutput {
                        call_id: call_id.clone(),
                        output: codex_protocol::models::FunctionCallOutputPayload {
                            content: output,
                            success: Some(success),
                            content_items: None,
                        },
                    });
                } else {
                    // Tool not allowed - return rejection message
                    let reason = self
                        .context
                        .tool_rejection_reason(name)
                        .unwrap_or_else(|| format!("Tool '{}' is not available", name));

                    tool_results.push(codex_protocol::models::ResponseItem::FunctionCallOutput {
                        call_id: call_id.clone(),
                        output: codex_protocol::models::FunctionCallOutputPayload {
                            content: reason,
                            success: Some(false),
                            content_items: None,
                        },
                    });
                }
            }
        }

        // Add tool results to turn items for conversation history
        turn_items.extend(tool_results);

        // Count tool calls
        metrics.tool_use_count = turn_items
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    codex_protocol::models::ResponseItem::FunctionCall { .. }
                )
            })
            .count() as i32;

        Ok((TurnResult::Continue, metrics, turn_items))
    }

    /// Get filtered tool specs based on agent definition.
    fn get_filtered_tools(&self) -> Vec<crate::client_common::tools::ToolSpec> {
        let all_tools = subagent_tools::get_all_subagent_tool_specs();

        all_tools
            .into_iter()
            .filter(|spec| {
                let name = spec.name();
                self.context.is_tool_allowed(name)
            })
            .collect()
    }

    /// Extract and validate output from complete_task arguments.
    ///
    /// If OutputConfig is defined with a schema, validates the output against it.
    fn extract_complete_task_output(&self, arguments: &str) -> Result<String, SubagentErr> {
        // Try to parse as JSON
        let parsed: serde_json::Value = serde_json::from_str(arguments).map_err(|e| {
            SubagentErr::OutputValidationError(format!("Invalid JSON in complete_task: {e}"))
        })?;

        // Extract the output value
        let (output_name, output_value) = if let Some(output) = parsed.get("output") {
            // Standard "output" field
            ("output".to_string(), output.clone())
        } else if let Some(config) = &self.context.definition.output_config {
            // Custom output_name from OutputConfig
            if let Some(output) = parsed.get(&config.output_name) {
                (config.output_name.clone(), output.clone())
            } else {
                return Err(SubagentErr::OutputValidationError(format!(
                    "Missing required output field '{}' in complete_task arguments",
                    config.output_name
                )));
            }
        } else {
            // No specific field found, use entire arguments
            return Ok(arguments.to_string());
        };

        // Validate against schema if OutputConfig exists
        if let Some(config) = &self.context.definition.output_config {
            self.validate_output_schema(&output_name, &output_value, &config.schema)?;
        }

        // Convert to string for return
        if let Some(s) = output_value.as_str() {
            Ok(s.to_string())
        } else {
            Ok(serde_json::to_string(&output_value).unwrap_or_default())
        }
    }

    /// Validate output value against JSON schema.
    ///
    /// Performs basic type validation based on schema type.
    fn validate_output_schema(
        &self,
        field_name: &str,
        value: &serde_json::Value,
        schema: &serde_json::Value,
    ) -> Result<(), SubagentErr> {
        // Get expected type from schema
        let expected_type = schema.get("type").and_then(|t| t.as_str());

        let is_valid = match expected_type {
            Some("string") => value.is_string(),
            Some("number") => value.is_number(),
            Some("integer") => value.is_i64(),
            Some("boolean") => value.is_boolean(),
            Some("array") => value.is_array(),
            Some("object") => value.is_object(),
            Some("null") => value.is_null(),
            None | Some(_) => true, // No type constraint or unknown type - accept any
        };

        if !is_valid {
            return Err(SubagentErr::OutputValidationError(format!(
                "Output field '{}' has invalid type. Expected {}, got {}",
                field_name,
                expected_type.unwrap_or("unknown"),
                value_type_name(value)
            )));
        }

        // Check required properties for objects
        if value.is_object() {
            if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                let obj = value.as_object().unwrap();
                for req_field in required {
                    if let Some(field_name) = req_field.as_str() {
                        if !obj.contains_key(field_name) {
                            return Err(SubagentErr::OutputValidationError(format!(
                                "Output missing required field '{}'",
                                field_name
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// Get a human-readable type name for a JSON value.
fn value_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

impl AgentExecutor {
    /// Stub for turn execution (fallback when no model bridge).
    async fn execute_turn_stub(&self) -> Result<(TurnResult, TurnMetrics), SubagentErr> {
        tokio::time::sleep(Duration::from_millis(10)).await;

        Ok((
            TurnResult::Completed("Stub execution completed".to_string()),
            TurnMetrics {
                tool_use_count: 1,
                input_tokens: 100,
                output_tokens: 50,
            },
        ))
    }

    /// Send an activity event if bridge is configured.
    async fn send_event(&self, event: SubagentActivityEvent) {
        if let Some(bridge) = &self.event_bridge {
            bridge.send(event).await;
        }
    }
}

impl std::fmt::Debug for AgentExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentExecutor")
            .field("agent_id", &self.context.agent_id)
            .field("agent_type", &self.context.definition.agent_type)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::definition::AgentRunConfig;
    use crate::subagent::definition::AgentSource;
    use crate::subagent::definition::ModelConfig;
    use crate::subagent::definition::PromptConfig;
    use crate::subagent::definition::ToolAccess;
    use std::path::PathBuf;

    fn test_definition() -> Arc<AgentDefinition> {
        Arc::new(AgentDefinition {
            agent_type: "TestAgent".to_string(),
            display_name: None,
            when_to_use: None,
            tools: ToolAccess::All,
            disallowed_tools: vec![],
            source: AgentSource::Builtin,
            model_config: ModelConfig::default(),
            fork_context: false,
            prompt_config: PromptConfig::default(),
            run_config: AgentRunConfig {
                max_time_seconds: 60,
                max_turns: 10,
                grace_period_seconds: 5,
            },
            input_config: None,
            output_config: None,
            approval_mode: Default::default(),
            critical_system_reminder: None,
        })
    }

    #[tokio::test]
    async fn test_executor_run() {
        let definition = test_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        let executor = AgentExecutor::new(context);
        let result = executor.run("Test prompt".to_string()).await.unwrap();

        assert!(matches!(result.status, SubagentStatus::Goal));
        assert!(!result.agent_id.is_empty());
    }

    #[tokio::test]
    async fn test_executor_with_resume() {
        let definition = test_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        let store = TranscriptStore::new();
        store.init_transcript("prev-agent".to_string(), "TestAgent".to_string());
        store.record_message(
            "prev-agent",
            TranscriptMessage {
                role: MessageRole::User,
                content: "Previous message".to_string(),
                tool_calls: None,
                tool_results: None,
                timestamp: 12345,
            },
        );

        let executor = AgentExecutor::new(context);
        let result = executor
            .run_with_resume("Continue task".to_string(), Some("prev-agent"), &store)
            .await
            .unwrap();

        assert!(matches!(result.status, SubagentStatus::Goal));
    }

    #[tokio::test]
    async fn test_executor_cancellation() {
        let definition = test_definition();
        let cancel_token = CancellationToken::new();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            cancel_token.clone(),
            "test-model".to_string(),
        );

        // Cancel before running
        cancel_token.cancel();

        let executor = AgentExecutor::new(context);
        let result = executor.run("Test prompt".to_string()).await.unwrap();

        assert!(matches!(result.status, SubagentStatus::Aborted));
    }
}
