//! Turn execution methods for SessionState.
//!
//! Contains `run_turn`, `run_turn_streaming`, `run_skill_turn`, `run_partial_compact`,
//! and the private wiring helpers they depend on (subagent execute/spawn closures,
//! background task collection).

use std::sync::Arc;

use cocode_context::ContextInjection;
use cocode_context::ConversationContext;
use cocode_context::EnvironmentInfo;
use cocode_context::InjectionPosition;
use cocode_hooks::HookDefinition;
use cocode_hooks::HookHandler;
use cocode_hooks::HookSource;
use cocode_loop::AgentLoop;
use cocode_loop::FallbackConfig;
use cocode_loop::LoopConfig;
use cocode_loop::StopReason;
use cocode_protocol::CoreEvent;
use cocode_protocol::RoleSelection;
use cocode_protocol::SubagentType;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;
use cocode_subagent::AgentExecuteParams;
use cocode_subagent::AgentStatus as SubagentStatus;
use cocode_subagent::IsolationMode;
use cocode_system_reminder::BackgroundTaskInfo;
use cocode_system_reminder::BackgroundTaskStatus;
use cocode_system_reminder::BackgroundTaskType;
use tokio::sync::mpsc;
use tracing::info;

use cocode_error::boxed_err;
use snafu::ResultExt;

use super::PartialCompactResult;
use super::SessionState;
use super::TurnResult;
use super::session_state_error;

impl SessionState {
    /// Run a single turn with the given user input.
    ///
    /// This creates an agent loop and runs it to completion,
    /// returning the result of the conversation turn.
    pub async fn run_turn(&mut self, user_input: &str) -> anyhow::Result<TurnResult> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn"
        );

        self.session.touch();

        // Create event channel (run_turn owns it; streaming callers provide their own)
        let (event_tx, mut event_rx) = mpsc::channel::<CoreEvent>(256);
        let cancel_token = self.cancel_token.clone();
        let event_task = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if cancel_token.is_cancelled() {
                    break;
                }
                tracing::debug!(?event, "Session event");
            }
        });

        let context = self
            .build_conversation_context()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.wire_subagent_manager(&event_tx).await;
        self.wire_hook_callbacks();

        let mut builder = self.build_agent_loop_builder(context, event_tx);
        builder = self.wire_optional_builder_fields(builder).await;
        let mut loop_instance = builder.build();
        loop_instance.set_background_agent_tasks(self.collect_background_agent_tasks().await);

        let result = loop_instance.run(user_input).await?;
        self.sync_loop_state(&mut loop_instance, &result).await;

        // Handle plan exit approval injection (run_turn only)
        if let StopReason::PlanModeExit {
            ref allowed_prompts,
            ..
        } = result.stop_reason
            && !allowed_prompts.is_empty()
        {
            self.inject_allowed_prompts(allowed_prompts).await;
            info!(
                count = allowed_prompts.len(),
                "Injected allowed prompts from plan exit into approval store"
            );
        }

        drop(loop_instance);
        let _ = event_task.await;

        Ok(TurnResult::from_loop_result(&result))
    }

    /// Run a skill turn with optional model override.
    ///
    /// When `model_override` is provided, temporarily switches the main model
    /// for this turn. The model override can be:
    /// - A full spec like "provider/model"
    /// - A short name like "sonnet" (resolved using current provider)
    pub async fn run_skill_turn(
        &mut self,
        prompt: &str,
        model_override: Option<&str>,
    ) -> anyhow::Result<TurnResult> {
        // If model override is requested, temporarily switch the main selection
        let saved_selection = if let Some(model_name) = model_override {
            let current = self.session.selections.get(ModelRole::Main).cloned();
            let spec = if model_name.contains('/') {
                model_name
                    .parse::<cocode_protocol::model::ModelSpec>()
                    .map_err(|e| anyhow::anyhow!("Invalid model spec '{model_name}': {e}"))?
            } else {
                // Use current provider with the given model name
                let provider = self.provider().to_string();
                cocode_protocol::model::ModelSpec::new(provider, model_name)
            };
            info!(
                model = %spec,
                "Overriding model for skill turn"
            );
            self.session
                .selections
                .set(ModelRole::Main, RoleSelection::new(spec));
            current
        } else {
            None
        };

        let result = self.run_turn(prompt).await;

        // Restore original selection if we overrode it
        if let Some(original) = saved_selection {
            self.session.selections.set(ModelRole::Main, original);
        } else if model_override.is_some() {
            // Edge case: there was no previous main selection (shouldn't happen)
            // Just leave the new one in place
        }

        result
    }

    /// Run a skill turn with optional model override, streaming events.
    ///
    /// Same as [`run_skill_turn`] but forwards events to the provided channel.
    pub async fn run_skill_turn_streaming(
        &mut self,
        prompt: &str,
        model_override: Option<&str>,
        event_tx: mpsc::Sender<CoreEvent>,
    ) -> Result<TurnResult, cocode_error::BoxedError> {
        let saved_selection = if let Some(model_name) = model_override {
            let current = self.session.selections.get(ModelRole::Main).cloned();
            let spec = if model_name.contains('/') {
                model_name
                    .parse::<cocode_protocol::model::ModelSpec>()
                    .context(session_state_error::InvalidModelSpecSnafu {
                        model_name: model_name.to_string(),
                    })
                    .map_err(boxed_err)?
            } else {
                let provider = self.provider().to_string();
                cocode_protocol::model::ModelSpec::new(provider, model_name)
            };
            info!(
                model = %spec,
                "Overriding model for skill turn (streaming)"
            );
            self.session
                .selections
                .set(ModelRole::Main, RoleSelection::new(spec));
            current
        } else {
            None
        };

        let result = self.run_turn_streaming(prompt, event_tx).await;

        if let Some(original) = saved_selection {
            self.session.selections.set(ModelRole::Main, original);
        }

        result
    }

    /// Spawn a subagent for a skill with `context: fork`.
    ///
    /// Bridges the skill execution layer to the subagent manager,
    /// converting model name -> `ExecutionIdentity` and invoking `spawn_full`.
    pub async fn spawn_subagent_for_skill(
        &mut self,
        agent_type: &str,
        prompt: &str,
        model: Option<&str>,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<cocode_tools::SpawnAgentResult> {
        let identity = model.map(ExecutionIdentity::parse_model_string);

        let spawn_input = cocode_subagent::SpawnInput {
            agent_type: agent_type.to_string(),
            prompt: prompt.to_string(),
            identity,
            max_turns: None,
            run_in_background: Some(false),
            allowed_tools,
            resume_from: None,
            name: None,
            team_name: None,
            mode: None,
            cwd: None,
            isolation_override: None,
            description: None,
        };

        let mut mgr = self.subagent_manager.lock().await;
        let result = mgr.spawn_full(spawn_input).await?;

        Ok(cocode_tools::SpawnAgentResult {
            agent_id: result.agent_id,
            output: result.output,
            output_file: result.background.as_ref().map(|bg| bg.output_file.clone()),
            cancel_token: result.cancel_token,
            color: result.color,
        })
    }

    // ==========================================================
    // Streaming Turn API
    // ==========================================================

    /// Run a single turn with the given user input, streaming events to the provided channel.
    ///
    /// This is similar to `run_turn` but forwards all events to the provided channel
    /// instead of handling them internally. This enables real-time streaming to a TUI
    /// or other consumer.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tokio::sync::mpsc;
    /// use cocode_protocol::CoreEvent;
    ///
    /// let (event_tx, mut event_rx) = mpsc::channel::<CoreEvent>(256);
    ///
    /// // Spawn task to handle events
    /// tokio::spawn(async move {
    ///     while let Some(event) = event_rx.recv().await {
    ///         // Process event (update TUI, etc.)
    ///     }
    /// });
    ///
    /// let result = state.run_turn_streaming("Hello!", event_tx).await?;
    /// ```
    pub async fn run_turn_streaming(
        &mut self,
        user_input: &str,
        event_tx: mpsc::Sender<CoreEvent>,
    ) -> Result<TurnResult, cocode_error::BoxedError> {
        self.run_turn_streaming_with_content(
            vec![cocode_inference::UserContentPart::text(user_input)],
            event_tx,
        )
        .await
    }

    /// Run a single streaming turn with multimodal content (text + images).
    pub async fn run_turn_streaming_with_content(
        &mut self,
        content: Vec<cocode_inference::UserContentPart>,
        event_tx: mpsc::Sender<CoreEvent>,
    ) -> Result<TurnResult, cocode_error::BoxedError> {
        info!(
            session_id = %self.session.id,
            content_blocks = content.len(),
            "Running turn with streaming"
        );

        self.session.touch();

        let context = self.build_conversation_context().map_err(|e| {
            Box::new(cocode_error::PlainError::new(
                e.to_string(),
                cocode_error::StatusCode::Internal,
            )) as cocode_error::BoxedError
        })?;
        self.wire_subagent_manager(&event_tx).await;
        self.wire_hook_callbacks();

        let mut builder = self.build_agent_loop_builder(context, event_tx);
        builder = self.wire_optional_builder_fields(builder).await;

        // Wire full system prompt override (SDK mode, streaming only)
        if let Some(ref prompt) = self.system_prompt_override {
            builder = builder.custom_system_prompt(prompt.clone());
        }

        let mut loop_instance = builder.build();
        loop_instance.set_background_agent_tasks(self.collect_background_agent_tasks().await);

        let result = loop_instance
            .run_with_content(content)
            .await
            .map_err(boxed_err)?;
        self.sync_loop_state(&mut loop_instance, &result).await;

        Ok(TurnResult::from_loop_result(&result))
    }

    /// Run partial compaction (summarize) from a specific turn onward.
    ///
    /// This summarizes the conversation from `from_turn_number` to the end,
    /// replacing those turns with an LLM-generated summary while keeping all
    /// earlier turns intact.
    ///
    /// If `user_context` is provided, it is included in the summarization prompt
    /// to guide what the summary should focus on.
    pub async fn run_partial_compact(
        &mut self,
        from_turn_number: i32,
        event_tx: mpsc::Sender<CoreEvent>,
        user_context: Option<&str>,
    ) -> anyhow::Result<PartialCompactResult> {
        info!(
            from_turn_number,
            ?user_context,
            "Running partial compaction (summarize from turn)"
        );

        let _ = event_tx
            .send(CoreEvent::Protocol(
                cocode_protocol::server_notification::ServerNotification::CompactionStarted(
                    cocode_protocol::server_notification::CompactionStartedParams {},
                ),
            ))
            .await;

        // 1. Build conversation text from turns at/after from_turn_number.
        // Extract Message objects (role + content) from turns, matching the
        // format used by the existing compact() in core/loop driver.
        let conversation_text: String = self
            .message_history
            .turns()
            .iter()
            .filter(|t| t.number >= from_turn_number)
            .flat_map(|t| {
                let mut msgs = vec![&t.user_message.inner];
                if let Some(ref asst) = t.assistant_message {
                    msgs.push(&asst.inner);
                }
                msgs
            })
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join("\n");

        if conversation_text.is_empty() {
            anyhow::bail!("No turns found at or after turn {from_turn_number}");
        }

        // 2. Build summarization prompt
        let max_output_tokens = 4096;
        let system_prompt = cocode_loop::build_compact_instructions(max_output_tokens);
        let context_instruction = match user_context {
            Some(ctx) if !ctx.is_empty() => {
                format!("\n\nThe user has requested that the summary focus on: {ctx}")
            }
            _ => String::new(),
        };
        let user_prompt = format!(
            "Please summarize the following conversation:\n\n---\n\n{conversation_text}\n\n---\n\nProvide your summary using the required section format.{context_instruction}"
        );

        let summary_messages = vec![
            cocode_inference::LanguageModelMessage::system(&system_prompt),
            cocode_inference::LanguageModelMessage::user_text(&user_prompt),
        ];

        // 3. Call LLM for summary
        let session_id = format!("summarize-{from_turn_number}");
        let turn_count = self.message_history.turn_count();
        let (ctx, compact_model) = self
            .model_hub
            .prepare_compact_with_selections(&self.session.selections, &session_id, turn_count)
            .map_err(|e| anyhow::anyhow!("Failed to prepare compact model: {e}"))?;

        let summary_request = cocode_inference::RequestBuilder::new(ctx)
            .messages(summary_messages)
            .max_tokens(max_output_tokens as u64)
            .build();

        let response = self
            .api_client
            .generate(&*compact_model, summary_request)
            .await
            .map_err(|e| anyhow::anyhow!("Summarization LLM call failed: {e}"))?;

        let summary_text: String = response
            .content
            .iter()
            .filter_map(|b| match b {
                cocode_inference::AssistantContentPart::Text(tp) => Some(tp.text.as_str()),
                _ => None,
            })
            .collect();

        if summary_text.is_empty() {
            anyhow::bail!("Summarization produced empty output");
        }

        // 4. Apply: truncate turns from from_turn_number, store summary
        let pre_tokens = self.message_history.estimate_tokens();

        // Calculate keep_turns: number of turns AFTER the summarized portion
        // (apply_compaction_with_metadata keeps the LAST keep_turns turns).
        // We want to keep turns BEFORE from_turn_number, so:
        //   keep_turns = number of turns with number < from_turn_number
        let keep_turns = self
            .message_history
            .turns()
            .iter()
            .filter(|t| t.number < from_turn_number)
            .count() as i32;

        // Use the standard compaction path, which:
        // - Stores the summary
        // - Removes older turns (keeping the last `keep_turns`)
        // - Records compaction boundary
        //
        // NOTE: apply_compaction_with_metadata keeps the LAST N turns.
        // For partial compact, we want the FIRST N turns (before from_turn).
        // So instead, we truncate from from_turn and set the summary directly.
        self.message_history.truncate_from_turn(from_turn_number);

        // Append to or replace the compacted summary
        let existing = self.message_history.compacted_summary().map(String::from);
        let final_summary = match existing {
            Some(prev) => format!("{prev}\n\n---\n\n{summary_text}"),
            None => summary_text,
        };
        let remaining_turns = self.message_history.turn_count();
        self.message_history.apply_compaction_with_metadata(
            final_summary,
            remaining_turns,
            "summarize",
            pre_tokens.saturating_sub(self.message_history.estimate_tokens()),
            cocode_protocol::CompactTrigger::Manual,
            pre_tokens,
            None,
            true,
        );

        let post_tokens = self.message_history.estimate_tokens();

        // 5. Set compaction boundary on snapshot manager
        if let Some(ref sm) = self.snapshot_manager {
            sm.set_compaction_boundary(from_turn_number).await;
        }

        let _ = event_tx
            .send(CoreEvent::Protocol(
                cocode_protocol::server_notification::ServerNotification::ContextCompacted(
                    cocode_protocol::server_notification::ContextCompactedParams {
                        removed_messages: 0,
                        summary_tokens: post_tokens,
                    },
                ),
            ))
            .await;

        // Rebuild todos and file tracker after partial compaction
        // This ensures state consistency with the retained history
        self.rebuild_todos_from_history();
        self.rebuild_reminder_file_tracker_from_history();

        info!(
            from_turn_number,
            pre_tokens, post_tokens, keep_turns, "Partial compaction completed"
        );

        Ok(PartialCompactResult {
            from_turn: from_turn_number,
            summary_tokens: post_tokens,
        })
    }

    // ==========================================================
    // Shared Turn Helpers
    // ==========================================================

    /// Build the conversation context used by both `run_turn` and `run_turn_streaming_with_content`.
    fn build_conversation_context(&self) -> anyhow::Result<ConversationContext> {
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .context_window(self.context_window)
            .max_output_tokens(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        let mut ctx_builder = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .injections(self.build_suffix_injections());

        if let Some(style_config) = self.resolve_output_style() {
            ctx_builder = ctx_builder.output_style(style_config);
        }

        ctx_builder = self.apply_sandbox_context(ctx_builder);
        ctx_builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))
    }

    /// Wire the subagent manager with fresh per-turn state.
    async fn wire_subagent_manager(&self, event_tx: &mpsc::Sender<CoreEvent>) {
        let execute_fn = self.build_execute_fn(event_tx.clone());
        let mut mgr = self.subagent_manager.lock().await;
        mgr.set_execute_fn(execute_fn);
        mgr.set_all_tools(self.tool_registry.tool_names());
        mgr.set_event_tx(event_tx.clone());
        mgr.set_background_stop_hook_fn(self.build_background_stop_hook_fn());
    }

    /// Wire hook callbacks for LLM model-call and agent-spawn handlers.
    fn wire_hook_callbacks(&self) {
        let spawn_fn_for_model = self.build_spawn_agent_fn();
        self.hook_registry.set_model_call_fn(std::sync::Arc::new(
            move |system_prompt: String, user_message: String| {
                let spawn_fn = spawn_fn_for_model.clone();
                Box::pin(async move {
                    let combined = format!("{system_prompt}\n\n{user_message}");
                    let input = cocode_tools::SpawnAgentInput {
                        agent_type: SubagentType::Explore.as_str().to_string(),
                        prompt: combined,
                        max_turns: Some(1),
                        allowed_tools: Some(vec![]),
                        ..Default::default()
                    };
                    let result = spawn_fn(input).await.map_err(|e| e.to_string())?;
                    Ok(result.output.unwrap_or_default())
                })
            },
        ));

        let spawn_fn_for_agent = self.build_spawn_agent_fn();
        self.hook_registry.set_agent_fn(std::sync::Arc::new(
            move |prompt: String, allowed_tools: Vec<String>, max_turns: i32| {
                let spawn_fn = spawn_fn_for_agent.clone();
                Box::pin(async move {
                    let input = cocode_tools::SpawnAgentInput {
                        agent_type: SubagentType::Explore.as_str().to_string(),
                        prompt,
                        max_turns: Some(max_turns),
                        allowed_tools: Some(allowed_tools),
                        ..Default::default()
                    };
                    let result = spawn_fn(input).await.map_err(|e| e.to_string())?;
                    Ok(result.output.unwrap_or_default())
                })
            },
        ));
    }

    /// Build the agent loop builder with all shared configuration.
    fn build_agent_loop_builder(
        &self,
        context: ConversationContext,
        event_tx: mpsc::Sender<CoreEvent>,
    ) -> cocode_loop::AgentLoopBuilder {
        AgentLoop::builder(
            self.api_client.clone(),
            self.model_hub.clone(),
            self.session.selections.clone(),
            self.tool_registry.clone(),
            context,
            event_tx,
        )
        .config(self.loop_config.clone())
        .fallback_config(FallbackConfig::default())
        .hooks(self.hook_registry.clone())
        .cancel_token(self.cancel_token.clone())
        .queued_commands(self.queued_commands.clone())
        .fast_mode(self.fast_mode.clone())
        .features(self.config.features.clone())
        .web_search_config(self.config.web_search_config.clone())
        .web_fetch_config(self.config.web_fetch_config.clone())
        .permission_rules(self.permission_rules.clone())
        .shell_executor(self.shell_executor.clone())
        .maybe_sandbox_state(self.sandbox_state.clone())
        .skill_manager(self.skill_manager.clone())
        .otel_manager(self.otel_manager.clone())
        .lsp_manager(self.lsp_manager.clone())
        .spawn_agent_fn(self.build_spawn_agent_fn())
        .plan_mode_state(self.plan_mode_state.clone())
        .question_responder(self.question_responder.clone())
        .approval_store(self.shared_approval_store.clone())
        .reminder_file_tracker_state(self.reminder_file_tracker_state.clone())
        .message_history(self.message_history.clone())
        .cocode_home(self.config.cocode_home.clone())
        .killed_agents(self.killed_agents.clone())
        .auto_memory_state(Arc::clone(&self.auto_memory_state))
        .team_store(Arc::clone(&self.team_store))
        .team_mailbox(Arc::clone(&self.team_mailbox))
    }

    /// Wire optional builder fields (snapshot, permissions, previous selections, extraction).
    async fn wire_optional_builder_fields(
        &self,
        mut builder: cocode_loop::AgentLoopBuilder,
    ) -> cocode_loop::AgentLoopBuilder {
        if let Some(ref prev) = self.previous_turn_selections {
            builder = builder.previous_selections(prev.clone());
        }
        if let Some(ref sm) = self.snapshot_manager {
            builder = builder.snapshot_manager(sm.clone());
        }
        if let Some(ref requester) = self.permission_requester {
            builder = builder.permission_requester(requester.clone());
        }
        if let Some(ref coordinator) = self.auto_memory_extraction {
            builder = builder.auto_memory_extraction(Arc::clone(coordinator));
        }

        // Build agent memory directory map from registered agent definitions.
        let agent_memory_dirs = self.build_agent_memory_dirs().await;
        if !agent_memory_dirs.is_empty() {
            builder = builder.agent_memory_dirs(agent_memory_dirs);
        }

        builder
    }

    /// Resolve agent memory directories from registered agent definitions.
    ///
    /// Iterates all agent definitions that have a `MemoryScope` and resolves
    /// each to a concrete directory path based on the scope:
    /// - User: `{cocode_home}/agent-memory/{agent_type}/`
    /// - Project: `{working_dir}/.cocode/agent-memory/{agent_type}/`
    /// - Local: `{working_dir}/.cocode/agent-memory-local/{agent_type}/`
    async fn build_agent_memory_dirs(
        &self,
    ) -> std::collections::HashMap<String, std::path::PathBuf> {
        let mgr = self.subagent_manager.lock().await;
        let mut dirs = std::collections::HashMap::new();
        for def in mgr.definitions() {
            if let Some(ref scope) = def.memory {
                let dir = scope.resolve_dir(
                    &self.config.cocode_home,
                    &self.session.working_dir,
                    &def.agent_type,
                );
                dirs.insert(def.agent_type.clone(), dir);
            }
        }
        dirs
    }

    /// Sync state from a completed loop back to this session.
    async fn sync_loop_state(
        &mut self,
        loop_instance: &mut AgentLoop,
        result: &cocode_loop::LoopResult,
    ) {
        self.previous_turn_selections = Some(self.session.selections.clone());

        if let Some(todos) = loop_instance.take_todos() {
            self.todos = todos;
        }
        if let Some(tasks) = loop_instance.take_structured_tasks() {
            self.structured_tasks = tasks;
        }
        if let Some(jobs) = loop_instance.take_cron_jobs() {
            self.cron_jobs = jobs;
        }

        self.message_history = loop_instance.message_history().clone();

        if let Some(plan_state) = loop_instance.take_plan_mode_state() {
            self.plan_mode_state = plan_state;
        }

        self.reminder_file_tracker_state = loop_instance.reminder_file_tracker_snapshot().await;

        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;
    }

    // ==========================================================
    // Subagent Wiring
    // ==========================================================

    /// Build the `AgentExecuteFn` closure that the `SubagentManager` calls
    /// to actually run a child `AgentLoop`.
    ///
    /// Captures per-turn state snapshots so the child loop is isolated.
    /// The `parent_event_tx` is forwarded to the child loop so subagent
    /// progress, text deltas, and tool activity are visible to the TUI.
    pub(crate) fn build_execute_fn(
        &self,
        parent_event_tx: mpsc::Sender<CoreEvent>,
    ) -> cocode_subagent::AgentExecuteFn {
        let api_client = self.api_client.clone();
        let model_hub = self.model_hub.clone();
        let tool_registry = self.tool_registry.clone();
        let hook_registry = self.hook_registry.clone();
        let shell_executor = self.shell_executor.clone();
        let sandbox_state = self.sandbox_state.clone();
        let working_dir = self.session.working_dir.clone();
        let cocode_home = self.config.cocode_home.clone();
        let context_window = self.context_window;
        let features = self.config.features.clone();
        let web_search_config = self.config.web_search_config.clone();
        let web_fetch_config = self.config.web_fetch_config.clone();
        let permission_rules = self.permission_rules.clone();
        let skill_manager = self.skill_manager.clone();
        let lsp_manager = self.lsp_manager.clone();
        let selections = self.session.selections.clone();
        let message_history = self.message_history.clone();
        let team_store = Arc::clone(&self.team_store);
        let team_mailbox = Arc::clone(&self.team_mailbox);
        // Capture parent plan state for subagent propagation
        let parent_plan_state = self.plan_mode_state.clone();

        Box::new(move |params: AgentExecuteParams| {
            let api_client = api_client.clone();
            let model_hub = model_hub.clone();
            let tool_registry = tool_registry.clone();
            let hook_registry = hook_registry.clone();
            let shell_executor = shell_executor.clone();
            let sandbox_state = sandbox_state.clone();
            let working_dir = working_dir.clone();
            let cocode_home = cocode_home.clone();
            let features = features.clone();
            let web_search_config = web_search_config.clone();
            let web_fetch_config = web_fetch_config.clone();
            let permission_rules = permission_rules.clone();
            let skill_manager = skill_manager.clone();
            let lsp_manager = lsp_manager.clone();
            let selections = selections.clone();
            let message_history = message_history.clone();
            let parent_event_tx = parent_event_tx.clone();
            let team_store = team_store.clone();
            let team_mailbox = team_mailbox.clone();
            let parent_plan_state = parent_plan_state.clone();

            Box::pin(async move {
                // ── CWD override from spawn input ──────────────────────
                let base_working_dir = if let Some(ref cwd_override) = params.cwd {
                    std::path::PathBuf::from(cwd_override)
                } else {
                    working_dir.clone()
                };

                // ── G5: Worktree isolation ──────────────────────────────
                let (effective_working_dir, worktree_path) = if params.isolation
                    == Some(IsolationMode::Worktree)
                {
                    let wt_path = base_working_dir
                        .join(".cocode")
                        .join("worktrees")
                        .join(format!(
                            "{}-{}",
                            params.agent_type,
                            uuid::Uuid::new_v4().simple()
                        ));
                    let output = tokio::process::Command::new("git")
                        .args(["worktree", "add", "--detach", &wt_path.to_string_lossy()])
                        .current_dir(&base_working_dir)
                        .output()
                        .await;
                    match output {
                        Ok(o) if o.status.success() => {
                            tracing::info!(
                                path = %wt_path.display(),
                                agent_type = %params.agent_type,
                                "Created git worktree for agent isolation"
                            );
                            (wt_path.clone(), Some(wt_path))
                        }
                        Ok(o) => {
                            tracing::warn!(
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                "Failed to create worktree, falling back to shared CWD"
                            );
                            (base_working_dir.clone(), None)
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Failed to run git worktree command, falling back to shared CWD"
                            );
                            (base_working_dir.clone(), None)
                        }
                    }
                } else {
                    (base_working_dir.clone(), None)
                };

                // Fork shell executor for isolated CWD tracking
                let forked_shell = shell_executor.fork_for_subagent(effective_working_dir.clone());

                // Build environment info for the child
                let environment = EnvironmentInfo::builder()
                    .cwd(&effective_working_dir)
                    .context_window(context_window)
                    .max_output_tokens(16_384)
                    .build()
                    .map_err(cocode_error::boxed_err)?;

                // GAP-3: Resolve preloaded skills into context injections
                let mut injections = Vec::new();
                if !params.skills.is_empty() {
                    for skill_name in &params.skills {
                        if let Some(skill) = skill_manager.get(skill_name) {
                            injections.push(ContextInjection {
                                label: format!("agent-skill:{skill_name}"),
                                content: skill.prompt.clone(),
                                position: InjectionPosition::EndOfPrompt,
                            });
                            tracing::debug!(
                                agent_type = %params.agent_type,
                                skill = %skill_name,
                                "Preloaded skill into subagent context"
                            );
                        } else {
                            tracing::warn!(
                                agent_type = %params.agent_type,
                                skill = %skill_name,
                                "Skill not found for preload, skipping"
                            );
                        }
                    }
                }

                let mut ctx_builder = ConversationContext::builder()
                    .environment(environment)
                    .tool_names(tool_registry.tool_names());
                if !injections.is_empty() {
                    ctx_builder = ctx_builder.injections(injections);
                }

                // Wire sandbox fields for subagent system prompt
                if let Some(ref state) = sandbox_state
                    && state.is_active()
                {
                    let settings = state.settings();
                    let enforcement_desc = format!("{:?}", state.enforcement());
                    ctx_builder = ctx_builder.sandbox(
                        /*active=*/ true,
                        /*enforcement_desc=*/ Some(enforcement_desc),
                        /*allow_unsandboxed=*/ settings.allow_unsandboxed_commands,
                        /*network_desc=*/
                        if state.network_active() {
                            Some("Proxy-filtered".to_string())
                        } else {
                            Some("Allowed".to_string())
                        },
                    );
                }

                let context = ctx_builder.build().map_err(cocode_error::boxed_err)?;

                // ── G1: Memory injection ───────────────────────────────
                let mut effective_prompt = params.prompt.clone();
                if let Some(ref scope) = params.memory {
                    let memory_dir =
                        scope.resolve_dir(&cocode_home, &effective_working_dir, &params.agent_type);
                    tokio::fs::create_dir_all(&memory_dir).await.ok();
                    let memory_file = memory_dir.join("MEMORY.md");
                    if memory_file.exists() {
                        match tokio::fs::read_to_string(&memory_file).await {
                            Ok(content) => {
                                let truncated: String =
                                    content.lines().take(200).collect::<Vec<_>>().join("\n");
                                if !truncated.is_empty() {
                                    effective_prompt = format!(
                                        "## Agent Memory\n\n{truncated}\n\n{effective_prompt}"
                                    );
                                    tracing::debug!(
                                        agent_type = %params.agent_type,
                                        memory_lines = content.lines().count().min(200),
                                        "Injected agent memory into prompt"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    path = %memory_file.display(),
                                    "Failed to read agent MEMORY.md"
                                );
                            }
                        }
                    }
                }

                // ── G2: Skills filtering ───────────────────────────────
                if !params.skills.is_empty() {
                    let mut skill_prefix = String::new();
                    for skill_name in &params.skills {
                        if let Some(skill) = skill_manager.get(skill_name) {
                            skill_prefix.push_str(&format!(
                                "\n<skill name=\"{skill_name}\">\n{}\n</skill>\n",
                                skill.prompt
                            ));
                        } else {
                            tracing::warn!(
                                skill = %skill_name,
                                agent_type = %params.agent_type,
                                "Skill not found for agent"
                            );
                        }
                    }
                    if !skill_prefix.is_empty() {
                        effective_prompt = format!("{skill_prefix}\n{effective_prompt}");
                        tracing::debug!(
                            agent_type = %params.agent_type,
                            skills = ?params.skills,
                            "Injected skill prompts into agent prompt"
                        );
                    }
                }

                // Resolve selections: env var override > params.identity > parent
                // COCODE_SUBAGENT_MODEL env var takes highest priority
                let env_identity = std::env::var("COCODE_SUBAGENT_MODEL")
                    .ok()
                    .map(|m| ExecutionIdentity::parse_model_string(&m));
                let effective_identity = env_identity.as_ref().or(params.identity.as_ref());
                let child_selections = if let Some(identity) = effective_identity {
                    let mut sel = selections.clone();
                    match identity {
                        ExecutionIdentity::Role(role) => {
                            // Use the model from the specified role
                            if let Some(role_sel) = sel.get(*role).cloned() {
                                sel.set(ModelRole::Main, role_sel);
                            }
                        }
                        ExecutionIdentity::Spec(spec) => {
                            sel.set(ModelRole::Main, RoleSelection::new(spec.clone()));
                        }
                        ExecutionIdentity::Inherit => {
                            // Keep parent selections as-is
                        }
                    }
                    sel
                } else {
                    selections.clone()
                };

                // Child loop config with permission mode from agent definition
                let child_config = LoopConfig {
                    max_turns: Some(params.max_turns.unwrap_or(10)),
                    permission_mode: params.permission_mode.unwrap_or_default(),
                    ..LoopConfig::default()
                };

                // ── G3: Agent-scoped hook registration ─────────────────
                let hook_group_id = format!(
                    "agent-{}-{}",
                    params.agent_type,
                    uuid::Uuid::new_v4().simple()
                );
                let has_agent_hooks = params.hooks.is_some();
                if let Some(ref agent_hooks) = params.hooks {
                    let hook_defs: Vec<HookDefinition> = agent_hooks
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, h)| {
                            // Remap Stop → SubagentStop
                            let event_str = if h.event == "Stop" || h.event == "stop" {
                                "SubagentStop"
                            } else {
                                &h.event
                            };
                            let event_type =
                                match event_str.parse::<cocode_protocol::HookEventType>() {
                                    Ok(et) => et,
                                    Err(e) => {
                                        tracing::warn!(
                                            event = %h.event,
                                            error = %e,
                                            "Skipping agent hook with unknown event type"
                                        );
                                        return None;
                                    }
                                };
                            let matcher = h
                                .matcher
                                .as_ref()
                                .map(|m| cocode_hooks::HookMatcher::Regex { pattern: m.clone() });
                            Some(HookDefinition {
                                name: format!("{hook_group_id}-hook-{idx}"),
                                event_type,
                                matcher,
                                handler: HookHandler::Command {
                                    command: h.command.clone(),
                                },
                                source: HookSource::Session,
                                enabled: true,
                                timeout_secs: h.timeout.unwrap_or(30) as i32,
                                once: false,
                                status_message: None,
                                group_id: None, // set by register_group
                                is_async: false,
                                force_sync_execution: false,
                            })
                        })
                        .collect();
                    if !hook_defs.is_empty() {
                        hook_registry.register_group(&hook_group_id, hook_defs);
                        tracing::debug!(
                            group_id = %hook_group_id,
                            agent_type = %params.agent_type,
                            "Registered agent-scoped hooks"
                        );
                    }
                }

                // Build the child loop — forward parent event_tx so the TUI
                // sees subagent progress, text deltas, and tool activity.
                let mut builder = AgentLoop::builder(
                    api_client,
                    model_hub,
                    child_selections,
                    tool_registry,
                    context,
                    parent_event_tx,
                )
                .config(child_config)
                .fallback_config(FallbackConfig::default())
                .hooks(hook_registry.clone())
                .cancel_token(params.cancel_token)
                .features(features)
                .web_search_config(web_search_config)
                .web_fetch_config(web_fetch_config)
                .permission_rules(permission_rules)
                .shell_executor(forked_shell)
                .maybe_sandbox_state(sandbox_state)
                .skill_manager(skill_manager)
                .lsp_manager(lsp_manager)
                .is_subagent(true)
                .task_type_restrictions(params.task_type_restrictions)
                .cocode_home(cocode_home.clone())
                .team_store(team_store.clone())
                .team_mailbox(team_mailbox.clone());
                // NO .spawn_agent_fn() — prevents infinite recursion

                // Propagate parent plan state to subagent so
                // SubagentPlanReminderGenerator can reference the plan file.
                if parent_plan_state.is_active {
                    let mut child_plan_state = cocode_plan_mode::PlanModeState::new();
                    child_plan_state.is_active = true;
                    if let Some(ref path) = parent_plan_state.plan_file_path {
                        child_plan_state.plan_file_path = Some(path.clone());
                    }
                    builder = builder.plan_mode_state(child_plan_state);
                }

                // Wire auto memory from parent into child loop
                if let Some(ref state) = params.auto_memory_state {
                    builder = builder.auto_memory_state(Arc::clone(state));
                }

                // Apply custom system prompt if the agent definition requested it
                if let Some(ref custom_prompt) = params.custom_system_prompt {
                    builder = builder.custom_system_prompt(custom_prompt.clone());
                }

                // Apply system prompt suffix (critical_reminder at system prompt level)
                if let Some(ref suffix) = params.system_prompt_suffix {
                    builder = builder.system_prompt_suffix(suffix.clone());
                }

                // Fork parent context if requested
                if params.fork_context {
                    builder = builder.message_history(message_history.clone());
                }

                let mut loop_instance = builder.build();

                // ── Agent identity propagation ─────────────────────────
                let parent = cocode_subagent::current_agent();
                let parent_id = parent.as_ref().map(|a| a.agent_id.clone());
                let parent_depth = parent.as_ref().map_or(0, |a| a.depth);
                let identity = cocode_subagent::AgentIdentity {
                    agent_id: uuid::Uuid::new_v4().to_string(),
                    agent_type: params.agent_type.clone(),
                    parent_agent_id: parent_id,
                    depth: parent_depth + 1,
                    name: params.name.clone(),
                    team_name: params.team_name.clone(),
                    color: params.color.clone(),
                    plan_mode_required: params.plan_mode_required,
                };
                let result = cocode_subagent::CURRENT_AGENT
                    .scope(identity, loop_instance.run(&effective_prompt))
                    .await;

                // ── G3: Unregister agent-scoped hooks ──────────────────
                if has_agent_hooks {
                    hook_registry.unregister_group(&hook_group_id);
                    tracing::debug!(
                        group_id = %hook_group_id,
                        "Unregistered agent-scoped hooks"
                    );
                }

                // ── G5: Worktree cleanup ───────────────────────────────
                if let Some(ref wt_path) = worktree_path {
                    let remove_output = tokio::process::Command::new("git")
                        .args(["worktree", "remove", "--force", &wt_path.to_string_lossy()])
                        .current_dir(&base_working_dir)
                        .output()
                        .await;
                    match remove_output {
                        Ok(o) if o.status.success() => {
                            tracing::info!(
                                path = %wt_path.display(),
                                "Removed git worktree after agent completion"
                            );
                        }
                        Ok(o) => {
                            tracing::warn!(
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                path = %wt_path.display(),
                                "Failed to remove git worktree"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                path = %wt_path.display(),
                                "Failed to run git worktree remove"
                            );
                        }
                    }
                }

                let result = result.map_err(cocode_error::boxed_err)?;
                Ok(result.final_text)
            })
        })
    }

    /// Collect background agent task info from the subagent manager.
    ///
    /// Called before each `loop_instance.run()` to populate the system reminder
    /// feedback loop with current agent statuses.
    pub(crate) async fn collect_background_agent_tasks(&self) -> Vec<BackgroundTaskInfo> {
        let mut mgr = self.subagent_manager.lock().await;
        let killed = self.killed_agents.lock().await;

        // Promote Failed → Killed for agents explicitly stopped via TaskStop.
        // After cancellation the completion handler marks them Failed; this
        // upgrades to Killed so GC, status reporting, and match arms work.
        if !killed.is_empty() {
            mgr.promote_killed(&killed);
        }

        // Auto-GC stale agents (completed/failed/killed for >5 min).
        mgr.gc_stale(std::time::Duration::from_secs(300));

        // Read delta output from background agents since last read.
        let deltas = mgr.read_deltas().await;
        let delta_map: std::collections::HashMap<String, String> = deltas.into_iter().collect();

        let agent_infos = mgr.agent_infos();

        // Mark completed agents as notified after including them in system reminders.
        for info in &agent_infos {
            if info.status.is_terminal() && !info.parent_notified {
                mgr.mark_notified(&info.id);
            }
        }

        agent_infos
            .into_iter()
            .map(|info| {
                let delta = delta_map.get(&info.id);
                let is_completed = info.status.is_terminal();
                BackgroundTaskInfo {
                    task_id: info.id,
                    task_type: BackgroundTaskType::AsyncAgent,
                    command: info.name.unwrap_or_else(|| info.agent_type.clone()),
                    status: match info.status {
                        SubagentStatus::Running | SubagentStatus::Backgrounded => {
                            BackgroundTaskStatus::Running
                        }
                        SubagentStatus::Completed => BackgroundTaskStatus::Completed,
                        SubagentStatus::Failed | SubagentStatus::Killed => {
                            BackgroundTaskStatus::Failed
                        }
                    },
                    exit_code: None,
                    has_new_output: delta.is_some(),
                    progress_message: None,
                    is_completion_notification: is_completed,
                    delta_summary: delta.cloned(),
                    description: None,
                }
            })
            .collect()
    }

    /// Build the callback for firing SubagentStop hooks when background agents complete.
    pub(crate) fn build_background_stop_hook_fn(&self) -> cocode_subagent::BackgroundStopHookFn {
        let hook_registry = self.hook_registry.clone();
        let session_id = self.session.id.clone();
        let cwd = self.session.working_dir.clone();

        Arc::new(move |agent_type: String, agent_id: String| {
            let hook_registry = hook_registry.clone();
            let session_id = session_id.clone();
            let cwd = cwd.clone();
            Box::pin(async move {
                let hook_ctx = cocode_hooks::HookContext::new(
                    cocode_hooks::HookEventType::SubagentStop,
                    session_id,
                    cwd,
                )
                .with_metadata("agent_type", agent_type)
                .with_metadata("agent_id", agent_id);
                let outcomes = hook_registry.execute(&hook_ctx).await;
                for outcome in &outcomes {
                    if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                        tracing::warn!(
                            hook = %outcome.hook_name,
                            %reason,
                            "SubagentStop hook rejected (ignored, background agent already completed)"
                        );
                    }
                }
            })
        })
    }

    /// Build the `SpawnAgentFn` closure that the Task tool calls.
    ///
    /// Bridges `SpawnAgentInput` (tools layer) to `SpawnInput` (subagent layer)
    /// and delegates to `SubagentManager::spawn_full()`.
    pub(crate) fn build_spawn_agent_fn(&self) -> cocode_tools::SpawnAgentFn {
        let subagent_manager = self.subagent_manager.clone();

        Arc::new(move |input: cocode_tools::SpawnAgentInput| {
            let subagent_manager = subagent_manager.clone();

            Box::pin(async move {
                // Model resolution: COCODE_SUBAGENT_MODEL env var (highest
                // priority) → per-invocation model → inherit
                let env_model = std::env::var("COCODE_SUBAGENT_MODEL").ok();
                let effective_model = env_model.as_deref().or(input.model.as_deref());
                let identity = effective_model.map(ExecutionIdentity::parse_model_string);

                let spawn_input = cocode_subagent::SpawnInput {
                    agent_type: input.agent_type,
                    prompt: input.prompt,
                    identity,
                    max_turns: input.max_turns,
                    run_in_background: input.run_in_background,
                    allowed_tools: input.allowed_tools,
                    resume_from: input.resume_from,
                    name: input.name,
                    team_name: input.team_name,
                    mode: input.mode,
                    cwd: input.cwd,
                    isolation_override: input.isolation,
                    description: input.description,
                };

                let mut mgr = subagent_manager.lock().await;
                let result = mgr
                    .spawn_full(spawn_input)
                    .await
                    .map_err(cocode_error::boxed_err)?;

                Ok(cocode_tools::SpawnAgentResult {
                    agent_id: result.agent_id,
                    output: result.output,
                    output_file: result.background.as_ref().map(|bg| bg.output_file.clone()),
                    cancel_token: result.cancel_token,
                    color: result.color,
                })
            })
        })
    }
}
