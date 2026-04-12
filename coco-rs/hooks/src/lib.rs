//! Hook system — pre/post event interception with scoped priority.
//!
//! TS: schemas/hooks.ts + utils/hooks/ (HookDefinition, HookExecutor, AsyncHookRegistry)

pub mod async_registry;
pub mod inputs;
pub mod orchestration;
pub mod ssrf;

use coco_types::HookEventType;
use coco_types::HookOutcome;
use coco_types::HookScope;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;

/// Hook definition — an event handler with matcher, command, and priority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    pub event: HookEventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub handler: HookHandler,
    /// Lower priority values execute first. Defaults to 0.
    #[serde(default)]
    pub priority: i32,
    /// Scope determines precedence: Session > Local > Project > User > Builtin.
    #[serde(default)]
    pub scope: HookScope,
    /// Permission-rule-syntax condition, e.g. `"Bash(git *)"`.
    /// When set, the hook only fires if the tool name and content match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    /// When true, the hook fires only once per session.
    #[serde(default)]
    pub once: bool,
    /// When true, the hook runs asynchronously (does not block the event).
    #[serde(default, rename = "async")]
    pub is_async: bool,
    /// When true, the hook runner should re-wake after async completion.
    #[serde(default)]
    pub async_rewake: bool,
    /// Override shell for Command hooks (e.g. "bash", "zsh").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Human-readable status message shown while the hook is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// How to handle a hook event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandler {
    /// Execute a shell command.
    Command {
        command: String,
        timeout_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
    },
    /// Inject prompt text into the conversation.
    Prompt { prompt: String },
    /// Execute an HTTP request (webhook).
    Http {
        url: String,
        method: Option<String>,
        headers: Option<HashMap<String, String>>,
        timeout_ms: Option<i64>,
    },
    /// Execute another agent as a hook.
    Agent {
        agent_name: String,
        prompt: Option<String>,
    },
}

/// Outcome of executing a single hook handler.
#[derive(Debug, Clone)]
pub enum HookExecutionResult {
    /// Command was executed, contains stdout.
    CommandOutput {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    /// Prompt text to inject into the conversation.
    PromptText(String),
}

/// Metadata returned alongside a hook execution result.
#[derive(Debug, Clone, Default)]
pub struct HookExecutionMeta {
    /// Human-readable status for progress display.
    pub status_message: Option<String>,
    /// When true, the hook runner should re-wake after async completion.
    pub async_rewake: bool,
}

/// Hook settings from config (deserialized from Settings.hooks Value).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksSettings {
    pub hooks: HashMap<String, Vec<HookDefinition>>,
}

/// Context for evaluating `if_condition` on hooks.
///
/// TS: `prepareIfConditionMatcher()` — builds a matcher from tool name + input.
#[derive(Debug, Clone)]
pub struct IfConditionContext {
    /// The tool being called (e.g. `"Bash"`).
    pub tool_name: String,
    /// The tool's primary content (e.g. the bash command string).
    pub tool_content: Option<String>,
}

/// Hook registry — manages hook definitions, matching, and execution.
#[derive(Default)]
pub struct HookRegistry {
    hooks: Vec<HookDefinition>,
    seen_keys: HashSet<String>,
    /// Tracks hooks with `once: true` that have already fired.
    /// Key is the dedup key of the hook definition.
    fired_once: HashSet<String>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, hook: HookDefinition) {
        self.hooks.push(hook);
    }

    /// Register a hook, skipping duplicates based on handler + if_condition + shell.
    ///
    /// Returns `true` if the hook was registered, `false` if it was a duplicate.
    pub fn register_deduped(&mut self, hook: HookDefinition) -> bool {
        let key = dedup_key(&hook);
        if self.seen_keys.contains(&key) {
            tracing::debug!("skipping duplicate hook: {key}");
            return false;
        }
        self.seen_keys.insert(key);
        self.hooks.push(hook);
        true
    }

    /// Find hooks matching an event type and optional match value.
    ///
    /// `match_value` is the event-specific field to match against:
    /// - PreToolUse / PostToolUse / PostToolUseFailure → tool_name
    /// - SessionStart / ConfigChange → source
    /// - SessionEnd → reason
    /// - Setup / PreCompact / PostCompact → trigger
    /// - SubagentStart / SubagentStop → agent_type
    /// - Notification → notification_type
    /// - Elicitation / ElicitationResult → mcp_server_name
    /// - FileChanged → basename of file_path
    /// - InstructionsLoaded → load_reason
    /// - StopFailure → error
    /// - All others → None
    ///
    /// A hook matches if:
    /// 1. Its event type matches the query event
    /// 2. Its matcher is `None` (matches everything), OR
    ///    the matcher is `"*"` (wildcard, requires match_value present), OR
    ///    the matcher matches via exact, pipe-separated, regex, or glob
    ///
    /// Results are sorted by scope (descending — Session first) then by
    /// priority within the same scope (ascending — lower values first).
    pub fn find_matching(
        &self,
        event: HookEventType,
        match_value: Option<&str>,
    ) -> Vec<&HookDefinition> {
        let mut matches: Vec<&HookDefinition> = self
            .hooks
            .iter()
            .filter(|h| {
                h.event == event
                    && matcher_matches(h.matcher.as_deref(), match_value)
                    && !(h.once && self.fired_once.contains(&dedup_key(h)))
            })
            .collect();

        matches.sort_by(|a, b| {
            // Higher scope first (descending), then lower priority first (ascending).
            b.scope
                .cmp(&a.scope)
                .then_with(|| a.priority.cmp(&b.priority))
        });
        matches
    }

    /// Find hooks matching an event + matcher + if_condition.
    ///
    /// Like `find_matching` but additionally filters hooks with `if_condition`
    /// against the provided tool call context.
    pub fn find_matching_with_if(
        &self,
        event: HookEventType,
        match_value: Option<&str>,
        if_ctx: Option<&IfConditionContext>,
    ) -> Vec<&HookDefinition> {
        let mut matches = self.find_matching(event, match_value);

        if let Some(ctx) = if_ctx {
            matches.retain(|h| {
                let Some(cond) = &h.if_condition else {
                    return true; // no condition → always passes
                };
                let rule = coco_types::parse_rule_pattern(cond);
                coco_types::matches_rule(&rule, &ctx.tool_name, ctx.tool_content.as_deref())
            });
        }

        matches
    }

    /// Backwards-compatible alias for `find_matching`.
    pub fn find(&self, event: HookEventType, match_value: Option<&str>) -> Vec<&HookDefinition> {
        self.find_matching(event, match_value)
    }

    /// Execute all matching hooks for an event, returning results in priority order.
    ///
    /// This is a convenience method that executes hooks without env injection
    /// or stdin input. For full orchestration with env vars, stdin, and
    /// parallel execution, use `orchestration::execute_hooks_parallel()`.
    pub async fn execute_hooks(
        &self,
        event: HookEventType,
        tool_name: Option<&str>,
    ) -> Vec<anyhow::Result<HookExecutionResult>> {
        let matching = self.find_matching(event, tool_name);
        let mut results = Vec::with_capacity(matching.len());
        let empty_env = HashMap::new();

        for hook in matching {
            results.push(execute_hook(&hook.handler, &empty_env, None).await);
        }

        results
    }

    /// Mark a `once` hook as fired (won't match again).
    pub fn mark_once_fired(&mut self, hook: &HookDefinition) {
        if hook.once {
            self.fired_once.insert(dedup_key(hook));
        }
    }

    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }
}

/// Check whether a hook matcher pattern matches a given value.
///
/// Matching cascade (TS parity — matchesPattern() in hooks.ts):
/// 1. `None` → matches everything
/// 2. `"*"` → wildcard, matches if value is present
/// 3. "Simple" pattern (only alphanumeric, `_`, `|`):
///    - Pipe-separated: `"Write|Edit"` → any exact match
///    - Otherwise: exact string match
/// 4. Regex pattern (anything with special chars)
/// 5. Glob pattern fallback (if regex is invalid)
///
/// TS uses `/^[a-zA-Z0-9_|]+$/` to distinguish simple from regex patterns.
fn matcher_matches(matcher: Option<&str>, value: Option<&str>) -> bool {
    match matcher {
        None => true,
        Some("*") => value.is_some(),
        Some(pattern) => match value {
            None => false,
            Some(name) => {
                // TS: /^[a-zA-Z0-9_|]+$/ — only alphanumeric, underscore, pipe
                let is_simple = pattern
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'|');

                if is_simple {
                    if pattern.contains('|') {
                        return pattern.split('|').any(|p| p.trim() == name);
                    }
                    return name == pattern;
                }

                // Otherwise treat as regex (TS: `new RegExp(matcher)`)
                match regex::Regex::new(pattern) {
                    Ok(re) => re.is_match(name),
                    Err(regex_err) => {
                        tracing::debug!("invalid regex in hook matcher: {pattern}: {regex_err}");
                        // Fallback to glob for non-regex special chars like `*`, `?`
                        match globset::Glob::new(pattern) {
                            Ok(glob) => glob.compile_matcher().is_match(name),
                            Err(glob_err) => {
                                tracing::debug!(
                                    "invalid glob in hook matcher: {pattern}: {glob_err}"
                                );
                                false
                            }
                        }
                    }
                }
            }
        },
    }
}

/// Prompt request from a hook during execution.
///
/// TS: promptRequestSchema — hooks can emit interactive prompts via stdout
/// and receive responses via stdin.
///
/// A hook outputs: `{"prompt": "request-id", "message": "question?", "options": [...]}`
/// The runner writes back: `{"prompt_response": "request-id", "selected": "key"}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    /// Request identifier.
    pub prompt: String,
    /// Question to display to the user.
    pub message: String,
    /// Available options.
    pub options: Vec<PromptOption>,
}

/// A single option in a hook prompt request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOption {
    /// Option key (returned in response).
    pub key: String,
    /// Display label.
    pub label: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response to a hook prompt request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    /// Request identifier (matches PromptRequest.prompt).
    pub prompt_response: String,
    /// Selected option key.
    pub selected: String,
}

/// Hook execution event — emitted during hook lifecycle.
///
/// TS: hookEvents.ts — HookStartedEvent, HookProgressEvent, HookResponseEvent.
#[derive(Debug, Clone)]
pub enum HookExecutionEvent {
    Started {
        hook_id: String,
        hook_name: String,
        hook_event: String,
    },
    Progress {
        hook_id: String,
        hook_name: String,
        stdout: String,
        stderr: String,
    },
    Response {
        hook_id: String,
        hook_name: String,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        outcome: HookOutcome,
    },
}

// HookOutcome is re-exported from coco_types::HookOutcome
// (4 variants: Success, Blocking, NonBlockingError, Cancelled)

/// Session hook event types.
///
/// TS: sessionHooks.ts — fired at session lifecycle boundaries.
pub const SESSION_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "Setup",
    "Stop",
    "StopFailure",
    "PreCompact",
    "PostCompact",
    "ModelSwitch",
    "ContextOverflow",
    "QueryStart",
];

/// Check if a hook event is a session-level event.
pub fn is_session_hook_event(event: &str) -> bool {
    SESSION_HOOK_EVENTS.contains(&event)
}

/// Execute a single hook handler.
///
/// `env_vars` are injected into the process environment for command hooks.
/// `stdin_input` is written to stdin for command hooks and sent as the request
/// body for HTTP hooks.
pub async fn execute_hook(
    handler: &HookHandler,
    env_vars: &HashMap<String, String>,
    stdin_input: Option<&str>,
) -> anyhow::Result<HookExecutionResult> {
    match handler {
        HookHandler::Command {
            command,
            timeout_ms,
            shell,
        } => {
            let shell_bin = shell.as_deref().unwrap_or("sh");

            // Shell prefix support (TS: CLAUDE_CODE_SHELL_PREFIX).
            let final_command = match std::env::var("CLAUDE_CODE_SHELL_PREFIX") {
                Ok(prefix) if !prefix.is_empty() => format!("{prefix} {command}"),
                _ => command.clone(),
            };

            let mut cmd = tokio::process::Command::new(shell_bin);
            cmd.arg("-c").arg(&final_command);
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            cmd.envs(env_vars);

            // CWD safety — validate cwd exists before spawn (TS parity).
            if let Ok(cwd) = std::env::current_dir()
                && cwd.exists()
            {
                cmd.current_dir(&cwd);
            }

            let timeout = std::time::Duration::from_millis(
                timeout_ms
                    .and_then(|ms| u64::try_from(ms).ok())
                    .unwrap_or(30_000),
            );

            let output = if let Some(input) = stdin_input {
                cmd.stdin(std::process::Stdio::piped());
                let mut child = cmd
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("failed to spawn hook command: {e}"))?;

                // Write stdin input and close the pipe.
                if let Some(mut stdin_handle) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = stdin_handle.write_all(input.as_bytes()).await {
                        // EPIPE is expected if hook exits before reading stdin.
                        if e.kind() != std::io::ErrorKind::BrokenPipe {
                            tracing::debug!("failed to write hook stdin: {e}");
                        }
                    }
                    let _ = stdin_handle.shutdown().await;
                }

                tokio::time::timeout(timeout, child.wait_with_output())
                    .await
                    .map_err(|_| anyhow::anyhow!("hook command timed out after {timeout_ms:?}ms"))?
                    .map_err(|e| anyhow::anyhow!("hook command failed: {e}"))?
            } else {
                let child = cmd
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("failed to spawn hook command: {e}"))?;

                tokio::time::timeout(timeout, child.wait_with_output())
                    .await
                    .map_err(|_| anyhow::anyhow!("hook command timed out after {timeout_ms:?}ms"))?
                    .map_err(|e| anyhow::anyhow!("hook command failed: {e}"))?
            };

            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !output.status.success() {
                tracing::warn!("hook command exited with code {exit_code}: {command}");
            }

            Ok(HookExecutionResult::CommandOutput {
                exit_code,
                stdout,
                stderr,
            })
        }
        HookHandler::Prompt { prompt } => Ok(HookExecutionResult::PromptText(prompt.clone())),
        HookHandler::Http {
            url,
            method,
            headers,
            timeout_ms,
        } => {
            // SSRF guard — block private/link-local addresses (TS parity).
            match crate::ssrf::check_url_ssrf(url).await {
                Ok(true) => {
                    return Err(anyhow::anyhow!(
                        "HTTP hook blocked: URL resolves to a private/link-local address: {url}"
                    ));
                }
                Err(e) => {
                    tracing::debug!("SSRF check failed for {url}: {e}");
                    // Allow the request to proceed if DNS resolution fails
                    // (the actual HTTP request will fail anyway).
                }
                Ok(false) => {} // allowed
            }

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(
                    timeout_ms
                        .and_then(|ms| u64::try_from(ms).ok())
                        .unwrap_or(10_000),
                ))
                .build()
                .map_err(|e| anyhow::anyhow!("failed to create HTTP client: {e}"))?;

            let method_str = method.as_deref().unwrap_or("POST");
            let req_method =
                reqwest::Method::from_bytes(method_str.as_bytes()).unwrap_or(reqwest::Method::POST);

            let mut req = client.request(req_method, url);
            if let Some(hdrs) = headers {
                for (k, v) in hdrs {
                    // Interpolate env vars ($VAR / ${VAR}) and sanitize CRLF.
                    let interpolated = interpolate_env_vars(v, env_vars);
                    req = req.header(k, sanitize_header_value(&interpolated));
                }
            }
            // Send the hook input JSON as the request body.
            if let Some(body) = stdin_input {
                req = req
                    .header("Content-Type", "application/json")
                    .body(body.to_string());
            }

            let resp = req
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("HTTP hook request failed: {e}"))?;

            let status = resp.status().as_u16() as i32;
            let body = resp.text().await.unwrap_or_default();

            Ok(HookExecutionResult::CommandOutput {
                exit_code: if (200..300).contains(&(status as u16)) {
                    0
                } else {
                    status
                },
                stdout: body,
                stderr: String::new(),
            })
        }
        HookHandler::Agent { agent_name, prompt } => {
            // Agent hooks are resolved by the caller (requires agent infrastructure).
            // Return the prompt text for the caller to handle.
            let text = prompt.as_deref().unwrap_or("(agent hook)");
            Ok(HookExecutionResult::PromptText(format!(
                "[Agent hook: {agent_name}] {text}"
            )))
        }
    }
}

/// Load hooks from a config JSON value.
///
/// The expected JSON format is:
/// ```json
/// {
///   "pre_tool_use": [{ "type": "command", "command": "echo hi", "matcher": "Bash" }],
///   "session_start": [{ "type": "prompt", "prompt": "hello" }]
/// }
/// ```
///
/// Event type keys use the snake_case serde format (e.g. `"pre_tool_use"`).
pub fn load_hooks_from_config(
    hooks_value: &serde_json::Value,
    scope: HookScope,
) -> anyhow::Result<Vec<HookDefinition>> {
    let obj = hooks_value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("hooks config must be a JSON object"))?;

    let mut definitions = Vec::new();

    for (event_key, hook_array) in obj {
        let event: HookEventType =
            serde_json::from_value(serde_json::Value::String(event_key.clone()))
                .map_err(|e| anyhow::anyhow!("unknown hook event type '{event_key}': {e}"))?;

        let hooks = hook_array
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("hooks for '{event_key}' must be an array"))?;

        for raw in hooks {
            let raw_obj = raw
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("hook definition must be a JSON object"))?;

            let handler = parse_hook_handler(raw_obj)?;
            let matcher = raw_obj.get("matcher").and_then(|v| {
                v.as_str().map(String::from).or_else(|| {
                    v.get("tool_name")
                        .and_then(|t| t.as_str())
                        .map(String::from)
                })
            });

            let if_condition = raw_obj.get("if").and_then(|v| v.as_str()).map(String::from);

            let timeout_secs = raw_obj.get("timeout").and_then(serde_json::Value::as_i64);
            let priority = raw_obj
                .get("priority")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0) as i32;

            let once = raw_obj
                .get("once")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let is_async = raw_obj
                .get("async")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let async_rewake = raw_obj
                .get("async_rewake")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let shell = raw_obj
                .get("shell")
                .and_then(|v| v.as_str())
                .map(String::from);
            let status_message = raw_obj
                .get("status_message")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Apply top-level timeout (seconds -> ms) to the handler if no handler-level timeout.
            let handler = apply_timeout_to_handler(handler, timeout_secs);

            definitions.push(HookDefinition {
                event,
                matcher,
                handler,
                priority,
                scope,
                if_condition,
                once,
                is_async,
                async_rewake,
                shell,
                status_message,
            });
        }
    }

    Ok(definitions)
}

/// Parse a hook handler from a raw JSON object.
fn parse_hook_handler(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<HookHandler> {
    let hook_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("command");

    match hook_type {
        "command" => {
            let command = obj
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("command hook requires a 'command' field"))?
                .to_string();
            let timeout_ms = obj.get("timeout_ms").and_then(serde_json::Value::as_i64);
            let shell = obj.get("shell").and_then(|v| v.as_str()).map(String::from);
            Ok(HookHandler::Command {
                command,
                timeout_ms,
                shell,
            })
        }
        "prompt" => {
            let prompt = obj
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("prompt hook requires a 'prompt' field"))?
                .to_string();
            Ok(HookHandler::Prompt { prompt })
        }
        "webhook" | "http" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("http hook requires a 'url' field"))?
                .to_string();
            let method = obj.get("method").and_then(|v| v.as_str()).map(String::from);
            let headers = obj.get("headers").and_then(|v| {
                v.as_object().map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
            });
            let timeout_ms = obj.get("timeout_ms").and_then(serde_json::Value::as_i64);
            Ok(HookHandler::Http {
                url,
                method,
                headers,
                timeout_ms,
            })
        }
        "agent" => {
            let agent_name = obj
                .get("agent_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("agent hook requires an 'agent_name' field"))?
                .to_string();
            let prompt = obj.get("prompt").and_then(|v| v.as_str()).map(String::from);
            Ok(HookHandler::Agent { agent_name, prompt })
        }
        other => Err(anyhow::anyhow!("unknown hook handler type: '{other}'")),
    }
}

/// Apply a top-level timeout (in seconds) to a handler that lacks its own timeout_ms.
fn apply_timeout_to_handler(handler: HookHandler, timeout_secs: Option<i64>) -> HookHandler {
    let Some(secs) = timeout_secs else {
        return handler;
    };
    let ms = secs * 1000;
    match handler {
        HookHandler::Command {
            command,
            timeout_ms: None,
            shell,
        } => HookHandler::Command {
            command,
            timeout_ms: Some(ms),
            shell,
        },
        HookHandler::Http {
            url,
            method,
            headers,
            timeout_ms: None,
        } => HookHandler::Http {
            url,
            method,
            headers,
            timeout_ms: Some(ms),
        },
        other => other,
    }
}

/// Sanitize an HTTP header value by stripping CR, LF, and NUL bytes.
///
/// TS: sanitizeHeaderValue() — prevents CRLF injection via header values.
fn sanitize_header_value(value: &str) -> String {
    value.replace(['\r', '\n', '\0'], "")
}

/// Interpolate `$VAR_NAME` and `${VAR_NAME}` patterns in a string.
///
/// TS: interpolateEnvVars() — looks up env vars from the hook env context
/// and from `std::env`. Only env vars present in the hook's env_vars map
/// or in the process environment are resolved.
fn interpolate_env_vars(value: &str, env_vars: &HashMap<String, String>) -> String {
    let Ok(re) = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}|\$([A-Z_][A-Z0-9_]*)") else {
        // Pattern is a compile-time constant — should never fail.
        return value.to_string();
    };
    re.replace_all(value, |caps: &regex::Captures<'_>| {
        let var_name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        // Check hook env vars first, then process env.
        env_vars
            .get(var_name)
            .cloned()
            .or_else(|| std::env::var(var_name).ok())
            .unwrap_or_default()
    })
    .to_string()
}

/// Compute a deduplication key for a hook definition.
///
/// Hooks with the same dedup key are considered duplicates. The key combines
/// the handler identity, if_condition, and shell override.
fn dedup_key(hook: &HookDefinition) -> String {
    let handler_key = match &hook.handler {
        HookHandler::Command { command, shell, .. } => format!("cmd:{shell:?}\0{command}"),
        HookHandler::Prompt { prompt } => format!("prompt:{prompt}"),
        HookHandler::Http { url, .. } => format!("http:{url}"),
        HookHandler::Agent { agent_name, .. } => format!("agent:{agent_name}"),
    };
    format!(
        "{handler_key}\0{}",
        hook.if_condition.as_deref().unwrap_or("")
    )
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
