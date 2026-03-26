//! Tool result processing methods for the agent loop.

use std::sync::Arc;

use cocode_inference::LanguageModelMessage;
use cocode_inference::ToolCall;
use cocode_message::TrackedMessage;
use cocode_message::Turn;
use cocode_protocol::ContextModifier;
use cocode_protocol::ToolResultContent;
use cocode_tools::FileReadState;
use cocode_tools::ToolExecutionResult;
use tracing::debug;

use super::AgentLoop;

impl AgentLoop {
    /// Add tool results to the message history and apply context modifiers.
    ///
    /// This creates proper tool_result messages that link back to the tool_use
    /// blocks via their call_id. The results are added to the current turn
    /// for tracking, and a new turn with tool result messages is created
    /// for the next API call.
    ///
    /// Context modifiers from tool outputs are applied to update:
    /// - `FileTracker`: Records file reads with content and timestamps
    /// - `ApprovalStore`: Records permission grants for future operations
    /// - Queued commands (logged but not yet executed)
    pub(crate) async fn add_tool_results_to_history(
        &mut self,
        results: &[ToolExecutionResult],
        _tool_calls: &[ToolCall],
    ) {
        if results.is_empty() {
            return;
        }

        // Collect all modifiers from successful tool executions
        let mut all_modifiers: Vec<ContextModifier> = Vec::new();

        // Add tool results to current turn for tracking
        for result in results {
            let (output, is_error) = match &result.result {
                Ok(output) => {
                    // Collect modifiers from successful executions
                    all_modifiers.extend(output.modifiers.clone());
                    (output.content.clone(), output.is_error)
                }
                Err(e) => (ToolResultContent::Text(e.to_string()), true),
            };
            self.message_history
                .add_tool_result(&result.call_id, &result.name, output, is_error);
        }

        // Apply context modifiers
        if !all_modifiers.is_empty() {
            self.apply_modifiers(&all_modifiers).await;
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

        // Collect images from tool results
        let all_images: Vec<&cocode_protocol::ImageData> = results
            .iter()
            .filter_map(|r| r.result.as_ref().ok())
            .flat_map(|output| &output.images)
            .collect();

        // Create a user message containing the tool results (and images if any)
        // This will be normalized by MessageHistory::messages_for_api() to the correct format
        let user_msg = if all_images.is_empty() {
            TrackedMessage::user(&tool_results_text, &next_turn_id)
        } else {
            let mut content_parts =
                vec![cocode_inference::UserContentPart::text(&tool_results_text)];
            for img in &all_images {
                content_parts.push(cocode_inference::UserContentPart::File(
                    cocode_inference::FilePart::image_base64(&img.data, &img.media_type),
                ));
            }
            let message = LanguageModelMessage::user(content_parts);
            TrackedMessage::new(message, &next_turn_id, cocode_message::MessageSource::User)
        };
        let turn = Turn::new(self.turn_number + 1, user_msg);
        self.message_history.add_turn(turn);
    }

    /// Generate synthetic interrupt results for tool calls that did not complete.
    ///
    /// When the user interrupts a turn, some tool calls may have completed (their
    /// results are already in `completed_call_ids`) while others were aborted or
    /// never started. For each incomplete tool call this method:
    ///
    /// 1. Adds a `TrackedToolCall` to the current turn (UI tracking only).
    /// 2. Builds `<tool_result>` XML text so `messages_for_api()` includes the
    ///    synthetic result — the Anthropic API requires every `tool_use` block
    ///    to have a matching `tool_result`.
    /// 3. Appends a guidance message so the model knows the turn was interrupted.
    ///
    /// Matches Claude Code's `createInterruptToolResults` (`Sp8`) +
    /// `createUserGuidanceMessage` (`Ug`) pattern.
    pub(crate) fn add_interrupt_results_to_history(
        &mut self,
        tool_calls: &[ToolCall],
        completed_call_ids: &std::collections::HashSet<String>,
        tool_use_in_progress: bool,
        completed_results_turn_added: bool,
    ) {
        let interrupt_text = if tool_use_in_progress {
            cocode_protocol::INTERRUPTED_FOR_TOOL_USE
        } else {
            cocode_protocol::INTERRUPTED_BY_USER
        };

        // Filter to only the tool calls that did NOT complete
        let interrupted_calls: Vec<&ToolCall> = tool_calls
            .iter()
            .filter(|tc| !completed_call_ids.contains(&tc.tool_call_id))
            .collect();

        if interrupted_calls.is_empty() {
            // All tools completed, but the turn was still interrupted.
            // Add guidance-only message so the model knows (matches CC's
            // separate Ug call that fires regardless of Sp8 output).
            let turn_id = uuid::Uuid::new_v4().to_string();
            let user_msg = TrackedMessage::user(interrupt_text, &turn_id);
            let turn_offset = if completed_results_turn_added { 2 } else { 1 };
            let turn = Turn::new(self.turn_number + turn_offset, user_msg);
            self.message_history.add_turn(turn);
            return;
        }

        // 1. UI tracking: add to current turn's tool_calls Vec
        for tc in &interrupted_calls {
            self.message_history.add_tool_result(
                &tc.tool_call_id,
                &tc.tool_name,
                ToolResultContent::Text(interrupt_text.to_string()),
                /*is_error=*/ true,
            );
        }

        // 2. API-visible: build <tool_result> XML (mirrors add_tool_results_to_history)
        let full_text = build_interrupt_tool_results_xml(&interrupted_calls, interrupt_text);

        // 3. Create a properly-numbered turn with the user message
        let turn_id = uuid::Uuid::new_v4().to_string();
        let user_msg = TrackedMessage::user(&full_text, &turn_id);
        let turn_offset = if completed_results_turn_added { 2 } else { 1 };
        let turn = Turn::new(self.turn_number + turn_offset, user_msg);
        self.message_history.add_turn(turn);

        debug!(
            interrupted = interrupted_calls.len(),
            completed = completed_call_ids.len(),
            "Added interrupt results to history"
        );
    }

    /// Apply context modifiers from tool execution results.
    ///
    /// This processes modifiers collected from tool outputs and updates the
    /// appropriate stores:
    /// - `FileRead`: Updates the FileTracker with file content and timestamps
    /// - `PermissionGranted`: Updates the ApprovalStore with granted permissions
    pub(crate) async fn apply_modifiers(&mut self, modifiers: &[ContextModifier]) {
        for modifier in modifiers {
            match modifier {
                ContextModifier::FileRead {
                    path,
                    content,
                    file_mtime_ms,
                    offset,
                    limit,
                    read_kind,
                } => {
                    // Convert mtime from ms if provided, otherwise get from filesystem.
                    // Done before acquiring the lock to avoid holding it across I/O.
                    let file_mtime = if let Some(ms) = file_mtime_ms {
                        std::time::UNIX_EPOCH
                            .checked_add(std::time::Duration::from_millis(*ms as u64))
                    } else {
                        tokio::fs::metadata(path)
                            .await
                            .ok()
                            .and_then(|m| m.modified().ok())
                    };
                    let state = match read_kind {
                        cocode_protocol::FileReadKind::FullContent => {
                            FileReadState::complete_with_turn(
                                content.clone(),
                                file_mtime,
                                self.turn_number,
                            )
                        }
                        cocode_protocol::FileReadKind::PartialContent => {
                            FileReadState::partial_with_turn(
                                offset.unwrap_or(0),
                                limit.unwrap_or(0),
                                file_mtime,
                                self.turn_number,
                            )
                        }
                        cocode_protocol::FileReadKind::MetadataOnly => {
                            FileReadState::metadata_only(file_mtime, self.turn_number)
                        }
                    };
                    // Update the shared file tracker with the file read state
                    let tracker = self.shared_tools_file_tracker.lock().await;
                    tracker.track_read(path.clone(), state);
                    debug!(
                        path = %path.display(),
                        content_len = content.len(),
                        read_kind = ?read_kind,
                        "Applied FileRead modifier"
                    );
                }
                ContextModifier::PermissionGranted { tool, pattern } => {
                    // Update the shared approval store with the granted permission
                    let mut store = self.shared_approval_store.lock().await;
                    store.approve_pattern(tool, pattern);
                    debug!(
                        tool = %tool,
                        pattern = %pattern,
                        "Applied PermissionGranted modifier"
                    );
                }
                ContextModifier::SkillAllowedTools {
                    skill_name,
                    allowed_tools,
                } => {
                    // Set skill-level tool restrictions for subsequent tool execution.
                    // Always include "Skill" itself so nested skill invocations work.
                    let mut allowed: std::collections::HashSet<String> =
                        allowed_tools.iter().cloned().collect();
                    allowed.insert(cocode_protocol::ToolName::Skill.as_str().to_string());
                    self.active_skill_allowed_tools = Some(allowed);
                    debug!(
                        skill = %skill_name,
                        tools = ?allowed_tools,
                        "Applied SkillAllowedTools modifier"
                    );
                }
                ContextModifier::TodosUpdated { todos } => {
                    self.current_todos = Some(todos.clone());
                    debug!(
                        count = todos.as_array().map_or(0, std::vec::Vec::len),
                        "Applied TodosUpdated modifier"
                    );
                }
                ContextModifier::StructuredTasksUpdated { tasks } => {
                    self.current_structured_tasks = Some(tasks.clone());
                    debug!("Applied StructuredTasksUpdated modifier");
                }
                ContextModifier::CronJobsUpdated { jobs } => {
                    self.current_cron_jobs = Some(jobs.clone());
                    debug!("Applied CronJobsUpdated modifier");
                }
                ContextModifier::TeamsUpdated { teams } => {
                    // Teams state is tracked for potential future use
                    debug!(
                        count = teams.as_object().map_or(0, serde_json::Map::len),
                        "Applied TeamsUpdated modifier"
                    );
                }
                ContextModifier::ModelOverride { model, skill_name } => {
                    self.model_override = Some(model.clone());
                    debug!(
                        model = %model,
                        skill = %skill_name,
                        "Applied ModelOverride modifier"
                    );
                }
                ContextModifier::DelegateModeChanged { active } => {
                    self.delegate_mode = *active;
                    debug!(active = %active, "Applied DelegateModeChanged modifier");
                }
                ContextModifier::TeammateJoined {
                    team_name,
                    agent_id,
                } => {
                    debug!(
                        team = %team_name,
                        agent = %agent_id,
                        "Applied TeammateJoined modifier"
                    );
                }
                ContextModifier::TeammateLeft {
                    team_name,
                    agent_id,
                } => {
                    debug!(
                        team = %team_name,
                        agent = %agent_id,
                        "Applied TeammateLeft modifier"
                    );
                }
                ContextModifier::RestoreDeferredMcpTools { names } => {
                    // Restore deferred MCP tools into the active registry so
                    // they become callable on subsequent turns.
                    if let Some(registry) = Arc::get_mut(&mut self.tool_registry) {
                        let restored = registry.restore_deferred_tools(names);
                        debug!(
                            count = restored.len(),
                            tools = ?restored,
                            "Restored deferred MCP tools"
                        );
                    } else {
                        debug!(
                            count = names.len(),
                            "Cannot restore deferred MCP tools: registry has other references"
                        );
                    }
                }
                ContextModifier::FileModified { path, content } => {
                    // Notify LSP servers of file modifications so diagnostics
                    // and symbol caches stay in sync with the actual file content.
                    // Sends both didChange (update_file) and didSave (notify_save)
                    // to match Claude Code's behavior.
                    if let Some(ref lsp) = self.lsp_manager {
                        // Clear delivered diagnostic hashes so fresh diagnostics
                        // for the modified file are not suppressed by dedup.
                        lsp.diagnostics().clear_delivered_for_file(path).await;

                        match lsp.get_client(path).await {
                            Ok(client) => {
                                if let Err(e) = client.update_file(path, content).await {
                                    debug!(
                                        path = %path.display(),
                                        error = %e,
                                        "LSP didChange failed (non-fatal)"
                                    );
                                }
                                if let Err(e) = client.notify_save(path).await {
                                    debug!(
                                        path = %path.display(),
                                        error = %e,
                                        "LSP didSave failed (non-fatal)"
                                    );
                                }
                            }
                            Err(e) => {
                                debug!(
                                    path = %path.display(),
                                    error = %e,
                                    "LSP client unavailable for file sync (non-fatal)"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Build `<tool_result>` XML blocks for interrupted tool calls, followed by
/// a guidance message. The format mirrors `add_tool_results_to_history`'s XML
/// (line 77) so `messages_for_api()` includes them in the conversation.
fn build_interrupt_tool_results_xml(
    interrupted_calls: &[&ToolCall],
    interrupt_text: &str,
) -> String {
    if interrupted_calls.is_empty() {
        // Guidance-only (no per-tool blocks)
        return interrupt_text.to_string();
    }

    let tool_results_xml: String = interrupted_calls
        .iter()
        .map(|tc| {
            format!(
                "<tool_result tool_use_id=\"{}\" name=\"{}\">\n{interrupt_text}\n</tool_result>",
                tc.tool_call_id, tc.tool_name
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // Append guidance text after the per-tool results
    format!("{tool_results_xml}\n\n{interrupt_text}")
}

#[cfg(test)]
#[path = "tool_results.test.rs"]
mod tests;
