//! Compaction orchestration methods for the agent loop.

use cocode_api::AssistantContentPart;
use cocode_api::LanguageModelMessage;
use cocode_api::RequestBuilder;
use cocode_api::TextPart;
use cocode_message::TrackedMessage;
use cocode_message::Turn;
use cocode_prompt::SystemPromptBuilder;
use cocode_protocol::AgentStatus;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::HookEventType;
use cocode_protocol::LoopEvent;
use cocode_protocol::QueryTracking;

use snafu::ResultExt;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::compaction::FileRestoration;
use crate::compaction::FileRestorationConfig;
use crate::compaction::InvokedSkillRestoration;
use crate::compaction::LRU_MAX_ENTRIES;
use crate::compaction::SessionMemorySummary;
use crate::compaction::TaskStatusRestoration;
use crate::compaction::ThresholdStatus;
use crate::compaction::build_compact_instructions;
use crate::compaction::build_context_restoration_with_config;
use crate::compaction::build_file_read_state;
use crate::compaction::calculate_keep_start_index;
use crate::compaction::find_session_memory_boundary;
use crate::compaction::format_restoration_message;
use crate::compaction::format_summary_with_transcript;
use crate::compaction::is_internal_file;
use crate::compaction::map_message_index_to_keep_turns;
use crate::compaction::wrap_hook_additional_context;
use crate::compaction::write_session_memory;
use crate::error::agent_loop_error;

use super::AgentLoop;
use super::format_language_model_message;

impl AgentLoop {
    /// Run micro-compaction (no LLM call).
    ///
    /// Clears old tool results from the message history when the context usage
    /// exceeds the warning threshold. Returns `(compacted_count, tokens_saved)`.
    pub(crate) async fn micro_compact(&mut self) -> (i32, i32) {
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

        // Emit started event before compaction begins
        self.emit(LoopEvent::MicroCompactionStarted {
            candidates: 0, // Exact count will be in MicroCompactionApplied
            potential_savings: 0,
        })
        .await;

        // Apply micro-compaction using configured recent_tool_results_to_keep
        // Get paths from ContextModifier::FileRead for FileTracker cleanup
        let keep_count = self.compact_config.recent_tool_results_to_keep;
        let outcome = self.message_history.micro_compact_outcome(keep_count);

        // Clean up FileTracker entries for compacted reads using paths from modifiers
        // This is more accurate than tool_id mapping since it uses actual file paths
        if !outcome.cleared_read_paths.is_empty() {
            // Determine how many recent turns to preserve files from
            // This matches Claude Code's collectFilesToKeep behavior
            let keep_recent_turns = self.compact_config.micro_compact_keep_recent_turns;
            let files_to_keep =
                crate::compaction::collect_files_to_keep(&self.message_history, keep_recent_turns);

            let tracker = self.shared_tools_file_tracker.lock().await;

            // Collect paths to remove (excluding preserved files)
            let paths_to_remove: Vec<_> = outcome
                .cleared_read_paths
                .iter()
                .filter(|p| !files_to_keep.contains(*p))
                .cloned()
                .collect();

            if !paths_to_remove.is_empty() {
                tracker.remove_paths(&paths_to_remove);
            }

            debug!(
                cleared_paths = outcome.cleared_read_paths.len(),
                removed_paths = paths_to_remove.len(),
                files_preserved = files_to_keep.len(),
                "Cleaned up FileTracker entries for compacted reads (preserved recent files)"
            );
        }

        // Calculate tokens saved
        let tokens_after = self.message_history.estimate_tokens();
        let tokens_saved = tokens_before - tokens_after;

        debug!(
            removed = outcome.compacted_count,
            tokens_before, tokens_after, tokens_saved, "Micro-compaction complete"
        );

        (outcome.compacted_count, tokens_saved)
    }

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
            self.emit(LoopEvent::HookExecuted {
                hook_type: HookEventType::PreCompact,
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            match &outcome.result {
                cocode_hooks::HookResult::Reject { reason } => {
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
        self.emit(LoopEvent::CompactionStarted).await;
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

            // All retries exhausted — update circuit breaker
            self.compact_failure_count += 1;
            warn!(
                attempts = attempt,
                error = %last_error,
                consecutive_failures = self.compact_failure_count,
                "Compaction failed after all retries"
            );
            self.emit(LoopEvent::CompactionFailed {
                attempts: attempt,
                error: last_error,
            })
            .await;

            // Trip circuit breaker after 3 consecutive failures
            if self.compact_failure_count >= 3 && !self.circuit_breaker_open {
                self.circuit_breaker_open = true;
                warn!(
                    consecutive_failures = self.compact_failure_count,
                    "Auto-compaction circuit breaker opened"
                );
                self.emit(LoopEvent::CompactionCircuitBreakerOpen {
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
        self.emit(LoopEvent::CompactionCompleted {
            removed_messages,
            summary_tokens: post_tokens,
        })
        .await;

        // Emit compact boundary inserted event
        self.emit(LoopEvent::CompactBoundaryInserted {
            trigger: cocode_protocol::CompactTrigger::Auto,
            pre_tokens: tokens_before,
            post_tokens,
        })
        .await;

        self.emit_invoked_skills_restored(&invoked_skills).await;

        // Context restoration: restore important files that were read before compaction
        self.restore_context_after_compaction(&invoked_skills, &task_status)
            .await;

        // Save to session memory for future Tier 1 compaction
        if self.compact_config.enable_sm_compact
            && let Some(ref path) = self.compact_config.summary_path
        {
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

        // Execute SessionStart hooks after compaction (with source: 'compact')
        // This allows hooks to provide additional context after compaction
        self.execute_post_compact_hooks(turn_id).await;

        Ok(())
    }

    /// Execute PostCompact hooks after compaction.
    ///
    /// Fires the dedicated `PostCompact` hook event to allow hooks to provide
    /// additional context for the resumed conversation after compaction.
    /// Any additional context provided by hooks is injected into the message
    /// history as a meta user message so the model can see it on the next turn.
    pub(crate) async fn execute_post_compact_hooks(&mut self, turn_id: &str) {
        let hook_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::PostCompact,
            turn_id.to_string(),
            self.context.environment.cwd.clone(),
        );

        let outcomes = self.hooks.execute(&hook_ctx).await;

        let mut hooks_executed = 0;
        let mut hook_contexts: Vec<cocode_protocol::HookAdditionalContext> = Vec::new();

        for outcome in &outcomes {
            // Emit HookExecuted event for each hook
            self.emit(LoopEvent::HookExecuted {
                hook_type: HookEventType::PostCompact,
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            hooks_executed += 1;

            // Collect additional context from hooks
            if let cocode_hooks::HookResult::ContinueWithContext {
                additional_context, ..
            } = &outcome.result
                && let Some(ctx) = additional_context
                && !ctx.is_empty()
            {
                debug!(
                    hook_name = %outcome.hook_name,
                    context_len = ctx.len(),
                    "Hook provided additional context"
                );
                hook_contexts.push(cocode_protocol::HookAdditionalContext {
                    content: ctx.clone(),
                    hook_name: outcome.hook_name.clone(),
                    suppress_output: false,
                });
            }
        }

        if hooks_executed > 0 {
            let additional_context_count = hook_contexts.len() as i32;
            self.emit(LoopEvent::PostCompactHooksExecuted {
                hooks_executed,
                additional_context_count,
            })
            .await;

            // Inject hook additional context into message history as a meta message
            if let Some(formatted) = wrap_hook_additional_context(&hook_contexts) {
                let meta_turn_id = uuid::Uuid::new_v4().to_string();
                let mut msg = TrackedMessage::user(&formatted, &meta_turn_id);
                msg.set_meta(true);
                let turn = Turn::new(self.turn_number, msg);
                self.message_history.add_turn(turn);
            }
        }
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
    pub(crate) async fn apply_session_memory_summary(
        &mut self,
        summary: SessionMemorySummary,
        turn_id: &str,
        tracking: &mut AutoCompactTracking,
    ) -> crate::error::Result<()> {
        let tokens_before = self.message_history.estimate_tokens();

        info!(
            summary_tokens = summary.token_estimate,
            last_id = ?summary.last_summarized_id,
            "Applying session memory summary (Tier 1)"
        );

        // Get transcript path from context if available
        let transcript_path = self.context.transcript_path.clone();

        // Calculate keep window using anchor-based session memory boundary algorithm
        let messages_json = self.message_history.messages_for_api_json();
        let keep_result = find_session_memory_boundary(
            &messages_json,
            &self.compact_config.keep_window,
            summary.last_summarized_id.as_deref(),
        );
        let keep_turns = map_message_index_to_keep_turns(
            self.message_history.turn_count(),
            &messages_json,
            keep_result.keep_start_index,
        );
        let tokens_saved = (tokens_before - summary.token_estimate).max(0);

        debug!(
            keep_turns,
            keep_start_index = keep_result.keep_start_index,
            messages_to_keep = keep_result.messages_to_keep,
            keep_tokens = keep_result.keep_tokens,
            text_messages_kept = keep_result.text_messages_kept,
            "Calculated keep window for session memory compact (anchor-based)"
        );

        // Wrap summary with continuation header and transcript reference
        let wrapped_summary = format_summary_with_transcript(
            &summary.summary,
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
            transcript_path,
            true, // Recent messages preserved
        );

        let post_tokens = self
            .finalize_compaction_tracking(tracking, turn_id, true)
            .await;

        // Emit events
        self.emit(LoopEvent::SessionMemoryCompactApplied {
            saved_tokens: tokens_saved,
            summary_tokens: summary.token_estimate,
        })
        .await;

        // Emit compact boundary inserted event
        self.emit(LoopEvent::CompactBoundaryInserted {
            trigger: cocode_protocol::CompactTrigger::Auto,
            pre_tokens: tokens_before,
            post_tokens,
        })
        .await;

        // Rebuild FileTracker from remaining messages after compaction
        self.rebuild_file_tracker_from_history().await;

        // Extract task status and invoked skills using dedup helper
        let (task_status, invoked_skills) = self.extract_tool_call_metadata();

        self.emit_invoked_skills_restored(&invoked_skills).await;

        // Full context restoration: files, todos, plans, skills
        self.restore_context_after_compaction(&invoked_skills, &task_status)
            .await;

        Ok(())
    }

    /// Collect tracked files suitable for context restoration after compaction.
    ///
    /// Reads current content from disk, applies exclusion patterns, skips internal
    /// files, and limits to the configured max_files count.
    pub(crate) async fn collect_restorable_tracked_files(
        &self,
        file_config: &FileRestorationConfig,
    ) -> Vec<FileRestoration> {
        // Collect files and their last_accessed times in a single lock acquisition
        let file_info: Vec<(std::path::PathBuf, i64)> = {
            let tracker = self.shared_tools_file_tracker.lock().await;
            tracker
                .tracked_files()
                .into_iter()
                .map(|path| {
                    let last_accessed = tracker
                        .read_state(&path)
                        .map(|s| s.read_turn as i64)
                        .unwrap_or(0);
                    (path, last_accessed)
                })
                .collect()
        };

        let mut files_for_restoration: Vec<FileRestoration> = Vec::new();

        for (path, last_accessed) in file_info {
            // Skip excluded patterns
            let path_str = path.to_string_lossy();
            if file_config.should_exclude(&path_str) {
                continue;
            }

            // Skip internal files (session memory, plan files, auto memory)
            if is_internal_file(&path, "") {
                debug!(path = %path.display(), "Skipping internal file for restoration");
                continue;
            }

            // Try to read the file content (re-read at compact time for current content)
            // Truncate to max_tokens_per_file limit to avoid large file overhead
            let max_chars = (file_config.max_tokens_per_file * 3) as usize;
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    // Truncate content if it exceeds per-file limit
                    let (content, truncated) = if content.len() > max_chars {
                        (content[..max_chars].to_string(), true)
                    } else {
                        (content, false)
                    };
                    let tokens = cocode_protocol::estimate_text_tokens(&content);

                    if truncated {
                        debug!(
                            path = %path.display(),
                            tokens = tokens,
                            max_tokens = file_config.max_tokens_per_file,
                            "File truncated to per-file token limit"
                        );
                    }

                    files_for_restoration.push(FileRestoration {
                        path,
                        content,
                        priority: 1, // Default priority
                        tokens,
                        last_accessed,
                    });
                }
                Err(e) => {
                    debug!(path = %path.display(), error = %e, "Failed to read file for restoration");
                }
            }
        }

        // Limit to configured max files
        if files_for_restoration.len() > file_config.max_files as usize {
            // Sort by last_accessed descending (most recent first)
            files_for_restoration.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));
            files_for_restoration.truncate(file_config.max_files as usize);
        }

        files_for_restoration
    }

    /// Restore context after compaction.
    ///
    /// This method restores important files, skills, and task status that were
    /// tracked before compaction. Files are prioritized by recency and importance.
    ///
    /// # Arguments
    /// * `invoked_skills` - Skills that were invoked before compaction
    /// * `task_status` - Task status restoration data
    pub(crate) async fn restore_context_after_compaction(
        &mut self,
        invoked_skills: &[InvokedSkillRestoration],
        task_status: &TaskStatusRestoration,
    ) {
        // Get file restoration config
        let file_config = &self.compact_config.file_restoration;

        // Run file collection and plan reading in parallel (both are async I/O)
        let plan_path = self.plan_mode_state.plan_file_path.clone();
        let plan_fut = async {
            if let Some(path) = &plan_path {
                tokio::fs::read_to_string(path).await.ok()
            } else {
                None
            }
        };
        let (files_for_restoration, plan) =
            tokio::join!(self.collect_restorable_tracked_files(file_config), plan_fut,);

        // Build todo list from task status, structured tasks, and cron jobs.
        // Include structured tasks and cron state so they survive compaction.
        let mut todo_parts: Vec<String> = Vec::new();

        if !task_status.tasks.is_empty() {
            let todo_text = task_status
                .tasks
                .iter()
                .map(|t| format!("- [{}] {}: {}", t.status, t.id, t.subject))
                .collect::<Vec<_>>()
                .join("\n");
            todo_parts.push(todo_text);
        }

        // Include structured tasks state in restoration
        if let Some(ref tasks_val) = self.current_structured_tasks
            && let Some(tasks_map) = tasks_val.as_object()
            && !tasks_map.is_empty()
        {
            let mut task_text = String::from("Structured Tasks:\n");
            for task in tasks_map.values() {
                let status = task["status"].as_str().unwrap_or("pending");
                if status == "deleted" {
                    continue;
                }
                let id = task["id"].as_str().unwrap_or("?");
                let subject = task["subject"].as_str().unwrap_or("?");
                task_text.push_str(&format!("- [{status}] {id}: {subject}\n"));
            }
            todo_parts.push(task_text);
        }

        // Include cron jobs state in restoration
        if let Some(ref jobs_val) = self.current_cron_jobs
            && let Some(jobs_map) = jobs_val.as_object()
            && !jobs_map.is_empty()
        {
            let mut cron_text = String::from("Scheduled Cron Jobs:\n");
            for job in jobs_map.values() {
                let id = job["id"].as_str().unwrap_or("?");
                let schedule = job["schedule"].as_str().unwrap_or("?");
                let desc = job["description"]
                    .as_str()
                    .or_else(|| job["prompt"].as_str())
                    .unwrap_or("?");
                cron_text.push_str(&format!("- {id}: [{schedule}] {desc}\n"));
            }
            todo_parts.push(cron_text);
        }

        let todos = if todo_parts.is_empty() {
            None
        } else {
            Some(todo_parts.join("\n"))
        };

        // Build skills list from invoked skills
        let skills: Vec<String> = invoked_skills.iter().map(|s| s.name.clone()).collect();

        // Mark that a plan file reference should be injected on the next turn
        // so the model knows the plan still exists after compaction
        if plan.is_some() {
            self.plan_mode_state.needs_plan_reference = true;
        }

        // Build context restoration
        let restoration = build_context_restoration_with_config(
            files_for_restoration,
            todos,
            plan,
            skills,
            file_config,
        );

        // Transfer compacted large file references so CompactFileReferenceGenerator
        // can notify the model on the next turn (one-shot drain pattern)
        self.pending_compacted_large_files = restoration.compacted_large_files.clone();

        // Format and inject restoration message if there's content to restore
        let restoration_message = format_restoration_message(&restoration);
        if !restoration_message.is_empty() {
            let files_count = restoration.files.len();
            debug!(
                files_restored = files_count,
                has_todos = restoration.todos.is_some(),
                has_plan = restoration.plan.is_some(),
                skills_count = restoration.skills.len(),
                "Context restoration completed"
            );

            // Rebuild FileTracker from restored files (Claude Code alignment: C4)
            // After compaction, the tracker must reflect the restored context only
            if !restoration.files.is_empty() {
                self.rebuild_trackers_from_restored_files(&restoration.files)
                    .await;
            }

            // Emit context restoration event
            self.emit(LoopEvent::ContextRestored {
                files_count: files_count as i32,
                has_todos: restoration.todos.is_some(),
                has_plan: restoration.plan.is_some(),
            })
            .await;
        }
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
    /// to reflect only the files referenced in the surviving history. This is
    /// the shared logic used by both `compact()` and
    /// `apply_session_memory_summary()`.
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
    async fn finalize_compaction_tracking(
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
    async fn emit_invoked_skills_restored(&self, invoked_skills: &[InvokedSkillRestoration]) {
        if !invoked_skills.is_empty() {
            let skill_names: Vec<String> = invoked_skills.iter().map(|s| s.name.clone()).collect();
            self.emit(LoopEvent::InvokedSkillsRestored {
                skills: skill_names,
            })
            .await;
        }
    }
}
