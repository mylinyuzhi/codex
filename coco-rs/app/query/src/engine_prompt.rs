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
use tracing::info;

use coco_messages::LlmMessage;
use coco_messages::MessageHistory;
use coco_types::ToolAppState;

use crate::engine::QueryEngine;

impl QueryEngine {
    /// Build the LLM prompt from message history.
    pub(crate) fn build_prompt(&self, history: &MessageHistory) -> Vec<LlmMessage> {
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
        let messages_for_api: Vec<coco_messages::Message> = if self.is_collapse_active() {
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

        // Convert history to LlmMessages
        let normalized = coco_messages::normalize_messages_for_api(&messages_for_api);
        prompt.extend(normalized);

        prompt
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
        let stub_ctx = coco_tool_runtime::ToolUseContext::stub_for_filtering(
            self.config.features.clone(),
            self.config.tool_overrides.clone(),
            self.config.tool_filter.clone(),
            self.config.permission_mode,
        );
        let loaded = self.tools.loaded_tools(&stub_ctx);
        let tool_names: Vec<String> = loaded.iter().map(|t| t.name().to_string()).collect();

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
        };

        let mut out = Vec::with_capacity(loaded.len());
        for tool in loaded {
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
            out.push(LanguageModelTool::Function(LanguageModelFunctionTool {
                name: tool.name().to_string(),
                description: Some(description),
                input_schema: json_schema,
                input_examples: None,
                strict: None,
                provider_options: None,
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
            hook_handle,
            // Real `AgentHandle` when the CLI / SDK / TUI installed
            // one via `with_agent_handle`; otherwise fall back to
            // `NoOpAgentHandle`.
            agent_handle: self.agent_handle.clone(),
            // `SkillHandle` same pattern — real handle when
            // installed, `NoOpSkillHandle` otherwise.
            skill_handle: self.skill_handle.clone(),
            // Session-scoped schema validator. Clone is cheap —
            // inner state is `Arc<RwLock<HashMap>>` shared across
            // per-turn ctx rebuilds so the compile cache persists.
            tool_schema_validator: Some(self.tool_schema_validator.clone()),
            // Agent definition catalog snapshot (T7). When the
            // bootstrap installed one via `with_agent_catalog`,
            // AgentTool reads `subagent_type → AgentDefinition` from
            // here at the spawn boundary.
            agent_catalog: self.agent_catalog.clone(),
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
