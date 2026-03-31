//! Hook integration for [`StreamingToolExecutor`].
//!
//! Contains pre-hook and post-hook execution methods used during
//! the tool execution lifecycle.

use crate::error::Result;
use crate::executor::PermissionLevel;
use crate::executor::PostHookAction;
use crate::executor::PreHookOutcome;
use crate::executor::StreamingToolExecutor;
use cocode_hooks::HookContext;
use cocode_hooks::HookEventType;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookResult;
use cocode_protocol::ToolOutput;
use cocode_protocol::server_notification::*;
use serde_json::Value;
use std::path::Path;
use tracing::debug;
use tracing::info;
use tracing::warn;

impl StreamingToolExecutor {
    /// Execute pre-tool-use hooks and return the (possibly modified) input.
    ///
    /// Returns `Err` if the tool call should be rejected.
    pub(crate) async fn execute_pre_hooks(
        &self,
        tool_name: &str,
        tool_use_id: &str,
        input: Value,
    ) -> std::result::Result<PreHookOutcome, String> {
        let hooks = match &self.hooks {
            Some(h) => h,
            None => {
                return Ok(PreHookOutcome {
                    input,
                    skip_permission: false,
                    additional_contexts: Vec::new(),
                });
            }
        };

        let ctx = HookContext::new(
            HookEventType::PreToolUse,
            self.config.session_id.clone(),
            self.config.cwd.clone(),
        )
        .with_tool(tool_name, input.clone())
        .with_tool_use_id(tool_use_id);

        let outcomes = hooks.execute(&ctx).await;
        let mut current_input = input;
        let mut additional_contexts = Vec::new();
        let mut aggregated_permission = PermissionLevel::Undefined;
        let mut first_deny_reason: Option<String> = None;

        for outcome in outcomes {
            // Emit hook executed event
            self.emit_protocol(ServerNotification::HookExecuted(HookExecutedParams {
                hook_type: HookEventType::PreToolUse.to_string(),
                hook_name: outcome.hook_name.clone(),
            }))
            .await;

            match outcome.result {
                HookResult::Continue => {
                    // Continue with current input
                }
                HookResult::ContinueWithContext {
                    additional_context, ..
                } => {
                    if let Some(ctx_str) = additional_context {
                        additional_contexts.push(ctx_str);
                    }
                }
                HookResult::Reject { reason } => {
                    warn!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        reason = %reason,
                        "Tool call rejected by pre-hook"
                    );
                    return Err(reason);
                }
                HookResult::ModifyInput { new_input } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        "Tool input modified by pre-hook"
                    );
                    current_input = new_input;
                }
                HookResult::Async { task_id, hook_name } => {
                    // Register async hook for tracking - result will be delivered via system reminders
                    self.async_hook_tracker
                        .register(task_id.clone(), hook_name.clone());
                    debug!(
                        tool = %tool_name,
                        task_id = %task_id,
                        async_hook = %hook_name,
                        "Async hook registered and running in background"
                    );
                }
                HookResult::PermissionOverride { decision, reason } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        decision = %decision,
                        reason = ?reason,
                        "Hook returned permission override"
                    );
                    let level = PermissionLevel::from_decision(&decision);
                    // Most-restrictive-wins: lower ordinal = more restrictive
                    if level < aggregated_permission {
                        aggregated_permission = level;
                        if level == PermissionLevel::Deny && first_deny_reason.is_none() {
                            first_deny_reason = reason;
                        }
                    }
                }
                HookResult::SystemMessage { message } => {
                    debug!(
                        tool = %tool_name,
                        hook = %outcome.hook_name,
                        "Pre-hook system message: {message}"
                    );
                }
                HookResult::ModifyOutput { .. } | HookResult::PreventContinuation { .. } => {
                    // ModifyOutput and PreventContinuation are only relevant for PostToolUse, ignore in PreToolUse
                }
            }
        }

        // Apply aggregated permission decision
        match aggregated_permission {
            PermissionLevel::Deny => {
                return Err(first_deny_reason
                    .unwrap_or_else(|| "Tool denied by hook permission override".to_string()));
            }
            PermissionLevel::Ask => {
                // Explicitly ask -- don't skip permission
            }
            PermissionLevel::Allow => {
                return Ok(PreHookOutcome {
                    input: current_input,
                    skip_permission: true,
                    additional_contexts,
                });
            }
            PermissionLevel::Undefined => {
                // No permission overrides -- no change
            }
        }

        Ok(PreHookOutcome {
            input: current_input,
            skip_permission: false,
            additional_contexts,
        })
    }

    /// Execute post-tool-use hooks.
    pub(crate) async fn execute_post_hooks(
        &self,
        tool_name: &str,
        tool_use_id: &str,
        input: &Value,
        result: &Result<ToolOutput>,
    ) -> (PostHookAction, Vec<String>) {
        run_post_hooks(
            self.hooks.as_deref(),
            tool_name,
            tool_use_id,
            input,
            result,
            &self.config.session_id,
            &self.config.cwd,
        )
        .await
    }
}

/// Shared post-hook execution logic.
///
/// Used by both `start_tool_execution` (spawned safe tools) and
/// `execute_single_tool` (inline unsafe tools) to ensure consistent behavior.
///
/// Returns a tuple of:
/// - [`PostHookAction`] indicating whether the result should be kept,
///   replaced with an error (Reject), substituted with hook-provided output
///   (ReplaceOutput), or the loop should halt (StopContinuation).
/// - `Vec<String>` of additional contexts collected from `ContinueWithContext` hooks.
pub(crate) async fn run_post_hooks(
    hooks: Option<&HookRegistry>,
    tool_name: &str,
    tool_use_id: &str,
    input: &Value,
    result: &Result<ToolOutput>,
    session_id: &str,
    cwd: &Path,
) -> (PostHookAction, Vec<String>) {
    let hooks = match hooks {
        Some(h) => h,
        None => return (PostHookAction::None, Vec::new()),
    };

    let is_error = result.is_err();
    let event_type = if is_error {
        HookEventType::PostToolUseFailure
    } else {
        HookEventType::PostToolUse
    };

    let mut ctx = HookContext::new(event_type, session_id.to_string(), cwd.to_path_buf())
        .with_tool(tool_name, input.clone())
        .with_tool_use_id(tool_use_id);

    // Enrich context with result-specific fields
    match result {
        Ok(output) => {
            // Convert ToolResultContent to serde_json::Value for tool_response
            if let Ok(response_value) = serde_json::to_value(&output.content) {
                ctx = ctx.with_tool_response(response_value);
            }
        }
        Err(e) => {
            ctx = ctx.with_error(e.to_string());
        }
    }

    let outcomes = hooks.execute(&ctx).await;
    let mut action = PostHookAction::None;
    let mut additional_contexts = Vec::new();
    for outcome in outcomes {
        match outcome.result {
            HookResult::Reject { reason } => {
                warn!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    reason = %reason,
                    "PostToolUse hook rejected -- tool output will be replaced with error"
                );
                action = PostHookAction::Reject(reason);
            }
            HookResult::ModifyOutput { new_output } => {
                debug!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    "PostToolUse hook provided replacement output"
                );
                let text = new_output
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| new_output.to_string());
                action = PostHookAction::ReplaceOutput(ToolOutput::text(text));
            }
            HookResult::ContinueWithContext {
                additional_context: Some(ctx_str),
                ..
            } => {
                additional_contexts.push(ctx_str);
            }
            HookResult::PreventContinuation { reason } => {
                let reason_text = reason.unwrap_or_else(|| outcome.hook_name.clone());
                info!(
                    tool = %tool_name,
                    hook = %outcome.hook_name,
                    reason = %reason_text,
                    "PostToolUse hook requested loop stop -- tool output preserved"
                );
                action = PostHookAction::StopContinuation(reason_text);
            }
            _ => {}
        }
    }
    (action, additional_contexts)
}

#[cfg(test)]
#[path = "executor_hooks.test.rs"]
mod tests;
