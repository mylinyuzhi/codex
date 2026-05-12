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
use serde_json::Value;

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
                "description": "The type of specialized agent to use (e.g., 'Explore', 'Plan', 'general-purpose')"
            }),
        );
        p.insert(
            "model".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional model override for this agent",
                "enum": ["sonnet", "opus", "haiku"]
            }),
        );
        p.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this agent in the background"
            }),
        );
        p.insert(
            "isolation".into(),
            serde_json::json!({
                "type": "string",
                // TS `AgentTool.tsx:99`: ant builds expose `["worktree", "remote"]`,
                // 3p builds expose `["worktree"]` only. coco-rs accepts both values
                // for schema compatibility — the remote path delegates to CCR via
                // the AgentHandle implementation, which can return an error if the
                // current build doesn't support remote isolation. `none` is the
                // explicit "no isolation" value (same behavior as omitting the
                // field) — included so models that pass an explicit value can
                // select it without falling outside the enum.
                "description": "Isolation mode. 'worktree' creates a temporary git worktree so the agent works on an isolated copy of the repo. 'remote' launches the agent in a remote CCR environment (always runs in background). 'none' (or omit) runs in the parent's working directory.",
                "enum": ["none", "worktree", "remote"]
            }),
        );
        p.insert(
            "name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Agent name for multi-agent teams (used with team_name)"
            }),
        );
        p.insert(
            "team_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Team name for spawning a teammate"
            }),
        );
        p.insert(
            "mode".into(),
            serde_json::json!({
                "type": "string",
                "description": "Permission mode override applied to the spawned subagent (subject to TS parent→child inheritance rule: trust modes on parent override agent's declared mode).",
                // Mirrors `coco_types::PermissionMode` wire form (camelCase).
                // Listing the enum lets the model know which values
                // round-trip through `serde_json::from_value::<PermissionMode>`
                // — anything outside the set is silently dropped at parse.
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
                ]
            }),
        );
        p.insert(
            "cwd".into(),
            serde_json::json!({
                "type": "string",
                "description": "Working directory override for the agent"
            }),
        );
        p.insert(
            "effort".into(),
            serde_json::json!({
                "type": "string",
                "description": "Reasoning effort override for the spawned agent. Mapped to a `ThinkingLevel` and threaded into `PerCallOverrides.thinking_level` at the engine boundary. `max` is an alias for the highest tier.",
                // Mirrors the canonical set accepted by
                // `coco_types::ReasoningEffort::from_str` (and the
                // frontmatter parser's `validate_effort`). Keep this
                // list aligned with the enum's `FromStr` impl.
                "enum": ["none", "minimal", "low", "medium", "high", "max"]
            }),
        );
        p.insert(
            "use_exact_tools".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Reuse the parent's exact tool definitions for prompt-cache parity"
            }),
        );
        p.insert(
            "mcp_servers".into(),
            serde_json::json!({
                "type": "array",
                "items": { "type": "string" },
                "description": "Allow only these MCP servers' tools for this agent"
            }),
        );
        p.insert(
            "disallowed_tools".into(),
            serde_json::json!({
                "type": "array",
                "items": { "type": "string" },
                "description": "Per-agent tool deny list (intersected with parent filter)"
            }),
        );
        p.insert(
            "max_turns".into(),
            serde_json::json!({
                "type": "integer",
                "minimum": 1,
                "description": "Hard cap on turns the agent will run before stopping"
            }),
        );
        p.insert(
            "initial_prompt".into(),
            serde_json::json!({
                "type": "string",
                "description": "Override the agent definition's initial prompt body"
            }),
        );
        ToolInputSchema { properties: p }
    }

    /// Render the spawn-result envelope into model-visible text per
    /// `data["status"]`. TS parity: `AgentTool.tsx::mapToolResultToToolResultBlockParam`
    /// (4 branches: teammate_spawned / async_launched / completed / failed).
    /// `remote_launched` is CCR-specific and has no coco-rs producer.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let status = data.get("status").and_then(Value::as_str).unwrap_or("");
        let text = match status {
            "teammate_spawned" => {
                // TS `AgentTool.tsx:1308-1312`. `name` and `team_name`
                // come from the spawn input (not from the response) and
                // are emitted as separate lines so the parent can grep
                // by either.
                let agent_id = data.get("agentId").and_then(Value::as_str).unwrap_or("");
                let name = data.get("name").and_then(Value::as_str).unwrap_or("");
                let team_name = data.get("team_name").and_then(Value::as_str).unwrap_or("");
                format!(
                    "Spawned successfully.\nagent_id: {agent_id}\nname: {name}\nteam_name: {team_name}\nThe agent is now running and will receive instructions via mailbox."
                )
            }
            "async_launched" => {
                let agent_id = data.get("agentId").and_then(Value::as_str).unwrap_or("");
                let output_file = data
                    .get("outputFile")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let prefix = format!(
                    "Async agent launched successfully.\nagentId: {agent_id} (internal ID - do not mention to user. Use SendMessage with to: '{agent_id}' to continue this agent.)\nThe agent is working in the background. You will be notified automatically when it completes."
                );
                let instructions = if output_file.is_empty() {
                    "Briefly tell the user what you launched and end your response. Do not generate any other text — agent results will arrive in a subsequent message.".to_string()
                } else {
                    format!(
                        "Do not duplicate this agent's work — avoid working with the same files or topics it is using. Work on non-overlapping tasks, or briefly tell the user what you launched and end your response.\noutput_file: {output_file}\nIf asked, you can check progress before completion by using FileRead or Bash tail on the output file."
                    )
                };
                format!("{prefix}\n{instructions}")
            }
            "completed" => {
                let content = data.get("content").and_then(Value::as_str).unwrap_or("");
                let one_shot = data
                    .get("oneShot")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let worktree_path = data.get("worktreePath").and_then(Value::as_str);
                let worktree_branch = data.get("worktreeBranch").and_then(Value::as_str);
                let has_worktree = worktree_path.is_some();

                // One-shot built-ins (Explore, Plan): drop the agentId
                // trailer + <usage> block when there's no worktree info,
                // since they cannot be re-addressed via SendMessage. TS
                // `AgentTool.tsx:1355-1361`.
                if one_shot && !has_worktree {
                    return vec![ToolResultContentPart::Text {
                        text: content.to_string(),
                        provider_options: None,
                    }];
                }

                let agent_id = data.get("agentId").and_then(Value::as_str).unwrap_or("");
                let total_tokens = data.get("totalTokens").and_then(Value::as_u64).unwrap_or(0);
                let total_tool_uses = data
                    .get("totalToolUseCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let duration_ms = data.get("durationMs").and_then(Value::as_u64).unwrap_or(0);

                let mut worktree_info = String::new();
                if let Some(wt) = worktree_path {
                    worktree_info.push_str(&format!("\nworktreePath: {wt}"));
                    if let Some(wb) = worktree_branch {
                        worktree_info.push_str(&format!("\nworktreeBranch: {wb}"));
                    }
                }
                format!(
                    "{content}\nagentId: {agent_id} (use SendMessage with to: '{agent_id}' to continue this agent){worktree_info}\n<usage>total_tokens: {total_tokens}\ntool_uses: {total_tool_uses}\nduration_ms: {duration_ms}</usage>"
                )
            }
            "failed" => {
                let error = data
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown error");
                format!("Agent failed: {error}")
            }
            _ => serde_json::to_string(data).unwrap_or_default(),
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

        let subagent_type = input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .map(String::from);
        // Snapshot for use after `subagent_type` moves into `request`
        // — needed by the result-rendering branch which gates the
        // `oneShot` flag on `ONE_SHOT_BUILTIN_AGENT_TYPES`.
        let subagent_type_for_render = subagent_type.clone();

        // Fork-mode dispatch (TS `forkSubagent.ts`): when the env gate is
        // on, agent-teams is enabled, the session is interactive, and
        // the caller omitted `subagent_type`, the child inherits the
        // parent's pre-rendered system-prompt bytes + full message
        // history (with `tool_result` blocks replaced by
        // `coco_subagent::FORK_PLACEHOLDER` for cache-identical request
        // prefixes).
        let spawn_mode = if subagent_type.is_none()
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
            let parent_messages_owned = ctx.messages.read().await.clone();
            let parent_messages_json: Vec<serde_json::Value> = parent_messages_owned
                .iter()
                .filter_map(|m| serde_json::to_value(m).ok())
                .collect();
            // Recursive-fork guard: TS `isInForkChild` rejects the fork
            // path when the parent's history already contains the
            // boilerplate tag.
            if coco_subagent::is_in_fork_child(&parent_messages_json) {
                return Err(ToolError::ExecutionFailed {
                    message: "Fork mode requested from inside a forked child — recursive \
                              forking is forbidden (TS `isInForkChild` guard)."
                        .into(),
                    source: None,
                });
            }
            coco_tool_runtime::SpawnMode::Fork {
                rendered_system_prompt: rendered_system_prompt.into_bytes(),
                parent_messages: parent_messages_json,
                inherit_tool_pool: true,
            }
        } else {
            coco_tool_runtime::SpawnMode::Fresh
        };

        // Resolve the AgentDefinition once at the boundary. Both the
        // `AgentSpawnRequest` and the `run_in_background` derivation
        // need it; keeping the lookup here avoids racing the catalog
        // twice.
        let resolved_definition = ctx
            .agent_catalog
            .as_deref()
            .zip(subagent_type_for_render.as_deref())
            .and_then(|(cat, name)| cat.find_active(name).cloned())
            .map(std::sync::Arc::new);

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

        // TS `AgentTool.tsx:278,361`: in-process teammates can't spawn
        // background sub-agents — their lifecycle is parent-bound and
        // a background child would outlive its supervisor. Both the
        // request flag AND the definition flag trigger the guard.
        let is_in_process_teammate = ctx.is_teammate;
        if is_in_process_teammate && (run_in_background_input || definition_forces_background) {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "In-process teammates cannot spawn background sub-agents. \
                     Use run_in_background=false (and an agent definition \
                     without `background: true`) for synchronous spawn from \
                     within a teammate. Tried agent_type='{}'.",
                    subagent_type.as_deref().unwrap_or("general-purpose"),
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

        let request = AgentSpawnRequest {
            prompt: prompt.to_string(),
            description: input
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            subagent_type,
            model: input
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from),
            run_in_background,
            enable_summarization,
            session_id: ctx.session_id_for_history.clone().unwrap_or_default(),
            isolation: input
                .get("isolation")
                .and_then(|v| v.as_str())
                .map(String::from),
            name: input.get("name").and_then(|v| v.as_str()).map(String::from),
            team_name: input
                .get("team_name")
                .and_then(|v| v.as_str())
                .map(String::from),
            mode: effective_mode_str,
            cwd: input
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from),
            effort: input
                .get("effort")
                .and_then(|v| v.as_str())
                .map(String::from),
            use_exact_tools: input
                .get("use_exact_tools")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            mcp_servers: input
                .get("mcp_servers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            disallowed_tools: input
                .get("disallowed_tools")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            max_turns: input
                .get("max_turns")
                .and_then(serde_json::Value::as_i64)
                .map(|v| v as i32),
            initial_prompt: input
                .get("initial_prompt")
                .and_then(|v| v.as_str())
                .map(String::from),
            // Subagent inheritance (Layers 1 + 2 + 4). Forward the
            // parent context's resolved values so the child can't see
            // tools gated off at the top level.
            features: Some(ctx.features.clone()),
            tool_overrides: Some(ctx.tool_overrides.clone()),
            parent_tool_filter: Some(ctx.tool_filter.clone()),
            spawn_mode,
            // Production wiring: when `ToolUseContext` learns to carry
            // the parent's `ProviderClientFingerprint` (T5 follow-up),
            // populate this from `fingerprint.to_snapshot()`.
            parent_runtime_snapshot: None,
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
        };

        let request_description = request.description.clone();

        let response =
            ctx.agent
                .spawn_agent(request)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        let data = match response.status {
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
                let is_one_shot = subagent_type_for_render
                    .as_deref()
                    .is_some_and(|t| coco_subagent::ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&t));

                let mut result = serde_json::json!({
                    "status": "completed",
                    "content": content,
                    "prompt": response.prompt.as_deref().unwrap_or(prompt),
                    "agentId": response.agent_id,
                    "totalToolUseCount": response.total_tool_use_count,
                    "totalTokens": response.total_tokens,
                    "durationMs": response.duration_ms,
                    "oneShot": is_one_shot,
                });
                if let Some(wt) = &response.worktree_path {
                    result["worktreePath"] = serde_json::json!(wt);
                }
                if let Some(wb) = &response.worktree_branch {
                    result["worktreeBranch"] = serde_json::json!(wb);
                }
                result
            }
            AgentSpawnStatus::AsyncLaunched => {
                let mut result = serde_json::json!({
                    "status": "async_launched",
                    "agentId": response.agent_id,
                    "prompt": response.prompt.as_deref().unwrap_or(prompt),
                    "description": request_description,
                });
                if let Some(of) = &response.output_file {
                    result["outputFile"] = serde_json::json!(of);
                }
                result
            }
            AgentSpawnStatus::TeammateSpawned => {
                // TS `AgentTool.tsx:1308-1312` exposes `name` and
                // `team_name` (not `prompt`) so the parent agent can
                // address the teammate by stable identifiers in
                // SendMessage payloads.
                let mut spawn = serde_json::json!({
                    "status": "teammate_spawned",
                    "agentId": response.agent_id,
                });
                if let Some(name) = input.get("name").and_then(|v| v.as_str())
                    && !name.is_empty()
                {
                    spawn["name"] = serde_json::json!(name);
                }
                if let Some(team_name) = input.get("team_name").and_then(|v| v.as_str())
                    && !team_name.is_empty()
                {
                    spawn["team_name"] = serde_json::json!(team_name);
                }
                spawn
            }
            AgentSpawnStatus::Failed => {
                serde_json::json!({
                    "status": "failed",
                    "error": response.error.unwrap_or_else(|| "Unknown error".into()),
                })
            }
        };

        Ok(ToolResult {
            data,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
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
    let connected = ctx.mcp.connected_servers().await;
    if connected.is_empty() {
        return Ok(());
    }
    let missing: Vec<String> = servers
        .iter()
        .filter(|name| !connected.iter().any(|c| c == *name))
        .map(|s| (*s).to_string())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(ToolError::ExecutionFailed {
        message: format!(
            "Requested MCP server(s) are not connected: {missing}. Connected servers: \
             {connected}. Either drop the declaration from `mcp_servers` or wait for \
             the server's bootstrap to complete.",
            missing = missing.join(", "),
            connected = connected.join(", "),
        ),
        source: None,
    })
}
