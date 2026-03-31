//! Context management methods for SessionState.
//!
//! Contains conversation rewind, history truncation, file tracker pruning/rebuilding,
//! todo reconstruction, and context clearing.

use cocode_plan_mode::PlanModeState;

use super::SessionState;

impl SessionState {
    /// Clear conversation context for plan exit (creates new session identity).
    ///
    /// This fires SessionEnd hooks for the old session, replaces it with
    /// a child session (new ID, parent tracking), clears message history,
    /// resets shell CWD, and fires SessionStart hooks for the new session.
    pub async fn clear_context(&mut self) {
        // Fire SessionEnd hooks with the OLD session ID
        let end_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionEnd,
            self.session.id.clone(),
            self.session.working_dir.clone(),
        )
        .with_reason("context_clear");
        self.hook_registry.execute(&end_ctx).await;

        // Create child session (new ID, parent tracking)
        let child = self.session.derive_child();
        self.session = child;

        // Clear message history
        self.message_history.clear();

        // Reset shell CWD to project root
        self.reset_shell_cwd();

        // Reset plan mode state for the fresh session
        self.plan_mode_state = PlanModeState::new();

        // Fire SessionStart hooks with the NEW session ID
        let start_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionStart,
            self.session.id.clone(),
            self.session.working_dir.clone(),
        )
        .with_source("context_clear");
        self.hook_registry.execute(&start_ctx).await;
    }

    /// Apply rewind mode to conversation state.
    ///
    /// Handles the three rewind modes:
    /// - `CodeAndConversation`: Truncate history, rebuild todos/tracker
    /// - `ConversationOnly`: Truncate history only, rebuild todos/tracker
    /// - `CodeOnly`: Keep history, rebuild file tracker from retained history
    ///
    /// File restoration is handled externally by `SnapshotManager` before
    /// this method is called. This method only manages conversation state.
    ///
    /// # Returns
    ///
    /// A tuple of (messages_removed, restored_prompt). The restored_prompt is the
    /// user message text from the rewound turn, which can be used to restore the
    /// UI input field after rewind.
    pub fn apply_rewind_mode_for_turn(
        &mut self,
        rewound_turn: i32,
        mode: cocode_protocol::RewindMode,
    ) -> (i32, Option<String>) {
        match mode {
            cocode_protocol::RewindMode::CodeAndConversation
            | cocode_protocol::RewindMode::ConversationOnly => {
                let (messages_removed, restored_prompt) =
                    self.rewind_conversation_state_from_turn(rewound_turn);
                tracing::debug!(
                    rewound_turn,
                    messages_removed,
                    has_prompt = restored_prompt.is_some(),
                    ?mode,
                    "Applied rewind"
                );
                (messages_removed, restored_prompt)
            }
            cocode_protocol::RewindMode::CodeOnly => {
                self.rebuild_reminder_file_tracker_from_history();
                tracing::debug!(rewound_turn, "Applied CodeOnly rewind");
                (0, None)
            }
        }
    }

    /// Rewind conversation state from a specific turn.
    ///
    /// This is a unified method for conversation state rollback that handles:
    /// - History truncation
    /// - Todos rebuilding
    /// - File tracker state rebuilding
    /// - Prompt capture for UI restoration
    ///
    /// # Arguments
    ///
    /// * `from_turn` - The turn number to rewind from (turns >= from_turn are removed)
    ///
    /// # Returns
    ///
    /// A tuple of (messages_removed, restored_prompt). The restored_prompt is the
    /// user message text from the rewound turn, which can be used to restore the
    /// UI input field after rewind.
    pub fn rewind_conversation_state_from_turn(&mut self, from_turn: i32) -> (i32, Option<String>) {
        // Capture prompt at the rewind turn BEFORE truncating
        let restored_prompt = self
            .message_history
            .turns()
            .iter()
            .find(|t| t.number == from_turn)
            .map(|t| t.user_message.text());

        let messages_removed = self.truncate_history_from_turn(from_turn);
        self.rebuild_todos_from_history();
        self.prune_reminder_file_tracker_for_turn_boundary(from_turn);
        (messages_removed, restored_prompt)
    }

    /// Prune reminder file tracker for turn boundary using merge-based approach.
    ///
    /// Instead of a simple retain, this method:
    /// 1. Rebuilds state from retained history turns
    /// 2. Prunes existing state to entries before the boundary and filters internal files
    /// 3. Merges: rebuilt has priority for same paths
    ///
    /// This handles:
    /// - Same-path overwrite drift (newer dropped reads hiding older retained reads)
    /// - Mention-driven reads that exist only in persisted snapshot
    /// - Internal file exclusion
    ///
    /// # Arguments
    ///
    /// * `boundary_turn` - The turn boundary; entries at or after this turn are removed
    pub fn prune_reminder_file_tracker_for_turn_boundary(&mut self, boundary_turn: i32) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;
        use cocode_system_reminder::merge_file_read_state;
        use cocode_system_reminder::should_skip_tracked_file;

        // 1. Rebuild from retained history turns
        let retained_turns = self.message_history.turns();
        let rebuilt = build_file_read_state_from_modifiers(
            retained_turns
                .iter()
                .filter(|turn| turn.number < boundary_turn)
                .flat_map(|turn| {
                    turn.tool_calls.iter().map(move |tc| {
                        (
                            tc.name.as_str(),
                            tc.modifiers.as_slice(),
                            turn.number,
                            tc.status.is_terminal(),
                        )
                    })
                }),
            100,
        );

        // 2. Prune existing state: keep entries before boundary, filter internal files
        let plan_path = self.plan_mode_state.plan_file_path.as_ref();
        let pruned: Vec<_> = self
            .reminder_file_tracker_state
            .iter()
            .filter(|(_, s)| s.read_turn < boundary_turn)
            .filter(|(p, _)| {
                !should_skip_tracked_file(p, plan_path.map(std::path::PathBuf::as_path), None, &[])
            })
            .cloned()
            .collect();

        // 3. Merge: rebuilt has priority for same paths (more accurate from history)
        self.reminder_file_tracker_state = merge_file_read_state(pruned, rebuilt);
    }

    /// Rebuild reminder file tracker with session memory exclusion.
    ///
    /// Rebuilds file tracker state from message history, filtering out
    /// internal files like session memory and plan files.
    ///
    /// # Arguments
    ///
    /// * `session_memory_path` - Optional path to the session memory file to exclude
    pub fn rebuild_reminder_file_tracker_with_session_memory(
        &mut self,
        session_memory_path: Option<&std::path::PathBuf>,
    ) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;
        use cocode_system_reminder::should_skip_tracked_file;

        let state = build_file_read_state_from_modifiers(
            self.message_history.turns().iter().flat_map(|turn| {
                turn.tool_calls.iter().map(move |tc| {
                    (
                        tc.name.as_str(),
                        tc.modifiers.as_slice(),
                        turn.number,
                        tc.status.is_terminal(),
                    )
                })
            }),
            100,
        );
        let plan_path = self.plan_mode_state.plan_file_path.as_ref();
        self.reminder_file_tracker_state = state
            .into_iter()
            .filter(|(p, _)| {
                !should_skip_tracked_file(
                    p,
                    plan_path.map(std::path::PathBuf::as_path),
                    session_memory_path.map(std::path::PathBuf::as_path),
                    &[],
                )
            })
            .collect();
    }

    /// Truncate message history from a specific turn.
    ///
    /// Removes all turns at or after the given turn number.
    pub(crate) fn truncate_history_from_turn(&mut self, from_turn: i32) -> i32 {
        let before = self.message_history.turn_count();
        self.message_history.truncate_from_turn(from_turn);
        before - self.message_history.turn_count()
    }

    /// Rebuild todos from retained message history.
    ///
    /// Scans through the message history for TodoWrite/TodoUpdate tool calls
    /// in reverse order to find the most recent todo state.
    pub(crate) fn rebuild_todos_from_history(&mut self) {
        let todos = self.reconstruct_todos_from_history();
        self.set_todos(todos);
    }

    /// Reconstruct todos from message history by finding the most recent TodoWrite result.
    ///
    /// Walks turns in reverse to find the most recent TodoWrite tool call
    /// with a successful output, then parses and returns the todo list.
    fn reconstruct_todos_from_history(&self) -> serde_json::Value {
        use cocode_protocol::ToolResultContent;

        let todo_write_name = cocode_protocol::ToolName::TodoWrite.as_str();

        // Walk turns in reverse to find the most recent TodoWrite result
        for turn in self.message_history.turns().iter().rev() {
            for tc in &turn.tool_calls {
                if tc.name == todo_write_name
                    && let Some(ref output) = tc.output
                {
                    match output {
                        ToolResultContent::Text(text) => {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                                return parsed;
                            }
                        }
                        ToolResultContent::Structured(value) => {
                            return value.clone();
                        }
                    }
                }
            }
        }

        // No todos found, return empty array
        serde_json::Value::Array(vec![])
    }

    /// Rebuild reminder file tracker from retained history.
    ///
    /// Extracts `ContextModifier::FileRead` entries from tool calls in the
    /// message history to reconstruct the file tracker state.
    pub(crate) fn rebuild_reminder_file_tracker_from_history(&mut self) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;

        // Build iterator of (tool_name, modifiers, turn_number, is_completed)
        let state = build_file_read_state_from_modifiers(
            self.message_history.turns().iter().flat_map(|turn| {
                turn.tool_calls.iter().map(move |tc| {
                    (
                        tc.name.as_str(),
                        tc.modifiers.as_slice(),
                        turn.number,
                        tc.status.is_terminal(),
                    )
                })
            }),
            100,
        );
        self.reminder_file_tracker_state = state;
    }
}
