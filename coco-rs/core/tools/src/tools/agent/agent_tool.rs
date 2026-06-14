//! `AgentTool` — launch a specialized agent for complex, multi-step tasks.

use coco_messages::ToolResult;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Default `auto_background_ms` when `COCO_AUTO_BACKGROUND_TASKS` is
/// truthy but doesn't carry a numeric value (120 000 ms).
pub const DEFAULT_AUTO_BACKGROUND_MS: u64 = 120_000;

/// Typed input for [`AgentTool`].
///
/// The model-facing schema is built by the manual
/// [`AgentTool::input_schema`] override (precise descriptions and enum
/// lists). This struct only owns the runtime shape used by
/// [`AgentTool::execute`] — adding fields here without adding them to
/// `input_schema()` keeps them as internal-passthrough (e.g.
/// `mcp_servers` is set by permission / hook rewrites, never by the
/// model).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AgentInput {
    /// The task for the agent to perform
    #[serde(default)]
    pub prompt: String,
    /// A short (3-5 word) description of the task
    #[serde(default)]
    pub description: String,
    /// The type of specialized agent to use for this task
    #[serde(default)]
    pub subagent_type: Option<String>,
    /// Set to true to run this agent in the background. You will be
    /// notified when it completes.
    #[serde(default)]
    pub run_in_background: bool,
    /// Isolation mode. "worktree" creates a temporary git worktree.
    #[serde(default)]
    pub isolation: Option<String>,
    /// Name for the spawned agent. Makes it addressable via
    /// SendMessage({to: name}) while running.
    #[serde(default)]
    pub name: Option<String>,
    /// Team name for spawning. Uses current team context if omitted.
    #[serde(default)]
    pub team_name: Option<String>,
    /// Permission mode for spawned teammate. Typed as
    /// [`coco_types::PermissionMode`] (camelCase wire, e.g. "plan",
    /// "acceptEdits") so an invalid value is rejected at deserialize
    /// instead of silently dropping to the parent mode.
    #[serde(default)]
    pub mode: Option<coco_types::PermissionMode>,
    /// Absolute path to run the agent in. Mutually exclusive with
    /// `isolation: "worktree"`.
    #[serde(default)]
    pub cwd: Option<String>,
    /// (Internal) MCP server allowlist for fail-fast readiness check.
    /// Not in the model-facing schema; set by permission / hook
    /// rewrites that scope a spawn to specific MCP servers.
    #[serde(default)]
    pub mcp_servers: Option<Vec<String>>,
}

/// Typed envelope returned by [`AgentTool::execute`] and consumed by
/// [`AgentTool::render_for_model`]. Replaces the previous untyped
/// `serde_json::Value` round-trip — both producer and consumer live in
/// this crate, so a discriminated union is strictly type-safer.
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

pub struct AgentTool;

#[async_trait::async_trait]
impl Tool for AgentTool {
    type Input = AgentInput;
    type Output = AgentSpawnRenderResult;

    // Static schema from a literal `json!`; a parse failure means the literal
    // is malformed (a programmer error), so panicking on first build is correct.
    #[allow(clippy::expect_used)]
    fn runtime_validation_schema(&self) -> &coco_tool_runtime::ToolInputSchema {
        static SCHEMA: std::sync::OnceLock<coco_tool_runtime::ToolInputSchema> =
            std::sync::OnceLock::new();
        SCHEMA.get_or_init(|| {
            // Runtime schema also accepts `mcp_servers` (permission/hook-injected,
            // never model-set); the model view omits it (see `tool_spec`).
            coco_tool_runtime::ToolInputSchema::from_static_value(serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The task for the agent to perform"
                    },
                    "description": {
                        "type": "string",
                        "description": "A short (3-5 word) description of the task"
                    },
                    "subagent_type": {
                        "type": "string",
                        "description": "The type of specialized agent to use for this task"
                    },
                    // Note: NEITHER `model` NOR `model_role` is in the model-facing
                    // schema. Both are operator-only knobs:
                    //   - `model` (e.g. `openai/gpt-4o`) — set in `.md` frontmatter
                    //     `model:` field by the author who knows the target provider.
                    //   - `model_role` (e.g. `Fast`, `Explore`) — set in `.md`
                    //     frontmatter `model_role:` field or derived from the
                    //     `subagent_type → ModelRole` built-in mapping.
                    // Why neither is LLM-pickable: `model` requires knowing the
                    // operator's `provider/model_id` config (multi-LLM ⇒ no closed
                    // enum to choose from); `model_role` requires knowing the
                    // operator's `settings.models.<role>` mappings. Static config is
                    // the source of truth: the LLM picks an agent by `subagent_type`,
                    // and the operator owns model selection. See root CLAUDE.md
                    // "Multi-Provider Boundaries".
                    "run_in_background": {
                        "type": "boolean",
                        "description": "Set to true to run this agent in the background. You will be notified when it completes."
                    },
                    "isolation": {
                        "type": "string",
                        "enum": ["worktree"],
                        "description": "Isolation mode. \"worktree\" creates a temporary git worktree so the agent works on an isolated copy of the repo."
                    },
                    "name": {
                        "type": "string",
                        "description": "Name for the spawned agent. Makes it addressable via SendMessage({to: name}) while running."
                    },
                    "team_name": {
                        "type": "string",
                        "description": "Team name for spawning. Uses current team context if omitted."
                    },
                    "mode": {
                        "type": "string",
                        // The `PermissionMode` wire values (camelCase) — these are
                        // the modes that round-trip through
                        // `serde_json::from_value::<PermissionMode>`.
                        // The INTERNAL set: the 5 external modes + `bubble`, plus
                        // `auto` under a feature gate. `ask`/`deny` are NOT modes —
                        // they are `PermissionBehavior` values and must not appear
                        // here (they fail to parse and are silently dropped to the
                        // parent mode by `resolve_subagent_mode`).
                        "enum": [
                            "default", "plan", "dontAsk", "acceptEdits", "bubble",
                            "bypassPermissions", "auto"
                        ],
                        "description": "Permission mode for spawned teammate (e.g., \"plan\" to require plan approval)."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Absolute path to run the agent in. Overrides the working directory for all filesystem and shell operations within this agent. Mutually exclusive with isolation: \"worktree\"."
                    },
                    // Runtime-only allowlist; `tool_spec` omits it from the model view.
                    "mcp_servers": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "(internal) permission/hook-injected MCP server allowlist"
                    }
                },
                // `description` and `prompt` are required strings. All other fields are optional.
                "required": ["description", "prompt"]
            }))
        })
    }

    async fn tool_spec(
        &self,
        ctx: &coco_tool_runtime::SchemaContext,
        prompt_opts: &coco_tool_runtime::PromptOptions,
    ) -> coco_tool_runtime::ToolSpec {
        // Always hide `mcp_servers`; hide `run_in_background` when the runtime
        // would veto it (background disabled / fork mode).
        let mut drop = vec!["mcp_servers"];
        if ctx.background_tasks_disabled || ctx.fork_mode_active {
            drop.push("run_in_background");
        }
        coco_tool_runtime::ToolSpec::Function(coco_tool_runtime::FunctionToolSpec {
            name: self.name().to_string(),
            description: self.prompt(prompt_opts).await,
            parameters: coco_tool_runtime::schema_omit_properties(
                self.runtime_validation_schema().as_value(),
                &drop,
            ),
            strict: self.strict(),
        })
    }

    fn to_auto_classifier_input(&self, input: &AgentInput) -> Option<String> {
        // The gate must see the security-relevant spawn parameters — which agent
        // type runs and at what permission mode — not the cosmetic 3-5 word
        // `description`.
        let mut tags: Vec<String> = Vec::new();
        if let Some(subagent_type) = input.subagent_type.as_deref().filter(|s| !s.is_empty()) {
            tags.push(subagent_type.to_string());
        }
        if let Some(mode) = input.mode
            && let Some(wire) = serde_json::to_value(mode)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
        {
            tags.push(format!("mode={wire}"));
        }
        let prefix = if tags.is_empty() {
            ": ".to_string()
        } else {
            format!("({}): ", tags.join(", "))
        };
        Some(format!("{prefix}{}", input.prompt))
    }

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Agent)
    }
    fn name(&self) -> &str {
        ToolName::Agent.as_str()
    }
    fn is_enabled(&self, _ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        true
    }
    fn description(&self, _input: &AgentInput, _options: &DescriptionOptions) -> String {
        // Static fallback when prompt() isn't called (e.g. tools that
        // route through `description` directly). The dynamic agent
        // listing lives in `prompt()` below where we have access to the
        // full catalog snapshot via `PromptOptions`.
        "Launch a new agent to handle complex, multi-step tasks autonomously.\n\n\
         The Agent tool launches specialized agents (subprocesses) that \
         autonomously handle complex tasks. Each agent type has specific \
         capabilities and tools available to it."
            .into()
    }

    /// Render the dynamic AgentTool description with the per-agent
    /// listing.
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
            return Tool::description(
                self,
                &AgentInput::default(),
                &options.as_description_options(),
            );
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
            file_write_tool: Some(coco_types::ToolName::write_tool_for(&options.tool_names)),
        };
        renderer.full_prompt(&render_opts)
    }
    /// Multiple agent spawns issued in the same turn are independent — each
    /// runs in its own context (and optionally its own worktree) — so the
    /// executor can batch them into a single `ConcurrentSafe` partition.
    /// Without this override they were forced into per-call `SingleUnsafe`
    /// batches, serializing parallel exploration workflows like
    /// `Agent(...) Agent(...) Agent(...)` and multiplying latency by N.
    fn is_concurrency_safe(&self, _input: &AgentInput) -> bool {
        true
    }

    /// Render the spawn-result envelope into model-visible text.
    /// 4 branches: teammate_spawned / async_launched / completed / failed.
    fn render_for_model(&self, envelope: &AgentSpawnRenderResult) -> Vec<ToolResultContentPart> {
        let text = match envelope.clone() {
            AgentSpawnRenderResult::TeammateSpawned {
                agent_id,
                name,
                team_name,
            } => {
                // `name` and `team_name` come from the spawn input (not
                // from the response) and are emitted as separate lines so
                // the parent can grep by either.
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
                // since they cannot be re-addressed via SendMessage.
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
        input: AgentInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<AgentSpawnRenderResult>, ToolError> {
        // Snapshot every `ctx.app_state` field we'll need into locals at
        // entry so subsequent awaits can't observe a torn read.
        let summaries_via_app_state = if let Some(handle) = ctx.app_state.as_ref() {
            handle.read().await.agent_progress_summaries_enabled
        } else {
            false
        };

        let prompt = input.prompt.as_str();

        if prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "prompt is required and must be non-empty".into(),
                error_code: None,
            });
        }

        // `description` is required (not optional). Already enforced by
        // `AgentInput` at deserialise time; defensive empty-string guard
        // catches the model literally sending `""`.
        if input.description.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "description is required and must be non-empty (3-5 word summary)".into(),
                error_code: None,
            });
        }

        // coco-rs settles the MCP lifecycle at session start (see `app/cli`
        // bootstrap), so by the time AgentTool runs the boot race window is
        // closed — any server still missing here is a user error (typo /
        // mis-configured server). Fail-fast lets the model retry with a
        // corrected `mcp_servers` argument instead of blocking the turn.
        if let Some(arr) = input.mcp_servers.as_ref()
            && !arr.is_empty()
        {
            let names: Vec<&str> = arr.iter().map(String::as_str).collect();
            check_mcp_ready(&names, ctx).await?;
        }

        // `input.mode` is already typed as `PermissionMode` (invalid values
        // were rejected at deserialize), so no string re-parse / silent
        // fallback is needed.
        let effective_mode =
            coco_permissions::resolve_subagent_mode(ctx.permission_context.mode, input.mode);
        let effective_mode_str = serde_json::to_value(effective_mode)
            .ok()
            .and_then(|v| v.as_str().map(String::from));

        let explicit_subagent_type = input.subagent_type.clone();
        let resolved_team_name = input
            .team_name
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| ctx.team_name.clone());
        let requested_name = input.name.clone().filter(|s| !s.is_empty());
        let is_team_spawn = resolved_team_name.is_some() && requested_name.is_some();

        if is_team_spawn && !ctx.features.enabled(coco_types::Feature::AgentTeams) {
            return Err(ToolError::ExecutionFailed {
                message: "Agent Teams is not available in this session.".into(),
                display_data: None,
                source: None,
            });
        }

        if ctx.is_teammate && is_team_spawn {
            return Err(ToolError::ExecutionFailed {
                message: "Teammates cannot spawn other teammates into a team.".into(),
                display_data: None,
                source: None,
            });
        }
        // Snapshot for use after `subagent_type` moves into `request`
        // — needed by the result-rendering branch which gates the
        // `oneShot` flag on `ONE_SHOT_BUILTIN_AGENT_TYPES`.
        let subagent_type_for_render = explicit_subagent_type.clone();

        // Fork-mode dispatch: when the env gate is on, agent-teams is
        // enabled, the session is interactive, and the caller omitted
        // `subagent_type`, the child inherits
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
                    display_data: None,
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
                              the runtime registry snapshot at bootstrap — \
                              without it Fork-mode prompt-cache parity cannot be guaranteed."
                        .into(),
                    display_data: None,
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
            let parent_messages: Vec<std::sync::Arc<coco_messages::Message>> =
                ctx.messages.iter().cloned().collect();
            // Recursive-fork guard: rejects the fork path when the
            // parent's history already contains the boilerplate tag.
            if coco_subagent::is_in_fork_child(&parent_messages) {
                return Err(ToolError::ExecutionFailed {
                    message: "Fork mode requested from inside a forked child — recursive \
                              forking is forbidden."
                        .into(),
                    display_data: None,
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
        //   — generic teammates are allowed.
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

        // Per-agentType deny enforcement. The central permission evaluator
        // defers `Agent(<type>)` content denies to the tool (see
        // core/permissions `central_rule_applies`), so the agentType scoping
        // MUST happen here or denied agents leak through. Skipped for pure
        // forks (no model-chosen agentType).
        if !is_fork
            && let Some(denied) =
                find_agent_deny_rule(&ctx.permission_context, &effective_subagent_type)
        {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "Agent type '{effective_subagent_type}' has been denied by permission \
                     rule '{}({effective_subagent_type})' from {:?}.",
                    ToolName::Agent.as_str(),
                    denied.source,
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
                    display_data: None,
                    source: None,
                });
            }
        }

        // Effective isolation: the explicit tool param overrides, else the
        // agent definition's frontmatter isolation. A definition declaring
        // `isolation: worktree` isolates even when the model omits the param.
        // `AgentIsolation::None` maps to `None` so the spawn-side
        // `Some("worktree")` gate stays correct.
        let effective_isolation: Option<String> = input.isolation.clone().or_else(|| {
            resolved_definition
                .as_ref()
                .and_then(|d| match d.isolation {
                    coco_types::AgentIsolation::None => None,
                    other => Some(other.to_string()),
                })
        });

        // Remote isolation is unsupported in this build. Gate on the EFFECTIVE
        // value so a definition-declared `isolation: remote` is rejected too.
        if effective_isolation.as_deref() == Some("remote") {
            return Err(ToolError::ExecutionFailed {
                message: "Isolation mode 'remote' is not supported in this build. \
                          Use 'worktree' for local isolation or omit the field for \
                          no isolation."
                    .into(),
                display_data: None,
                source: None,
            });
        }

        // `cwd` is mutually exclusive with `isolation: 'worktree'`. Reject
        // the conflict upfront — the worktree's CWD is the worktree dir,
        // can't override.
        let requested_cwd = input
            .cwd
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from);
        let requested_isolation = effective_isolation.as_deref();
        if requested_cwd.is_some() && requested_isolation == Some("worktree") {
            return Err(ToolError::InvalidInput {
                message: "`cwd` and `isolation: \"worktree\"` are mutually exclusive — \
                          a worktree-isolated agent runs in the worktree's path; \
                          drop one of the two."
                    .into(),
                error_code: None,
            });
        }

        // Run async when any flag requests it, unless background tasks are
        // disabled:
        //   run_in_background = (run_in_background
        //                        || selectedAgent.background
        //                        || isCoordinator
        //                        || forceAsync)
        //                       && !isBackgroundTasksDisabled
        let run_in_background_input = input.run_in_background;
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

        // Reads `COCO_AUTO_BACKGROUND_TASKS` env var. Two accepted forms:
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

        // In-process teammates can't spawn background sub-agents — their
        // lifecycle is parent-bound and a background child would outlive
        // its supervisor. Both the
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
                display_data: None,
                source: None,
            });
        }

        // `summaries_via_app_state` was snapshotted at function entry to avoid
        // mid-execute torn reads against `ctx.app_state`.
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
            description: if input.description.is_empty() {
                None
            } else {
                Some(input.description.clone())
            },
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
            isolation: effective_isolation,
            name: requested_name,
            team_name: resolved_team_name.clone(),
            mode: effective_mode_str,
            // `cwd` is read from the tool input.
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
            skill_overrides: Some(ctx.skill_overrides.clone()),
            tool_overrides: Some(ctx.tool_overrides.clone()),
            active_shell_tool: ctx.active_shell_tool,
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
            // Thread the parent's tool_use_id and invoker agent_id through to
            // the background task registration so the `<task-notification>`
            // envelope carries the right routing tags. Without these,
            // completion notifications were routed to the main thread
            // regardless of which agent spawned them, and the
            // `<tool-use-id>` tag was missing.
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
                    display_data: None,
                    source: None,
                })?;

        let envelope = match response.status {
            AgentSpawnStatus::Completed => {
                // When the subagent produced no text, surface the canonical
                // empty marker so the model can distinguish "ran successfully
                // but intentionally silent" from "no output yet".
                let raw_content = response.result.unwrap_or_default();
                let content = if raw_content.is_empty() {
                    coco_subagent::EMPTY_AGENT_OUTPUT_MARKER.to_string()
                } else {
                    raw_content
                };

                // `Explore` and `Plan` can't be re-addressed via
                // `SendMessage`. Forward the flag so consumers can suppress
                // the "follow up via SendMessage" trailer.
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
            data: envelope,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

/// Resolve the `COCO_AUTO_BACKGROUND_TASKS` env var into the
/// `auto_background_ms` value to thread onto `AgentSpawnRequest`.
///
/// Acceptance rules:
/// - Unset / empty → `None`.
/// - Numeric (`"90000"`) → `Some(parsed_u64)` — caller-specified ms.
/// - Truthy non-numeric (`"1"`, `"true"`, `"yes"`, `"on"`) →
///   `Some(DEFAULT_AUTO_BACKGROUND_MS)` (120 000 ms).
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
        display_data: None,
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

/// Find an `Agent(<agent_type>)` deny rule: matches deny rules whose
/// `tool_pattern == Agent` and `rule_content == agent_type`. The central
/// evaluator defers these content denies to the tool, so `execute` enforces them.
fn find_agent_deny_rule<'a>(
    context: &'a coco_types::ToolPermissionContext,
    agent_type: &str,
) -> Option<&'a coco_types::PermissionRule> {
    context.deny_rules.values().flatten().find(|r| {
        r.value.tool_pattern == ToolName::Agent.as_str()
            && r.value.rule_content.as_deref() == Some(agent_type)
    })
}
