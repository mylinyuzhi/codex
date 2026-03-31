use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Instant;

use cocode_error::BoxedError;
use cocode_protocol::CoreEvent;
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
    Killed,
}

impl AgentStatus {
    /// Whether this status represents a terminal (finished) state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// How an agent ended up in the background.
///
/// CC distinguishes `background: true` (explicit) from `isBackgrounded: true`
/// (Ctrl+B / timeout). This enum captures that distinction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackgroundOrigin {
    /// Spawned with `run_in_background: true`.
    Explicit,
    /// Transitioned via Ctrl+B signal.
    Signal,
    /// Auto-backgrounded after timeout.
    Timeout,
}

/// Generate a type-prefixed task ID matching CC's `a{8hex}` / `b{8hex}` format.
fn generate_prefixed_id(prefix: char) -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}{}", &hex[..8])
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

    /// How this agent ended up in the background (if applicable).
    pub background_origin: Option<BackgroundOrigin>,

    /// When this agent completed/failed/was killed (for GC).
    pub completed_at: Option<Instant>,

    /// Byte offset for incremental delta reads of the output file.
    pub last_read_offset: u64,

    /// Whether the parent agent has been notified of this agent's completion.
    pub parent_notified: bool,
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
    /// Auto memory state shared from the parent agent.
    ///
    /// Enables the child's system-reminder generators to inject memory prompts
    /// and the tool permission pipeline to auto-allow memory file writes.
    pub auto_memory_state: Option<std::sync::Arc<cocode_auto_memory::AutoMemoryState>>,
    /// Team to associate the agent with.
    ///
    /// When set, the agent's `AgentIdentity` will carry this team name,
    /// enabling team-aware system reminders and mailbox access.
    pub team_name: Option<String>,
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
    event_tx: Option<&'a mpsc::Sender<CoreEvent>>,
    stop_hook_fn: Option<&'a BackgroundStopHookFn>,
    agent_type: &'a str,
    transitioned_from_foreground: bool,
    /// Guard to prevent duplicate completion notifications.
    /// Set to `true` on first call; subsequent calls are no-ops.
    notified: &'a AtomicBool,
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
        notified,
    } = ctx;

    // Guard against duplicate notifications (e.g., completion racing with kill)
    if notified.swap(true, Ordering::SeqCst) {
        tracing::debug!(
            agent_id = %agent_id,
            "Skipping duplicate background completion notification"
        );
        return;
    }

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
        let is_error = result.is_err();
        let output_str = match result {
            Ok(s) => s.to_string(),
            Err(e) => e.output_msg(),
        };
        let _ = tx
            .send(CoreEvent::Protocol(
                cocode_protocol::server_notification::ServerNotification::SubagentCompleted(
                    cocode_protocol::server_notification::SubagentCompletedParams {
                        agent_id: agent_id.to_string(),
                        result: output_str,
                        is_error,
                    },
                ),
            ))
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
    let prompt = input.prompt.clone();
    if definition.use_custom_prompt {
        ResolvedPrompt {
            prompt,
            custom_system_prompt: definition.critical_reminder.clone(),
            system_prompt_suffix: None,
        }
    } else {
        ResolvedPrompt {
            prompt,
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
    auto_memory_state: Option<std::sync::Arc<cocode_auto_memory::AutoMemoryState>>,
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
        auto_memory_state,
        team_name: input.team_name.clone(),
    }
}

/// Default limit on concurrent background agents.
const DEFAULT_MAX_BACKGROUND_AGENTS: usize = 8;

/// Lightweight snapshot of an agent instance for status reporting.
#[derive(Debug, Clone)]
pub struct AgentInstanceInfo {
    /// Agent instance ID.
    pub id: String,
    /// The agent type (e.g., "Explore", "Plan").
    pub agent_type: String,
    /// Display name (from spawn input).
    pub name: Option<String>,
    /// Current execution status.
    pub status: AgentStatus,
    /// How this agent ended up in the background (if applicable).
    pub background_origin: Option<BackgroundOrigin>,
    /// Background output file path (if running in background).
    pub output_file: Option<PathBuf>,
    /// Whether the parent agent has been notified of this agent's completion.
    pub parent_notified: bool,
}

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
    event_tx: Option<mpsc::Sender<CoreEvent>>,
    /// Optional callback for SubagentStop hooks on background completion.
    background_stop_hook_fn: Option<BackgroundStopHookFn>,
    /// Maximum number of concurrent background agents.
    max_background_agents: usize,
    /// Auto-background timeout for foreground agents.
    /// If a foreground agent runs longer than this, it is automatically
    /// transitioned to background. `None` disables auto-backgrounding.
    auto_background_timeout: Option<std::time::Duration>,
    /// Auto memory state to propagate to child agents.
    auto_memory_state: Option<std::sync::Arc<cocode_auto_memory::AutoMemoryState>>,
}

impl SubagentManager {
    /// Create a new empty subagent manager.
    ///
    /// Auto-background timeout defaults to `None` (disabled). Use
    /// [`with_auto_background_timeout`] to enable it — the config layer
    /// should read `COCODE_AUTO_BACKGROUND_TASKS` and pass the value in.
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
            auto_background_timeout: None,
            auto_memory_state: None,
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

    /// Set the auto memory state to propagate to child agents.
    pub fn with_auto_memory_state(
        mut self,
        state: std::sync::Arc<cocode_auto_memory::AutoMemoryState>,
    ) -> Self {
        self.auto_memory_state = Some(state);
        self
    }

    /// Set the maximum number of concurrent background agents.
    pub fn with_max_background_agents(mut self, n: usize) -> Self {
        self.max_background_agents = n;
        self
    }

    /// Set the auto-background timeout for foreground agents.
    ///
    /// If a foreground agent runs longer than this duration, it is automatically
    /// transitioned to background (same as Ctrl+B). `None` disables auto-backgrounding.
    pub fn with_auto_background_timeout(mut self, timeout: Option<std::time::Duration>) -> Self {
        self.auto_background_timeout = timeout;
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
    pub fn set_event_tx(&mut self, tx: mpsc::Sender<CoreEvent>) {
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

    /// Get agent definitions filtered by MCP server availability.
    ///
    /// Agents that declare required `mcp_servers` are excluded if none of
    /// the currently available MCP servers match. Agents with no MCP
    /// requirements pass through.
    ///
    /// Matches Claude Code's `filterByMcpServers` / `validateMcpServers`.
    pub fn available_definitions(&self) -> Vec<&AgentDefinition> {
        let available_servers: std::collections::HashSet<String> = self
            .all_tools
            .iter()
            .filter_map(|t| {
                t.strip_prefix(cocode_protocol::MCP_TOOL_PREFIX)
                    .and_then(|rest| rest.split(cocode_protocol::MCP_TOOL_SEPARATOR).next())
                    .map(str::to_lowercase)
            })
            .collect();

        self.definitions
            .iter()
            .filter(|d| Self::validate_mcp_servers(d, &available_servers))
            .collect()
    }

    /// Check whether all MCP servers required by an agent are available.
    ///
    /// Case-insensitive substring match: `available.contains(required)`.
    fn validate_mcp_servers(
        definition: &AgentDefinition,
        available_servers: &std::collections::HashSet<String>,
    ) -> bool {
        let required = match &definition.mcp_servers {
            Some(servers) if !servers.is_empty() => servers,
            _ => return true,
        };

        required.iter().all(|server| {
            let name = server.name.to_lowercase();
            available_servers
                .iter()
                .any(|available| available.contains(&name))
        })
    }

    /// Register a new agent type definition.
    pub fn register_agent_type(&mut self, definition: AgentDefinition) {
        tracing::info!(agent_type = %definition.agent_type, "Registering agent type");
        self.definitions.push(definition);
    }

    /// Retain only definitions matching the predicate.
    pub fn retain_definitions(&mut self, f: impl Fn(&AgentDefinition) -> bool) {
        self.definitions.retain(f);
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

        let agent_id = generate_prefixed_id('a');
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
                background_origin: Some(BackgroundOrigin::Explicit),
                completed_at: None,
                last_read_offset: 0,
                parent_notified: false,
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
                    self.auto_memory_state.clone(),
                );

                let prompt_for_transcript = resolved.prompt.clone();
                let event_tx = self.event_tx.clone();
                let stop_hook_fn = self.background_stop_hook_fn.clone();
                let agent_type_for_hook = input.agent_type.clone();
                let notified = Arc::new(AtomicBool::new(false));
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
                        notified: &notified,
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
                background_origin: None,
                completed_at: None,
                last_read_offset: 0,
                parent_notified: false,
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
                    self.auto_memory_state.clone(),
                );

                // Pin the future so it can be moved into a background task on Ctrl+B
                let mut execute_future = Box::pin(execute_fn(params));

                // Determine auto-background timeout
                let auto_bg_timeout = self.auto_background_timeout;

                // Use select! to handle normal completion, background signal, and auto-background timeout
                tokio::select! {
                    result = &mut execute_future => {
                        // Normal completion - unregister from background signals
                        crate::signal::unregister_backgroundable_agent(&agent_id);

                        match result {
                            Ok(result) => {
                                if let Some(instance) = self.agents.get_mut(&agent_id) {
                                    instance.status = AgentStatus::Completed;
                                    instance.output = Some(result.clone());
                                    instance.completed_at = Some(Instant::now());
                                }
                                Some(result)
                            }
                            Err(e) => {
                                if let Some(instance) = self.agents.get_mut(&agent_id) {
                                    instance.status = AgentStatus::Failed;
                                    instance.completed_at = Some(Instant::now());
                                }
                                return Err(subagent_error::ExecuteSnafu {
                                    message: "Foreground subagent execution".to_string(),
                                }
                                .into_error(e));
                            }
                        }
                    }
                    _ = bg_signal_rx => {
                        // Background signal received (Ctrl+B) - transition to background
                        tracing::info!(
                            agent_id = %agent_id,
                            trigger = "signal",
                            "Agent transitioned to background"
                        );

                        return self.transition_to_background(
                            agent_id,
                            cancel_token,
                            &definition,
                            &resolved,
                            &input,
                            execute_future,
                            BackgroundOrigin::Signal,
                        ).await;
                    }
                    _ = async {
                        match auto_bg_timeout {
                            Some(d) => tokio::time::sleep(d).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        // Auto-background timeout reached
                        tracing::info!(
                            agent_id = %agent_id,
                            timeout_secs = ?auto_bg_timeout,
                            trigger = "timeout",
                            "Agent auto-transitioned to background"
                        );

                        // Notify main agent of auto-background transition
                        if let Some(ref tx) = self.event_tx {
                            let _ = tx
                                .send(CoreEvent::Protocol(
                                    cocode_protocol::server_notification::ServerNotification::SubagentBackgrounded(
                                        cocode_protocol::server_notification::SubagentBackgroundedParams {
                                            agent_id: agent_id.clone(),
                                            output_file: self.output_dir.join(format!("{agent_id}.jsonl")).to_string_lossy().into_owned(),
                                        },
                                    ),
                                ))
                                .await;
                        }

                        return self.transition_to_background(
                            agent_id,
                            cancel_token,
                            &definition,
                            &resolved,
                            &input,
                            execute_future,
                            BackgroundOrigin::Timeout,
                        ).await;
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
                    instance.completed_at = Some(Instant::now());
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

    /// Transition a foreground agent's in-flight future to a background task.
    ///
    /// Shared logic for Ctrl+B signal and auto-background timeout transitions.
    #[allow(clippy::too_many_arguments)]
    async fn transition_to_background(
        &mut self,
        agent_id: String,
        cancel_token: CancellationToken,
        definition: &AgentDefinition,
        resolved: &ResolvedPrompt,
        input: &SpawnInput,
        execute_future: std::pin::Pin<
            Box<dyn std::future::Future<Output = std::result::Result<String, BoxedError>> + Send>,
        >,
        origin: BackgroundOrigin,
    ) -> Result<SpawnResult> {
        // Unregister from background signal registry (no longer foreground)
        crate::signal::unregister_backgroundable_agent(&agent_id);

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
            instance.background_origin = Some(origin);
            instance.last_read_offset = 0;
            instance.parent_notified = false;
        }

        // Move the in-flight future into a background task
        let agent_id_clone = agent_id.clone();
        let output_file_clone = output_file.clone();
        let prompt_for_transcript = resolved.prompt.clone();
        let event_tx = self.event_tx.clone();
        let stop_hook_fn = self.background_stop_hook_fn.clone();
        let agent_type_for_hook = input.agent_type.clone();
        let notified = Arc::new(AtomicBool::new(false));

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
                notified: &notified,
            })
            .await;
        });

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

    /// Remove a completed/failed/killed agent from tracking.
    ///
    /// Returns `None` if the agent is still running or backgrounded.
    pub fn remove_agent(&mut self, agent_id: &str) -> Option<AgentInstance> {
        match self.agents.get(agent_id).map(|a| a.status.is_terminal()) {
            Some(true) => self.agents.remove(agent_id),
            _ => None,
        }
    }

    /// Remove all completed, failed, and killed agents. Returns the count removed.
    pub fn gc_completed(&mut self) -> usize {
        let before = self.agents.len();
        self.agents.retain(|_, a| !a.status.is_terminal());
        before - self.agents.len()
    }

    /// Remove completed/failed/killed agents older than `max_age`. Returns count removed.
    ///
    /// Unlike `gc_completed()`, this preserves recently finished agents so they
    /// remain visible to TaskOutput for a while after completion. Agents whose
    /// parent has not yet been notified are always retained regardless of age.
    pub fn gc_stale(&mut self, max_age: std::time::Duration) -> usize {
        let before = self.agents.len();
        let now = Instant::now();
        self.agents.retain(|_, a| {
            if !a.status.is_terminal() {
                return true; // keep running/backgrounded agents
            }
            if !a.parent_notified {
                return true; // keep unnotified agents regardless of age
            }
            match a.completed_at {
                Some(at) => now.duration_since(at) < max_age,
                None => true, // no timestamp yet, keep
            }
        });
        before - self.agents.len()
    }

    /// Kill all running and backgrounded agents. Returns IDs of killed agents.
    pub fn kill_all_running(&mut self) -> Vec<String> {
        let mut killed = Vec::new();
        for (id, instance) in &mut self.agents {
            if matches!(
                instance.status,
                AgentStatus::Running | AgentStatus::Backgrounded
            ) {
                if let Some(token) = &instance.cancel_token {
                    token.cancel();
                }
                instance.status = AgentStatus::Killed;
                instance.completed_at = Some(Instant::now());
                killed.push(id.clone());
            }
        }
        killed
    }

    /// Get count of tracked agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Promote `Failed` agents to `Killed` if their ID is in the given set.
    ///
    /// After `TaskStop` cancels an agent, the background completion handler
    /// marks it `Failed`. This method upgrades the status so GC, status
    /// reporting, and match arms correctly reflect user-initiated cancellation.
    pub fn promote_killed(&mut self, killed_ids: &std::collections::HashSet<String>) {
        for (id, instance) in &mut self.agents {
            if instance.status == AgentStatus::Failed && killed_ids.contains(id) {
                instance.status = AgentStatus::Killed;
            }
        }
    }

    /// Returns lightweight info snapshots of all tracked agents.
    pub fn agent_infos(&self) -> Vec<AgentInstanceInfo> {
        self.agents
            .values()
            .map(|a| AgentInstanceInfo {
                id: a.id.clone(),
                agent_type: a.agent_type.clone(),
                name: a.name.clone(),
                status: a.status.clone(),
                background_origin: a.background_origin.clone(),
                output_file: a.output_file.clone(),
                parent_notified: a.parent_notified,
            })
            .collect()
    }

    /// Read delta output from background agents since last read.
    ///
    /// Returns `(agent_id, delta_text)` pairs. Updates internal offsets so
    /// subsequent calls only return new content.
    pub async fn read_deltas(&mut self) -> Vec<(String, String)> {
        let mut deltas = Vec::new();
        for (id, instance) in &mut self.agents {
            let Some(ref output_file) = instance.output_file else {
                continue;
            };
            // Skip only Killed agents — all others may have useful output
            if instance.status == AgentStatus::Killed {
                continue;
            }
            match crate::transcript::read_from_offset(output_file, instance.last_read_offset).await
            {
                Ok((entries, new_offset)) => {
                    if new_offset > instance.last_read_offset {
                        instance.last_read_offset = new_offset;
                        let summary: Vec<String> = entries
                            .iter()
                            .filter_map(|e| {
                                e.get("output")
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                                    .or_else(|| {
                                        e.get("message").and_then(|v| v.as_str()).map(String::from)
                                    })
                                    .or_else(|| {
                                        e.get("text").and_then(|v| v.as_str()).map(String::from)
                                    })
                            })
                            .collect();
                        if !summary.is_empty() {
                            let joined = summary.join(" | ");
                            deltas.push((
                                id.clone(),
                                cocode_utils_string::truncate_str(&joined, 500),
                            ));
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(agent_id = %id, error = %e, "Failed to read agent delta");
                }
            }
        }
        deltas
    }

    /// Mark an agent as notified by the parent. Returns `true` if the agent was found.
    pub fn mark_notified(&mut self, agent_id: &str) -> bool {
        if let Some(instance) = self.agents.get_mut(agent_id) {
            instance.parent_notified = true;
            true
        } else {
            false
        }
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
