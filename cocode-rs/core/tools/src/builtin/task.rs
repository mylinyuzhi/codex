//! Task tool for launching sub-agents.

use super::prompts;
use crate::context::SpawnAgentInput;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for launching specialized sub-agents.
///
/// Delegates to a SubagentManager (connected externally).
pub struct TaskTool;

impl TaskTool {
    /// Create a new Task tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TaskTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Task.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "The type of specialized agent to use"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run agent in background",
                    "default": false
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this agent. Takes precedence over the agent definition's model frontmatter. If omitted, uses the agent definition's model, or inherits from the parent."
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum turns before stopping"
                },
                "resume": {
                    "type": "string",
                    "description": "Agent ID to resume from"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tools to grant this agent"
                },
                "isolation": {
                    "type": "string",
                    "description": "Isolation mode. \"worktree\" creates a temporary git worktree so the agent works on an isolated copy of the repo.",
                    "enum": ["worktree"]
                },
                "name": {
                    "type": "string",
                    "description": "Display name for the spawned agent"
                },
                "team_name": {
                    "type": "string",
                    "description": "Team to auto-join the agent to after spawn"
                },
                "mode": {
                    "type": "string",
                    "enum": ["normal", "plan", "auto"],
                    "description": "Agent execution mode (default: normal)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the spawned agent"
                }
            },
            "required": ["description", "prompt"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        // Default to Unsafe (foreground blocks); background overridden by is_concurrency_safe_for
        ConcurrencySafety::Unsafe
    }

    fn is_concurrency_safe_for(&self, input: &Value) -> bool {
        // Background tasks are safe for concurrent execution; foreground tasks block
        super::input_helpers::bool_or(input, "run_in_background", false)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let description = input["description"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "description must be a string",
            }
            .build()
        })?;
        let prompt = input["prompt"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "prompt must be a string",
            }
            .build()
        })?;
        let subagent_type = input["subagent_type"].as_str().unwrap_or("general-purpose");

        // Parse optional fields, gated by BackgroundTasks feature flag
        let run_in_background = if ctx
            .features
            .enabled(cocode_protocol::Feature::BackgroundTasks)
        {
            input["run_in_background"].as_bool()
        } else {
            // Feature disabled: force foreground execution
            Some(false)
        };
        let model = input["model"].as_str().map(String::from);
        let max_turns = input["max_turns"].as_i64().map(|n| n as i32);
        let allowed_tools = input["allowed_tools"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });
        let resume_from = input["resume"].as_str().map(String::from);
        let isolation = input["isolation"].as_str().map(String::from);

        ctx.emit_progress(format!("Launching {subagent_type} agent: {description}"))
            .await;

        // Check if spawning is available
        if !ctx.can_spawn_agent() {
            return Ok(ToolOutput::text(format!(
                "Agent '{subagent_type}' launched for: {description}\nPrompt: {prompt}\n\n\
                 [SubagentManager not configured - returning stub response]"
            )));
        }

        // Check Task(type) restrictions: only allow spawning permitted agent types
        if let Some(ref restrictions) = ctx.task_type_restrictions
            && !restrictions.iter().any(|t| t == subagent_type)
        {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: format!(
                    "Agent type '{subagent_type}' is not allowed. Permitted types: {}",
                    restrictions.join(", ")
                ),
            }
            .build());
        }

        // Execute SubagentStart hooks before spawning (non-blocking per CC semantics)
        let mut hook_additional_context: Option<String> = None;
        if let Some(hooks) = &ctx.hook_registry {
            let hook_ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::SubagentStart,
                ctx.session_id.clone(),
                ctx.cwd.clone(),
            )
            .with_agent_type(subagent_type)
            .with_metadata("description", description);
            let outcomes = hooks.execute(&hook_ctx).await;
            for outcome in &outcomes {
                match &outcome.result {
                    cocode_hooks::HookResult::Reject { reason } => {
                        // Non-blocking: warn but don't prevent spawning
                        tracing::warn!(
                            hook = %outcome.hook_name,
                            %reason,
                            "SubagentStart hook rejected (non-blocking)"
                        );
                    }
                    cocode_hooks::HookResult::ContinueWithContext {
                        additional_context: Some(ctx_text),
                        ..
                    } => {
                        hook_additional_context = Some(ctx_text.clone());
                    }
                    _ => {}
                }
            }
        }

        // Prepend hook context to prompt if provided
        let effective_prompt = match hook_additional_context {
            Some(ref ctx_text) => format!("{ctx_text}\n\n{prompt}"),
            None => prompt.to_string(),
        };

        // Build spawn input with parent's selections for isolation
        let spawn_input = SpawnAgentInput {
            agent_type: subagent_type.to_string(),
            prompt: effective_prompt,
            model,
            max_turns,
            run_in_background,
            allowed_tools,
            parent_selections: ctx.parent_selections.clone(),
            permission_mode: None, // Resolved by driver from AgentDefinition
            resume_from,
            isolation,
            name: input["name"].as_str().map(String::from),
            team_name: input["team_name"].as_str().map(String::from),
            mode: input["mode"].as_str().map(String::from),
            cwd: input["cwd"].as_str().map(String::from),
            description: Some(description.to_string()),
        };

        // Spawn the agent
        match ctx.spawn_agent(spawn_input).await {
            Ok(result) => {
                // Register cancel token so TaskStop can cancel this agent by ID
                if let Some(token) = result.cancel_token.clone() {
                    ctx.agent_cancel_tokens
                        .lock()
                        .await
                        .insert(result.agent_id.clone(), token);
                }

                // Emit SubagentSpawned event for TUI visibility
                ctx.emit_event(cocode_protocol::CoreEvent::Protocol(
                    cocode_protocol::server_notification::ServerNotification::SubagentSpawned(
                        cocode_protocol::server_notification::SubagentSpawnedParams {
                            agent_id: result.agent_id.clone(),
                            agent_type: subagent_type.to_string(),
                            description: description.to_string(),
                            color: result.color.clone(),
                        },
                    ),
                ))
                .await;

                if result.output_file.is_some() {
                    // Emit SubagentBackgrounded event
                    let output_file = result.output_file.clone().unwrap_or_default();
                    ctx.emit_event(
                        cocode_protocol::CoreEvent::Protocol(
                            cocode_protocol::server_notification::ServerNotification::SubagentBackgrounded(
                                cocode_protocol::server_notification::SubagentBackgroundedParams {
                                    agent_id: result.agent_id.clone(),
                                    output_file: output_file.to_string_lossy().into_owned(),
                                },
                            ),
                        ),
                    )
                    .await;

                    // Background agent - return ID and output file path
                    let output_path = result
                        .output_file
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    Ok(ToolOutput::text(format!(
                        "Agent '{subagent_type}' started in background.\n\
                         Agent ID: {}\n\
                         Output file: {output_path}",
                        result.agent_id
                    )))
                } else {
                    // Execute SubagentStop hooks BEFORE emitting completion event
                    // (blocking: exit 2 / Reject prevents agent from stopping)
                    let output = result.output.clone().unwrap_or_else(|| {
                        format!("Agent '{subagent_type}' completed with no output.")
                    });

                    let mut stop_blocked = false;
                    let mut stop_reason = String::new();
                    if let Some(hooks) = &ctx.hook_registry {
                        let hook_ctx = cocode_hooks::HookContext::new(
                            cocode_hooks::HookEventType::SubagentStop,
                            ctx.session_id.clone(),
                            ctx.cwd.clone(),
                        )
                        .with_agent_type(subagent_type)
                        .with_agent_id(result.agent_id.clone())
                        .with_last_assistant_message(output.clone());
                        let outcomes = hooks.execute(&hook_ctx).await;
                        for outcome in &outcomes {
                            if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                                stop_blocked = true;
                                stop_reason = reason.clone();
                                tracing::info!(
                                    hook = %outcome.hook_name,
                                    %reason,
                                    "SubagentStop hook blocked completion"
                                );
                            }
                        }
                    }

                    if stop_blocked {
                        // Agent continues — return output with stop reason
                        let blocked_output =
                            format!("{output}\n\n[SubagentStop hook blocked: {stop_reason}]");
                        Ok(ToolOutput::text(format!(
                            "agentId: {}\n\n{}",
                            result.agent_id, blocked_output
                        )))
                    } else {
                        // Normal completion — emit event AFTER hooks pass
                        ctx.emit_event(
                            cocode_protocol::CoreEvent::Protocol(
                                cocode_protocol::server_notification::ServerNotification::SubagentCompleted(
                                    cocode_protocol::server_notification::SubagentCompletedParams {
                                        agent_id: result.agent_id.clone(),
                                        result: output.clone(),
                                    },
                                ),
                            ),
                        )
                        .await;

                        // Fire TaskCompleted hooks
                        if let Some(hooks) = &ctx.hook_registry {
                            let hook_ctx = cocode_hooks::HookContext::new(
                                cocode_hooks::HookEventType::TaskCompleted,
                                ctx.session_id.clone(),
                                ctx.cwd.clone(),
                            )
                            .with_task_id(&result.agent_id)
                            .with_task_subject(description);
                            let outcomes = hooks.execute(&hook_ctx).await;
                            for outcome in &outcomes {
                                if let cocode_hooks::HookResult::Reject { reason } = &outcome.result
                                {
                                    tracing::info!(
                                        hook = %outcome.hook_name,
                                        %reason,
                                        "TaskCompleted hook rejected (informational)"
                                    );
                                }
                            }
                        }

                        Ok(ToolOutput::text(format!(
                            "agentId: {}\n\n{}",
                            result.agent_id, output
                        )))
                    }
                }
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to spawn agent '{subagent_type}': {}",
                e.output_msg()
            ))),
        }
    }
}

#[cfg(test)]
#[path = "task.test.rs"]
mod tests;
