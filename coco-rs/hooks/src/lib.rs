//! Hook system — pre/post event interception with scoped priority.
//!
//! TS: schemas/hooks.ts + utils/hooks/ (HookDefinition, HookExecutor, AsyncHookRegistry)

pub mod async_registry;
mod error;
pub mod function_hook;
pub mod inputs;
pub mod llm_handle;
pub mod orchestration;
pub mod reminder_source;
pub mod ssrf;
pub mod sync_hook_buffer;

pub use error::HooksError;
pub use error::Result;
pub use function_hook::FUNCTION_HOOK_SUPPORTED_EVENTS;
pub use function_hook::FunctionHook;
pub use function_hook::FunctionHookPredicate;
pub use function_hook::RegisterFunctionHookError;
pub use llm_handle::HookEvaluationResult;
pub use llm_handle::HookLlmHandle;
pub use sync_hook_buffer::SyncHookEventBuffer;

use coco_config::EnvKey;
use coco_config::env;
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
    /// Human-readable status message shown while the hook is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// How to handle a hook event.
///
/// TS schema: `src/schemas/hooks.ts` — discriminated union on `type`
/// (`command` / `prompt` / `http` / `agent`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandler {
    /// Execute a shell command.
    Command {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
    },
    /// Evaluate the prompt with an LLM and parse `{ok, reason?}` JSON.
    /// TS: `execPromptHook.ts`. Returns blocking when `ok=false`.
    Prompt {
        prompt: String,
        /// Model to use (e.g. `"claude-sonnet-4-6"`). When `None` the
        /// runner falls back to the small/fast model.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Timeout in milliseconds for this prompt evaluation. TS:
        /// `PromptHookSchema.timeout` (seconds). The loader converts
        /// the top-level `timeout` (sec) to ms when set here is None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<i64>,
    },
    /// POST hook input JSON to a URL (TS `execHttpHook.ts`). Method is
    /// always POST — TS schema has no method field.
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<i64>,
        /// Per-hook env-var allowlist. Only names present here are
        /// expanded in `headers` values; all other `$VAR` references
        /// resolve to the empty string. Required for env-var
        /// interpolation to do anything (parity with TS:
        /// `execHttpHook.ts:89-108`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_env_vars: Vec<String>,
    },
    /// Spawn an agentic verifier to evaluate `prompt` and return
    /// structured `{ok, reason?}`. TS: `execAgentHook.ts`.
    Agent {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Timeout in milliseconds for the agent run. TS:
        /// `AgentHookSchema.timeout` (seconds, default 60).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<i64>,
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
///
/// All mutable state is held behind interior `RwLock`s so callers
/// holding `Arc<HookRegistry>` can register and reload without
/// rebuilding the Arc — required because the registry is shared across
/// 8+ consumers (engine, hook controllers, file/elicitation watchers).
#[derive(Default)]
pub struct HookRegistry {
    hooks: std::sync::RwLock<Vec<HookDefinition>>,
    seen_keys: std::sync::RwLock<HashSet<String>>,
    /// Tracks hooks with `once: true` that have already fired.
    /// Key is the dedup key of the hook definition.
    /// Intentionally preserved across `reload_from_runtime` so a
    /// `once` hook that already fired doesn't re-fire after reload.
    fired_once: std::sync::RwLock<HashSet<String>>,
    /// Per-agent overlay registries — populated by
    /// `register_for_agent(agent_id, hooks)` at SubagentStart and
    /// cleared by `clear_agent_scope(agent_id)` at SubagentStop.
    /// `find_matching_for_agent(event, match_value, agent_id)`
    /// merges these with `hooks` for the duration of the spawn.
    /// TS parity: `registerFrontmatterHooks(setAppState, agentId, ...)`
    /// + `clearSessionHooks(setAppState, agentId)`.
    agent_scoped: std::sync::RwLock<std::collections::HashMap<String, Vec<HookDefinition>>>,
    /// In-memory function hooks (`type: 'function'` in TS). Registered
    /// at session bootstrap via [`Self::register_function_hook`] and
    /// dispatched by [`orchestration::execute_stop`] (and other events
    /// that thread message history) in parallel with settings hooks.
    ///
    /// Stored separately from [`Self::hooks`] because [`FunctionHook`]
    /// carries an `Arc<dyn FunctionHookPredicate>` which cannot be
    /// `Serialize` / `Deserialize`'d — keeping them apart preserves
    /// the settings-hook round-trip invariant.
    ///
    /// TS source: `AppState.sessionHooks` (`utils/hooks/sessionHooks.ts`).
    function_hooks: std::sync::RwLock<Vec<FunctionHook>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, hook: HookDefinition) {
        if let Ok(mut hooks) = self.hooks.write() {
            hooks.push(hook);
        }
    }

    /// Register a hook, skipping duplicates based on handler + if_condition + shell.
    ///
    /// Returns `true` if the hook was registered, `false` if it was a duplicate.
    pub fn register_deduped(&self, hook: HookDefinition) -> bool {
        let key = dedup_key(&hook);
        let mut keys = match self.seen_keys.write() {
            Ok(k) => k,
            Err(_) => return false,
        };
        if keys.contains(&key) {
            tracing::debug!("skipping duplicate hook: {key}");
            return false;
        }
        keys.insert(key);
        if let Ok(mut hooks) = self.hooks.write() {
            hooks.push(hook);
        }
        true
    }

    /// Reload session-scoped hooks from a fresh `RuntimeConfig` snapshot.
    ///
    /// Replaces the [`hooks`](Self) and `seen_keys` sets atomically;
    /// preserves `fired_once` (a `once` hook that already fired
    /// shouldn't re-fire after reload) and `agent_scoped` (per-spawn
    /// overlay owned by the coordinator).
    ///
    /// **Must be called at a turn boundary.** Calling mid-turn would
    /// risk `PreToolUse` and `PostToolUse` for the same call seeing
    /// different hook sets — TS deliberately gates this through
    /// `updateHooksConfigSnapshot()` only via `/hooks` UI, never
    /// auto-reload. In the Rust port the dispatch loop in
    /// `tui_runner` serialises turns via `drain_active_turn`, so
    /// slash-command handlers (which call this) only ever run between
    /// turns.
    ///
    /// `sources` is the ordered list of `(scope, raw_settings_value)`
    /// the caller wants registered. Stable order: lowest precedence
    /// first so the registry's iteration order matches the previous
    /// build (User → Project → Local → Flag → Policy → Plugin).
    pub fn reload_from_runtime(
        &self,
        sources: &[(coco_types::HookScope, serde_json::Value)],
        policy: LoaderPolicy,
    ) -> crate::Result<usize> {
        let mut new_hooks: Vec<HookDefinition> = Vec::new();
        let mut new_keys: HashSet<String> = HashSet::new();

        for (scope, value) in sources {
            let definitions = load_hooks_from_config_with_policy(value, *scope, policy)?;
            for def in definitions {
                let key = dedup_key(&def);
                if new_keys.insert(key) {
                    new_hooks.push(def);
                }
            }
        }

        let count = new_hooks.len();
        // Atomic swap (drop the old guards before returning).
        if let Ok(mut hooks) = self.hooks.write() {
            *hooks = new_hooks;
        }
        if let Ok(mut keys) = self.seen_keys.write() {
            *keys = new_keys;
        }
        Ok(count)
    }

    /// Register hooks scoped to a specific spawned agent. Replaces
    /// any existing entry for `agent_id`. Use `&self` (interior
    /// mutability) so callers holding `Arc<HookRegistry>` can register
    /// without re-building the Arc. TS parity:
    /// `registerFrontmatterHooks(setAppState, agentId, definition.hooks)`.
    ///
    /// `is_agent`: when `true`, any `Stop`-event hook is rewritten to
    /// `SubagentStop` because subagent termination fires `SubagentStop`,
    /// not `Stop` (TS `registerFrontmatterHooks.ts:38-45`). Skill
    /// frontmatter passes `false`; agent frontmatter passes `true`.
    pub fn register_for_agent(&self, agent_id: String, hooks: Vec<HookDefinition>, is_agent: bool) {
        let rewritten: Vec<HookDefinition> = if is_agent {
            hooks
                .into_iter()
                .map(|mut h| {
                    if h.event == HookEventType::Stop {
                        tracing::debug!(
                            %agent_id,
                            "rewriting frontmatter Stop hook to SubagentStop"
                        );
                        h.event = HookEventType::SubagentStop;
                    }
                    h
                })
                .collect()
        } else {
            hooks
        };
        if let Ok(mut map) = self.agent_scoped.write() {
            map.insert(agent_id, rewritten);
        }
    }

    /// Remove all hooks scoped to `agent_id`. Called at SubagentStop
    /// so the spawn's frontmatter hooks don't leak across spawns.
    /// TS parity: `clearSessionHooks(setAppState, agentId)`.
    pub fn clear_agent_scope(&self, agent_id: &str) {
        if let Ok(mut map) = self.agent_scoped.write() {
            map.remove(agent_id);
        }
    }

    /// Register an in-memory function hook.
    ///
    /// `id` is caller-supplied for stable removal — typically a
    /// `uuid::Uuid::new_v4().to_string()` for one-shot registrations
    /// or a stable token when the caller wants to update the hook by
    /// re-registration. **Duplicate ids are rejected**: re-registering
    /// the same id is a programmer error (silent duplicate would be
    /// matched twice by lookup + nuked together by removal).
    ///
    /// `event` MUST be one of
    /// [`FUNCTION_HOOK_SUPPORTED_EVENTS`](crate::FUNCTION_HOOK_SUPPORTED_EVENTS).
    /// Unsupported events are rejected with
    /// [`RegisterFunctionHookError::UnsupportedEvent`] because the
    /// dispatch path for those events doesn't thread message history,
    /// so the hook would persist but never fire.
    ///
    /// Returns the hook's id on success (same as the supplied id —
    /// the return value exists for chaining and for parity with TS
    /// `addFunctionHook(...).id`).
    ///
    /// TS parity: `addFunctionHook(setAppState, sessionId, event,
    /// matcher, callback, errorMessage, options)` in
    /// `utils/hooks/sessionHooks.ts:93`.
    pub fn register_function_hook(
        &self,
        id: impl Into<String>,
        event: HookEventType,
        matcher: Option<String>,
        timeout: std::time::Duration,
        predicate: std::sync::Arc<dyn FunctionHookPredicate>,
        error_message: impl Into<String>,
    ) -> std::result::Result<String, RegisterFunctionHookError> {
        if !FUNCTION_HOOK_SUPPORTED_EVENTS.contains(&event) {
            return Err(RegisterFunctionHookError::UnsupportedEvent(event));
        }
        let id = id.into();
        if let Ok(hooks) = self.function_hooks.read()
            && hooks.iter().any(|h| h.id == id)
        {
            return Err(RegisterFunctionHookError::DuplicateId(id));
        }
        let hook = FunctionHook {
            id: id.clone(),
            event,
            matcher,
            timeout,
            predicate,
            error_message: error_message.into(),
        };
        if let Ok(mut hooks) = self.function_hooks.write() {
            // Double-check under the write lock to close the
            // read-then-write TOCTOU window.
            if hooks.iter().any(|h| h.id == id) {
                return Err(RegisterFunctionHookError::DuplicateId(id));
            }
            hooks.push(hook);
        }
        Ok(id)
    }

    /// Remove a previously-registered function hook by `id`. Returns
    /// `true` when a hook was found and removed.
    ///
    /// TS parity: `removeFunctionHook(setAppState, sessionId, event,
    /// hookId)` in `utils/hooks/sessionHooks.ts:120`. The TS API
    /// requires `event` because TS stores hooks in a nested map keyed
    /// by event; coco-rs flattens them, so the id alone is enough.
    pub fn remove_function_hook(&self, id: &str) -> bool {
        if let Ok(mut hooks) = self.function_hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| h.id != id);
            hooks.len() < before
        } else {
            false
        }
    }

    /// Snapshot every function hook whose `event` matches and whose
    /// `matcher` (if set) matches the supplied `match_value`. Returns
    /// owned clones so the caller can drop the read lock immediately.
    ///
    /// Matcher semantics mirror [`Self::find_matching`]:
    ///   - `matcher == None` matches any value
    ///   - non-empty matcher runs the regex/glob fallback shared with
    ///     settings hooks (see [`matcher_matches`]).
    pub fn find_matching_function_hooks(
        &self,
        event: HookEventType,
        match_value: Option<&str>,
    ) -> Vec<FunctionHook> {
        let Ok(hooks) = self.function_hooks.read() else {
            return Vec::new();
        };
        hooks
            .iter()
            .filter(|h| h.event == event)
            .filter(|h| matcher_matches(h.matcher.as_deref(), match_value))
            .cloned()
            .collect()
    }

    /// Total registered function hooks. Mainly for telemetry / tests.
    pub fn function_hook_count(&self) -> usize {
        self.function_hooks.read().map(|v| v.len()).unwrap_or(0)
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
    ) -> Vec<HookDefinition> {
        let fired_once_snapshot: HashSet<String> = self
            .fired_once
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        let mut matches: Vec<HookDefinition> = match self.hooks.read() {
            Ok(hooks) => hooks
                .iter()
                .filter(|h| {
                    h.event == event
                        && matcher_matches(h.matcher.as_deref(), match_value)
                        && !(h.once && fired_once_snapshot.contains(&dedup_key(h)))
                })
                .cloned()
                .collect(),
            Err(_) => Vec::new(),
        };

        // Merge in agent-scoped hooks. TS parity:
        // `registerFrontmatterHooks` adds the agent's hooks to the
        // shared session-state hooks list; they're visible to every
        // event firing until `clearSessionHooks(agentId)` removes
        // them at SubagentStop. We mirror that by flattening every
        // bucket into the match list — identity is by agent_id key,
        // not by hook instance.
        if let Ok(agent_scoped) = self.agent_scoped.read() {
            for hooks in agent_scoped.values() {
                for h in hooks {
                    if h.event == event && matcher_matches(h.matcher.as_deref(), match_value) {
                        matches.push(h.clone());
                    }
                }
            }
        }

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
    ) -> Vec<HookDefinition> {
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
    pub fn find(&self, event: HookEventType, match_value: Option<&str>) -> Vec<HookDefinition> {
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
    ) -> Vec<crate::Result<HookExecutionResult>> {
        let matching = self.find_matching(event, tool_name);
        let mut results = Vec::with_capacity(matching.len());
        let empty_env = HashMap::new();

        for hook in matching {
            results.push(execute_hook(&hook.handler, &empty_env, None).await);
        }

        results
    }

    /// Mark a `once` hook as fired (won't match again).
    pub fn mark_once_fired(&self, hook: &HookDefinition) {
        if hook.once
            && let Ok(mut fired) = self.fired_once.write()
        {
            fired.insert(dedup_key(hook));
        }
    }

    pub fn len(&self) -> usize {
        self.hooks.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.read().map(|g| g.is_empty()).unwrap_or(true)
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
            Some(raw_name) => {
                // TS parity: `utils/hooks.ts:matchesPattern` normalizes
                // the incoming match value AND every alternate in a
                // pipe pattern through `normalizeLegacyToolName`, then
                // additionally tries every legacy alias for the value
                // when the regex path is taken (`getLegacyToolNames`).
                let name = coco_types::normalize_legacy_tool_name(raw_name);

                // TS: /^[a-zA-Z0-9_|]+$/ — only alphanumeric, underscore, pipe
                let is_simple = pattern
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'|');

                if is_simple {
                    if pattern.contains('|') {
                        return pattern
                            .split('|')
                            .any(|p| coco_types::normalize_legacy_tool_name(p.trim()) == name);
                    }
                    return name == coco_types::normalize_legacy_tool_name(pattern);
                }

                // Otherwise treat as regex (TS: `new RegExp(matcher)`)
                match regex::Regex::new(pattern) {
                    Ok(re) => {
                        if re.is_match(name) {
                            return true;
                        }
                        // TS also tests every legacy alias for the value
                        // so `^Task$` survives the rename to `Agent`.
                        coco_types::legacy_tool_name_aliases_of(name)
                            .iter()
                            .any(|legacy| re.is_match(legacy))
                    }
                    Err(regex_err) => {
                        tracing::debug!("invalid regex in hook matcher: {pattern}: {regex_err}");
                        // Fallback to glob for non-regex special chars like `*`, `?`
                        match globset::Glob::new(pattern) {
                            Ok(glob) => {
                                let matcher = glob.compile_matcher();
                                if matcher.is_match(name) {
                                    return true;
                                }
                                coco_types::legacy_tool_name_aliases_of(name)
                                    .iter()
                                    .any(|legacy| matcher.is_match(legacy))
                            }
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

/// Shell flavour selected by `HookHandler::Command::shell`.
///
/// TS: `bash` (default) and `powershell` are the only accepted values
/// per `schemas/hooks.ts:BashCommandHookSchema.shell`. Unknown values
/// fall back to bash with a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    Bash,
    PowerShell,
}

impl ShellKind {
    fn from_field(field: Option<&str>) -> Self {
        match field {
            Some("powershell") | Some("pwsh") => Self::PowerShell,
            Some("bash") | Some("sh") | None => Self::Bash,
            Some(other) => {
                tracing::warn!("unknown hook shell {other:?}, falling back to bash");
                Self::Bash
            }
        }
    }
}

/// Apply Windows-bash autoprefix: `command-with-foo.sh ...` → `bash command-with-foo.sh ...`.
///
/// TS: `utils/hooks.ts:860-862`. On Windows, naked `.sh` script invocations
/// would open in the OS file handler instead of executing — Git Bash needs
/// the `bash ` prefix to actually run them.
#[allow(dead_code)]
fn maybe_apply_sh_prefix(cmd: &str, shell_kind: ShellKind) -> String {
    #[cfg(target_os = "windows")]
    {
        if !matches!(shell_kind, ShellKind::Bash) {
            return cmd.to_string();
        }
        let trimmed = cmd.trim_start();
        if trimmed.starts_with("bash ") {
            return cmd.to_string();
        }
        // Match `.sh` followed by whitespace, end-of-string, or `"`.
        let bytes = trimmed.as_bytes();
        let mut idx = 0;
        while idx + 3 <= bytes.len() {
            if bytes[idx..].starts_with(b".sh") {
                let after = bytes.get(idx + 3).copied();
                let is_boundary = matches!(after, None | Some(b' ') | Some(b'\t') | Some(b'"'));
                if is_boundary {
                    return format!("bash {cmd}");
                }
            }
            idx += 1;
        }
        cmd.to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = shell_kind;
        cmd.to_string()
    }
}

/// Apply `creation_flags(CREATE_NO_WINDOW)` on Windows to suppress the
/// console flash that would otherwise appear for every spawned hook.
///
/// TS: `windowsHide: true` option on every `child_process.spawn` call
/// (`utils/hooks.ts:967, 981`).
#[allow(dead_code)]
fn apply_windows_hide(cmd: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

/// Build the spawnable `tokio::process::Command` for a Command hook.
///
/// Branches on `ShellKind`:
/// - `PowerShell`: `pwsh -NoProfile -NonInteractive -Command <cmd>`
///   (cross-platform — pwsh runs on Linux/macOS too).
/// - `Bash` on Windows: Git Bash with `-c <cmd>`.
/// - `Bash` elsewhere: `/bin/sh -c <cmd>`.
async fn build_command_for_shell(
    final_command: &str,
    shell_kind: ShellKind,
) -> crate::Result<tokio::process::Command> {
    match shell_kind {
        ShellKind::PowerShell => {
            let pwsh = coco_shell_discovery::cached_powershell_path()
                .await
                .ok_or_else(|| {
                    crate::error::HooksError::generic(
                        "Hook has shell: 'powershell' but no PowerShell executable \
                         (pwsh or powershell) was found on PATH. Install PowerShell, \
                         or remove \"shell\": \"powershell\" to use bash.",
                    )
                })?;
            let mut cmd = tokio::process::Command::new(pwsh);
            cmd.args(coco_shell_discovery::build_powershell_args(final_command));
            Ok(cmd)
        }
        ShellKind::Bash => {
            #[cfg(target_os = "windows")]
            {
                let bash = coco_shell_discovery::find_git_bash_path().ok_or_else(|| {
                    crate::error::HooksError::generic(
                        "Bash hook on Windows requires Git Bash (bash.exe) on PATH \
                         or in standard install location",
                    )
                })?;
                let mut cmd = tokio::process::Command::new(bash);
                cmd.arg("-c").arg(final_command);
                Ok(cmd)
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut cmd = tokio::process::Command::new("sh");
                cmd.arg("-c").arg(final_command);
                Ok(cmd)
            }
        }
    }
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
) -> crate::Result<HookExecutionResult> {
    match handler {
        HookHandler::Command {
            command,
            timeout_ms,
            shell,
        } => {
            let shell_kind = ShellKind::from_field(shell.as_deref());

            // Plugin / skill / user_config substitution. TS:
            // `utils/hooks.ts:execCommandHook` replaces
            // `${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_DATA}` /
            // `${user_config.<key>}` in the command body before spawn.
            // The substitution happens BEFORE the optional shell-prefix
            // wrap so the prefix can also reference these tokens.
            let substituted = substitute_plugin_vars(command, env_vars, shell_kind);

            // Windows-only: auto-prepend `bash ` to naked `.sh` script
            // invocations so Git Bash actually executes them.
            let with_sh_prefix = maybe_apply_sh_prefix(&substituted, shell_kind);

            // Optional shell prefix support (bash only — TS skips this for
            // PowerShell per `utils/hooks.ts:870-873`).
            let final_command = match env::var(EnvKey::CocoShellPrefix) {
                Ok(prefix) if !prefix.is_empty() && matches!(shell_kind, ShellKind::Bash) => {
                    format!("{prefix} {with_sh_prefix}")
                }
                _ => with_sh_prefix,
            };

            let mut cmd = build_command_for_shell(&final_command, shell_kind).await?;
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            cmd.envs(env_vars);
            apply_windows_hide(&mut cmd);

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
                let mut child = cmd.spawn().map_err(|e| {
                    crate::HooksError::exec_failed(format!("failed to spawn hook command: {e}"))
                })?;

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
                    .map_err(|_| crate::HooksError::HookTimeout {
                        timeout_ms: timeout_ms
                            .and_then(|ms| u64::try_from(ms).ok())
                            .unwrap_or(30_000),
                    })?
                    .map_err(|e| {
                        crate::HooksError::exec_failed(format!("hook command failed: {e}"))
                    })?
            } else {
                let child = cmd.spawn().map_err(|e| {
                    crate::HooksError::exec_failed(format!("failed to spawn hook command: {e}"))
                })?;

                tokio::time::timeout(timeout, child.wait_with_output())
                    .await
                    .map_err(|_| crate::HooksError::HookTimeout {
                        timeout_ms: timeout_ms
                            .and_then(|ms| u64::try_from(ms).ok())
                            .unwrap_or(30_000),
                    })?
                    .map_err(|e| {
                        crate::HooksError::exec_failed(format!("hook command failed: {e}"))
                    })?
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
        HookHandler::Prompt { prompt, .. } => {
            // Prompt hooks need an LLM. With no `HookLlmHandle` wired
            // (see Phase 2), fall back to returning the prompt text so
            // existing fixtures keep working — but log a warning so
            // production callers know the LLM path is missing.
            tracing::warn!(
                "Prompt hook executed without HookLlmHandle — returning prompt text. \
                 TS execPromptHook.ts evaluates with the LLM and parses {{ok, reason}}."
            );
            Ok(HookExecutionResult::PromptText(prompt.clone()))
        }
        HookHandler::Http {
            url,
            headers,
            timeout_ms,
            allowed_env_vars,
        } => {
            // SSRF guard — block private/link-local addresses (TS parity).
            match crate::ssrf::check_url_ssrf(url).await {
                Ok(true) => {
                    return Err(crate::HooksError::SsrfFailed {
                        url: url.clone(),
                        message: "URL resolves to a private/link-local address".to_string(),
                    });
                }
                Err(e) => {
                    tracing::debug!("SSRF check failed for {url}: {e}");
                    // Allow the request to proceed if DNS resolution fails
                    // (the actual HTTP request will fail anyway).
                }
                Ok(false) => {} // allowed
            }

            // TS default: TOOL_HOOK_EXECUTION_TIMEOUT_MS = 10 minutes
            // (`utils/hooks/execHttpHook.ts:12`). Rust used to default
            // to 10 s, which would clip any HTTP hook doing real work.
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(
                    timeout_ms
                        .and_then(|ms| u64::try_from(ms).ok())
                        .unwrap_or(10 * 60 * 1000),
                ))
                .build()
                .map_err(|e| crate::HooksError::HttpFailed {
                    message: format!("failed to create HTTP client: {e}"),
                })?;

            // TS hardcodes POST (`execHttpHook.ts:201`).
            let mut req = client.post(url);

            // Send the hook input JSON as the request body, always set
            // Content-Type (TS unconditionally injects it at line 158).
            req = req.header("Content-Type", "application/json");
            if let Some(body) = stdin_input {
                req = req.body(body.to_string());
            }

            if let Some(hdrs) = headers {
                let allowed: HashSet<&str> = allowed_env_vars.iter().map(String::as_str).collect();
                for (k, v) in hdrs {
                    // Per-hook allowlist gating: vars not in
                    // `allowed_env_vars` resolve to "" — matches
                    // `execHttpHook.ts:89-108` security boundary.
                    let interpolated = interpolate_env_vars_allowlisted(v, &allowed, env_vars);
                    req = req.header(k, sanitize_header_value(&interpolated));
                }
            }

            let resp = req
                .send()
                .await
                .map_err(|e| crate::HooksError::HttpFailed {
                    message: format!("HTTP hook request failed: {e}"),
                })?;

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
        HookHandler::Agent { prompt, .. } => {
            // Agent hooks need an LLM + multi-turn agent runtime. Until
            // `HookLlmHandle` is wired (Phase 2), fall back to returning
            // the prompt as text and log a warning. TS spawns
            // `query()` with MAX_AGENT_TURNS=50 and StructuredOutputTool.
            tracing::warn!(
                "Agent hook executed without HookLlmHandle — returning prompt text. \
                 TS execAgentHook.ts spawns a multi-turn agent."
            );
            Ok(HookExecutionResult::PromptText(prompt.clone()))
        }
    }
}

/// Loader-level policy gates.
///
/// TS: `shouldDisableAllHooksIncludingManaged()` /
/// `shouldDisableAllNonManagedHooks()` from `utils/settings/`. These
/// settings come from `policySettings` and apply at the load boundary
/// — non-managed hooks aren't even registered when
/// `allow_managed_hooks_only` is true.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoaderPolicy {
    /// Drop ALL hooks regardless of scope. TS: `disableAllHooks`.
    pub disable_all_hooks: bool,
    /// Drop hooks unless they came from a managed (Policy) or
    /// programmatic (Session) source. TS: `allowManagedHooksOnly`.
    pub allow_managed_hooks_only: bool,
}

/// Load hooks from a config JSON value.
///
/// The expected JSON format is:
/// ```json
/// {
///   "PreToolUse": [{ "type": "command", "command": "echo hi", "matcher": "Bash" }],
///   "SessionStart": [{ "type": "prompt", "prompt": "hello" }]
/// }
/// ```
///
/// Event type keys are PascalCase, matching the TS settings.json format
/// (`HOOK_EVENTS` in `coreSchemas.ts:355-383`).
pub fn load_hooks_from_config(
    hooks_value: &serde_json::Value,
    scope: HookScope,
) -> crate::Result<Vec<HookDefinition>> {
    load_hooks_from_config_with_policy(hooks_value, scope, LoaderPolicy::default())
}

/// Same as [`load_hooks_from_config`] but applies enterprise-policy
/// gates at load time — `disable_all_hooks` skips everything;
/// `allow_managed_hooks_only` skips anything not in `Policy` or
/// `Session` scope. TS parity:
/// `hooksConfigManager.ts:getRegisteredHooks` filters per setting.
pub fn load_hooks_from_config_with_policy(
    hooks_value: &serde_json::Value,
    scope: HookScope,
    policy: LoaderPolicy,
) -> crate::Result<Vec<HookDefinition>> {
    if policy.disable_all_hooks {
        tracing::debug!(
            ?scope,
            "skipping hook load: disable_all_hooks policy is set"
        );
        return Ok(Vec::new());
    }
    if policy.allow_managed_hooks_only && !matches!(scope, HookScope::Policy | HookScope::Session) {
        tracing::debug!(
            ?scope,
            "skipping hook load: allow_managed_hooks_only policy excludes this scope"
        );
        return Ok(Vec::new());
    }
    let obj = hooks_value
        .as_object()
        .ok_or_else(|| crate::HooksError::invalid_config("hooks config must be a JSON object"))?;

    let mut definitions = Vec::new();

    for (event_key, hook_array) in obj {
        let event: HookEventType =
            serde_json::from_value(serde_json::Value::String(event_key.clone())).map_err(|e| {
                crate::HooksError::invalid_config(format!(
                    "unknown hook event type '{event_key}': {e}"
                ))
            })?;

        let hooks = hook_array.as_array().ok_or_else(|| {
            crate::HooksError::invalid_config(format!("hooks for '{event_key}' must be an array"))
        })?;

        for raw in hooks {
            let raw_obj = raw.as_object().ok_or_else(|| {
                crate::HooksError::invalid_config("hook definition must be a JSON object")
            })?;

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
                status_message,
            });
        }
    }

    Ok(definitions)
}

/// Parse a hook handler from a raw JSON object.
fn parse_hook_handler(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> crate::Result<HookHandler> {
    let hook_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("command");

    match hook_type {
        "command" => {
            let command = obj
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    crate::HooksError::invalid_config("command hook requires a 'command' field")
                })?
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
                .ok_or_else(|| {
                    crate::HooksError::invalid_config("prompt hook requires a 'prompt' field")
                })?
                .to_string();
            let model = obj.get("model").and_then(|v| v.as_str()).map(String::from);
            let timeout_ms = obj.get("timeout_ms").and_then(serde_json::Value::as_i64);
            Ok(HookHandler::Prompt {
                prompt,
                model,
                timeout_ms,
            })
        }
        "webhook" | "http" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    crate::HooksError::invalid_config("http hook requires a 'url' field")
                })?
                .to_string();
            let headers = obj.get("headers").and_then(|v| {
                v.as_object().map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
            });
            let timeout_ms = obj.get("timeout_ms").and_then(serde_json::Value::as_i64);
            // TS uses camelCase `allowedEnvVars`; accept both.
            let allowed_env_vars = obj
                .get("allowed_env_vars")
                .or_else(|| obj.get("allowedEnvVars"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Ok(HookHandler::Http {
                url,
                headers,
                timeout_ms,
                allowed_env_vars,
            })
        }
        "agent" => {
            let prompt = obj
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    crate::HooksError::invalid_config("agent hook requires a 'prompt' field")
                })?
                .to_string();
            let model = obj.get("model").and_then(|v| v.as_str()).map(String::from);
            let timeout_ms = obj.get("timeout_ms").and_then(serde_json::Value::as_i64);
            Ok(HookHandler::Agent {
                prompt,
                model,
                timeout_ms,
            })
        }
        other => Err(crate::HooksError::invalid_config(format!(
            "unknown hook handler type: '{other}'"
        ))),
    }
}

/// Apply a top-level timeout (in seconds) to a handler that lacks its own timeout_ms.
///
/// TS: every variant accepts `timeout: number` (seconds) — the loader
/// converts to ms here for any handler whose explicit `timeout_ms` is
/// unset.
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
            headers,
            timeout_ms: None,
            allowed_env_vars,
        } => HookHandler::Http {
            url,
            headers,
            timeout_ms: Some(ms),
            allowed_env_vars,
        },
        HookHandler::Prompt {
            prompt,
            model,
            timeout_ms: None,
        } => HookHandler::Prompt {
            prompt,
            model,
            timeout_ms: Some(ms),
        },
        HookHandler::Agent {
            prompt,
            model,
            timeout_ms: None,
        } => HookHandler::Agent {
            prompt,
            model,
            timeout_ms: Some(ms),
        },
        other => other,
    }
}

/// Substitute `${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_DATA}` /
/// `${user_config.<key>}` tokens in a command body.
///
/// TS: `utils/hooks.ts:execCommandHook` replaces these tokens against
/// the resolved plugin context before the shell sees the command. The
/// values are sourced from the env-var bag the orchestration layer
/// builds via `build_hook_env_with_plugin` (`CLAUDE_PLUGIN_ROOT` /
/// `CLAUDE_PLUGIN_DATA` / `CLAUDE_PLUGIN_OPTION_<KEY>`), so a single
/// pass over `env_vars` is enough — no separate plugin-context arg.
///
/// `${user_config.foo-bar}` is converted to a sanitized env-key lookup
/// (matches the TS sanitizer in `build_hook_env_with_plugin`); missing
/// keys resolve to the empty string rather than failing the hook,
/// because TS surfaces those as command-time bash errors anyway and
/// preserving the unexpanded literal would mis-fire matchers.
///
/// `shell_kind` controls path conversion for plugin-root tokens: bash
/// on Windows expects POSIX-shaped paths (`/c/Users/foo`); PowerShell
/// uses native Windows paths (`C:\Users\foo`). On non-Windows the
/// conversion is a no-op so Linux/macOS stays unchanged.
fn substitute_plugin_vars(
    command: &str,
    env_vars: &HashMap<String, String>,
    shell_kind: ShellKind,
) -> String {
    let path_xform = |raw: &str| -> String {
        match shell_kind {
            ShellKind::Bash => coco_shell_discovery::windows_path_to_posix_path(raw),
            ShellKind::PowerShell => raw.to_string(),
        }
    };

    let mut out = command.to_string();

    if let Some(root) = env_vars.get("CLAUDE_PLUGIN_ROOT") {
        out = out.replace("${CLAUDE_PLUGIN_ROOT}", &path_xform(root));
    }
    if let Some(data) = env_vars.get("CLAUDE_PLUGIN_DATA") {
        out = out.replace("${CLAUDE_PLUGIN_DATA}", &path_xform(data));
    }

    // `${user_config.<key>}` — only substitute if a regex match succeeds.
    let Ok(re) = regex::Regex::new(r"\$\{user_config\.([A-Za-z_][A-Za-z0-9_\-]*)\}") else {
        return out;
    };
    re.replace_all(&out, |caps: &regex::Captures<'_>| {
        let raw_key = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let env_key = format!(
            "CLAUDE_PLUGIN_OPTION_{}",
            raw_key
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                })
                .collect::<String>()
                .to_ascii_uppercase()
        );
        // user_config values are not paths — pass them through verbatim.
        env_vars.get(&env_key).cloned().unwrap_or_default()
    })
    .to_string()
}

/// Sanitize an HTTP header value by stripping CR, LF, and NUL bytes.
///
/// TS: sanitizeHeaderValue() — prevents CRLF injection via header values.
fn sanitize_header_value(value: &str) -> String {
    value.replace(['\r', '\n', '\0'], "")
}

/// Allowlist-gated env-var interpolation for HTTP hook headers.
///
/// TS: `execHttpHook.ts:89-108` — references to vars not in
/// `allowed` resolve to empty string to prevent project-configured
/// hooks from exfiltrating arbitrary process environment values
/// (e.g. `Authorization: Bearer $AWS_SECRET_ACCESS_KEY`).
fn interpolate_env_vars_allowlisted(
    value: &str,
    allowed: &HashSet<&str>,
    env_vars: &HashMap<String, String>,
) -> String {
    let Ok(re) = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}|\$([A-Z_][A-Z0-9_]*)") else {
        return value.to_string();
    };
    re.replace_all(value, |caps: &regex::Captures<'_>| {
        let var_name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        if !allowed.contains(var_name) {
            tracing::debug!(
                "HTTP hook header references env var ${var_name} not in allowed_env_vars; skipping interpolation"
            );
            return String::new();
        }
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
        HookHandler::Prompt { prompt, .. } => format!("prompt:{prompt}"),
        HookHandler::Http { url, .. } => format!("http:{url}"),
        HookHandler::Agent { prompt, .. } => format!("agent:{prompt}"),
    };
    format!(
        "{handler_key}\0{}",
        hook.if_condition.as_deref().unwrap_or("")
    )
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
