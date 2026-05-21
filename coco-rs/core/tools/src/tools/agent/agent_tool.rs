//! `AgentTool` — launch a specialized agent for complex, multi-step tasks.
//!
//! TS: `tools/AgentTool/AgentTool.tsx` + `tools/AgentTool/runAgent.ts`.

use std::collections::HashMap;

use coco_messages::ToolResult;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default `auto_background_ms` when `COCO_AUTO_BACKGROUND_TASKS` is
/// truthy but doesn't carry a numeric value. Matches TS
/// `getAutoBackgroundMs() = 120_000` (`AgentTool.tsx:74`).
pub const DEFAULT_AUTO_BACKGROUND_MS: u64 = 120_000;

/// Typed envelope returned by [`AgentTool::execute`] and consumed by
/// [`AgentTool::render_for_model`]. Replaces the previous untyped
/// `serde_json::Value` round-trip — both producer and consumer live in
/// this crate, so a discriminated union is strictly type-safer and
/// matches TS's tagged union (`AgentToolToolResultParam`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentSpawnRenderResult {
    /// Synchronous spawn returned a final result.
    Completed {
        /// Final assistant text (after `EMPTY_AGENT_OUTPUT_MARKER`
        /// fallback for silent agents).
        content: String,
        /// Echo of the original prompt the model supplied.
        prompt: String,
        /// Spawned agent id. Stripped from the model-visible trailer
        /// for one-shot built-ins when no worktree info is present.
        #[serde(rename = "agentId")]
        agent_id: Option<String>,
        /// Aggregate tool-use count.
        #[serde(rename = "totalToolUseCount")]
        total_tool_use_count: i64,
        /// Combined input + output tokens.
        #[serde(rename = "totalTokens")]
        total_tokens: i64,
        /// Wall-clock duration of the spawn.
        #[serde(rename = "durationMs")]
        duration_ms: i64,
        /// True iff `subagent_type` is in `ONE_SHOT_BUILTIN_AGENT_TYPES`.
        /// Drives the "drop agentId trailer" rendering branch.
        #[serde(rename = "oneShot")]
        one_shot: bool,
        /// Set when isolation was `"worktree"`.
        #[serde(rename = "worktreePath", skip_serializing_if = "Option::is_none")]
        worktree_path: Option<String>,
        #[serde(rename = "worktreeBranch", skip_serializing_if = "Option::is_none")]
        worktree_branch: Option<String>,
    },
    /// Background spawn started; the model receives an immediate ack
    /// and the result will arrive via a `<task-notification>`.
    AsyncLaunched {
        #[serde(rename = "agentId")]
        agent_id: Option<String>,
        prompt: String,
        description: Option<String>,
        #[serde(rename = "outputFile", skip_serializing_if = "Option::is_none")]
        output_file: Option<String>,
        #[serde(rename = "canReadOutputFile", skip_serializing_if = "Option::is_none")]
        can_read_output_file: Option<bool>,
    },
    /// Teammate spawned into the active team via `name` / `team_name`.
    TeammateSpawned {
        #[serde(rename = "agentId")]
        agent_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(rename = "team_name", skip_serializing_if = "Option::is_none")]
        team_name: Option<String>,
    },
    /// Spawn failed before the engine could produce a response.
    Failed { error: String },
}

impl AgentSpawnRenderResult {
    /// Convenience: `serde_json::to_value` for callers that still
    /// stage the data inside the legacy `ToolResult.data: Value`
    /// field. Internal consumers should prefer the typed variant
    /// directly via `match`.
    pub fn into_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

pub struct AgentTool;

#[async_trait::async_trait]
impl Tool for AgentTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Agent)
    }
    fn name(&self) -> &str {
        ToolName::Agent.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        // Static fallback when prompt() isn't called (e.g. tools that
        // route through `description` directly). The dynamic agent
        // listing — TS parity for `getPrompt(filteredAgents, ...)` —
        // lives in `prompt()` below where we have access to the full
        // catalog snapshot via `PromptOptions`.
        "Launch a new agent to handle complex, multi-step tasks autonomously.\n\n\
         The Agent tool launches specialized agents (subprocesses) that \
         autonomously handle complex tasks. Each agent type has specific \
         capabilities and tools available to it."
            .into()
    }

    /// Render the dynamic AgentTool description with the per-agent
    /// listing. TS parity: `AgentTool.tsx:218-225` →
    /// `getPrompt(filteredAgents, isCoordinator, allowedAgentTypes)`.
    ///
    /// Filtering applied (in order):
    /// 1. `allowed_agent_types` from `Agent(...)` permission rule
    /// 2. `required_mcp_servers` against `ready_mcp_servers` (case-
    ///    insensitive substring) — pre-filter so the model never sees
    ///    an agent whose MCP servers aren't connected.
    /// 3. Coordinator + fork-mode prompt sections gated by flags.
    ///
    /// Falls through to the static `description()` when no catalog
    /// snapshot was threaded into `PromptOptions` (e.g. test paths).
    async fn prompt(&self, options: &coco_tool_runtime::PromptOptions) -> String {
        let Some(catalog) = options.agent_catalog.as_ref() else {
            return self.description(&Value::Null, &options.as_description_options());
        };
        let renderer = coco_subagent::AgentToolPromptRenderer::new(catalog);
        let render_opts = coco_subagent::PromptOptions {
            allowed_agent_types: options.allowed_agent_types.clone(),
            ready_mcp_servers: options.ready_mcp_servers.clone(),
            coordinator_mode: options.coordinator_mode,
            fork_enabled: options.fork_enabled,
            has_embedded_search_tools: options.has_embedded_search_tools,
            is_in_process_teammate: options.is_in_process_teammate,
            is_teammate: options.is_teammate,
            list_via_attachment: options.agent_list_via_attachment,
            is_pro_subscription: options.is_pro_subscription,
            background_tasks_disabled: options.background_tasks_disabled,
            ant_build: options.ant_build,
        };
        renderer.full_prompt(&render_opts)
    }
    /// TS `AgentTool.tsx`: `isConcurrencySafe() { return true }`. Multiple
    /// agent spawns issued in the same turn are independent — each runs in
    /// its own context (and optionally its own worktree) — so the executor
    /// can batch them into a single `ConcurrentSafe` partition. Without
    /// this override they were forced into per-call `SingleUnsafe` batches,
    /// serializing parallel exploration workflows like
    /// `Agent(...) Agent(...) Agent(...)` and multiplying latency by N.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    /// TS-mirror schema (`AgentTool.tsx:82-101`).
    ///
    /// Fields exposed (11):
    /// - `description`, `prompt`, `subagent_type`, `model`, `run_in_background`
    ///   from TS `baseInputSchema`.
    /// - `name`, `team_name`, `mode` from TS `multiAgentInputSchema`.
    /// - `isolation`, `cwd` from TS `fullInputSchema` (TS gates `cwd` behind
    ///   `feature('KAIROS')`; coco-rs exposes it unconditionally — operators
    ///   wanting to hide it can use `disallowed_tool_params` once that gate
    ///   lands).
    /// - `model_role` is coco-rs-specific (multi-LLM addition, no TS
    ///   equivalent — TS is Anthropic-only and uses `model` aliases
    ///   directly).
    ///
    /// Fields NOT exposed (TS internal-only knobs that coco-rs callers set
    /// on `AgentSpawnRequest` directly): `effort`, `use_exact_tools`,
    /// `mcp_servers`, `disallowed_tools`, `max_turns`, `initial_prompt`.
    /// Memory crate / coordinator resume populate these from frontmatter
    /// or service-internal logic — exposing them to the LLM was a
    /// coco-rs-only extension that diverged from TS.
    ///
    /// **Schema-honesty gap (tracked)**: TS dynamically `.omit({
    /// run_in_background: true })` when `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS`
    /// is set or fork mode is enabled. coco-rs's `Tool::input_schema(&self)`
    /// trait method has no options parameter, so this gating happens at
    /// runtime in `execute()` instead (the value is silently overridden when
    /// `COCO_BACKGROUND_TASKS_DISABLE` is set). A typed `input_schema(opts)`
    /// migration would close this gap.
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "prompt".into(),
            serde_json::json!({
                "type": "string",
                "description": "The task for the agent to perform"
            }),
        );
        p.insert(
            "description".into(),
            serde_json::json!({
                "type": "string",
                "description": "A short (3-5 word) description of the task"
            }),
        );
        p.insert(
            "subagent_type".into(),
            serde_json::json!({
                "type": "string",
                "description": "The type of specialized agent to use for this task"
            }),
        );
        // Note: NEITHER `model` NOR `model_role` is in the model-facing
        // schema. Both are operator-only knobs:
        //   - `model` (e.g. `openai/gpt-4o`) — set in `.md` frontmatter
        //     `model:` field by the author who knows the target provider.
        //   - `model_role` (e.g. `Fast`, `Explore`) — set in `.md`
        //     frontmatter `model_role:` field or derived from the
        //     `subagent_type → ModelRole` built-in mapping.
        //
        // Why neither is LLM-pickable:
        //   - `model` requires knowledge of operator's `provider/model_id`
        //     configuration; coco-rs is multi-LLM so there is no closed
        //     enum the LLM can choose from.
        //   - `model_role` requires knowing operator's role mappings
        //     in `settings.models.<role>`; even if the LLM picks a role,
        //     it's pretending to make an informed choice it can't make.
        //
        // The catalog-only principle (matching TS) says: static
        // configuration is the source of truth, the LLM picks an agent
        // by `subagent_type` (semantic identity), and the operator
        // owns model selection. See the root CLAUDE.md "Multi-Provider
        // Boundaries" rule.
        p.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this agent in the background. You will be notified when it completes."
            }),
        );
        p.insert(
            "isolation".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["worktree"],
                "description": "Isolation mode. \"worktree\" creates a temporary git worktree so the agent works on an isolated copy of the repo."
            }),
        );
        p.insert(
            "name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Name for the spawned agent. Makes it addressable via SendMessage({to: name}) while running."
            }),
        );
        p.insert(
            "team_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Team name for spawning. Uses current team context if omitted."
            }),
        );
        p.insert(
            "mode".into(),
            serde_json::json!({
                "type": "string",
                // PermissionMode wire form (camelCase). Listing the
                // enum lets the model know which values round-trip
                // through `serde_json::from_value::<PermissionMode>`.
                "enum": [
                    "default",
                    "plan",
                    "dontAsk",
                    "acceptEdits",
                    "bubble",
                    "bypassPermissions",
                    "auto",
                    "ask",
                    "deny"
                ],
                "description": "Permission mode for spawned teammate (e.g., \"plan\" to require plan approval)."
            }),
        );
        p.insert(
            "cwd".into(),
            serde_json::json!({
                "type": "string",
                "description": "Absolute path to run the agent in. Overrides the working directory for all filesystem and shell operations within this agent. Mutually exclusive with isolation: \"worktree\"."
            }),
        );
        ToolInputSchema {
            properties: p,
            // TS zod requires `description` and `prompt`
            // (`AgentTool.tsx:82-88`): `z.string()` without `.optional()`.
            // All other fields are `.optional()`.
            required: vec!["description".into(), "prompt".into()],
        }
    }

    /// Render the spawn-result envelope into model-visible text. TS
    /// parity: `AgentTool.tsx::mapToolResultToToolResultBlockParam`
    /// (4 branches: teammate_spawned / async_launched / completed /
    /// failed). `remote_launched` is CCR-specific with no coco-rs
    /// producer.
    ///
    /// Consumes the typed [`AgentSpawnRenderResult`] envelope that
    /// [`AgentTool::execute`] stages on `ToolResult.data`. The legacy
    /// `serde_json::Value` field-poking path is gone — both producer
    /// and consumer live in this crate, so a `match` on the typed
    /// envelope is strictly safer.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        // Producer guarantee: `data` is always the wire form of
        // `AgentSpawnRenderResult`. Tolerate the case where it doesn't
        // deserialise (e.g. a third-party caller staged a different
        // shape) by falling back to a serialised dump rather than
        // panicking.
        let envelope: AgentSpawnRenderResult = match serde_json::from_value(data.clone()) {
            Ok(v) => v,
            Err(_) => {
                return vec![ToolResultContentPart::Text {
                    text: serde_json::to_string(data).unwrap_or_default(),
                    provider_options: None,
                }];
            }
        };
        let text = match envelope {
            AgentSpawnRenderResult::TeammateSpawned {
                agent_id,
                name,
                team_name,
            } => {
                // TS `AgentTool.tsx:1308-1312`. `name` and `team_name`
                // come from the spawn input (not from the response) and
                // are emitted as separate lines so the parent can grep
                // by either.
                format!(
                    "Spawned successfully.\nagent_id: {}\nname: {}\nteam_name: {}\nThe agent is now running and will receive instructions via mailbox.",
                    agent_id.as_deref().unwrap_or(""),
                    name.as_deref().unwrap_or(""),
                    team_name.as_deref().unwrap_or(""),
                )
            }
            AgentSpawnRenderResult::AsyncLaunched {
                agent_id,
                output_file,
                ..
            } => {
                let agent_id = agent_id.unwrap_or_default();
                let prefix = format!(
                    "Async agent launched successfully.\nagentId: {agent_id} (internal ID - do not mention to user. Use SendMessage with to: '{agent_id}' to continue this agent.)\nThe agent is working in the background. You will be notified automatically when it completes."
                );
                let instructions = match output_file.as_deref() {
                    None | Some("") => "Briefly tell the user what you launched and end your response. Do not generate any other text — agent results will arrive in a subsequent message.".to_string(),
                    Some(output_file) => format!(
                        "Do not duplicate this agent's work — avoid working with the same files or topics it is using. Work on non-overlapping tasks, or briefly tell the user what you launched and end your response.\noutput_file: {output_file}\nIf asked, you can check progress before completion by using FileRead or Bash tail on the output file."
                    ),
                };
                format!("{prefix}\n{instructions}")
            }
            AgentSpawnRenderResult::Completed {
                content,
                agent_id,
                total_tool_use_count,
                total_tokens,
                duration_ms,
                one_shot,
                worktree_path,
                worktree_branch,
                ..
            } => {
                let has_worktree = worktree_path.is_some();
                // One-shot built-ins (Explore, Plan): drop the agentId
                // trailer + <usage> block when there's no worktree info,
                // since they cannot be re-addressed via SendMessage. TS
                // `AgentTool.tsx:1355-1361`.
                if one_shot && !has_worktree {
                    return vec![ToolResultContentPart::Text {
                        text: content,
                        provider_options: None,
                    }];
                }
                let agent_id = agent_id.unwrap_or_default();
                let mut worktree_info = String::new();
                if let Some(wt) = &worktree_path {
                    worktree_info.push_str(&format!("\nworktreePath: {wt}"));
                    if let Some(wb) = &worktree_branch {
                        worktree_info.push_str(&format!("\nworktreeBranch: {wb}"));
                    }
                }
                format!(
                    "{content}\nagentId: {agent_id} (use SendMessage with to: '{agent_id}' to continue this agent){worktree_info}\n<usage>total_tokens: {total_tokens}\ntool_uses: {total_tool_use_count}\nduration_ms: {duration_ms}</usage>"
                )
            }
            AgentSpawnRenderResult::Failed { error } => format!("Agent failed: {error}"),
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "prompt is required and must be non-empty".into(),
                error_code: None,
            });
        }

        // TS `AgentTool.tsx:83` — `description: z.string()` (required, not
        // `.optional()`). `ToolInputSchema` in coco-rs doesn't carry a
        // `required` list yet (one-off field — the schema is just a
        // properties map), so enforce here at the boundary.
        let description_str = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if description_str.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "description is required and must be non-empty (3-5 word summary)".into(),
                error_code: None,
            });
        }

        // Isolation-mode early gate. Remote isolation is explicitly
        // unsupported in the current coco-rs build — TS parity: ant
        // builds forward to CCR, but the 3p Rust agent returns a
        // clean model-visible error instead of silently falling back
        // to sync mode.
        if let Some("remote") = input.get("isolation").and_then(|v| v.as_str()) {
            return Err(ToolError::ExecutionFailed {
                message: "Isolation mode 'remote' is not supported in this build. \
                          Use 'worktree' for local isolation or omit the field for \
                          no isolation."
                    .into(),
                source: None,
            });
        }

        // TS `AgentTool.tsx:375-391` polls for pending MCP servers with a
        // 30 s deadline before tool-availability check. coco-rs settles
        // the MCP lifecycle at session start (see `app/cli` bootstrap), so
        // by the time AgentTool runs the boot race window is closed —
        // any server still missing here is a user error (typo /
        // mis-configured server). Fail-fast lets the model retry with a
        // corrected `mcp_servers` argument instead of blocking the turn.
        if let Some(arr) = input.get("mcp_servers").and_then(|v| v.as_array()) {
            let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            if !names.is_empty() {
                check_mcp_ready(&names, ctx).await?;
            }
        }

        let requested_mode_str = input.get("mode").and_then(|v| v.as_str()).map(String::from);
        let requested_mode_enum = requested_mode_str.as_deref().and_then(|s| {
            serde_json::from_value::<coco_types::PermissionMode>(serde_json::json!(s)).ok()
        });
        let effective_mode = coco_permissions::resolve_subagent_mode(
            ctx.permission_context.mode,
            requested_mode_enum,
        );
        let effective_mode_str = serde_json::to_value(effective_mode)
            .ok()
            .and_then(|v| v.as_str().map(String::from));

        let explicit_subagent_type = input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .map(String::from);
        let resolved_team_name = input
            .get("team_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| ctx.team_name.clone());
        let requested_name = input
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        let is_team_spawn = resolved_team_name.is_some() && requested_name.is_some();

        if ctx.is_teammate && is_team_spawn {
            return Err(ToolError::ExecutionFailed {
                message: "Teammates cannot spawn other teammates into a team.".into(),
                source: None,
            });
        }
        // Snapshot for use after `subagent_type` moves into `request`
        // — needed by the result-rendering branch which gates the
        // `oneShot` flag on `ONE_SHOT_BUILTIN_AGENT_TYPES`.
        let subagent_type_for_render = explicit_subagent_type.clone();

        // Fork-mode dispatch (TS `forkSubagent.ts`): when the env gate
        // is on, agent-teams is enabled, the session is interactive,
        // and the caller omitted `subagent_type`, the child inherits
        // the parent's pre-rendered system prompt + full message
        // history (with `tool_result` blocks replaced by
        // `coco_subagent::FORK_PLACEHOLDER` for cache-identical
        // request prefixes). The coordinator wraps the user-facing
        // directive in `<fork-boilerplate>` so the worker receives
        // its rules and a downstream recursion guard
        // (`is_in_fork_child`) can detect fork-of-fork.
        let spawn_mode = if explicit_subagent_type.is_none()
            && coco_subagent::is_fork_subagent_active(&ctx.features, ctx.is_non_interactive)
        {
            let Some(rendered_system_prompt) = ctx.rendered_system_prompt.clone() else {
                return Err(ToolError::ExecutionFailed {
                    message: "Fork mode requested but parent's rendered system prompt is \
                              unavailable. The runtime must populate \
                              `ToolUseContext.rendered_system_prompt` before fork-eligible \
                              tool calls — without it the prompt-cache invariant cannot be \
                              upheld."
                        .into(),
                    source: None,
                });
            };
            // Fork requires the parent's runtime snapshot for cache
            // parity. Without it we'd resolve the model via live
            // `RuntimeConfig` and hot-reload would silently bust the
            // cache. Fail loud rather than fall back.
            let Some(parent_snapshot) = ctx.parent_runtime_snapshot.clone() else {
                return Err(ToolError::ExecutionFailed {
                    message: "Fork mode requested but parent's runtime snapshot is \
                              unavailable. The engine must populate \
                              `ToolUseContext.parent_runtime_snapshot` from \
                              `ApiClient::fingerprint().to_snapshot()` at bootstrap — \
                              without it Fork-mode prompt-cache parity cannot be guaranteed."
                        .into(),
                    source: None,
                });
            };
            // Snapshot the parent's history into shared `Arc<Message>`
            // entries. `ctx.messages` is the immutable post-budget
            // snapshot the engine threaded onto this turn's ctx —
            // each entry is already `Arc<Message>`, so `.iter().cloned()`
            // gives a `Vec<Arc<Message>>` via cheap atomic ref-count
            // bumps. Downstream `build_fork_context` then only allocates
            // fresh messages for the tool-result FORK_PLACEHOLDER
            // rewrite; everything else stays shared.
            //
            // TS parity: `AgentTool.tsx:630` passes
            // `toolUseContext.messages` verbatim as
            // `forkContextMessages` and `AgentTool.tsx:332` reads the
            // same array for the `isInForkChild` recursion guard.
            let parent_messages: Vec<std::sync::Arc<coco_messages::Message>> =
                ctx.messages.iter().cloned().collect();
            // Recursive-fork guard: TS `isInForkChild` rejects the fork
            // path when the parent's history already contains the
            // boilerplate tag.
            if coco_subagent::is_in_fork_child(&parent_messages) {
                return Err(ToolError::ExecutionFailed {
                    message: "Fork mode requested from inside a forked child — recursive \
                              forking is forbidden (TS `isInForkChild` guard)."
                        .into(),
                    source: None,
                });
            }
            coco_tool_runtime::SpawnMode::Fork {
                rendered_system_prompt,
                parent_messages,
                parent_snapshot,
            }
        } else {
            coco_tool_runtime::SpawnMode::Fresh
        };
        let is_fork = matches!(spawn_mode, coco_tool_runtime::SpawnMode::Fork { .. });

        let effective_subagent_type = explicit_subagent_type
            .clone()
            .unwrap_or_else(|| "general-purpose".to_string());

        // Resolve the AgentDefinition once at the boundary. Both the
        // `AgentSpawnRequest` and the `run_in_background` derivation
        // need it; keeping the lookup here avoids racing the catalog
        // twice.
        let definition_lookup_type = if is_team_spawn {
            explicit_subagent_type.as_deref()
        } else {
            Some(effective_subagent_type.as_str())
        };
        let resolved_definition = ctx
            .agent_catalog
            .as_deref()
            .zip(definition_lookup_type)
            .and_then(|(cat, name)| cat.find_active(name).cloned())
            .map(std::sync::Arc::new);

        // Latent-bug fix: an explicit `subagent_type` that doesn't resolve
        // to a catalog entry would silently degrade — no system prompt,
        // no tool filter, no model_role, no `required_mcp_servers` check.
        // Reject upfront with the catalog list so the model corrects
        // itself rather than producing a half-configured spawn.
        //
        // Three legitimate "no definition" paths skip this guard:
        // - Fork mode (`subagent_type` omitted + fork gate on) — child
        //   inherits parent's prompt, no AgentDefinition needed.
        // - Team spawn without type (`name + team_name + no subagent_type`)
        //   — TS allows generic teammates.
        // - Test context (`ctx.agent_catalog` is `None`) — can't validate
        //   without a catalog handle.
        if let (Some(catalog), Some(explicit_name)) = (
            ctx.agent_catalog.as_deref(),
            explicit_subagent_type.as_deref(),
        ) && resolved_definition.is_none()
        {
            let mut available: Vec<String> =
                catalog.active().map(|d| d.agent_type.to_string()).collect();
            available.sort();
            available.dedup();
            return Err(ToolError::InvalidInput {
                message: format!(
                    "Unknown subagent_type '{explicit_name}'. Available types: {}. \
                     Add a `.md` file under `~/.coco/agents/` or `<project>/.coco/agents/` \
                     to define a new agent type, then retry.",
                    if available.is_empty() {
                        "none (catalog empty — built-ins didn't load?)".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
                error_code: None,
            });
        }

        if let Some(def) = resolved_definition.as_ref() {
            let servers_with_tools = mcp_servers_with_tools(ctx).await;
            if !coco_subagent::has_required_mcp_servers(def, &servers_with_tools) {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "Agent '{}' requires MCP server(s) {:?}, but MCP servers with tools are: {}",
                        def.agent_type,
                        def.required_mcp_servers,
                        if servers_with_tools.is_empty() {
                            "none".to_string()
                        } else {
                            servers_with_tools.join(", ")
                        },
                    ),
                    source: None,
                });
            }
        }

        // TS `AgentTool.tsx:100`: `cwd` is "Mutually exclusive with
        // isolation: 'worktree'". Reject the conflict upfront — the
        // worktree's CWD is the worktree dir, can't override.
        let requested_cwd = input
            .get("cwd")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from);
        let requested_isolation = input.get("isolation").and_then(|v| v.as_str());
        if requested_cwd.is_some() && requested_isolation == Some("worktree") {
            return Err(ToolError::InvalidInput {
                message: "`cwd` and `isolation: \"worktree\"` are mutually exclusive — \
                          a worktree-isolated agent runs in the worktree's path; \
                          drop one of the two."
                    .into(),
                error_code: None,
            });
        }

        // D5 / P1' parity with TS `AgentTool.tsx:567 shouldRunAsync`:
        //   shouldRunAsync = (run_in_background
        //                     || selectedAgent.background
        //                     || isCoordinator
        //                     || forceAsync)
        //                    && !isBackgroundTasksDisabled
        let run_in_background_input = input
            .get("run_in_background")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let definition_forces_background = resolved_definition
            .as_ref()
            .map(|d| d.background)
            .unwrap_or(false);
        let coordinator_forces_background =
            coco_subagent::is_coordinator_mode(ctx.features.as_ref());
        let background_disabled =
            coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBackgroundTasksDisable);
        let run_in_background = !background_disabled
            && (run_in_background_input
                || definition_forces_background
                || coordinator_forces_background
                || matches!(spawn_mode, coco_tool_runtime::SpawnMode::Fork { .. }));

        // TS `AgentTool.tsx:826` passes `autoBackgroundMs:
        // getAutoBackgroundMs() || undefined` into
        // `registerAgentForeground`. coco-rs reads the env var directly
        // (no GrowthBook shim). Two accepted forms:
        //   - bare truthy (`1` / `true` / `yes`) → DEFAULT_AUTO_BACKGROUND_MS
        //   - numeric (`90000`) → that many ms
        // Falsy / unset → `None` (no auto-detach).
        // Only applies when the spawn is foreground; background spawns
        // detach immediately via `run_in_background`.
        let auto_background_ms = if run_in_background {
            None
        } else {
            resolve_auto_background_ms()
        };

        // TS `AgentTool.tsx:278,361`: in-process teammates can't spawn
        // background sub-agents — their lifecycle is parent-bound and
        // a background child would outlive its supervisor. Both the
        // request flag AND the definition flag trigger the guard.
        let is_in_process_teammate = ctx.is_in_process_teammate;
        if is_in_process_teammate && (run_in_background_input || definition_forces_background) {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "In-process teammates cannot spawn background sub-agents. \
                     Use run_in_background=false (and an agent definition \
                     without `background: true`) for synchronous spawn from \
                     within a teammate. Tried agent_type='{effective_subagent_type}'.",
                ),
                source: None,
            });
        }

        // TS parity: `AgentTool.tsx:750` `enableSummarization` =
        // `isCoordinator || isForkSubagentEnabled || getSdkAgentProgressSummariesEnabled`.
        let summaries_via_app_state = if let Some(handle) = ctx.app_state.as_ref() {
            handle.read().await.agent_progress_summaries_enabled
        } else {
            false
        };
        let enable_summarization = coordinator_forces_background
            || coco_subagent::is_fork_subagent_active(
                ctx.features.as_ref(),
                ctx.is_non_interactive,
            )
            || summaries_via_app_state;

        // `model` is intentionally NOT a model-facing tool input (see
        // schema comment for the multi-LLM rationale). The model that
        // the spawned subagent runs on is resolved entirely from the
        // catalog: `AgentDefinition.model` (.md frontmatter, set by
        // the author who knows the target provider) +
        // `AgentDefinition.model_role` + LLM-supplied `model_role`
        // override. `resolve_subagent_selection` does the precedence.
        let request_subagent_type = if is_team_spawn {
            explicit_subagent_type.clone()
        } else if is_fork && explicit_subagent_type.is_none() {
            None
        } else {
            Some(effective_subagent_type.clone())
        };
        let request = AgentSpawnRequest {
            prompt: prompt.to_string(),
            description: input
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            subagent_type: request_subagent_type,
            // `model` / `model_role` are NOT on `AgentSpawnRequest` —
            // model routing flows from `AgentDefinition` only. See the
            // field-comment block on `AgentSpawnRequest` for the
            // rationale. Memory forks use
            // `AgentSpawnConstraints.forced_model_role` as the
            // internal-only escape hatch.
            run_in_background,
            auto_background_ms,
            enable_summarization,
            session_id: ctx.session_id_for_history.clone().unwrap_or_default(),
            isolation: input
                .get("isolation")
                .and_then(|v| v.as_str())
                .map(String::from),
            name: requested_name,
            team_name: resolved_team_name.clone(),
            mode: effective_mode_str,
            // `cwd` is read from the tool input (TS `AgentTool.tsx:100`).
            // Mutually-exclusive-with-worktree validation runs above.
            //
            // The previous five "internal-only knobs" (`effort`,
            // `use_exact_tools`, `mcp_servers`, `disallowed_tools`,
            // `max_turns`, `initial_prompt`) have been removed from
            // `AgentSpawnRequest` — they were dead pass-through slots.
            // The coordinator now reads them directly from
            // `request.definition` (the resolved AgentDefinition).
            cwd: requested_cwd,
            // Subagent inheritance (Layers 1 + 2 + 4). Forward the
            // parent context's resolved values so the child can't see
            // tools gated off at the top level.
            features: Some(ctx.features.clone()),
            tool_overrides: Some(ctx.tool_overrides.clone()),
            parent_tool_filter: Some(ctx.tool_filter.clone()),
            // `spawn_mode` carries the parent_snapshot embedded in the
            // Fork variant (type invariant: Fork without snapshot is
            // unconstructable). Resume/Fresh don't pin to a snapshot —
            // they read live RuntimeConfig at spawn time.
            spawn_mode,
            // T7: resolve the agent definition once at the AgentTool
            // boundary so the runner reads `definition.model` and
            // `definition.model_role` consistently.
            definition: resolved_definition.clone(),
            // Memory extraction / auto-dream agents inject their own
            // constraints + fork_context_messages via dedicated entry
            // points (`ExtractService` / `DreamService`). The user-
            // facing AgentTool path leaves them at defaults — the
            // child inherits the engine's standard caps.
            constraints: None,
            fork_context_messages: Vec::new(),
            // User-driven AgentTool spawns are user-visible work; the
            // subagent's tool-use entries SHOULD land in the
            // transcript. Memory-side forks set this true via their
            // own service.
            skip_transcript: false,
            // User-driven AgentTool spawns inherit the parent
            // permission pipeline (allow / deny rules + tool's own
            // check_permissions). Memory-side forks override this
            // via dedicated entry points to install per-policy
            // canUseTool callbacks.
            can_use_tool: None,
            require_can_use_tool: false,
            fork_label: None,
            is_non_interactive: ctx.is_non_interactive,
            // D3 / D4 (PR-1 W1): thread the parent's tool_use_id and
            // invoker agent_id through to the background task
            // registration so the `<task-notification>` envelope
            // carries the right routing tags. Without these, completion
            // notifications were routed to the main thread regardless
            // of which agent spawned them, and the `<tool-use-id>` tag
            // was missing. TS parity: `AgentTool.tsx` passes both
            // `toolUseContext.toolUseId` and `toolUseContext.agentId`
            // into `registerAgentForeground` / `registerAsyncAgent`.
            tool_use_id: ctx.tool_use_id.clone(),
            invoking_agent_id: ctx.agent_id.as_ref().map(|a| a.as_str().to_string()),
        };

        let request_description = request.description.clone();
        let request_name_for_render = request.name.clone();
        let request_team_for_render = request.team_name.clone();

        let response =
            ctx.agent
                .spawn_agent(request)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        let envelope = match response.status {
            AgentSpawnStatus::Completed => {
                // TS `AgentTool.tsx:1347-1350` — when the subagent
                // produced no text, surface the canonical empty marker
                // so the model can distinguish "ran successfully but
                // intentionally silent" from "no output yet".
                let raw_content = response.result.unwrap_or_default();
                let content = if raw_content.is_empty() {
                    coco_subagent::EMPTY_AGENT_OUTPUT_MARKER.to_string()
                } else {
                    raw_content
                };

                // TS `constants.ts:9-12` (`ONE_SHOT_BUILTIN_AGENT_TYPES`)
                // — `Explore` and `Plan` can't be re-addressed via
                // `SendMessage`. Forward the flag so consumers can
                // suppress the "follow up via SendMessage" trailer.
                let one_shot = subagent_type_for_render
                    .as_deref()
                    .is_some_and(|t| coco_subagent::ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&t));

                AgentSpawnRenderResult::Completed {
                    content,
                    prompt: response.prompt.as_deref().unwrap_or(prompt).to_string(),
                    agent_id: response.agent_id,
                    total_tool_use_count: response.total_tool_use_count,
                    total_tokens: response.total_tokens,
                    duration_ms: response.duration_ms,
                    one_shot,
                    worktree_path: response
                        .worktree_path
                        .as_ref()
                        .map(|p| p.display().to_string()),
                    worktree_branch: response.worktree_branch,
                }
            }
            AgentSpawnStatus::AsyncLaunched => {
                let output_file = response
                    .output_file
                    .as_ref()
                    .map(|p| p.display().to_string());
                let can_read_output_file = output_file.as_ref().map(|_| {
                    ctx.tool_filter
                        .allows_name(coco_types::ToolName::Read.as_str())
                        || ctx
                            .tool_filter
                            .allows_name(coco_types::ToolName::Bash.as_str())
                });
                AgentSpawnRenderResult::AsyncLaunched {
                    agent_id: response.agent_id,
                    prompt: response.prompt.as_deref().unwrap_or(prompt).to_string(),
                    description: request_description,
                    output_file,
                    can_read_output_file,
                }
            }
            AgentSpawnStatus::TeammateSpawned => AgentSpawnRenderResult::TeammateSpawned {
                agent_id: response.agent_id,
                name: request_name_for_render,
                team_name: request_team_for_render,
            },
            AgentSpawnStatus::Failed => AgentSpawnRenderResult::Failed {
                error: response.error.unwrap_or_else(|| "Unknown error".into()),
            },
        };

        Ok(ToolResult {
            data: envelope.into_value(),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

/// Resolve the `COCO_AUTO_BACKGROUND_TASKS` env var into the
/// `auto_background_ms` value to thread onto `AgentSpawnRequest`. TS:
/// `AgentTool.tsx:72-77 getAutoBackgroundMs`.
///
/// Acceptance rules:
/// - Unset / empty → `None`.
/// - Numeric (`"90000"`) → `Some(parsed_u64)` — caller-specified ms.
/// - Truthy non-numeric (`"1"`, `"true"`, `"yes"`, `"on"`) →
///   `Some(DEFAULT_AUTO_BACKGROUND_MS)` (TS default of 120 000 ms).
/// - Falsy (`"0"`, `"false"`, `"no"`, `"off"`) → `None`.
fn resolve_auto_background_ms() -> Option<u64> {
    let raw = match std::env::var(coco_config::EnvKey::CocoAutoBackgroundTasks.as_str()) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(n) = trimmed.parse::<u64>() {
        return if n == 0 { None } else { Some(n) };
    }
    if coco_config::env::is_env_truthy(coco_config::EnvKey::CocoAutoBackgroundTasks) {
        Some(DEFAULT_AUTO_BACKGROUND_MS)
    } else {
        None
    }
}

/// Fail-fast guard that rejects an AgentTool spawn whose declared
/// `mcp_servers` aren't all connected yet.
///
/// Without this guard, a child declaring `mcp_servers: ["github"]` lands
/// before its server transitions to `connected` and runs with an empty
/// tool pool — the request ships, the runner just can't find the tools.
///
/// Empty `connected_servers()` (the test `NoOpMcpHandle`) is treated as
/// "no MCP layer wired" and lets the spawn through, so this guard only
/// fires in production sessions where the MCP layer is live.
async fn check_mcp_ready(servers: &[&str], ctx: &ToolUseContext) -> Result<(), ToolError> {
    let servers_with_tools = mcp_servers_with_tools(ctx).await;
    let missing: Vec<String> = servers
        .iter()
        .filter(|name| !servers_with_tools.iter().any(|c| c == *name))
        .map(|s| (*s).to_string())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(ToolError::ExecutionFailed {
        message: format!(
            "Requested MCP server(s) do not have ready tools: {missing}. MCP servers with tools: \
             {servers_with_tools}. Either drop the declaration from `mcp_servers` or wait for \
             authentication/bootstrap to complete.",
            missing = missing.join(", "),
            servers_with_tools = if servers_with_tools.is_empty() {
                "none".to_string()
            } else {
                servers_with_tools.join(", ")
            },
        ),
        source: None,
    })
}

async fn mcp_servers_with_tools(ctx: &ToolUseContext) -> Vec<String> {
    let mut servers = Vec::new();
    for tool in ctx.mcp.list_tools().await {
        if !servers.iter().any(|s| s == &tool.server_name) {
            servers.push(tool.server_name);
        }
    }
    servers
}
