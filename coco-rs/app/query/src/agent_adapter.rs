//! Agent query adapter — bridges QueryEngine to AgentQueryEngine trait.
//!
//! TS: runAgent() in tools/AgentTool/runAgent.ts drives the query loop
//! for subagents. This module adapts the existing QueryEngine to provide
//! the same capability via the AgentQueryEngine trait.
//!
//! **Dependency flow**:
//! ```text
//! coco-tool-runtime  (defines AgentQueryEngine trait)
//!     ↓
//! coco-query (this adapter implements it via QueryEngine)
//!     ↓
//! coco-state (SwarmAgentHandle / InProcessTeammateRunner consumes it)
//! ```

use std::sync::Arc;

use coco_tool_runtime::AgentQueryConfig;
use coco_tool_runtime::AgentQueryEngine;
use coco_tool_runtime::AgentQueryResult;
use coco_types::Features;
use coco_types::LlmModelSelection;
use coco_types::ThinkingLevel;
use coco_types::ToolFilter;
use coco_types::ToolOverrides;
use tokio_util::sync::CancellationToken;

use crate::engine::QueryEngine;
use crate::engine::QueryEngineConfig;

/// Factory function type for creating QueryEngine instances.
///
/// Each agent query gets a fresh engine with its own config plus an
/// typed model selection that the factory uses to select the right
/// primary `ApiClient` + fallback chain. `InheritMain` defaults to
/// the parent session's model (TS parity: `runAgent.ts` inherits the
/// parent client unless the agent definition specifies a model).
///
/// The factory is async because production implementations (see
/// `app/cli/src/agent_handle_factory.rs`) need to call into the
/// session runtime's role-client resolver and engine builder, both
/// of which are async. The adapter calls `(factory)(cfg, role).await`
/// from inside `execute_query`, which itself runs in an async context
/// — see `coco_query::agent_adapter::QueryEngineAdapter::execute_query`.
pub type QueryEngineFactory = Arc<
    dyn Fn(
            QueryEngineConfig,
            LlmModelSelection,
            Option<CancellationToken>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = QueryEngine> + Send>>
        + Send
        + Sync,
>;

/// Adapter that wraps QueryEngine to implement AgentQueryEngine.
///
/// Each subagent gets its own `QueryEngineAdapter` with a dedicated
/// QueryEngine instance configured for the agent's model, tools, and budget.
pub struct QueryEngineAdapter {
    /// Factory function to create QueryEngine instances per query.
    engine_factory: QueryEngineFactory,
}

impl QueryEngineAdapter {
    pub fn new(engine_factory: QueryEngineFactory) -> Self {
        Self { engine_factory }
    }
}

#[async_trait::async_trait]
impl AgentQueryEngine for QueryEngineAdapter {
    async fn execute_query(
        &self,
        prompt: &str,
        config: AgentQueryConfig,
    ) -> Result<AgentQueryResult, coco_error::BoxedError> {
        // Resolve the subagent's permission mode. Parent is expected to
        // have applied the TS inheritance rule before calling; we just
        // parse/fall back. TS: runAgent.ts:412-434.
        let permission_mode = config
            .permission_mode
            .as_deref()
            .and_then(|s| {
                serde_json::from_value::<coco_types::PermissionMode>(serde_json::json!(s)).ok()
            })
            .unwrap_or(coco_types::PermissionMode::Default);
        let model_selection = effective_model_selection(&config);
        let engine_model_id = model_selection
            .display_model_id()
            .unwrap_or_else(|| config.model.clone());
        let initial_rule_maps = build_initial_rule_maps(&config.extra_permission_rules);

        let engine_config = QueryEngineConfig {
            max_turns: config.max_turns.unwrap_or(30),
            max_tokens: None,
            prompt_cache: config.prompt_cache.clone(),
            system_prompt: Some(config.system_prompt),
            append_system_prompt: None,
            model_id: engine_model_id,
            permission_mode,
            // Inherit the parent session's bypass capability. TS
            // parity: `spawnUtils.ts:53` / `spawnMultiAgent.ts:223`
            // forward `--dangerously-skip-permissions` to spawned
            // child processes; the in-process analog is this field.
            bypass_permissions_available: config.bypass_permissions_available,
            context_window: config.context_window.unwrap_or(200_000),
            max_output_tokens: config.max_output_tokens.unwrap_or(16_384),
            max_budget_usd: None,
            streaming_tool_execution: true,
            is_non_interactive: true,
            // Subagents inherit the parent's debug/verbose surface only
            // when the parent piped that into `AgentQueryConfig`; today
            // we don't propagate, so default to `false`. TS parity:
            // `toolUseContext.options.{debug,verbose}` is set per-process.
            debug: false,
            verbose: false,
            // Subagent reasoning-effort override (TS parity:
            // `AgentTool.tsx:154-159`). The resolver in
            // `core/subagent/src/spawn_resolution.rs` carries the
            // effort string forward; here we parse it into a
            // `ThinkingLevel` so the engine threads it into
            // `QueryParams.thinking_level` → `PerCallOverrides`. An
            // unrecognized string degrades to `None` (the model's
            // `default_thinking_level` from `ModelInfo` then applies)
            // rather than failing the spawn — surface as a warning
            // path later if it becomes useful.
            thinking_level: config
                .effort
                .as_deref()
                .and_then(|s| s.parse::<ThinkingLevel>().ok()),
            session_id: config.session_id.unwrap_or_default(),
            project_dir: None,
            // Subagent rule maps start empty, then we fold in any
            // `extra_permission_rules` the caller (today: fork-mode
            // `SkillTool` forwarding skill frontmatter `allowed-tools`,
            // plus teammate control updates) wants pre-populated.
            // TS parity: `createGetAppStateWithAllowedTools`
            // (`forkedAgent.ts:147-171`) wraps `getAppState` to inject
            // the same rules into the subagent's evaluation context.
            allow_rules: initial_rule_maps.allow_rules,
            deny_rules: initial_rule_maps.deny_rules,
            ask_rules: initial_rule_maps.ask_rules,
            live_permission_rules: config.live_permission_rules.clone(),
            live_permission_mode: config.live_permission_mode.clone(),
            permission_rule_source_roots: Default::default(),
            session_additional_dirs: Default::default(),
            // Propagate the subagent's cwd_override (set by worktree
            // isolation or explicit `cwd:` input) so the child
            // engine's ToolContextFactory installs it onto every
            // ToolUseContext. Absolute-path tools ignore it; Glob /
            // Grep / Bash operate inside the override.
            cwd_override: config.cwd_override.clone(),
            plans_directory: None,
            agent_id: config.agent_id,
            is_teammate: config.is_teammate,
            is_in_process_teammate: config.is_in_process_teammate,
            plan_mode_required: config.plan_mode_required,
            plan_mode_settings: coco_config::PlanModeSettings::default(),
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
            enable_token_budget_continuation: false,
            compact: coco_config::CompactConfig::default(),
            system_reminder: coco_config::SystemReminderConfig::default(),
            tool_config: coco_config::ToolConfig::default(),
            sandbox_config: coco_config::SandboxSettings::default(),
            // Subagent spawn path does not yet propagate parent sandbox
            // state — `AgentQueryConfig` carries no slot for it. Children
            // run unsandboxed via this entry point; revisit when
            // teammate/swarm flows need parity with the CLI bootstrap.
            sandbox_state: None,
            memory_config: coco_config::MemoryConfig::default(),
            shell_config: coco_config::ShellConfig::default(),
            // Subagent flows don't carry the parent's shell provider
            // (snapshot/session-env/`/env`/shell-prefix). Worktree-isolated
            // subagents set `cwd_override` so the bash tool's spawn already
            // points at the right directory; running without snapshot is
            // an acceptable tradeoff for an isolated transient session.
            shell_provider: None,
            // No session-level CWD persistence for subagents — their cwd
            // is fenced via `cwd_override` and they don't share state
            // with the parent session.
            original_cwd: None,
            session_cwd: None,
            web_fetch_config: coco_config::WebFetchConfig::default(),
            web_search_config: coco_config::WebSearchConfig::default(),
            lsp_config: coco_config::LspConfig::default(),
            // Layer 1 — inherit parent's resolved features. Defaulting
            // to `with_defaults()` would silently re-enable gates the
            // user disabled at the top level (Sandbox, WebSearch, ...).
            // The Option fallback only kicks in when the caller really
            // doesn't have a parent context (no test path takes this
            // branch in production).
            features: config
                .features
                .clone()
                .unwrap_or_else(|| Arc::new(Features::with_defaults())),
            // Layer 2 — inherit parent's resolved tool overrides (filled
            // in by the parent before handing off `AgentQueryConfig`).
            // Falling back to `none()` would WIDEN the set beyond what
            // the active model actually accepts; we'd expose tools the
            // model can't call. The factory may replace this with
            // role-resolved overrides when it builds the child engine.
            tool_overrides: config
                .tool_overrides
                .clone()
                .unwrap_or_else(|| Arc::new(ToolOverrides::none())),
            // Layer 4 — derive the subagent's allow/deny from its
            // AgentDefinition, then narrow against the parent's filter
            // so a child's `allowed_tools` cannot widen what the parent
            // restricted. Empty allow + deny ⇒ filter is permissive on
            // the child side, but `narrow_with(parent)` keeps every
            // parent-side restriction.
            tool_filter: {
                let child = ToolFilter::new(
                    config.allowed_tools.clone(),
                    config.disallowed_tools.clone(),
                );
                match &config.parent_tool_filter {
                    Some(parent) => child.narrow_with(parent),
                    None => child,
                }
            },
            // Sandboxed write fence — propagated as-is. Empty = no fence.
            allowed_write_roots: config.allowed_write_roots.clone(),
            // Subagents inherit the SDK opt-in: stay false by default
            // so background subagent runs don't flood the parent's
            // SDK stream with hook events.
            include_hook_events: false,
            // Subagents get their own mailbox: nothing the parent has
            // queued is relevant to the child's first turn, and a
            // shared mailbox would let the child observe reminders
            // intended for the parent. Cheap: an empty `Mutex<State>`.
            reminder_mailbox: coco_system_reminder::ReminderMailbox::new(),
            // Per-fork canUseTool plumbing — inherits from
            // AgentQueryConfig so fork-spawned subagents (memory /
            // dream / session services) honour their per-policy
            // callbacks. Other (AgentTool) spawns leave it `None`.
            can_use_tool: config.can_use_tool.clone(),
            query_source_override: None,
            fork_label: config.fork_label,
            // PR #18143 cache-bust risk — only memory/compact callers
            // intentionally set this; user-driven AgentTool spawns
            // pass through unchanged.
            max_output_tokens_override: config.max_output_tokens_override,
            // Sub-context isolation for fork-flavored subagent spawns.
            // When `fork_label` is set (memory services: extract /
            // dream / session_memory; agent_summary timer), build a
            // `ForkContextOverrides` so the per-call ToolUseContext
            // builder applies auto agent_id, fresh DenialTracker,
            // query_chain_id / query_depth bump, and write fence.
            // User-invoked AgentTool spawns leave `fork_label = None`
            // and skip isolation (they inherit the parent context).
            // TS parity: `forkedAgent.ts::createSubagentContext` runs
            // for every framework-spawned fork.
            fork_isolation: config.fork_label.map(|label| {
                let mut iso = crate::fork_context::ForkContextOverrides::for_label(label);
                iso.can_use_tool = config.can_use_tool.clone();
                iso.require_can_use_tool = config.require_can_use_tool;
                if !config.allowed_write_roots.is_empty() {
                    iso.allowed_write_roots = config.allowed_write_roots.clone();
                }
                std::sync::Arc::new(iso)
            }),
        };

        // Model resolution: the adapter threads the subagent's typed
        // selection through to the factory so concrete provider/model
        // selections build their own ApiClient and role selections
        // install the role-specific client.
        let mut engine =
            (self.engine_factory)(engine_config, model_selection, config.cancel.clone()).await;
        // D3: install the per-spawn permission bridge if one was
        // threaded through. AgentTool spawns set this so worker tool
        // deny paths forward to the leader instead of failing closed.
        // `None` keeps the factory-default bridge (typically the
        // parent's, installed by `wire_engine`).
        if let Some(bridge) = config.permission_bridge.clone() {
            engine = engine.with_permission_bridge(bridge);
        }

        // Fork mode: if the parent surfaced context messages, use
        // `run_with_messages` so the child's first turn sees the
        // parent's history prepended. TS parity:
        // `AgentTool.tsx:627-630` passes `forkContextMessages:
        // toolUseContext.messages` for `isForkPath`.
        //
        // Caller-supplied `event_tx` lets bg AgentTool spawns
        // observe live `Stream::TextDelta` events (TaskOutput live
        // streaming). When `None`, fall back to a discarded channel
        // so the engine still has somewhere to write.
        let event_tx = config.event_tx.clone().unwrap_or_else(|| {
            let (tx, _rx) = tokio::sync::mpsc::channel::<crate::CoreEvent>(16);
            tx
        });

        let result = if !config.fork_context_messages.is_empty() {
            // Parent history is already shared via `Arc<Message>`; just
            // clone the Arc-slice (cheap pointer bumps) and append the
            // new user prompt after the inherited history.
            let mut messages: Vec<std::sync::Arc<coco_messages::Message>> =
                config.fork_context_messages.clone();
            messages.push(std::sync::Arc::new(coco_messages::create_user_message(
                prompt,
            )));
            engine
                .run_with_messages(messages, event_tx)
                .await
                .map_err(|e| {
                    Box::new(coco_error::PlainError::new(
                        e.to_string(),
                        coco_error::StatusCode::Internal,
                    )) as coco_error::BoxedError
                })?
        } else {
            // The single-prompt path uses `run_with_events` so the
            // caller's `event_tx` (or our discarded fallback) drives
            // the same emission stream as the fork path.
            engine
                .run_with_events(prompt, event_tx)
                .await
                .map_err(|e| {
                    Box::new(coco_error::PlainError::new(
                        e.to_string(),
                        coco_error::StatusCode::Internal,
                    )) as coco_error::BoxedError
                })?
        };

        // Count ToolResult messages as a proxy for tool_use_count —
        // every committed tool_use produces exactly one tool_result
        // per I1, so this tracks TS `runAgent.ts`'s
        // `toolUseCount` increment on each assistant tool_use block.
        let tool_use_count = result
            .final_messages
            .iter()
            .filter(|m| matches!(m.as_ref(), coco_messages::Message::ToolResult(_)))
            .count() as i64;
        // Return the engine's authoritative `Arc<Message>` history
        // directly — callers (SwarmAgentHandle, teammate runner)
        // forward the same Arcs through transcript / audit pipelines
        // without paying a serialize / deserialize round-trip.
        Ok(AgentQueryResult {
            response_text: Some(result.response_text),
            messages: result.final_messages,
            turns: result.turns,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            tool_use_count,
            cancelled: result.cancelled,
        })
    }
}

fn effective_model_selection(config: &AgentQueryConfig) -> LlmModelSelection {
    if config.model_selection != LlmModelSelection::InheritMain {
        return config.model_selection.clone();
    }

    LlmModelSelection::from_model_and_role(
        (!config.model.trim().is_empty()).then_some(config.model.as_str()),
        config.model_role,
    )
}

#[derive(Default)]
struct InitialRuleMaps {
    allow_rules: coco_types::PermissionRulesBySource,
    deny_rules: coco_types::PermissionRulesBySource,
    ask_rules: coco_types::PermissionRulesBySource,
}

/// Build the initial permission-rule maps for a fork-spawned subagent.
fn build_initial_rule_maps(extra: &[coco_types::PermissionRule]) -> InitialRuleMaps {
    let mut maps = InitialRuleMaps::default();
    for rule in extra {
        let map = match rule.behavior {
            coco_types::PermissionBehavior::Allow => &mut maps.allow_rules,
            coco_types::PermissionBehavior::Deny => &mut maps.deny_rules,
            coco_types::PermissionBehavior::Ask => &mut maps.ask_rules,
        };
        map.entry(rule.source).or_default().push(rule.clone());
    }
    if !extra.is_empty() {
        tracing::info!(
            extra = extra.len(),
            allow_sources = ?maps.allow_rules.keys().collect::<Vec<_>>(),
            deny_sources = ?maps.deny_rules.keys().collect::<Vec<_>>(),
            ask_sources = ?maps.ask_rules.keys().collect::<Vec<_>>(),
            "agent_adapter: built initial permission rules for subagent"
        );
    }
    maps
}

#[cfg(test)]
#[path = "agent_adapter.test.rs"]
mod tests;
