//! Context restoration after compaction: session memory, file collection, and restoration.

use cocode_message::TrackedMessage;
use cocode_message::Turn;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::LoopEvent;

use tracing::debug;
use tracing::info;

use crate::compaction::FileRestoration;
use crate::compaction::FileRestorationConfig;
use crate::compaction::InvokedSkillRestoration;
use crate::compaction::SessionMemorySummary;
use crate::compaction::TaskStatusRestoration;
use crate::compaction::build_context_restoration_with_config;
use crate::compaction::find_session_memory_boundary;
use crate::compaction::format_restoration_with_tasks;
use crate::compaction::format_summary_with_transcript;
use crate::compaction::is_internal_file;
use crate::compaction::map_message_index_to_keep_turns;
use crate::compaction::wrap_hook_additional_context;
use crate::compaction::write_session_memory;

use super::AgentLoop;

impl AgentLoop {
    /// Apply a cached session memory summary (Tier 1 compaction).
    ///
    /// This is the zero-cost compaction path that uses a previously saved summary
    /// instead of making an LLM API call. The summary is stored in the session memory
    /// file and can be reused across conversation continuations.
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
        let restoration_message = format_restoration_with_tasks(&restoration, None);
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

    /// Save the final summary to session memory for future Tier 1 compaction.
    pub(crate) fn spawn_session_memory_write(&self, final_summary: String, turn_id: &str) {
        if self.compact_config.enable_sm_compact
            && let Some(ref path) = self.compact_config.summary_path
        {
            let turn_id_owned = turn_id.to_string();
            let path_owned = path.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    write_session_memory(&path_owned, &final_summary, &turn_id_owned).await
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
                hook_type: cocode_protocol::HookEventType::PostCompact,
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
}
