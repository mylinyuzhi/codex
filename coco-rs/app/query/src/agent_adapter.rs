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
use coco_types::ModelRole;
use coco_types::ThinkingLevel;
use coco_types::ToolFilter;
use coco_types::ToolOverrides;

use crate::engine::QueryEngine;
use crate::engine::QueryEngineConfig;

/// Factory function type for creating QueryEngine instances.
///
/// Each agent query gets a fresh engine with its own config plus an
/// optional `ModelRole` that the factory uses to select the right
/// primary `ApiClient` + fallback chain. `None` defaults to the
/// parent session's model (TS parity: `runAgent.ts` inherits the
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
            Option<ModelRole>,
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
        let engine_config = QueryEngineConfig {
            max_turns: config.max_turns.unwrap_or(30),
            max_tokens: None,
            prompt_cache: config.prompt_cache.clone(),
            system_prompt: Some(config.system_prompt),
            append_system_prompt: None,
            model_id: config.model,
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
            // `extra_allow_rules` the caller (today: fork-mode
            // `SkillTool` forwarding skill frontmatter `allowed-tools`)
            // wants pre-populated under `PermissionRuleSource::Command`.
            // TS parity: `createGetAppStateWithAllowedTools`
            // (`forkedAgent.ts:147-171`) wraps `getAppState` to inject
            // the same rules into the subagent's evaluation context.
            allow_rules: build_initial_allow_rules(&config.extra_allow_rules),
            deny_rules: Default::default(),
            ask_rules: Default::default(),
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

        // Role resolution: the adapter threads the subagent's role
        // through to the factory so the correct primary+fallback
        // chain is installed. `None` defers to the factory's
        // default (typically the parent session's Main role).
        let role = config.model_role;
        let mut engine = (self.engine_factory)(engine_config, role).await;
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
            // Deserialize each message JSON back into typed
            // `Message`. Any entry that fails deserialization is
            // dropped with a warn — fork context is best-effort
            // (the child will simply lack that message).
            let mut messages: Vec<coco_messages::Message> = Vec::new();
            for (i, v) in config.fork_context_messages.iter().enumerate() {
                match serde_json::from_value::<coco_messages::Message>(v.clone()) {
                    Ok(m) => messages.push(m),
                    Err(e) => {
                        tracing::warn!(
                            index = i,
                            error = %e,
                            "fork_context_messages[i] failed to deserialize; dropping"
                        );
                    }
                }
            }
            // Append the new user prompt after the fork history.
            messages.push(coco_messages::create_user_message(prompt));
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
            .filter(|m| matches!(m, coco_messages::Message::ToolResult(_)))
            .count() as i64;
        // Serialize the final history so the caller (SwarmAgentHandle,
        // teammate runner) can route it through transcript / audit
        // pipelines. `serde_json::Value` is the agreed boundary type
        // on `AgentQueryResult` because this hop crosses the
        // `coco-tool-runtime` → `coco-state` layer.
        let messages = result
            .final_messages
            .iter()
            .map(|m| serde_json::to_value(m).unwrap_or_default())
            .collect();

        Ok(AgentQueryResult {
            response_text: Some(result.response_text),
            messages,
            turns: result.turns,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            tool_use_count,
            cancelled: result.cancelled,
        })
    }
}

/// Pure helper: micro-compact a teammate worker's serialized history.
///
/// Implements the body of [`coco_coordinator::runner_loop::AgentExecutionEngine::compact_messages`]
/// for production engines that wrap a `coco-compact`-aware runtime.
/// Lives here in `coco-query` (alongside the engine adapter) rather
/// than in `coco-coordinator` so the coordinator stays free of the
/// `coco-messages` / `coco-compact` deps.
///
/// Routes through [`coco_compact::micro_compact`] which clears
/// resolved tool results while preserving recent compactable IDs — no
/// API call needed. `keep_recent` is the TS default (5); engines that
/// hold a `CompactConfig` should call `coco_compact::micro_compact`
/// directly with the user's configured value instead of this helper.
///
/// Falls back to a no-op when message deserialization fails — a
/// malformed history shouldn't break the runner's compaction path;
/// the runner_loop's safety-valve sliding-window then kicks in.
pub fn micro_compact_serialized_messages(
    messages: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    const KEEP_RECENT: usize = 5;
    let mut typed: Vec<coco_messages::Message> = match messages
        .iter()
        .map(|v| serde_json::from_value::<coco_messages::Message>(v.clone()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "micro_compact_serialized_messages: malformed history; returning input unchanged"
            );
            return messages;
        }
    };
    let _result = coco_compact::micro_compact(&mut typed, KEEP_RECENT);
    typed
        .iter()
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .collect()
}

/// Build the initial `allow_rules` map for a fork-spawned subagent.
///
/// Groups `extra_allow_rules` by `(source, behavior)` — today every
/// rule lands under `PermissionRuleSource::Command` because fork-mode
/// skills are the only producer. The grouping iterates anyway so a
/// future producer that emits mixed sources continues to slot
/// correctly without a downstream change.
fn build_initial_allow_rules(
    extra: &[coco_types::PermissionRule],
) -> coco_types::PermissionRulesBySource {
    let mut map: coco_types::PermissionRulesBySource = Default::default();
    let mut skipped = 0usize;
    for rule in extra {
        if rule.behavior != coco_types::PermissionBehavior::Allow {
            // Deny / Ask rules don't belong in `allow_rules`. Drop with a
            // warning so a misbehaving caller doesn't silently widen the
            // wrong bucket.
            skipped += 1;
            tracing::warn!(
                behavior = ?rule.behavior,
                "build_initial_allow_rules: skipping non-Allow rule in extra_allow_rules"
            );
            continue;
        }
        map.entry(rule.source).or_default().push(rule.clone());
    }
    if !extra.is_empty() {
        // Useful at subagent spawn time: confirms the parent's
        // `extra_allow_rules` (today: fork-mode skill `allowed-tools`)
        // landed in the subagent's BASE `allow_rules` bucket, not the
        // per-engine live overlay. The live overlay starts empty;
        // subagent skills can still emit into it during execution.
        tracing::info!(
            extra = extra.len(),
            kept = extra.len() - skipped,
            skipped,
            sources = ?map.keys().collect::<Vec<_>>(),
            "agent_adapter: built initial allow_rules for forked subagent"
        );
    }
    map
}

#[cfg(test)]
#[path = "agent_adapter.test.rs"]
mod tests;
