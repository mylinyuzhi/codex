//! LLM-based compaction orchestration and shared compaction utilities.

use cocode_inference::AssistantContentPart;
use cocode_inference::LanguageModelMessage;
use cocode_inference::RequestBuilder;
use cocode_inference::TextPart;
use cocode_prompt::SystemPromptBuilder;
use cocode_protocol::AgentStatus;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::HookEventType;
use cocode_protocol::QueryTracking;
use cocode_protocol::TuiEvent;
use cocode_protocol::server_notification::*;

use snafu::ResultExt;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::compaction::InvokedSkillRestoration;
use crate::compaction::LRU_MAX_ENTRIES;
use crate::compaction::TaskStatusRestoration;
use crate::compaction::build_compact_instructions;
use crate::compaction::build_file_read_state;
use crate::compaction::calculate_keep_start_index;
use crate::compaction::format_summary_with_transcript;
use crate::compaction::map_message_index_to_keep_turns;
use crate::error::agent_loop_error;

use super::AgentLoop;
use super::format_language_model_message;

impl AgentLoop {
    /// Run auto-compaction (LLM-based summarization).
    ///
    /// Uses the 9-section compact instructions from `build_compact_instructions()`
    /// to generate a comprehensive conversation summary.
    ///
    /// Before compaction begins, PreCompact hooks are executed. If any hook
    /// returns `Reject`, compaction is skipped and the rejection is logged.
    pub(crate) async fn compact(
        &mut self,
        tracking: &mut AutoCompactTracking,
        turn_id: &str,
        query_tracking: &QueryTracking,
    ) -> crate::error::Result<()> {
        // Execute PreCompact hooks before starting compaction
        let hook_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::PreCompact,
            turn_id.to_string(),
            self.context.environment.cwd.clone(),
        );

        let outcomes = self.hooks.execute(&hook_ctx).await;

        // Check if any hook rejected compaction and collect additional context
        let mut hook_additional_context = Vec::new();
        for outcome in &outcomes {
            // Emit HookExecuted event for each hook
            self.emit_protocol(ServerNotification::HookExecuted(HookExecutedParams {
                hook_type: HookEventType::PreCompact.to_string(),
                hook_name: outcome.hook_name.clone(),
            }))
            .await;

            match &outcome.result {
                cocode_hooks::HookResult::Reject { reason } => {
                    info!(
                        hook_name = %outcome.hook_name,
                        reason = %reason,
                        "Compaction skipped by hook"
                    );
                    return Ok(());
                }
                cocode_hooks::HookResult::ContinueWithContext {
                    additional_context, ..
                } => {
                    hook_additional_context.push(additional_context.clone());
                }
                _ => {}
            }
        }

        // Update status to compacting
        self.set_status(AgentStatus::Compacting);
        self.emit_protocol(ServerNotification::CompactionStarted(
            CompactionStartedParams {},
        ))
        .await;
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.compaction.started", 1, &[]);
        }

        // Estimate tokens before compaction
        let tokens_before = self.message_history.estimate_tokens();

        // Build summarization prompt from conversation text
        let messages = self.message_history.messages_for_api();
        let conversation_text: String = messages
            .iter()
            .map(format_language_model_message)
            .collect::<Vec<_>>()
            .join("\n\n");

        // Use the 9-section compact instructions
        let max_output_tokens = self.compact_config.max_compact_output_tokens;
        let system_prompt = build_compact_instructions(max_output_tokens);

        // Build user prompt, injecting any PreCompact hook context
        let (_, mut user_prompt) =
            SystemPromptBuilder::build_summarization(&conversation_text, None);
        {
            let extra: Vec<&str> = hook_additional_context
                .iter()
                .filter_map(|c| c.as_deref())
                .collect();
            if !extra.is_empty() {
                let ctx = extra.join("\n\n");
                user_prompt = format!("{ctx}\n\n---\n\n{user_prompt}");
            }
        }

        // Use the API client to get a summary with retry mechanism
        let max_retries = self.compact_config.max_summary_retries;
        let mut attempt = 0;

        let summary_text = loop {
            attempt += 1;
            let last_error: String;

            // Build request for each attempt
            let summary_messages = vec![
                LanguageModelMessage::system(&system_prompt),
                LanguageModelMessage::user_text(&user_prompt),
            ];

            // Get compact model and build request using ModelHub
            // Use the real session_id from query_tracking
            let session_id = &query_tracking.chain_id;
            let (ctx, compact_model) = self
                .model_hub
                .prepare_compact_with_selections(&self.selections, session_id, self.turn_number)
                .context(agent_loop_error::PrepareCompactModelSnafu)?;

            // Use RequestBuilder for the summary request
            let summary_request = RequestBuilder::new(ctx)
                .messages(summary_messages.clone())
                .max_tokens(max_output_tokens as u64)
                .build();

            match self
                .api_client
                .generate(&*compact_model, summary_request)
                .await
            {
                Ok(response) => {
                    // Extract summary text
                    let text: String = response
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            AssistantContentPart::Text(TextPart { text, .. }) => {
                                Some(text.as_str())
                            }
                            _ => None,
                        })
                        .collect();

                    if text.is_empty() {
                        last_error = "Empty summary produced".to_string();
                        if attempt <= max_retries {
                            // Exponential backoff: 1s, 2s, 4s, ...
                            let delay_ms = 1000 * (1 << (attempt - 1));
                            tracing::debug!(attempt, max_attempts = max_retries + 1, delay_ms, reason = %last_error, "Compaction retry");
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
                        tracing::debug!(attempt, max_attempts = max_retries + 1, delay_ms, reason = %last_error, "Compaction retry");
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64))
                            .await;
                        continue;
                    }
                }
            }

            // All retries exhausted — update circuit breaker
            self.compact_failure_count += 1;
            warn!(
                attempts = attempt,
                error = %last_error,
                consecutive_failures = self.compact_failure_count,
                "Compaction failed after all retries"
            );
            self.emit_protocol(ServerNotification::CompactionFailed(
                CompactionFailedParams {
                    error: last_error,
                    attempts: attempt,
                },
            ))
            .await;

            // Trip circuit breaker after 3 consecutive failures
            if self.compact_failure_count >= 3 && !self.circuit_breaker_open {
                self.circuit_breaker_open = true;
                warn!(
                    consecutive_failures = self.compact_failure_count,
                    "Auto-compaction circuit breaker opened"
                );
                self.emit_tui(TuiEvent::CompactionCircuitBreakerOpen {
                    consecutive_failures: self.compact_failure_count,
                })
                .await;
            }
            return Ok(());
        };

        // Extract task status and invoked skills using dedup helper
        let (task_status, invoked_skills) = self.extract_tool_call_metadata();

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

        // Track message count before compaction for accurate removal reporting
        let turn_count_before = self.message_history.turn_count();

        // Calculate keep window using token-based algorithm
        let messages_json = self.message_history.messages_for_api_json();
        let keep_result =
            calculate_keep_start_index(&messages_json, &self.compact_config.keep_window);
        let keep_turns = map_message_index_to_keep_turns(
            self.message_history.turn_count(),
            &messages_json,
            keep_result.keep_start_index,
        );
        let tokens_saved = (tokens_before - self.message_history.estimate_tokens()).max(0);

        debug!(
            keep_turns,
            keep_start_index = keep_result.keep_start_index,
            messages_to_keep = keep_result.messages_to_keep,
            keep_tokens = keep_result.keep_tokens,
            text_messages_kept = keep_result.text_messages_kept,
            "Calculated keep window for compaction"
        );

        // Get transcript path from context if available
        let transcript_path = self.context.transcript_path.clone();

        // Wrap summary with continuation header and transcript reference
        let wrapped_summary = format_summary_with_transcript(
            &final_summary,
            transcript_path.as_ref(),
            true, // recent_messages_preserved
            tokens_before,
        );

        self.message_history.apply_compaction_with_metadata(
            wrapped_summary,
            keep_turns,
            turn_id,
            tokens_saved,
            cocode_protocol::CompactTrigger::Auto,
            tokens_before,
            transcript_path.clone(),
            true, // Recent messages are preserved
        );

        let post_tokens = self
            .finalize_compaction_tracking(tracking, turn_id, false)
            .await;

        // Rebuild FileTracker from remaining messages after compaction
        self.rebuild_file_tracker_from_history().await;

        // Compaction complete - restore status to Idle
        self.set_status(AgentStatus::Idle);
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.compaction.completed", 1, &[]);
        }
        let removed_messages = (turn_count_before - self.message_history.turn_count()).max(0);
        self.emit_protocol(ServerNotification::ContextCompacted(
            ContextCompactedParams {
                removed_messages,
                summary_tokens: post_tokens,
            },
        ))
        .await;

        tracing::debug!(
            pre_tokens = tokens_before,
            post_tokens,
            "Compact boundary inserted"
        );

        self.emit_invoked_skills_restored(&invoked_skills).await;

        // Context restoration: restore important files that were read before compaction
        self.restore_context_after_compaction(&invoked_skills, &task_status)
            .await;

        // Save to session memory for future Tier 1 compaction
        self.spawn_session_memory_write(final_summary, turn_id);

        // Execute PostCompact hooks
        self.execute_post_compact_hooks(turn_id).await;

        Ok(())
    }

    /// Extract task status and invoked skills from tool call history.
    ///
    /// Scans all turns in the message history, collecting tool call names and
    /// inputs, then derives `TaskStatusRestoration` and `InvokedSkillRestoration`
    /// from those calls. This is the shared logic used by both `compact()` and
    /// `apply_session_memory_summary()`.
    pub(crate) fn extract_tool_call_metadata(
        &self,
    ) -> (TaskStatusRestoration, Vec<InvokedSkillRestoration>) {
        let tool_calls_with_turns: Vec<(String, serde_json::Value, i32)> = self
            .message_history
            .turns()
            .iter()
            .flat_map(|turn| {
                let turn_num = turn.number;
                turn.tool_calls
                    .iter()
                    .map(move |tc| (tc.name.clone(), tc.input.clone(), turn_num))
            })
            .collect();

        let tool_calls: Vec<(String, serde_json::Value)> = tool_calls_with_turns
            .iter()
            .map(|(name, input, _)| (name.clone(), input.clone()))
            .collect();

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
        let invoked_skills = InvokedSkillRestoration::from_tool_calls(&tool_calls_with_turns);

        (task_status, invoked_skills)
    }

    /// Clear and rebuild the `FileTracker` from remaining messages.
    ///
    /// After compaction removes old messages, the file tracker must be rebuilt
    /// to reflect only the files referenced in the surviving history.
    pub(crate) async fn rebuild_file_tracker_from_history(&self) {
        let cwd = self.context.environment.cwd.clone();
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.clear();

        // Rebuild from remaining messages
        let new_tracker = build_file_read_state(&self.message_history, &cwd, LRU_MAX_ENTRIES);

        // Copy state from new tracker
        for (path, state) in new_tracker.read_files_with_state() {
            tracker.record_read_with_state(path, state.clone());
        }

        debug!(
            tracked_files = tracker.len(),
            "FileTracker rebuilt after compaction"
        );
    }

    /// Finalize compaction tracking state after successful compaction.
    ///
    /// Shared between Tier 1 (session memory) and Tier 2 (LLM) compaction.
    /// Marks the compaction in tracking, resets circuit breaker state, updates
    /// the post-compaction token boundary, and sets the snapshot compaction fence.
    ///
    /// # `reset_circuit_breaker`
    ///
    /// Currently both callers run inside the `!self.circuit_breaker_open` guard
    /// in `core_message_loop`, so the breaker is always `false` at call time.
    /// The parameter is retained for forward-compatibility: if a future path
    /// allows Tier 1 compaction while the breaker is open, passing `true`
    /// will automatically clear it on success.
    ///
    /// Returns the post-compaction token estimate.
    pub(crate) async fn finalize_compaction_tracking(
        &mut self,
        tracking: &mut AutoCompactTracking,
        turn_id: &str,
        reset_circuit_breaker: bool,
    ) -> i32 {
        tracking.mark_compacted(turn_id, self.turn_number);

        self.compact_failure_count = 0;
        if reset_circuit_breaker {
            self.circuit_breaker_open = false;
        }

        let post_tokens = self.message_history.estimate_tokens();
        self.message_history
            .update_boundary_post_tokens(post_tokens);

        if let Some(ref sm) = self.snapshot_manager {
            sm.set_compaction_boundary(self.turn_number).await;
        }

        post_tokens
    }

    /// Emit `InvokedSkillsRestored` event if any skills were found.
    pub(crate) async fn emit_invoked_skills_restored(
        &self,
        invoked_skills: &[InvokedSkillRestoration],
    ) {
        if !invoked_skills.is_empty() {
            let skill_names: Vec<String> = invoked_skills.iter().map(|s| s.name.clone()).collect();
            tracing::debug!(?skill_names, "Invoked skills restored after compaction");
        }
    }
}
