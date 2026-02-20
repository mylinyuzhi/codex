use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cocode_protocol::LoopEvent;
use cocode_protocol::execution::ExecutionIdentity;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::background::BackgroundAgent;
use crate::definition::AgentDefinition;
use crate::filter::filter_tools_for_agent;
use crate::spawn::SpawnInput;

/// Runtime status of a subagent instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Callback type for executing an agent with filtered tools.
///
/// Returns the agent output as a string on success.
pub type AgentExecuteFn = Box<
    dyn Fn(
            AgentExecuteParams,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

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
    pub async fn spawn(&mut self, agent_type: &str, prompt: &str) -> anyhow::Result<String> {
        let input = SpawnInput {
            agent_type: agent_type.to_string(),
            prompt: prompt.to_string(),
            identity: None,
            max_turns: None,
            run_in_background: false,
            allowed_tools: None,
            resume_from: None,
        };
        let result = self.spawn_full(input).await?;
        Ok(result.agent_id)
    }

    /// Spawn a subagent with full configuration and tool filtering.
    ///
    /// This is the main entry point for spawning subagents:
    /// 1. Resolves the agent definition
    /// 2. Filters tools based on definition and spawn input
    /// 3. If resuming, loads prior output and prepends to prompt
    /// 4. Executes the agent (foreground or background)
    /// 5. Returns the result
    pub async fn spawn_full(&mut self, mut input: SpawnInput) -> anyhow::Result<SpawnResult> {
        let definition = self
            .definitions
            .iter()
            .find(|d| d.agent_type == input.agent_type)
            .ok_or_else(|| anyhow::anyhow!("Unknown agent type: {}", input.agent_type))?
            .clone();

        // Handle resume: load full transcript and reconstruct context
        if let Some(ref resume_id) = input.resume_from {
            let output_file = self.output_dir.join(format!("{resume_id}.jsonl"));
            if output_file.exists() {
                match crate::transcript::TranscriptRecorder::read_transcript(&output_file) {
                    Ok(entries) if !entries.is_empty() => {
                        // Reconstruct conversation context from all transcript entries
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
            } else {
                tracing::warn!(
                    resume_id = %resume_id,
                    "Prior agent output file not found, starting fresh"
                );
            }
        }

        let agent_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            agent_id = %agent_id,
            agent_type = %input.agent_type,
            prompt_len = input.prompt.len(),
            background = input.run_in_background,
            resume_from = ?input.resume_from,
            "Spawning subagent"
        );

        // Resolve identity (spawn input > definition > inherit parent)
        // Priority: input.identity > definition.identity > None (inherit)
        let identity = input
            .identity
            .clone()
            .or_else(|| definition.identity.clone());

        // Resolve max_turns (spawn input > definition)
        let max_turns = input.max_turns.or(definition.max_turns);

        // Inject critical_reminder into prompt (prepend before user prompt)
        let prompt = if let Some(ref reminder) = definition.critical_reminder {
            format!("{reminder}\n\n{}", input.prompt)
        } else {
            input.prompt.clone()
        };

        tracing::debug!(
            agent_id = %agent_id,
            has_critical_reminder = definition.critical_reminder.is_some(),
            permission_mode = ?definition.permission_mode,
            fork_context = definition.fork_context,
            "Resolved agent definition fields"
        );

        // Apply three-layer tool filtering
        let tools_to_filter = if let Some(ref allowed) = input.allowed_tools {
            // If spawn input specifies tools, use those as the base
            allowed.clone()
        } else {
            self.all_tools.clone()
        };
        let filtered_tools =
            filter_tools_for_agent(&tools_to_filter, &definition, input.run_in_background);

        tracing::debug!(
            agent_id = %agent_id,
            tools_count = filtered_tools.len(),
            "Filtered tools for subagent"
        );

        // Create cancellation token for this agent
        let cancel_token = CancellationToken::new();

        if input.run_in_background {
            // Background execution
            let output_file = self.output_dir.join(format!("{agent_id}.jsonl"));

            // Ensure output directory exists
            if let Err(e) = tokio::fs::create_dir_all(&self.output_dir).await {
                tracing::warn!(error = %e, "Failed to create output directory");
            }

            let instance = AgentInstance {
                id: agent_id.clone(),
                agent_type: input.agent_type.clone(),
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

                let params = AgentExecuteParams {
                    agent_type: input.agent_type.clone(),
                    prompt: prompt.clone(),
                    identity,
                    max_turns,
                    tools: filtered_tools,
                    cancel_token: cancel_token.clone(),
                    permission_mode: definition.permission_mode,
                    fork_context: definition.fork_context,
                };

                let prompt_for_transcript = prompt.clone();
                let event_tx = self.event_tx.clone();
                tokio::spawn(async move {
                    let result = execute_fn(params).await;

                    // Write transcript entry with prompt + output for rich resume
                    let recorder =
                        crate::transcript::TranscriptRecorder::new(output_file_clone.clone());
                    let entry = match &result {
                        Ok(output) => serde_json::json!({
                            "status": "completed",
                            "agent_id": agent_id_clone,
                            "prompt": prompt_for_transcript,
                            "output": output
                        }),
                        Err(e) => serde_json::json!({
                            "status": "failed",
                            "agent_id": agent_id_clone,
                            "prompt": prompt_for_transcript,
                            "error": e.to_string()
                        }),
                    };
                    if let Err(e) = recorder.record(&entry) {
                        tracing::error!(error = %e, "Failed to write agent transcript");
                    }

                    // Notify main agent of completion
                    if let Some(tx) = event_tx {
                        let output_str = result.as_deref().unwrap_or("[agent failed]").to_string();
                        let _ = tx
                            .send(LoopEvent::SubagentCompleted {
                                agent_id: agent_id_clone.clone(),
                                result: output_str,
                            })
                            .await;
                    }
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
                let params = AgentExecuteParams {
                    agent_type: input.agent_type.clone(),
                    prompt: prompt.clone(),
                    identity: identity.clone(),
                    max_turns,
                    tools: filtered_tools.clone(),
                    cancel_token: cancel_token.clone(),
                    permission_mode: definition.permission_mode,
                    fork_context: definition.fork_context,
                };

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
                                return Err(e);
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
                        let prompt_for_transcript = prompt.clone();
                        let event_tx = self.event_tx.clone();

                        tokio::spawn(async move {
                            let result = execute_future.await;

                            // Write transcript entry with prompt + output for rich resume
                            let recorder = crate::transcript::TranscriptRecorder::new(
                                output_file_clone.clone(),
                            );
                            let entry = match &result {
                                Ok(output) => serde_json::json!({
                                    "status": "completed",
                                    "agent_id": agent_id_clone,
                                    "prompt": prompt_for_transcript,
                                    "output": output,
                                    "transitioned_from_foreground": true
                                }),
                                Err(e) => serde_json::json!({
                                    "status": "failed",
                                    "agent_id": agent_id_clone,
                                    "prompt": prompt_for_transcript,
                                    "error": e.to_string(),
                                    "transitioned_from_foreground": true
                                }),
                            };
                            if let Err(e) = recorder.record(&entry) {
                                tracing::error!(
                                    error = %e,
                                    "Failed to write agent transcript"
                                );
                            }

                            // Notify main agent of completion
                            if let Some(tx) = event_tx {
                                let output_str = result
                                    .as_deref()
                                    .unwrap_or("[agent failed]")
                                    .to_string();
                                let _ = tx
                                    .send(LoopEvent::SubagentCompleted {
                                        agent_id: agent_id_clone.clone(),
                                        result: output_str,
                                    })
                                    .await;
                            }
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
    pub async fn resume(&mut self, agent_id: &str) -> anyhow::Result<String> {
        let instance = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found: {agent_id}"))?;

        if instance.status != AgentStatus::Backgrounded {
            anyhow::bail!(
                "Agent {agent_id} is not backgrounded (status: {:?})",
                instance.status
            );
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
}

impl Default for SubagentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
