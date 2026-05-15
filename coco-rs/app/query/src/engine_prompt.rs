//! Prompt + tool-definition + factory builders for [`QueryEngine`].
//!
//! Owns the per-turn assembly that feeds the LLM call:
//! - [`QueryEngine::build_prompt`] — system prompt (config or CLAUDE.md
//!   discovery) + staged-collapse application + history normalization.
//! - [`QueryEngine::build_tool_definitions`] — `LanguageModelTool` schemas
//!   resolved per-turn so Agent/Skill tools can inject live runtime state.
//! - [`QueryEngine::tool_context_factory`] — `ToolContextFactory` carrying
//!   the structured hook handle + every shared resource the executor reads.
//! - [`QueryEngine::observe_date_change`] — local-date rollover latch driving
//!   the `date_change` system reminder.
//!
//! Extracted from `engine.rs` so the multi-turn loop stays focused on flow
//! control rather than per-turn data marshalling.

use coco_inference::LanguageModelFunctionTool;
use coco_inference::LanguageModelTool;
use coco_inference::ToolResultContent as LlmToolResultContent;
use tracing::info;

use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_messages::ToolContent;
use coco_messages::ToolResultMessage;
use coco_types::ToolAppState;

use crate::engine::QueryEngine;

impl QueryEngine {
    /// Build the LLM prompt from message history.
    pub(crate) async fn build_prompt(&self, history: &MessageHistory) -> Vec<LlmMessage> {
        let mut prompt = Vec::new();

        // System prompt assembly:
        //
        //   1. If `coco_subagent::is_coordinator_mode(features)` is on, the
        //      leader becomes a coordinator and uses the coordinator-mode
        //      system prompt verbatim (TS `coordinatorMode.ts:110-300`).
        //      The `simple_mode` toggle (TS `CLAUDE_CODE_SIMPLE`) maps to
        //      `EnvKey::CocoSimple` and narrows the worker tool list.
        //   2. Otherwise: explicit config override > built-in default +
        //      CLAUDE.md discovery.
        let system_text = if coco_subagent::is_coordinator_mode(&self.config.features) {
            let simple_mode = coco_config::env::is_env_truthy(coco_config::EnvKey::CocoSimple);
            coco_subagent::coordinator_system_prompt(simple_mode)
        } else if let Some(ref sys) = self.config.system_prompt {
            sys.clone()
        } else {
            let mut text =
                String::from("You are coco, an AI coding assistant. Be concise and helpful.\n\n");
            let cwd = std::env::current_dir().unwrap_or_default();
            let claude_files = coco_context::discover_memory_files(&cwd);
            for f in &claude_files {
                text.push_str(&format!("# {}\n{}\n\n", f.path.display(), f.content));
            }
            text
        };
        prompt.push(LlmMessage::system(&system_text));

        // Pre-build hook: apply staged-collapse commits so each
        // archived range is a single placeholder rather than full turns.
        // TS: query.ts:441 `applyCollapsesIfNeeded()` runs before every
        // prompt build. No-op when collapse is inactive.
        let mut messages_for_api: Vec<coco_messages::Message> = if self.is_collapse_active() {
            if let Some(ledger) = &self.staged_ledger {
                let commits: Vec<_> = match ledger.try_lock() {
                    Ok(g) => g.commits.clone(),
                    Err(_) => Vec::new(),
                };
                let (collapsed, applied) =
                    coco_compact::apply_collapses_if_needed(&history.messages, &commits);
                if applied > 0 {
                    info!(applied, "applied {applied} staged collapses to prompt");
                }
                collapsed
            } else {
                history.messages.clone()
            }
        } else {
            history.messages.clone()
        };

        self.apply_tool_result_budget_to_prompt(&mut messages_for_api)
            .await;

        // Convert history to LlmMessages
        let normalized = coco_messages::normalize_messages_for_api(&messages_for_api);
        prompt.extend(normalized);

        prompt
    }

    async fn apply_tool_result_budget_to_prompt(&self, messages: &mut [Message]) {
        let budget = &self.config.compact.tool_result_budget;
        if !budget.enabled {
            return;
        }

        let Some(session_dir) = self.tool_result_session_dir_for_prompt() else {
            return;
        };

        {
            let mut state = self.tool_result_replacement_state.write().await;
            state.per_message_chars = budget.per_message_chars;
        }

        for group in collect_api_user_tool_result_groups(messages) {
            let mut candidates = Vec::new();
            for idx in &group {
                let Message::ToolResult(tr) = &messages[*idx] else {
                    continue;
                };
                let Some(projected) = project_tool_result_content(tr) else {
                    continue;
                };
                let persistence_opted_out = self
                    .tools
                    .get(&tr.tool_id)
                    .is_some_and(|tool| tool.max_result_size_bound().is_unbounded());
                candidates.push(
                    coco_tool_runtime::tool_result_storage::ToolResultCandidate {
                        tool_use_id: tr.tool_use_id.clone(),
                        content_chars: projected.content.len() as i64,
                        content: projected.content,
                        tool_name: Some(tr.tool_id.to_string()),
                        persistence_opted_out,
                        is_json: projected.is_json,
                    },
                );
            }
            if candidates.is_empty() {
                continue;
            }

            let outcome = coco_tool_runtime::tool_result_storage::apply_tool_result_budget(
                &candidates,
                &self.tool_result_replacement_state,
                &session_dir,
            )
            .await;

            if budget.persist_records
                && !outcome.newly_replaced.is_empty()
                && let (Some(store), Some(session_id)) =
                    (&self.transcript_store, &self.transcript_session_id)
            {
                let records: Vec<coco_session::ContentReplacementRecord> = outcome
                    .newly_replaced
                    .iter()
                    .map(|r| {
                        coco_session::ContentReplacementRecord::tool_result(
                            r.tool_use_id.clone(),
                            r.replacement.clone(),
                        )
                    })
                    .collect();
                if let Err(e) = store.insert_content_replacement(session_id, &records) {
                    tracing::warn!(
                        error = %e,
                        "failed to persist tool-result content replacement records"
                    );
                }
            }

            let replacements = {
                let state = self.tool_result_replacement_state.read().await;
                candidates
                    .iter()
                    .filter_map(|c| {
                        state
                            .replacements
                            .get(&c.tool_use_id)
                            .map(|replacement| (c.tool_use_id.clone(), replacement.clone()))
                    })
                    .collect::<std::collections::HashMap<_, _>>()
            };
            if replacements.is_empty() {
                continue;
            }
            for idx in &group {
                let Message::ToolResult(tr) = &mut messages[*idx] else {
                    continue;
                };
                if let Some(replacement) = replacements.get(&tr.tool_use_id) {
                    replace_tool_result_content(tr, replacement);
                }
            }
        }
    }

    fn tool_result_session_dir_for_prompt(&self) -> Option<std::path::PathBuf> {
        if let (Some(store), Some(session_id)) =
            (&self.transcript_store, &self.transcript_session_id)
        {
            return Some(store.session_artifact_dir(session_id));
        }
        self.config_home
            .as_ref()
            .map(|home| home.join("sessions").join(&self.config.session_id))
    }

    /// Build tool definitions for the LLM (function tool schemas).
    ///
    /// TS parity: each `Tool::prompt(&PromptOptions)` call returns the
    /// description the model sees that turn. Agent/Skill tools use
    /// this hook to inject live runtime state (current agent / skill
    /// listings) into their description. For tools that don't
    /// override `prompt`, the trait default delegates to
    /// `description()`, preserving the legacy behavior.
    pub(crate) async fn build_tool_definitions(
        &self,
        app_state: &ToolAppState,
    ) -> Vec<LanguageModelTool> {
        // Carry the `ToolSearch` discovery set into the filter pipeline
        // so deferred tools the model has unlocked get their schema in
        // this turn's request.
        let discovered = std::sync::Arc::new(app_state.discovered_tool_names.clone());

        // Resolve both `ToolSearch`-related capabilities from the
        // active client's `ModelInfo`. Three-state outcome:
        //   - server (Anthropic Sonnet 4.5+/Opus 4+ via beta
        //     `tool-search-tool-2025-10-19`): tools array carries every
        //     enabled tool with `deferLoading: true` on deferred ones.
        //     Server expands `tool_reference` content blocks into
        //     `<functions>` markup inline — `tools` shape is constant
        //     across turns, prompt-cache prefix stays warm.
        //   - client-side only (capable models without server beta —
        //     GPT-5, Gemini, DeepSeek, Haiku, …): tools array contains
        //     only the loaded set; deferred tools enter on the next
        //     turn after `ToolSearch` writes
        //     `discovered_tool_names`. One cache break per discovery.
        //   - neither (unknown / custom model that didn't declare
        //     either capability): the registry filter auto-disables
        //     deferral via `tool_search_active`, so every enabled
        //     tool's full schema lands on turn 1 (safe default).
        let supports_tool_reference = self.client.model_info().is_some_and(|info| {
            info.has_capability(coco_types::Capability::ServerSideToolReference)
        });
        let supports_client_side_tool_search = self
            .client
            .model_info()
            .is_some_and(|info| info.has_capability(coco_types::Capability::ClientSideToolSearch));

        let stub_ctx = coco_tool_runtime::ToolUseContext::stub_for_filtering(
            self.config.features.clone(),
            self.config.tool_overrides.clone(),
            self.config.tool_filter.clone(),
            self.config.permission_mode,
        )
        .with_discovered_tool_names(discovered.clone())
        .with_model_capabilities(supports_tool_reference, supports_client_side_tool_search);

        // The tool list sent to the model. When the server-side path
        // is live (capability declared AND `Feature::ToolSearch` on),
        // `enabled` includes deferred tools too; `deferred_marker`
        // captures which names need the `deferLoading` provider-option
        // patch below. Otherwise (client-side path OR feature off OR
        // capability missing), `loaded_tools` handles the partition
        // — its short-circuit on `tool_search_active` covers the
        // capability-missing case automatically.
        let use_server_side_path = supports_tool_reference && stub_ctx.tool_search_active();
        let (model_tools, deferred_marker): (Vec<_>, std::collections::HashSet<String>) =
            if use_server_side_path {
                let enabled = self.tools.enabled(&stub_ctx);
                let deferred: std::collections::HashSet<String> = self
                    .tools
                    .deferred_tools(&stub_ctx)
                    .iter()
                    .map(|t| t.name().to_string())
                    .collect();
                (enabled, deferred)
            } else {
                (
                    self.tools.loaded_tools(&stub_ctx),
                    std::collections::HashSet::new(),
                )
            };
        let tool_names: Vec<String> = model_tools.iter().map(|t| t.name().to_string()).collect();

        let agent_names: Vec<String> = self
            .session_bootstrap
            .as_ref()
            .map(|b| b.agents.clone())
            .unwrap_or_default();
        let skill_names: Vec<String> = {
            let mut names = self
                .session_bootstrap
                .as_ref()
                .map(|b| b.skills.clone())
                .unwrap_or_default();
            names.sort();
            names
        };

        let permission_mode = app_state
            .permission_mode
            .unwrap_or(self.config.permission_mode);
        let permission_context = coco_types::ToolPermissionContext {
            mode: permission_mode,
            additional_dirs: self.config.session_additional_dirs.clone(),
            allow_rules: std::collections::HashMap::new(),
            deny_rules: std::collections::HashMap::new(),
            ask_rules: std::collections::HashMap::new(),
            bypass_available: self.config.bypass_permissions_available,
            pre_plan_mode: app_state.pre_plan_mode,
            stripped_dangerous_rules: app_state.stripped_dangerous_rules.clone(),
            session_plan_file: None,
        };
        // Round 7: thread the agent catalog + connected MCP servers
        // through to PromptOptions so AgentTool::prompt can render
        // the dynamic per-agent listing with MCP-availability filter.
        // TS parity: `AgentTool.tsx:218-225` filters agents by
        // `mcpServersWithTools` before passing to `getPrompt`.
        let agent_catalog = self.agent_catalog.clone();
        // Read the MCP-ready set off the engine's installed handle
        // (`with_mcp_handle` at session bootstrap). When unset
        // (tests / minimal embeddings), pass `None` to the renderer
        // which then SKIPS the MCP-availability filter rather than
        // hiding everything — closer to TS behaviour where an empty
        // mcp set still lets non-MCP-required agents through.
        let ready_mcp_servers = self.mcp_servers_ready_snapshot().await;
        let coordinator_mode = coco_subagent::is_coordinator_mode(self.config.features.as_ref());
        let fork_enabled = coco_subagent::is_fork_subagent_active(
            &self.config.features,
            self.config.is_non_interactive,
        );
        // TS parity: `isPlanModeInterviewPhaseEnabled()` —
        // settings-only in coco-rs (no Growthbook, no
        // `USER_TYPE=ant`, no env var). See `core/context/CLAUDE.md`.
        let is_plan_interview_phase = matches!(
            self.config.plan_mode_settings.workflow,
            coco_config::PlanModeWorkflow::Interview
        );

        // TS `prompt.ts:222-231,259-283` — these flags shape the
        // model-visible AgentTool description. We resolve them from
        // env / config / runtime state and let
        // `AgentToolPromptRenderer` swap section bodies accordingly.
        let background_tasks_disabled =
            coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBackgroundTasksDisable);
        let agent_list_via_attachment =
            coco_config::env::is_env_truthy(coco_config::EnvKey::CocoAgentListInMessages);
        // 3p builds (the default) keep `ant_build` off — coco-rs ships
        // only worktree isolation. The flag is wired here so an internal
        // build can flip it via config without re-rendering callers.
        let ant_build = false;
        // `has_embedded_search_tools` is host-build dependent. Coco-rs
        // ships the dedicated `Glob`/`Grep` tools, so the flag stays
        // off — the AgentTool description points at them rather than
        // `find` / `grep` via Bash.
        let has_embedded_search_tools = false;
        // Subscription tier and teammate flags do not yet have a
        // resolved source in coco-rs. Defaulting to `false` keeps the
        // inline concurrency hint and full subagent prompt — the most
        // permissive 3p shape.
        let is_pro_subscription = false;
        let is_in_process_teammate = false;
        let is_teammate = false;

        let prompt_options = coco_tool_runtime::PromptOptions {
            is_non_interactive: self.config.is_non_interactive,
            tool_names,
            agent_names,
            allowed_agent_types: None,
            skill_names,
            permission_context: Some(permission_context),
            agent_catalog,
            ready_mcp_servers,
            coordinator_mode,
            fork_enabled,
            is_plan_interview_phase,
            has_embedded_search_tools,
            is_in_process_teammate,
            is_teammate,
            agent_list_via_attachment,
            is_pro_subscription,
            background_tasks_disabled,
            ant_build,
        };

        let mut out = Vec::with_capacity(model_tools.len());
        for tool in model_tools {
            // `Tool::input_json_schema` returns a fully-formed JSON Schema
            // when the tool ships one. Otherwise we synthesize one from
            // `Tool::input_schema`'s loose `properties` map and wrap it
            // as `{"type":"object","properties":{...}}` — strict
            // providers (DeepSeek, OpenAI Responses on some models)
            // reject schemas missing a top-level `type`. Without the
            // wrap they fail the request with HTTP 400.
            let json_schema = tool.input_json_schema().unwrap_or_else(|| {
                let schema = tool.input_schema();
                let props = serde_json::to_value(&schema.properties)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                serde_json::json!({ "type": "object", "properties": props })
            });
            let description = tool.prompt(&prompt_options).await;
            // Attach `deferLoading: true` to tools the model has not
            // yet discovered via `ToolSearch`. Anthropic adapter reads
            // this in `prepare_tools::prepare_anthropic_tools` and
            // emits `defer_loading: true` on the wire so the server
            // hides the schema from the model. Other providers ignore
            // unknown provider_options blocks — no-op for them.
            let tool_name = tool.name();
            let provider_options = if deferred_marker.contains(tool_name) {
                let mut anthropic = std::collections::HashMap::new();
                anthropic.insert("deferLoading".to_string(), serde_json::Value::Bool(true));
                let mut po_map = std::collections::HashMap::new();
                po_map.insert("anthropic".to_string(), anthropic);
                Some(coco_inference::ProviderOptions(po_map))
            } else {
                None
            };
            out.push(LanguageModelTool::Function(LanguageModelFunctionTool {
                name: tool_name.to_string(),
                description: Some(description),
                input_schema: json_schema,
                input_examples: None,
                strict: None,
                provider_options,
            }));
        }
        out
    }

    /// Build a factory that knows how to construct [`ToolUseContext`]
    /// snapshots from the engine's current config + shared handles.
    ///
    /// Each turn calls `factory.build(...)` to get a fresh context; the
    /// factory re-reads live `ToolAppState` per call so permission-mode
    /// mutations from a prior batch (e.g. `EnterPlanMode`) propagate
    /// without a config reload.
    ///
    /// The field mapping itself — including the five previously-hardcoded
    /// fields (`thinking_level`, `is_non_interactive`, `max_budget_usd`,
    /// `custom_system_prompt`, `append_system_prompt`) — is verified in
    /// `tool_context.test.rs`.
    /// Snapshot the connected MCP server names. Returns the list
    /// emptied to `None` when no MCP handle was installed — the
    /// AgentTool prompt renderer treats absent ready_mcp_servers as
    /// "no filter applies" rather than "filter everything out".
    pub(crate) async fn mcp_servers_ready_snapshot(&self) -> Option<Vec<String>> {
        let handle = self.mcp_handle.as_ref()?;
        Some(handle.connected_servers().await)
    }

    pub(crate) fn tool_context_factory(
        &self,
        hook_tx: Option<&tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
    ) -> crate::tool_context::ToolContextFactory {
        // Build the structured hook handle here — every tool call built
        // through this factory gets the same `QueryHookHandle`, so
        // PreToolUse/PostToolUse/PostToolUseFailure fire consistently
        // regardless of the call site. When the session has no hook
        // registry (tests / single-turn helpers) we pass `None` and the
        // `ToolUseContext` receives no handle — executor treats that as
        // a no-op, matching legacy behavior.
        let hook_handle = self.hooks.as_ref().map(|registry| {
            let handle: coco_tool_runtime::HookHandleRef =
                std::sync::Arc::new(crate::hook_adapter::QueryHookHandle::new(
                    registry.clone(),
                    self.orchestration_ctx(),
                    hook_tx.cloned(),
                ));
            handle
        });
        crate::tool_context::ToolContextFactory {
            config: self.config.clone(),
            tools: self.tools.clone(),
            cancel: self.cancel.clone(),
            mailbox: self.mailbox.clone(),
            task_list: self.task_list.clone(),
            todo_list: self.todo_list.clone(),
            task_handle: self.task_handle.clone(),
            permission_bridge: self.permission_bridge.clone(),
            app_state: self.app_state.clone(),
            file_read_state: self.file_read_state.clone(),
            file_history: self.file_history.clone(),
            config_home: self.config_home.clone(),
            tool_result_session_dir: self.tool_result_session_dir_for_prompt(),
            hook_handle,
            // Real `AgentHandle` when the CLI / SDK / TUI installed
            // one via `with_agent_handle`; otherwise fall back to
            // `NoOpAgentHandle`.
            agent_handle: self.agent_handle.clone(),
            // `SkillHandle` same pattern — real handle when
            // installed, `NoOpSkillHandle` otherwise.
            skill_handle: self.skill_handle.clone(),
            // `LspHandle` same pattern — real handle when installed via
            // `with_lsp_handle` at session bootstrap; otherwise
            // `NoOpLspHandle` so `is_connected() = false` and `LspTool`
            // is filtered out of the model's tool list.
            lsp_handle: self.lsp_handle.clone(),
            // `McpHandle` is installed via `with_mcp_handle` from
            // `SessionRuntime.wire_engine` (which reads the late-bound
            // slot populated by `install_session_late_binds`).
            // Without this, MCP tools (`McpAuthTool`, list/read
            // resources, dynamic `McpTool`) fall back to
            // `NoOpMcpHandle` and surface "not configured" errors.
            mcp_handle: self.mcp_handle.clone(),
            // Session-scoped schema validator. Clone is cheap —
            // inner state is `Arc<RwLock<HashMap>>` shared across
            // per-turn ctx rebuilds so the compile cache persists.
            tool_schema_validator: Some(self.tool_schema_validator.clone()),
            // Agent definition catalog snapshot (T7). When the
            // bootstrap installed one via `with_agent_catalog`,
            // AgentTool reads `subagent_type → AgentDefinition` from
            // here at the spawn boundary.
            agent_catalog: self.agent_catalog.clone(),
            // Per-engine live Command-source rule store. Same Arc as
            // `QueryEngine.live_command_rules` and the
            // `EngineLiveRulesHandle` installed on the executor —
            // factory.build merges it into `allow_rules[Command]` per
            // batch. See `engine_live_rules` for the lifecycle.
            live_command_rules: self.live_command_rules.clone(),
        }
    }

    /// Detect local-date rollover for the `date_change` system reminder.
    ///
    /// Reads `ToolAppState::last_emitted_date`, compares it to today's
    /// local ISO date, and:
    ///
    /// - seeds the latch on first observation, returning `None`
    ///   (no reminder — TS `getDateChangeAttachments` matches: the first
    ///   turn of a session never emits because there's no prior date);
    /// - returns `Some(today)` and updates the latch on a mismatch
    ///   (engine passes it to `TurnReminderInput.new_date` and the
    ///   `DateChangeGenerator` emits once);
    /// - returns `None` when the latch already matches today.
    ///
    /// No-op (returns `None`) when `self.app_state` is `None`.
    pub(crate) async fn observe_date_change(&self) -> Option<String> {
        let state = self.app_state.as_ref()?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut guard = state.write().await;
        match guard.last_emitted_date.as_deref() {
            Some(prev) if prev == today => None,
            Some(_) => {
                guard.last_emitted_date = Some(today.clone());
                Some(today)
            }
            None => {
                // First observation: seed without emitting.
                guard.last_emitted_date = Some(today);
                None
            }
        }
    }
}

#[derive(Debug)]
struct ProjectedToolResultContent {
    content: String,
    is_json: bool,
}

fn collect_api_user_tool_result_groups(messages: &[Message]) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut seen_assistant_ids = std::collections::HashSet::new();

    for (idx, msg) in messages.iter().enumerate() {
        match msg {
            Message::Assistant(asst) => {
                let assistant_id = asst
                    .request_id
                    .clone()
                    .unwrap_or_else(|| asst.uuid.to_string());
                if seen_assistant_ids.insert(assistant_id) && !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            }
            Message::ToolResult(_) => current.push(idx),
            Message::User(_)
            | Message::System(_)
            | Message::Attachment(_)
            | Message::Progress(_)
            | Message::Tombstone(_)
            | Message::ToolUseSummary(_) => {}
        }
    }

    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn project_tool_result_content(tr: &ToolResultMessage) -> Option<ProjectedToolResultContent> {
    let LlmMessage::Tool { content, .. } = &tr.message else {
        return None;
    };
    let part = content.iter().find_map(|part| match part {
        ToolContent::ToolResult(result) if result.tool_call_id == tr.tool_use_id => Some(result),
        _ => None,
    })?;

    match &part.output {
        LlmToolResultContent::Text { value, .. }
        | LlmToolResultContent::ErrorText { value, .. } => Some(ProjectedToolResultContent {
            content: value.clone(),
            is_json: false,
        }),
        LlmToolResultContent::Json { value, .. }
        | LlmToolResultContent::ErrorJson { value, .. } => {
            let content = serde_json::to_string(value).ok()?;
            Some(ProjectedToolResultContent {
                content,
                is_json: true,
            })
        }
        LlmToolResultContent::ExecutionDenied { reason, .. } => Some(ProjectedToolResultContent {
            content: reason.clone().unwrap_or_default(),
            is_json: false,
        }),
        LlmToolResultContent::Content { value, .. } => {
            let mut texts = Vec::new();
            for part in value {
                match part {
                    coco_inference::ToolResultContentPart::Text { text, .. } => {
                        texts.push(text.clone());
                    }
                    _ => return None,
                }
            }
            Some(ProjectedToolResultContent {
                content: texts.join("\n\n"),
                is_json: false,
            })
        }
    }
}

fn replace_tool_result_content(tr: &mut ToolResultMessage, replacement: &str) -> bool {
    let LlmMessage::Tool { content, .. } = &mut tr.message else {
        return false;
    };
    for part in content.iter_mut() {
        let ToolContent::ToolResult(result) = part else {
            continue;
        };
        if result.tool_call_id != tr.tool_use_id {
            continue;
        }
        result.output = if result.is_error {
            LlmToolResultContent::error_text(replacement)
        } else {
            LlmToolResultContent::text(replacement)
        };
        return true;
    }
    false
}
