use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cocode_error::BoxedError;
use cocode_protocol::LoopEvent;
use cocode_protocol::execution::ExecutionIdentity;
use serde::Deserialize;
use serde::Serialize;
use snafu::IntoError;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::background::BackgroundAgent;
use crate::definition::AgentDefinition;
use crate::error::subagent_error;
use crate::filter::filter_tools_for_agent;
use crate::spawn::SpawnInput;

/// Runtime status of a subagent instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentStatus {
    Running,
    Completed,
    Failed,
    Backgrounded,
}

/// Result of spawning a subagent.
#[derive(Debug, Clone)]
pub struct SpawnResult {
    /// Unique identifier for the spawned agent.
    pub agent_id: String,

    /// Final output (only for foreground agents that completed).
    pub output: Option<String>,

    /// Background agent info (only for background agents).
    pub background: Option<BackgroundAgent>,

    /// Cancellation token for the spawned agent.
    ///
    /// Callers should register this in the shared `agent_cancel_tokens`
    /// map so TaskStop can cancel the agent by ID.
    pub cancel_token: Option<CancellationToken>,

    /// Display color from the agent definition (for TUI rendering).
    pub color: Option<String>,
}

/// A live subagent instance.
pub struct AgentInstance {
    /// Unique identifier for this instance.
    pub id: String,

    /// The agent type this instance was spawned from.
    pub agent_type: String,

    /// Display name for this instance (from spawn input).
    pub name: Option<String>,

    /// Current execution status.
    pub status: AgentStatus,

    /// Final output text (populated on completion).
    pub output: Option<String>,

    /// Cancellation token for aborting the agent.
    pub cancel_token: Option<CancellationToken>,

    /// Background output file path (if running in background).
    pub output_file: Option<PathBuf>,
}

/// Parameters for executing an agent.
///
/// Replaces positional arguments with a named struct for clarity
/// and extensibility (permission_mode, fork_context, etc.).
#[derive(Debug, Clone)]
pub struct AgentExecuteParams {
    /// The type of agent being spawned.
    pub agent_type: String,
    /// The task prompt for the agent.
    pub prompt: String,
    /// Optional execution identity for model selection.
    pub identity: Option<ExecutionIdentity>,
    /// Optional turn limit override.
    pub max_turns: Option<i32>,
    /// Filtered list of available tool names.
    pub tools: Vec<String>,
    /// Token for cancellation.
    pub cancel_token: CancellationToken,
    /// Override permission mode for the child agent.
    pub permission_mode: Option<cocode_protocol::PermissionMode>,
    /// Whether to fork the parent conversation context.
    pub fork_context: bool,
    /// Optional custom system prompt (replaces default system prompt entirely).
    pub custom_system_prompt: Option<String>,
    /// Optional suffix appended to the generated system prompt.
    ///
    /// Used for `critical_reminder` when `use_custom_prompt` is false.
    /// Appended at the end of the system prompt (highest authority position),
    /// matching CC's `criticalSystemReminder_EXPERIMENTAL` behavior.
    pub system_prompt_suffix: Option<String>,
    /// Skills to load for this agent (by name).
    pub skills: Vec<String>,
    /// Memory scope for persistent agent memory.
    pub memory: Option<crate::definition::MemoryScope>,
    /// MCP server references required by this agent.
    pub mcp_servers: Option<Vec<crate::definition::McpServerRef>>,
    /// Isolation mode for this agent's execution environment.
    pub isolation: Option<crate::definition::IsolationMode>,
    /// Agent-scoped hook definitions.
    ///
    /// Registered before the agent loop starts and unregistered after it completes.
    /// `Stop` events are remapped to `SubagentStop`.
    pub hooks: Option<Vec<crate::definition::AgentHookDefinition>>,
    /// Allowed subagent types when `Task(type1, type2)` is in the tools list.
    ///
    /// When set, the Task tool will only allow spawning the specified types.
    /// `None` means no restriction (all agent types are available).
    pub task_type_restrictions: Option<Vec<String>>,
    /// Display name for the agent.
    pub name: Option<String>,
    /// Working directory override for the agent.
    pub cwd: Option<String>,
    /// Background agent output file path (passed so execute_fn can tee progress).
    pub output_file: Option<PathBuf>,
    /// Display color from the agent definition (for TUI rendering).
    pub color: Option<String>,
    /// Whether the agent operates in plan mode (read-only until approved).
    pub plan_mode_required: bool,
}

/// Callback type for executing an agent with filtered tools.
///
/// Returns the agent output as a string on success.
pub type AgentExecuteFn = Box<
    dyn Fn(
            AgentExecuteParams,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::result::Result<String, BoxedError>> + Send>,
        > + Send
        + Sync,
>;

/// Callback type for firing hooks when a background agent completes.
///
/// Receives `(agent_type, agent_id)`. Called from the background task
/// after the agent finishes, so hooks can observe completion.
pub type BackgroundStopHookFn = Arc<
    dyn Fn(String, String) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Context for background agent completion handling.
struct BackgroundCompletionCtx<'a> {
    agent_id: &'a str,
    result: &'a std::result::Result<String, BoxedError>,
    output_file: &'a std::path::Path,
    prompt: &'a str,
    event_tx: Option<&'a mpsc::Sender<LoopEvent>>,
    stop_hook_fn: Option<&'a BackgroundStopHookFn>,
    agent_type: &'a str,
    transitioned_from_foreground: bool,
}

/// Shared logic for background agent completion (transcript write, event send, hook fire).
///
/// Used by both the initial background spawn path and the Ctrl+B foreground-to-background
/// transition path.
async fn handle_background_completion(ctx: BackgroundCompletionCtx<'_>) {
    let BackgroundCompletionCtx {
        agent_id,
        result,
        output_file,
        prompt,
        event_tx,
        stop_hook_fn,
        agent_type,
        transitioned_from_foreground,
    } = ctx;
    if let Err(e) = result {
        tracing::error!(
            agent_id = %agent_id,
            status = ?e.status_code(),
            error = ?e,
            transitioned = transitioned_from_foreground,
            "Background subagent execution failed"
        );
    }

    // Write transcript entry with prompt + output for rich resume
    let recorder = crate::transcript::TranscriptRecorder::new(output_file.to_path_buf());
    let entry = match result {
        Ok(output) => serde_json::json!({
            "status": "completed",
            "agent_id": agent_id,
            "prompt": prompt,
            "output": output,
            "transitioned_from_foreground": transitioned_from_foreground,
        }),
        Err(e) => serde_json::json!({
            "status": "failed",
            "agent_id": agent_id,
            "prompt": prompt,
            "error": e.output_msg(),
            "transitioned_from_foreground": transitioned_from_foreground,
        }),
    };
    if let Err(e) = recorder.record(&entry).await {
        tracing::error!(error = %e, "Failed to write agent transcript");
    }

    // Notify main agent of completion
    if let Some(tx) = event_tx {
        let output_str = result.as_deref().unwrap_or("[agent failed]").to_string();
        let _ = tx
            .send(LoopEvent::SubagentCompleted {
                agent_id: agent_id.to_string(),
                result: output_str,
            })
            .await;
    }

    // Fire SubagentStop hook for background agents
    if let Some(hook_fn) = stop_hook_fn {
        hook_fn(agent_type.to_string(), agent_id.to_string()).await;
    }
}

/// Resolved prompt components from a spawn input + definition.
struct ResolvedPrompt {
    /// The effective user prompt.
    prompt: String,
    /// Full custom system prompt (replaces generated system prompt entirely).
    custom_system_prompt: Option<String>,
    /// Suffix appended to the generated system prompt (highest authority position).
    /// Used for critical_reminder when `use_custom_prompt` is false.
    system_prompt_suffix: Option<String>,
}

/// Resolve the effective prompt and system prompt components from a spawn input + definition.
///
/// When `use_custom_prompt` is set, `critical_reminder` becomes the full system prompt.
/// Otherwise, `critical_reminder` is passed as `system_prompt_suffix` to be appended
/// to the generated system prompt (matching CC's `criticalSystemReminder_EXPERIMENTAL`
/// positioning at the end of the system prompt for highest authority).
fn resolve_prompt(input: &SpawnInput, definition: &AgentDefinition) -> ResolvedPrompt {
    if definition.use_custom_prompt {
        ResolvedPrompt {
            prompt: input.prompt.clone(),
            custom_system_prompt: definition.critical_reminder.clone(),
            system_prompt_suffix: None,
        }
    } else {
        ResolvedPrompt {
            prompt: input.prompt.clone(),
            custom_system_prompt: None,
            system_prompt_suffix: definition.critical_reminder.clone(),
        }
    }
}

/// Build an `AgentExecuteParams` from resolved inputs.
#[allow(clippy::too_many_arguments)]
fn build_execute_params(
    input: &SpawnInput,
    definition: &AgentDefinition,
    resolved: &ResolvedPrompt,
    identity: Option<ExecutionIdentity>,
    max_turns: Option<i32>,
    tools: Vec<String>,
    cancel_token: CancellationToken,
    task_type_restrictions: Option<Vec<String>>,
    output_file: Option<PathBuf>,
) -> AgentExecuteParams {
    AgentExecuteParams {
        agent_type: input.agent_type.clone(),
        prompt: resolved.prompt.clone(),
        identity,
        max_turns,
        tools,
        cancel_token,
        permission_mode: definition.permission_mode,
        fork_context: definition.fork_context,
        custom_system_prompt: resolved.custom_system_prompt.clone(),
        system_prompt_suffix: resolved.system_prompt_suffix.clone(),
        skills: definition.skills.clone(),
        memory: definition.memory,
        mcp_servers: definition.mcp_servers.clone(),
        isolation: definition.isolation,
        hooks: definition.hooks.clone(),
        task_type_restrictions,
        name: input.name.clone(),
        cwd: input.cwd.clone(),
        output_file,
        color: definition.color.clone(),
        plan_mode_required: definition.permission_mode
            == Some(cocode_protocol::PermissionMode::Plan),
    }
}

/// Default limit on concurrent background agents.
const DEFAULT_MAX_BACKGROUND_AGENTS: usize = 8;

/// Manages subagent registration, spawning, and lifecycle tracking.
pub struct SubagentManager {
    agents: HashMap<String, AgentInstance>,
    definitions: Vec<AgentDefinition>,
    /// All available tool names (used for filtering).
    all_tools: Vec<String>,
    /// Optional callback for actual agent execution.
    execute_fn: Option<Arc<AgentExecuteFn>>,
    /// Base directory for background agent output files.
    output_dir: PathBuf,
    /// Optional event sender for background agent completion notifications.
    event_tx: Option<mpsc::Sender<LoopEvent>>,
    /// Optional callback for SubagentStop hooks on background completion.
    background_stop_hook_fn: Option<BackgroundStopHookFn>,
    /// Maximum number of concurrent background agents.
    max_background_agents: usize,
}

impl SubagentManager {
    /// Create a new empty subagent manager.
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            definitions: Vec::new(),
            all_tools: Vec::new(),
            execute_fn: None,
            output_dir: std::env::temp_dir().join("cocode-agents"),
            event_tx: None,
            background_stop_hook_fn: None,
            max_background_agents: DEFAULT_MAX_BACKGROUND_AGENTS,
        }
    }

    /// Set the available tool names for filtering.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.all_tools = tools;
        self
    }

    /// Set the agent execution callback.
    pub fn with_execute_fn(mut self, f: AgentExecuteFn) -> Self {
        self.execute_fn = Some(Arc::new(f));
        self
    }

    /// Set the output directory for background agents.
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = dir;
        self
    }

    /// Set the maximum number of concurrent background agents.
    pub fn with_max_background_agents(mut self, n: usize) -> Self {
        self.max_background_agents = n;
        self
    }

    /// Set the agent execution callback.
    ///
    /// Called per-turn from the session layer so the closure captures
    /// fresh state (model selections, config, etc.).
    pub fn set_execute_fn(&mut self, f: AgentExecuteFn) {
        self.execute_fn = Some(Arc::new(f));
    }

    /// Set the available tool names for filtering.
    ///
    /// Called per-turn alongside `set_execute_fn` so the tool list
    /// reflects the current registry state.
    pub fn set_all_tools(&mut self, tools: Vec<String>) {
        self.all_tools = tools;
    }

    /// Set the event sender for background completion notifications.
    ///
    /// Called per-turn so background agents can emit `SubagentCompleted`
    /// events when they finish, notifying the main agent.
    pub fn set_event_tx(&mut self, tx: mpsc::Sender<LoopEvent>) {
        self.event_tx = Some(tx);
    }

    /// Set the callback for SubagentStop hooks on background completion.
    pub fn set_background_stop_hook_fn(&mut self, f: BackgroundStopHookFn) {
        self.background_stop_hook_fn = Some(f);
    }

    /// Get registered agent type definitions.
    pub fn definitions(&self) -> &[AgentDefinition] {
        &self.definitions
    }

    /// Register a new agent type definition.
    pub fn register_agent_type(&mut self, definition: AgentDefinition) {
        tracing::info!(agent_type = %definition.agent_type, "Registering agent type");
        self.definitions.push(definition);
    }

    /// Spawn a new subagent instance of the given type (simple version).
    ///
    /// Returns the unique agent ID on success. This is a basic spawn that
    /// just registers the agent without executing it.
    pub async fn spawn(&mut self, agent_type: &str, prompt: &str) -> Result<String> {
        let input = SpawnInput {
            agent_type: agent_type.to_string(),
            prompt: prompt.to_string(),
            identity: None,
            max_turns: None,
            run_in_background: Some(false),
            allowed_tools: None,
            resume_from: None,
            name: None,
            team_name: None,
            mode: None,
            cwd: None,
            isolation_override: None,
            description: None,
        };
        let result = self.spawn_full(input).await?;
        Ok(result.agent_id)
    }

    /// Look up a registered agent definition by type name, merging from multiple sources.
    ///
    /// When multiple definitions exist for the same `agent_type`, they are merged
    /// in source priority order (BuiltIn < Plugin < UserSettings < ProjectSettings < CliFlag).
    /// This matches CC's definition merging behavior where scalar fields are overridden,
    /// array fields are unioned, and hooks are merged.
    fn resolve_definition(&self, agent_type: &str) -> Result<AgentDefinition> {
        let mut matching: Vec<&AgentDefinition> = self
            .definitions
            .iter()
            .filter(|d| d.agent_type == agent_type)
            .collect();

        if matching.is_empty() {
            return Err(subagent_error::UnknownAgentTypeSnafu {
                agent_type: agent_type.to_string(),
            }
            .build());
        }

        // Sort by source priority (lowest first, so we merge low → high)
        matching.sort_by_key(|d| d.source.priority());

        let mut merged = matching[0].clone();
        for higher in &matching[1..] {
            merged = merged.merge_with(higher);
        }

        Ok(merged)
    }

    /// Load prior transcript and prepend context when resuming an agent.
    async fn handle_resume(&self, input: &mut SpawnInput) {
        let Some(ref resume_id) = input.resume_from else {
            return;
        };
        let output_file = self.output_dir.join(format!("{resume_id}.jsonl"));
        if !output_file.exists() {
            tracing::warn!(
                resume_id = %resume_id,
                "Prior agent output file not found, starting fresh"
            );
            return;
        }
        match crate::transcript::TranscriptRecorder::read_transcript(&output_file).await {
            Ok(entries) if !entries.is_empty() => {
                // Sanitize transcript: filter out entries with empty/whitespace output
                let entries = crate::transcript::filter_empty_entries(&entries);

                let mut context_parts = Vec::new();
                for entry in &entries {
                    if let Some(prompt) = entry["prompt"].as_str() {
                        context_parts.push(format!("[Previous prompt]\n{prompt}"));
                    }
                    if let Some(output) = entry["output"].as_str() {
                        context_parts.push(format!("[Previous output]\n{output}"));
                    }
                }
                let full_context = context_parts.join("\n\n");
                input.prompt = format!(
                    "[Resuming from previous agent {resume_id}]\n\
                     {full_context}\n\n\
                     Continue with: {}",
                    input.prompt
                );
                tracing::info!(
                    resume_id = %resume_id,
                    entries = entries.len(),
                    context_len = full_context.len(),
                    "Resuming agent with full transcript context"
                );
            }
            Ok(_) => {
                tracing::warn!(
                    resume_id = %resume_id,
                    "Prior agent transcript is empty, starting fresh"
                );
            }
            Err(e) => {
                tracing::warn!(
                    resume_id = %resume_id,
                    error = %e,
                    "Failed to read prior agent transcript, starting fresh"
                );
            }
        }
    }

    /// Spawn a subagent with full configuration and tool filtering.
    ///
    /// This is the main entry point for spawning subagents:
    /// 1. Resolves the agent definition
    /// 2. Filters tools based on definition and spawn input
    /// 3. If resuming, loads prior output and prepends to prompt
    /// 4. Executes the agent (foreground or background)
    /// 5. Returns the result
    pub async fn spawn_full(&mut self, mut input: SpawnInput) -> Result<SpawnResult> {
        let definition = self.resolve_definition(&input.agent_type)?;
        self.handle_resume(&mut input).await;

        let agent_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            agent_id = %agent_id,
            agent_type = %input.agent_type,
            prompt_len = input.prompt.len(),
            background = ?input.run_in_background,
            resume_from = ?input.resume_from,
            "Spawning subagent"
        );

        // Resolve identity (spawn input > definition > inherit parent)
        let identity = input
            .identity
            .clone()
            .or_else(|| definition.identity.clone());

        // Resolve max_turns (spawn input > definition)
        let max_turns = input.max_turns.or(definition.max_turns);

        let resolved = resolve_prompt(&input, &definition);

        tracing::debug!(
            agent_id = %agent_id,
            has_critical_reminder = definition.critical_reminder.is_some(),
            has_system_prompt_suffix = resolved.system_prompt_suffix.is_some(),
            permission_mode = ?definition.permission_mode,
            fork_context = definition.fork_context,
            "Resolved agent definition fields"
        );

        // Resolve run_in_background: input override > definition default
        let run_in_background = input.run_in_background.unwrap_or(definition.background);

        // Apply four-layer tool filtering
        let tools_to_filter = if let Some(ref allowed) = input.allowed_tools {
            allowed.clone()
        } else {
            self.all_tools.clone()
        };
        let filter_result = filter_tools_for_agent(
            &tools_to_filter,
            &definition,
            run_in_background,
            definition.permission_mode.as_ref(),
        );
        let filtered_tools = filter_result.tools;
        let task_type_restrictions = filter_result.task_type_restrictions;

        tracing::debug!(
            agent_id = %agent_id,
            tools_count = filtered_tools.len(),
            ?task_type_restrictions,
            "Filtered tools for subagent"
        );

        // Create cancellation token for this agent
        let cancel_token = CancellationToken::new();

        if run_in_background {
            // Check background agent concurrency limit
            let bg_count = self
                .agents
                .values()
                .filter(|a| a.status == AgentStatus::Backgrounded)
                .count();
            if bg_count >= self.max_background_agents {
                return Err(subagent_error::BackgroundLimitSnafu {
                    limit: self.max_background_agents,
                }
                .build());
            }

            // Background execution
            let output_file = self.output_dir.join(format!("{agent_id}.jsonl"));

            // Ensure output directory exists
            if let Err(e) = tokio::fs::create_dir_all(&self.output_dir).await {
                tracing::warn!(error = %e, "Failed to create output directory");
            }

            let instance = AgentInstance {
                id: agent_id.clone(),
                agent_type: input.agent_type.clone(),
                name: input.name.clone(),
                status: AgentStatus::Backgrounded,
                output: None,
                cancel_token: Some(cancel_token.clone()),
                output_file: Some(output_file.clone()),
            };
            self.agents.insert(agent_id.clone(), instance);

            // Spawn background task if we have an execute function
            if let Some(execute_fn) = &self.execute_fn {
                let execute_fn = execute_fn.clone();
                let agent_id_clone = agent_id.clone();
                let output_file_clone = output_file.clone();

                let params = build_execute_params(
                    &input,
                    &definition,
                    &resolved,
                    identity,
                    max_turns,
                    filtered_tools,
                    cancel_token.clone(),
                    task_type_restrictions.clone(),
                    Some(output_file.clone()),
                );

                let prompt_for_transcript = resolved.prompt.clone();
                let event_tx = self.event_tx.clone();
                let stop_hook_fn = self.background_stop_hook_fn.clone();
                let agent_type_for_hook = input.agent_type.clone();
                tokio::spawn(async move {
                    let result = execute_fn(params).await;
                    handle_background_completion(BackgroundCompletionCtx {
                        agent_id: &agent_id_clone,
                        result: &result,
                        output_file: &output_file_clone,
                        prompt: &prompt_for_transcript,
                        event_tx: event_tx.as_ref(),
                        stop_hook_fn: stop_hook_fn.as_ref(),
                        agent_type: &agent_type_for_hook,
                        transitioned_from_foreground: false,
                    })
                    .await;
                });
            }

            let bg_agent = BackgroundAgent {
                agent_id: agent_id.clone(),
                output_file,
            };

            Ok(SpawnResult {
                agent_id,
                output: None,
                background: Some(bg_agent),
                cancel_token: Some(cancel_token),
                color: definition.color.clone(),
            })
        } else {
            // Foreground execution
            let instance = AgentInstance {
                id: agent_id.clone(),
                agent_type: input.agent_type.clone(),
                name: input.name.clone(),
                status: AgentStatus::Running,
                output: None,
                cancel_token: Some(cancel_token.clone()),
                output_file: None,
            };
            self.agents.insert(agent_id.clone(), instance);

            // Register for background signal (Ctrl+B support)
            let bg_signal_rx = crate::signal::register_backgroundable_agent(agent_id.clone());

            // Execute the agent if we have an execute function
            let output = if let Some(execute_fn) = &self.execute_fn {
                let params = build_execute_params(
                    &input,
                    &definition,
                    &resolved,
                    identity.clone(),
                    max_turns,
                    filtered_tools,
                    cancel_token.clone(),
                    task_type_restrictions,
                    None,
                );

                // Pin the future so it can be moved into a background task on Ctrl+B
                let mut execute_future = Box::pin(execute_fn(params));

                // Use select! to handle both normal completion and background signal
                tokio::select! {
                    result = &mut execute_future => {
                        // Normal completion - unregister from background signals
                        crate::signal::unregister_backgroundable_agent(&agent_id);

                        match result {
                            Ok(result) => {
                                if let Some(instance) = self.agents.get_mut(&agent_id) {
                                    instance.status = AgentStatus::Completed;
                                    instance.output = Some(result.clone());
                                }
                                Some(result)
                            }
                            Err(e) => {
                                if let Some(instance) = self.agents.get_mut(&agent_id) {
                                    instance.status = AgentStatus::Failed;
                                }
                                return Err(subagent_error::ExecuteSnafu {
                                    message: "Foreground subagent execution".to_string(),
                                }
                                .into_error(e));
                            }
                        }
                    }
                    _ = bg_signal_rx => {
                        // Background signal received - transition to background
                        tracing::info!(
                            agent_id = %agent_id,
                            "Agent transitioned to background via signal"
                        );

                        // Create output file for background results
                        let output_file = self.output_dir.join(format!("{agent_id}.jsonl"));

                        // Ensure output directory exists
                        if let Err(e) = tokio::fs::create_dir_all(&self.output_dir).await {
                            tracing::warn!(error = %e, "Failed to create output directory");
                        }

                        // Update instance to background status
                        if let Some(instance) = self.agents.get_mut(&agent_id) {
                            instance.status = AgentStatus::Backgrounded;
                            instance.output_file = Some(output_file.clone());
                        }

                        // Move the in-flight future into a background task
                        // (continues existing execution instead of restarting)
                        let agent_id_clone = agent_id.clone();
                        let output_file_clone = output_file.clone();
                        let prompt_for_transcript = resolved.prompt.clone();
                        let event_tx = self.event_tx.clone();
                        let stop_hook_fn = self.background_stop_hook_fn.clone();
                        let agent_type_for_hook = input.agent_type.clone();

                        tokio::spawn(async move {
                            let result = execute_future.await;
                            handle_background_completion(BackgroundCompletionCtx {
                                agent_id: &agent_id_clone,
                                result: &result,
                                output_file: &output_file_clone,
                                prompt: &prompt_for_transcript,
                                event_tx: event_tx.as_ref(),
                                stop_hook_fn: stop_hook_fn.as_ref(),
                                agent_type: &agent_type_for_hook,
                                transitioned_from_foreground: true,
                            })
                            .await;
                        });

                        let bg_agent = BackgroundAgent {
                            agent_id: agent_id.clone(),
                            output_file,
                        };

                        return Ok(SpawnResult {
                            agent_id,
                            output: None,
                            background: Some(bg_agent),
                            cancel_token: Some(cancel_token),
                            color: definition.color.clone(),
                        });
                    }
                }
            } else {
                // No execute function - return stub (no background signal handling)
                crate::signal::unregister_backgroundable_agent(&agent_id);
                tracing::warn!(
                    agent_id = %agent_id,
                    "No execute_fn configured, returning stub response"
                );
                let stub_output = format!(
                    "Agent '{}' completed task (stub - no executor configured)",
                    input.agent_type
                );
                if let Some(instance) = self.agents.get_mut(&agent_id) {
                    instance.status = AgentStatus::Completed;
                    instance.output = Some(stub_output.clone());
                }
                Some(stub_output)
            };

            Ok(SpawnResult {
                agent_id,
                output,
                background: None,
                // Foreground agents have completed — no token needed
                cancel_token: None,
                color: definition.color.clone(),
            })
        }
    }

    /// Resume a previously backgrounded agent.
    pub async fn resume(&mut self, agent_id: &str) -> Result<String> {
        let instance = self.agents.get_mut(agent_id).ok_or_else(|| {
            subagent_error::AgentNotFoundSnafu {
                agent_id: agent_id.to_string(),
            }
            .build()
        })?;

        if instance.status != AgentStatus::Backgrounded {
            return Err(subagent_error::AgentInvalidStateSnafu {
                agent_id: agent_id.to_string(),
                status: format!("{:?}", instance.status),
            }
            .build());
        }

        tracing::info!(agent_id, "Resuming backgrounded agent");
        instance.status = AgentStatus::Running;
        Ok(agent_id.to_string())
    }

    /// Get the output of a completed agent.
    pub async fn get_output(&self, agent_id: &str) -> Option<String> {
        self.agents.get(agent_id).and_then(|a| a.output.clone())
    }

    /// Get the current status of an agent.
    pub fn get_status(&self, agent_id: &str) -> Option<AgentStatus> {
        self.agents.get(agent_id).map(|a| a.status.clone())
    }

    /// Remove a completed/failed agent from tracking.
    ///
    /// Returns `None` if the agent is still running or backgrounded.
    pub fn remove_agent(&mut self, agent_id: &str) -> Option<AgentInstance> {
        match self.agents.get(agent_id).map(|a| &a.status) {
            Some(AgentStatus::Completed | AgentStatus::Failed) => self.agents.remove(agent_id),
            _ => None,
        }
    }

    /// Remove all completed and failed agents. Returns the count removed.
    pub fn gc_completed(&mut self) -> usize {
        let before = self.agents.len();
        self.agents
            .retain(|_, a| !matches!(a.status, AgentStatus::Completed | AgentStatus::Failed));
        before - self.agents.len()
    }

    /// Get count of tracked agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Get count of agents by status.
    pub fn status_counts(&self) -> HashMap<AgentStatus, usize> {
        let mut counts = HashMap::new();
        for agent in self.agents.values() {
            *counts.entry(agent.status.clone()).or_insert(0) += 1;
        }
        counts
    }
}

impl Default for SubagentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
