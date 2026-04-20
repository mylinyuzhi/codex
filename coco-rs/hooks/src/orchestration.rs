//! Hook orchestration — parallel execution, result aggregation, and env injection.
//!
//! TS: utils/hooks.ts (executeHooks, executeHooksOutsideREPL, executePreToolHooks,
//! executePostToolHooks, executePreCompactHooks, executePostCompactHooks,
//! executeSessionStartHooks, executeSessionEndHooks, executeStopFailureHooks).
//!
//! This module provides the high-level orchestration layer that:
//! 1. Builds structured hook inputs (serialized as JSON for command stdin)
//! 2. Injects environment variables (tool name, session ID, CWD, etc.)
//! 3. Executes matching hooks in parallel with per-hook timeouts
//! 4. Parses hook stdout as JSON or plain text
//! 5. Aggregates results into a single `AggregatedHookResult`

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use coco_config::EnvKey;
use coco_config::env;
use coco_types::HookEventType;
use coco_types::HookOutcome;
use coco_types::PermissionBehavior;

use crate::HookExecutionResult;
use crate::HookHandler;
use crate::HookRegistry;
use crate::execute_hook;

/// Default timeout for tool-related hook execution (10 minutes).
///
/// TS: TOOL_HOOK_EXECUTION_TIMEOUT_MS = 10 * 60 * 1000
const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Default timeout for SessionEnd hooks (1.5 seconds).
///
/// TS: SESSION_END_HOOK_TIMEOUT_MS_DEFAULT = 1500
const SESSION_END_HOOK_TIMEOUT: Duration = Duration::from_millis(1500);

// ---------------------------------------------------------------------------
// Hook input types — re-exported from inputs module
// ---------------------------------------------------------------------------

pub use crate::inputs::BaseHookInput;
pub use crate::inputs::CompactHookInput;
pub use crate::inputs::HookInput;
pub use crate::inputs::PostToolUseInput;
pub use crate::inputs::PreToolUseInput;
pub use crate::inputs::SessionEndInput;
pub use crate::inputs::SessionStartInput;
pub use crate::inputs::StopFailureInput;
pub use crate::inputs::StopInput;
pub use crate::inputs::base_from_ctx;

// ---------------------------------------------------------------------------
// Hook JSON output parsing (stdout)
// ---------------------------------------------------------------------------

/// Structured JSON output from a hook command's stdout.
///
/// TS: hookJSONOutputSchema — the hook writes JSON to stdout for structured control.
/// Supports both flat fields (Rust-native) and nested `hookSpecificOutput` (TS compat).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HookJsonOutput {
    /// If false, the agent should stop after this turn.
    #[serde(rename = "continue")]
    pub should_continue: Option<bool>,
    /// Suppress default output display.
    #[serde(alias = "suppressOutput")]
    pub suppress_output: Option<bool>,
    /// Reason string when stopping.
    #[serde(alias = "stopReason")]
    pub stop_reason: Option<String>,
    /// "approve" or "block" — used for permission-decision hooks.
    pub decision: Option<String>,
    /// Human-readable reason for the decision.
    pub reason: Option<String>,
    /// Message to inject into conversation as a system message.
    #[serde(alias = "systemMessage")]
    pub system_message: Option<String>,
    /// Permission decision override for PreToolUse hooks (flat format).
    #[serde(alias = "permissionDecision")]
    pub permission_decision: Option<String>,
    /// Additional context to inject (flat format).
    #[serde(alias = "additionalContext")]
    pub additional_context: Option<String>,
    /// Updated tool input (PreToolUse only, flat format).
    #[serde(alias = "updatedInput")]
    pub updated_input: Option<serde_json::Value>,
    /// Updated MCP tool output (PostToolUse only, flat format).
    #[serde(alias = "updatedMCPToolOutput")]
    pub updated_mcp_tool_output: Option<serde_json::Value>,
    /// Initial user message to inject (SessionStart hooks, flat format).
    #[serde(alias = "initialUserMessage")]
    pub initial_user_message: Option<String>,
    /// File watch paths to register (CwdChanged/FileChanged hooks).
    #[serde(default, alias = "watchPaths")]
    pub watch_paths: Vec<String>,
    /// Whether to retry the operation (PermissionDenied hooks).
    #[serde(default)]
    pub retry: bool,
    /// Human-readable status for progress display.
    #[serde(alias = "statusMessage")]
    pub status_message: Option<String>,
    /// When true, the hook runner should re-wake after async completion.
    #[serde(default, alias = "asyncRewake")]
    pub async_rewake: bool,
    /// TS-style nested event-specific output.
    ///
    /// TS: hookSpecificOutput — event-tagged output with event-specific fields.
    /// When present, fields from this object override flat-format fields.
    #[serde(default, alias = "hookSpecificOutput")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

/// Event-specific hook output (TS parity).
///
/// TS: hookSpecificOutput in syncHookResponseSchema — tagged by hookEventName.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hookEventName")]
pub enum HookSpecificOutput {
    PreToolUse {
        #[serde(default, alias = "permissionDecision")]
        permission_decision: Option<String>,
        #[serde(default, alias = "permissionDecisionReason")]
        permission_decision_reason: Option<String>,
        #[serde(default, alias = "updatedInput")]
        updated_input: Option<serde_json::Value>,
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    PostToolUse {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
        #[serde(default, alias = "updatedMCPToolOutput")]
        updated_mcp_tool_output: Option<serde_json::Value>,
    },
    PostToolUseFailure {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    UserPromptSubmit {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    SessionStart {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
        #[serde(default, alias = "initialUserMessage")]
        initial_user_message: Option<String>,
        #[serde(default, alias = "watchPaths")]
        watch_paths: Option<Vec<String>>,
    },
    Setup {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    SubagentStart {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    PermissionDenied {
        #[serde(default)]
        retry: Option<bool>,
    },
    Notification {
        #[serde(default, alias = "additionalContext")]
        additional_context: Option<String>,
    },
    PermissionRequest {
        decision: Option<PermissionRequestDecision>,
    },
    Elicitation {
        action: Option<String>,
        content: Option<serde_json::Value>,
    },
    ElicitationResult {
        action: Option<String>,
        content: Option<serde_json::Value>,
    },
    CwdChanged {
        #[serde(default, alias = "watchPaths")]
        watch_paths: Option<Vec<String>>,
    },
    FileChanged {
        #[serde(default, alias = "watchPaths")]
        watch_paths: Option<Vec<String>>,
    },
    WorktreeCreate {
        #[serde(default, alias = "worktreePath")]
        worktree_path: Option<String>,
    },
}

/// Decision from a PermissionRequest hook.
///
/// TS: PermissionRequestResult — allow with optional updatedInput, or deny
/// with optional message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior")]
pub enum PermissionRequestDecision {
    #[serde(rename = "allow")]
    Allow {
        #[serde(default, alias = "updatedInput")]
        updated_input: Option<serde_json::Value>,
    },
    #[serde(rename = "deny")]
    Deny {
        message: Option<String>,
        interrupt: Option<bool>,
    },
}

/// Parse hook stdout, attempting JSON first, falling back to plain text.
///
/// TS: parseHookOutput()
pub fn parse_hook_output(stdout: &str) -> ParsedHookOutput {
    let trimmed = stdout.trim();
    if !trimmed.starts_with('{') {
        return ParsedHookOutput::PlainText(stdout.to_string());
    }
    match serde_json::from_str::<HookJsonOutput>(trimmed) {
        Ok(json) => ParsedHookOutput::Json(Box::new(json)),
        Err(e) => {
            tracing::debug!("failed to parse hook JSON output: {e}");
            ParsedHookOutput::PlainText(stdout.to_string())
        }
    }
}

/// Result of parsing hook stdout.
#[derive(Debug, Clone)]
pub enum ParsedHookOutput {
    Json(Box<HookJsonOutput>),
    PlainText(String),
}

// ---------------------------------------------------------------------------
// Aggregated result
// ---------------------------------------------------------------------------

/// Blocking error from a hook.
///
/// TS: HookBlockingError
#[derive(Debug, Clone)]
pub struct HookBlockingError {
    pub blocking_error: String,
    pub command: String,
}

/// Aggregated result from executing all matching hooks for a single event.
///
/// TS: AggregatedHookResult
#[derive(Debug, Clone, Default)]
pub struct AggregatedHookResult {
    pub blocking_error: Option<HookBlockingError>,
    pub prevent_continuation: bool,
    pub stop_reason: Option<String>,
    pub permission_behavior: Option<PermissionBehavior>,
    pub hook_permission_decision_reason: Option<String>,
    pub additional_contexts: Vec<String>,
    pub updated_input: Option<serde_json::Value>,
    /// Updated MCP tool output (PostToolUse only).
    pub updated_mcp_tool_output: Option<serde_json::Value>,
    pub system_message: Option<String>,
    /// Whether to suppress default output display.
    pub suppress_output: bool,
    /// Initial user message injected by SessionStart hooks.
    pub initial_user_message: Option<String>,
    /// File watch paths to register (from CwdChanged/FileChanged hooks).
    pub watch_paths: Vec<String>,
    /// Whether to retry the operation (from PermissionDenied hooks).
    pub retry: bool,
    /// Human-readable status from the last hook that provided one.
    pub status_message: Option<String>,
    /// True if any hook requested async re-wake.
    pub async_rewake: bool,
    /// PermissionRequest hook decision.
    pub permission_request_result: Option<PermissionRequestDecision>,
    /// Elicitation hook response.
    pub elicitation_response: Option<ElicitationResponse>,
    /// ElicitationResult hook response.
    pub elicitation_result_response: Option<ElicitationResponse>,
}

/// Elicitation response from a hook.
///
/// TS: elicitationResponse in HookResult.
#[derive(Debug, Clone)]
pub struct ElicitationResponse {
    pub action: String,
    pub content: Option<serde_json::Value>,
}

impl AggregatedHookResult {
    pub fn is_blocked(&self) -> bool {
        self.blocking_error.is_some()
    }
}

/// Result of executing a single hook (command, prompt, http, etc).
///
/// TS: HookOutsideReplResult
#[derive(Debug, Clone)]
pub struct SingleHookResult {
    pub command: String,
    pub succeeded: bool,
    pub output: String,
    pub blocked: bool,
    pub outcome: HookOutcome,
    /// Human-readable status for progress display.
    pub status_message: Option<String>,
    /// When true, the hook runner should re-wake after async completion.
    pub async_rewake: bool,
}

// ---------------------------------------------------------------------------
// Environment variable injection
// ---------------------------------------------------------------------------

/// Plugin context for hook environment variables.
///
/// TS: execCommandHook() sets CLAUDE_PLUGIN_ROOT, CLAUDE_PLUGIN_DATA,
/// and CLAUDE_PLUGIN_OPTION_* env vars for plugin/skill hooks.
#[derive(Debug, Clone, Default)]
pub struct HookPluginContext {
    /// Root directory for the plugin or skill.
    pub plugin_root: Option<String>,
    /// Plugin identifier (for data directory).
    pub plugin_id: Option<String>,
    /// Plugin configuration options (exposed as CLAUDE_PLUGIN_OPTION_*).
    pub plugin_options: HashMap<String, String>,
    /// Root directory for the skill (uses CLAUDE_PLUGIN_ROOT for compat).
    pub skill_root: Option<String>,
}

/// Build the set of environment variables to inject into a hook command process.
///
/// TS: env vars built inside execCommandHook() — CLAUDE_PROJECT_DIR, session_id, etc.
pub fn build_hook_env(
    session_id: &str,
    cwd: &str,
    tool_name: Option<&str>,
    hook_event: &str,
    project_dir: Option<&str>,
) -> HashMap<String, String> {
    build_hook_env_with_plugin(
        session_id,
        cwd,
        tool_name,
        hook_event,
        project_dir,
        None,
        None,
    )
}

/// Build hook environment with optional plugin context.
pub fn build_hook_env_with_plugin(
    session_id: &str,
    cwd: &str,
    tool_name: Option<&str>,
    hook_event: &str,
    project_dir: Option<&str>,
    plugin_ctx: Option<&HookPluginContext>,
    hook_index: Option<usize>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("HOOK_EVENT".to_string(), hook_event.to_string());
    env.insert("HOOK_SESSION_ID".to_string(), session_id.to_string());
    env.insert("HOOK_CWD".to_string(), cwd.to_string());
    if let Some(name) = tool_name {
        env.insert("HOOK_TOOL_NAME".to_string(), name.to_string());
    }
    if let Some(dir) = project_dir {
        env.insert("CLAUDE_PROJECT_DIR".to_string(), dir.to_string());
    }

    // Plugin/skill env vars (TS parity).
    if let Some(ctx) = plugin_ctx {
        if let Some(root) = &ctx.plugin_root {
            env.insert("CLAUDE_PLUGIN_ROOT".to_string(), root.clone());
        }
        if let Some(id) = &ctx.plugin_id {
            // Plugin data dir convention: ~/.claude/plugins/<plugin_id>/data
            if let Ok(home) = std::env::var("HOME") {
                let data_dir = format!("{home}/.claude/plugins/{id}/data");
                env.insert("CLAUDE_PLUGIN_DATA".to_string(), data_dir);
            }
        }
        for (key, value) in &ctx.plugin_options {
            // Sanitize key to valid env var identifier (TS parity).
            let env_key = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .to_ascii_uppercase();
            env.insert(format!("CLAUDE_PLUGIN_OPTION_{env_key}"), value.clone());
        }
        // Skill root uses CLAUDE_PLUGIN_ROOT for consistency (TS parity).
        if let Some(root) = &ctx.skill_root {
            env.insert("CLAUDE_PLUGIN_ROOT".to_string(), root.clone());
        }
    }

    // CLAUDE_ENV_FILE — for SessionStart, Setup, CwdChanged, FileChanged.
    // Each hook gets a unique file via hook_index to avoid overwrites.
    if let Some(idx) = hook_index
        && matches!(
            hook_event,
            "SessionStart" | "Setup" | "CwdChanged" | "FileChanged"
        )
        && let Ok(tmp) = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TMP"))
            .or_else(|_| Ok::<String, std::env::VarError>("/tmp".to_string()))
    {
        let env_file = format!("{tmp}/claude-hook-env-{session_id}-{hook_event}-{idx}.sh");
        env.insert("CLAUDE_ENV_FILE".to_string(), env_file);
    }

    env
}

// ---------------------------------------------------------------------------
// Core parallel execution engine
// ---------------------------------------------------------------------------

/// Execute matching hooks in parallel with per-hook timeouts.
///
/// TS: executeHooksOutsideREPL runs all hooks via `Promise.all(hookPromises)`.
///
/// Each hook runs in its own tokio task. A `CancellationToken` can abort all
/// outstanding hooks. Returns one `SingleHookResult` per matched hook.
#[allow(clippy::too_many_arguments)]
pub async fn execute_hooks_parallel(
    registry: &HookRegistry,
    event: HookEventType,
    tool_name: Option<&str>,
    hook_input_json: &str,
    env_vars: &HashMap<String, String>,
    cancel: &CancellationToken,
    default_timeout: Duration,
    event_tx: Option<&tokio::sync::mpsc::Sender<crate::HookExecutionEvent>>,
) -> Vec<SingleHookResult> {
    execute_hooks_parallel_filtered(
        registry,
        event,
        tool_name,
        hook_input_json,
        env_vars,
        cancel,
        default_timeout,
        event_tx,
        /*allow_managed_hooks_only*/ false,
    )
    .await
}

/// Execute hooks with managed-only filtering support.
#[allow(clippy::too_many_arguments)]
async fn execute_hooks_parallel_filtered(
    registry: &HookRegistry,
    event: HookEventType,
    tool_name: Option<&str>,
    hook_input_json: &str,
    env_vars: &HashMap<String, String>,
    cancel: &CancellationToken,
    default_timeout: Duration,
    event_tx: Option<&tokio::sync::mpsc::Sender<crate::HookExecutionEvent>>,
    allow_managed_hooks_only: bool,
) -> Vec<SingleHookResult> {
    let matching = registry.find_matching(event, tool_name);
    if matching.is_empty() {
        return Vec::new();
    }

    // Filter hooks based on policies.
    let matching: Vec<_> = matching
        .into_iter()
        .filter(|h| {
            // HTTP hooks are not supported for SessionStart/Setup events.
            if matches!(event, HookEventType::SessionStart | HookEventType::Setup)
                && matches!(h.handler, HookHandler::Http { .. })
            {
                tracing::debug!("HTTP hooks not supported for {event:?}, skipping");
                return false;
            }
            // When allow_managed_hooks_only, skip non-managed hooks.
            // Managed hooks use Builtin scope (from policy config).
            // Session hooks are also allowed (programmatically added).
            if allow_managed_hooks_only
                && !matches!(
                    h.scope,
                    coco_types::HookScope::Builtin | coco_types::HookScope::Session
                )
            {
                tracing::debug!(
                    "skipping non-managed hook for {event:?} (scope={:?})",
                    h.scope
                );
                return false;
            }
            true
        })
        .collect();
    if matching.is_empty() {
        return Vec::new();
    }

    let (tx, mut rx) = mpsc::channel::<SingleHookResult>(matching.len());

    for (idx, hook) in matching.iter().enumerate() {
        let handler = hook.handler.clone();
        let cancel = cancel.clone();
        let env = env_vars.clone();
        let input_json = hook_input_json.to_string();
        let timeout = resolve_timeout(&handler, default_timeout);
        let command_label = handler_label(&handler);
        let hook_id = format!("hook-{idx}");
        let hook_event_str = format!("{event:?}");
        let event_tx = event_tx.cloned();
        let is_async = hook.is_async;
        // Only clone sender for sync hooks — async hooks are fire-and-forget.
        let tx = if is_async { None } else { Some(tx.clone()) };

        // Emit Started event before spawning the task.
        if let Some(etx) = &event_tx {
            let _ = etx
                .send(crate::HookExecutionEvent::Started {
                    hook_id: hook_id.clone(),
                    hook_name: command_label.clone(),
                    hook_event: hook_event_str.clone(),
                })
                .await;
        }

        tokio::spawn(async move {
            // Progress polling: emit Progress events every 1s while hook runs.
            // TS: hookEvents.ts — progress interval polling.
            let progress_handle = if let Some(etx) = event_tx.clone() {
                let hid = hook_id.clone();
                let hname = command_label.clone();
                Some(tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(1));
                    interval.tick().await; // skip immediate first tick
                    loop {
                        interval.tick().await;
                        let send_result = etx
                            .send(crate::HookExecutionEvent::Progress {
                                hook_id: hid.clone(),
                                hook_name: hname.clone(),
                                stdout: String::new(),
                                stderr: String::new(),
                            })
                            .await;
                        if send_result.is_err() {
                            break;
                        }
                    }
                }))
            } else {
                None
            };

            let result = tokio::select! {
                _ = cancel.cancelled() => {
                    SingleHookResult {
                        command: command_label.clone(),
                        succeeded: false,
                        output: "Hook cancelled".to_string(),
                        blocked: false,
                        outcome: HookOutcome::Cancelled,
                        status_message: None,
                        async_rewake: false,
                    }
                }
                res = tokio::time::timeout(timeout, execute_hook(&handler, &env, Some(&input_json))) => {
                    match res {
                        Ok(Ok(exec_result)) => process_execution_result(exec_result, &command_label),
                        Ok(Err(e)) => SingleHookResult {
                            command: command_label.clone(),
                            succeeded: false,
                            output: format!("{e}"),
                            blocked: false,
                            outcome: HookOutcome::NonBlockingError,
                            status_message: None,
                            async_rewake: false,
                        },
                        Err(_elapsed) => SingleHookResult {
                            command: command_label.clone(),
                            succeeded: false,
                            output: format!("hook timed out after {timeout:?}"),
                            blocked: false,
                            outcome: HookOutcome::NonBlockingError,
                            status_message: None,
                            async_rewake: false,
                        },
                    }
                }
            };

            // Stop progress polling.
            if let Some(h) = progress_handle {
                h.abort();
            }

            // Emit Response event after completion.
            if let Some(etx) = &event_tx {
                let _ = etx
                    .send(crate::HookExecutionEvent::Response {
                        hook_id,
                        hook_name: result.command.clone(),
                        exit_code: None,
                        stdout: result.output.clone(),
                        stderr: String::new(),
                        outcome: result.outcome,
                    })
                    .await;
            }

            // Async hooks are fire-and-forget — don't block on result.
            // TS: executeInBackground() for hooks with async flag.
            if let Some(tx) = tx {
                let _ = tx.send(result).await;
            }
        });
    }

    // Drop our copy of the sender so the channel closes when all tasks finish.
    drop(tx);

    let mut results = Vec::with_capacity(matching.len());
    while let Some(r) = rx.recv().await {
        results.push(r);
    }
    results
}

/// Aggregate individual hook results into a single `AggregatedHookResult`.
///
/// TS: result aggregation inside the executeHooks() async generator and
/// processHookJSONOutput().
pub fn aggregate_results(results: &[SingleHookResult]) -> AggregatedHookResult {
    let mut agg = AggregatedHookResult::default();

    for r in results {
        if r.blocked {
            agg.blocking_error = Some(HookBlockingError {
                blocking_error: r.output.clone(),
                command: r.command.clone(),
            });
        }

        if r.status_message.is_some() {
            agg.status_message.clone_from(&r.status_message);
        }
        if r.async_rewake {
            agg.async_rewake = true;
        }

        // Parse stdout for JSON control signals.
        match parse_hook_output(&r.output) {
            ParsedHookOutput::Json(json) => {
                if json.should_continue == Some(false) {
                    agg.prevent_continuation = true;
                    if json.stop_reason.is_some() {
                        agg.stop_reason = json.stop_reason.clone();
                    }
                }

                match json.decision.as_deref() {
                    Some("approve") => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Allow,
                        ));
                    }
                    Some("block") => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Deny,
                        ));
                        agg.blocking_error = Some(HookBlockingError {
                            blocking_error: json
                                .reason
                                .clone()
                                .unwrap_or_else(|| "Blocked by hook".to_string()),
                            command: r.command.clone(),
                        });
                    }
                    _ => {}
                }

                match json.permission_decision.as_deref() {
                    Some("allow") => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Allow,
                        ));
                    }
                    Some("deny") => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Deny,
                        ));
                    }
                    Some("ask") => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Ask,
                        ));
                    }
                    _ => {}
                }

                if let Some(reason) = &json.reason
                    && agg.permission_behavior.is_some()
                {
                    agg.hook_permission_decision_reason = Some(reason.clone());
                }

                if let Some(ctx) = &json.additional_context {
                    agg.additional_contexts.push(ctx.clone());
                }

                if json.updated_input.is_some() {
                    agg.updated_input = json.updated_input.clone();
                }

                if json.updated_mcp_tool_output.is_some() {
                    agg.updated_mcp_tool_output = json.updated_mcp_tool_output.clone();
                }

                if json.system_message.is_some() {
                    agg.system_message = json.system_message.clone();
                }

                if json.status_message.is_some() {
                    agg.status_message = json.status_message.clone();
                }

                if json.suppress_output == Some(true) {
                    agg.suppress_output = true;
                }

                if json.initial_user_message.is_some() && agg.initial_user_message.is_none() {
                    agg.initial_user_message = json.initial_user_message.clone();
                }

                agg.watch_paths.extend(json.watch_paths.iter().cloned());

                if json.retry {
                    agg.retry = true;
                }

                if json.async_rewake {
                    agg.async_rewake = true;
                }

                // Process hookSpecificOutput (TS-style nested output).
                // Fields from hookSpecificOutput override flat-format fields.
                if let Some(specific) = &json.hook_specific_output {
                    apply_hook_specific_output(&mut agg, specific, &r.command);
                }
            }
            ParsedHookOutput::PlainText(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    agg.additional_contexts.push(trimmed.to_string());
                }
            }
        }
    }

    agg
}

/// Merge two permission behaviors, applying TS precedence: deny > ask > allow.
fn merge_permission(
    current: Option<PermissionBehavior>,
    new: PermissionBehavior,
) -> PermissionBehavior {
    match (current, new) {
        (_, PermissionBehavior::Deny) | (Some(PermissionBehavior::Deny), _) => {
            PermissionBehavior::Deny
        }
        (_, PermissionBehavior::Ask) | (Some(PermissionBehavior::Ask), _) => {
            PermissionBehavior::Ask
        }
        _ => PermissionBehavior::Allow,
    }
}

/// Apply event-specific output from `hookSpecificOutput` to the aggregated result.
///
/// TS: processHookJSONOutput() — switches on hookSpecificOutput.hookEventName.
fn apply_hook_specific_output(
    agg: &mut AggregatedHookResult,
    specific: &HookSpecificOutput,
    command: &str,
) {
    match specific {
        HookSpecificOutput::PreToolUse {
            permission_decision,
            permission_decision_reason,
            updated_input,
            additional_context,
        } => {
            if let Some(pd) = permission_decision {
                match pd.as_str() {
                    "allow" => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Allow,
                        ));
                    }
                    "deny" => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Deny,
                        ));
                        agg.blocking_error = Some(HookBlockingError {
                            blocking_error: permission_decision_reason
                                .clone()
                                .unwrap_or_else(|| "Blocked by hook".to_string()),
                            command: command.to_string(),
                        });
                    }
                    "ask" => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Ask,
                        ));
                    }
                    _ => {}
                }
            }
            if let Some(reason) = permission_decision_reason {
                agg.hook_permission_decision_reason = Some(reason.clone());
            }
            if updated_input.is_some() {
                agg.updated_input.clone_from(updated_input);
            }
            if let Some(ctx) = additional_context {
                agg.additional_contexts.push(ctx.clone());
            }
        }
        HookSpecificOutput::PostToolUse {
            additional_context,
            updated_mcp_tool_output,
        } => {
            if let Some(ctx) = additional_context {
                agg.additional_contexts.push(ctx.clone());
            }
            if updated_mcp_tool_output.is_some() {
                agg.updated_mcp_tool_output
                    .clone_from(updated_mcp_tool_output);
            }
        }
        HookSpecificOutput::PostToolUseFailure { additional_context }
        | HookSpecificOutput::UserPromptSubmit { additional_context }
        | HookSpecificOutput::Setup { additional_context }
        | HookSpecificOutput::SubagentStart { additional_context }
        | HookSpecificOutput::Notification { additional_context } => {
            if let Some(ctx) = additional_context {
                agg.additional_contexts.push(ctx.clone());
            }
        }
        HookSpecificOutput::SessionStart {
            additional_context,
            initial_user_message,
            watch_paths,
        } => {
            if let Some(ctx) = additional_context {
                agg.additional_contexts.push(ctx.clone());
            }
            if initial_user_message.is_some() && agg.initial_user_message.is_none() {
                agg.initial_user_message.clone_from(initial_user_message);
            }
            if let Some(paths) = watch_paths {
                agg.watch_paths.extend(paths.iter().cloned());
            }
        }
        HookSpecificOutput::PermissionDenied { retry } => {
            if *retry == Some(true) {
                agg.retry = true;
            }
        }
        HookSpecificOutput::PermissionRequest { decision } => {
            if let Some(d) = decision {
                match d {
                    PermissionRequestDecision::Allow { updated_input } => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Allow,
                        ));
                        if updated_input.is_some() {
                            agg.updated_input.clone_from(updated_input);
                        }
                        agg.permission_request_result = Some(d.clone());
                    }
                    PermissionRequestDecision::Deny { .. } => {
                        agg.permission_behavior = Some(merge_permission(
                            agg.permission_behavior,
                            PermissionBehavior::Deny,
                        ));
                        agg.permission_request_result = Some(d.clone());
                    }
                }
            }
        }
        HookSpecificOutput::Elicitation { action, content } => {
            if let Some(act) = action {
                if act == "decline" {
                    agg.blocking_error = Some(HookBlockingError {
                        blocking_error: "Elicitation denied by hook".to_string(),
                        command: command.to_string(),
                    });
                }
                agg.elicitation_response = Some(ElicitationResponse {
                    action: act.clone(),
                    content: content.clone(),
                });
            }
        }
        HookSpecificOutput::ElicitationResult { action, content } => {
            if let Some(act) = action {
                if act == "decline" {
                    agg.blocking_error = Some(HookBlockingError {
                        blocking_error: "Elicitation result blocked by hook".to_string(),
                        command: command.to_string(),
                    });
                }
                agg.elicitation_result_response = Some(ElicitationResponse {
                    action: act.clone(),
                    content: content.clone(),
                });
            }
        }
        HookSpecificOutput::CwdChanged { watch_paths }
        | HookSpecificOutput::FileChanged { watch_paths } => {
            if let Some(paths) = watch_paths {
                agg.watch_paths.extend(paths.iter().cloned());
            }
        }
        HookSpecificOutput::WorktreeCreate { .. } => {
            // WorktreeCreate-specific output is informational only.
        }
    }
}

// ---------------------------------------------------------------------------
// Event-specific orchestration functions
// ---------------------------------------------------------------------------

/// Context passed to all orchestration functions.
pub struct OrchestrationContext {
    pub session_id: String,
    pub cwd: PathBuf,
    pub project_dir: Option<PathBuf>,
    pub permission_mode: Option<String>,
    pub cancel: CancellationToken,
    /// When true, all hooks are disabled and execute_event returns immediately.
    pub disable_all_hooks: bool,
    /// When true, only managed (policy-level) hooks are allowed.
    pub allow_managed_hooks_only: bool,
}

/// Generic event execution — builds env, runs hooks in parallel, aggregates.
///
/// Use this for events that don't need special return types. The event-specific
/// functions below are convenience wrappers that build the correct input and
/// pass the appropriate `match_value` for each event type.
pub async fn execute_event(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    event: HookEventType,
    match_value: Option<&str>,
    input: &crate::inputs::HookInput,
    timeout: Duration,
) -> anyhow::Result<AggregatedHookResult> {
    if ctx.disable_all_hooks {
        return Ok(AggregatedHookResult::default());
    }
    let json_input = serde_json::to_string(input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        match_value,
        &format!("{event:?}"),
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        event,
        match_value,
        &json_input,
        &env,
        &ctx.cancel,
        timeout,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(aggregate_results(&results))
}

/// Execute PreToolUse hooks and return the aggregated result.
///
/// TS: executePreToolHooks()
pub async fn execute_pre_tool_use(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    tool_name: &str,
    tool_use_id: &str,
    tool_input: &serde_json::Value,
    event_tx: Option<&tokio::sync::mpsc::Sender<crate::HookExecutionEvent>>,
) -> anyhow::Result<AggregatedHookResult> {
    let input = PreToolUseInput {
        base: base_from_ctx(ctx),
        hook_event_name: "PreToolUse".to_string(),
        tool_name: tool_name.to_string(),
        tool_input: tool_input.clone(),
        tool_use_id: tool_use_id.to_string(),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        Some(tool_name),
        "PreToolUse",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::PreToolUse,
        Some(tool_name),
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        event_tx,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(aggregate_results(&results))
}

/// Execute PostToolUse hooks and return the aggregated result.
///
/// TS: executePostToolHooks()
pub async fn execute_post_tool_use(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    tool_name: &str,
    tool_use_id: &str,
    tool_input: &serde_json::Value,
    tool_response: &serde_json::Value,
    event_tx: Option<&tokio::sync::mpsc::Sender<crate::HookExecutionEvent>>,
) -> anyhow::Result<AggregatedHookResult> {
    let input = PostToolUseInput {
        base: base_from_ctx(ctx),
        hook_event_name: "PostToolUse".to_string(),
        tool_name: tool_name.to_string(),
        tool_input: tool_input.clone(),
        tool_response: tool_response.clone(),
        tool_use_id: tool_use_id.to_string(),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        Some(tool_name),
        "PostToolUse",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::PostToolUse,
        Some(tool_name),
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        event_tx,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(aggregate_results(&results))
}

/// Execute PreCompact hooks and return custom instructions / display messages.
///
/// TS: executePreCompactHooks() — returns {newCustomInstructions, userDisplayMessage}.
pub async fn execute_pre_compact(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    trigger: &str,
    custom_instructions: Option<&str>,
) -> anyhow::Result<PreCompactResult> {
    let input = CompactHookInput {
        base: base_from_ctx(ctx),
        hook_event_name: "PreCompact".to_string(),
        trigger: trigger.to_string(),
        custom_instructions: custom_instructions.map(String::from),
        compact_summary: None,
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "PreCompact",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::PreCompact,
        None,
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    let mut new_instructions: Vec<String> = Vec::new();
    let mut display_messages: Vec<String> = Vec::new();

    for r in &results {
        let trimmed = r.output.trim();
        if r.succeeded {
            if !trimmed.is_empty() {
                new_instructions.push(trimmed.to_string());
                display_messages.push(format!(
                    "PreCompact [{}] completed successfully: {trimmed}",
                    r.command
                ));
            } else {
                display_messages.push(format!("PreCompact [{}] completed successfully", r.command));
            }
        } else if !trimmed.is_empty() {
            display_messages.push(format!("PreCompact [{}] failed: {trimmed}", r.command));
        } else {
            display_messages.push(format!("PreCompact [{}] failed", r.command));
        }
    }

    Ok(PreCompactResult {
        new_custom_instructions: if new_instructions.is_empty() {
            None
        } else {
            Some(new_instructions.join("\n\n"))
        },
        user_display_message: if display_messages.is_empty() {
            None
        } else {
            Some(display_messages.join("\n"))
        },
    })
}

/// Result of PreCompact hook execution.
#[derive(Debug, Clone, Default)]
pub struct PreCompactResult {
    pub new_custom_instructions: Option<String>,
    pub user_display_message: Option<String>,
}

/// Execute PostCompact hooks and return display messages.
///
/// TS: executePostCompactHooks() — returns {userDisplayMessage}.
pub async fn execute_post_compact(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    trigger: &str,
    compact_summary: &str,
) -> anyhow::Result<PostCompactResult> {
    let input = CompactHookInput {
        base: base_from_ctx(ctx),
        hook_event_name: "PostCompact".to_string(),
        trigger: trigger.to_string(),
        custom_instructions: None,
        compact_summary: Some(compact_summary.to_string()),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "PostCompact",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::PostCompact,
        None,
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    let mut display_messages: Vec<String> = Vec::new();
    for r in &results {
        let trimmed = r.output.trim();
        if r.succeeded {
            if !trimmed.is_empty() {
                display_messages.push(format!(
                    "PostCompact [{}] completed successfully: {trimmed}",
                    r.command
                ));
            } else {
                display_messages.push(format!(
                    "PostCompact [{}] completed successfully",
                    r.command
                ));
            }
        } else if !trimmed.is_empty() {
            display_messages.push(format!("PostCompact [{}] failed: {trimmed}", r.command));
        } else {
            display_messages.push(format!("PostCompact [{}] failed", r.command));
        }
    }

    Ok(PostCompactResult {
        user_display_message: if display_messages.is_empty() {
            None
        } else {
            Some(display_messages.join("\n"))
        },
    })
}

/// Result of PostCompact hook execution.
#[derive(Debug, Clone, Default)]
pub struct PostCompactResult {
    pub user_display_message: Option<String>,
}

/// Execute SessionStart hooks.
///
/// TS: executeSessionStartHooks()
pub async fn execute_session_start(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    source: &str,
    agent_type: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<AggregatedHookResult> {
    let input = SessionStartInput {
        base: base_from_ctx(ctx),
        hook_event_name: "SessionStart".to_string(),
        source: source.to_string(),
        agent_type: agent_type.map(String::from),
        model: model.map(String::from),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "SessionStart",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::SessionStart,
        Some(source),
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(aggregate_results(&results))
}

/// Execute SessionEnd hooks with a tighter timeout.
///
/// TS: executeSessionEndHooks() — uses SESSION_END_HOOK_TIMEOUT_MS_DEFAULT.
pub async fn execute_session_end(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    reason: &str,
) -> anyhow::Result<Vec<SingleHookResult>> {
    let timeout = session_end_timeout();
    let input = SessionEndInput {
        base: base_from_ctx(ctx),
        hook_event_name: "SessionEnd".to_string(),
        reason: reason.to_string(),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "SessionEnd",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::SessionEnd,
        Some(reason),
        &json_input,
        &env,
        &ctx.cancel,
        timeout,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(results)
}

/// Execute StopFailure hooks.
///
/// TS: executeStopFailureHooks()
/// Execute `Stop` hooks and return the aggregated result.
///
/// Stop hooks fire when a turn ends naturally (no tool calls, `end_turn` stop).
/// A blocking Stop hook's feedback is injected back into the conversation and
/// the loop continues — matching TS `query.ts` `handleStopHooks()` behavior.
///
/// TS: `services/tools/stopHooks.ts` + `handleStopHooks()` in query.ts.
pub async fn execute_stop(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    reason: Option<&str>,
    event_tx: Option<&tokio::sync::mpsc::Sender<crate::HookExecutionEvent>>,
) -> anyhow::Result<AggregatedHookResult> {
    if ctx.disable_all_hooks {
        return Ok(AggregatedHookResult::default());
    }
    let input = StopInput {
        base: base_from_ctx(ctx),
        hook_event_name: "Stop".to_string(),
        reason: reason.map(String::from),
    };
    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "Stop",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::Stop,
        None,
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        event_tx,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(aggregate_results(&results))
}

pub async fn execute_stop_failure(
    registry: &HookRegistry,
    ctx: &OrchestrationContext,
    error: &str,
    error_details: Option<&str>,
    last_assistant_message: Option<&str>,
) -> anyhow::Result<Vec<SingleHookResult>> {
    let input = StopFailureInput {
        base: base_from_ctx(ctx),
        hook_event_name: "StopFailure".to_string(),
        error: error.to_string(),
        error_details: error_details.map(String::from),
        last_assistant_message: last_assistant_message.map(String::from),
    };

    let json_input = serde_json::to_string(&input)?;
    let env = build_hook_env(
        &ctx.session_id,
        &ctx.cwd.to_string_lossy(),
        None,
        "StopFailure",
        ctx.project_dir.as_deref().and_then(|p| p.to_str()),
    );

    let results = execute_hooks_parallel_filtered(
        registry,
        HookEventType::StopFailure,
        Some(error),
        &json_input,
        &env,
        &ctx.cancel,
        DEFAULT_HOOK_TIMEOUT,
        /*event_tx*/ None,
        ctx.allow_managed_hooks_only,
    )
    .await;

    Ok(results)
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format a blocking error from a PreToolUse hook.
///
/// TS: getPreToolHookBlockingMessage()
pub fn format_pre_tool_blocking_message(hook_name: &str, error: &HookBlockingError) -> String {
    format!("{hook_name} hook error: {}", error.blocking_error)
}

/// Format a blocking error from a Stop hook.
///
/// TS: getStopHookMessage()
pub fn format_stop_hook_message(error: &HookBlockingError) -> String {
    format!("Stop hook feedback:\n{}", error.blocking_error)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

// base_from_ctx() is now in crate::inputs and re-exported above.

/// Resolve timeout for a hook handler — uses the handler's explicit timeout if set.
fn resolve_timeout(handler: &HookHandler, default: Duration) -> Duration {
    match handler {
        HookHandler::Command { timeout_ms, .. } => timeout_ms
            .and_then(|ms| u64::try_from(ms).ok())
            .map(Duration::from_millis)
            .unwrap_or(default),
        HookHandler::Http { timeout_ms, .. } => timeout_ms
            .and_then(|ms| u64::try_from(ms).ok())
            .map(Duration::from_millis)
            .unwrap_or(default),
        _ => default,
    }
}

/// Human-readable label for a hook handler (used in result reporting).
fn handler_label(handler: &HookHandler) -> String {
    match handler {
        HookHandler::Command { command, .. } => command.clone(),
        HookHandler::Prompt { prompt } => format!("prompt:{prompt}"),
        HookHandler::Http { url, .. } => url.clone(),
        HookHandler::Agent { agent_name, .. } => format!("agent:{agent_name}"),
    }
}

/// Process a raw `HookExecutionResult` into a `SingleHookResult`.
fn process_execution_result(exec: HookExecutionResult, label: &str) -> SingleHookResult {
    match exec {
        HookExecutionResult::CommandOutput {
            exit_code,
            stdout,
            stderr,
        } => {
            // Exit code 2 is the TS "blocking error" convention.
            let blocked = exit_code == 2;
            let output = if exit_code == 0 { stdout } else { stderr };
            SingleHookResult {
                command: label.to_string(),
                succeeded: exit_code == 0,
                output,
                blocked,
                outcome: match exit_code {
                    0 => HookOutcome::Success,
                    2 => HookOutcome::Blocking,
                    _ => HookOutcome::NonBlockingError,
                },
                status_message: None,
                async_rewake: false,
            }
        }
        HookExecutionResult::PromptText(text) => SingleHookResult {
            command: label.to_string(),
            succeeded: true,
            output: text,
            blocked: false,
            outcome: HookOutcome::Success,
            status_message: None,
            async_rewake: false,
        },
    }
}

/// Get the SessionEnd hook timeout, optionally overridden via env var.
///
/// TS: getSessionEndHookTimeoutMs()
fn session_end_timeout() -> Duration {
    env::env_opt(EnvKey::CocoSessionEndHooksTimeoutMs)
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|&ms| ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(SESSION_END_HOOK_TIMEOUT)
}

#[cfg(test)]
#[path = "orchestration.test.rs"]
mod tests;
