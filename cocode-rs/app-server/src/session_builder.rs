//! Wiring logic for SDK session parameters.
//!
//! Converts SDK-facing protocol types (from `cocode-app-server-protocol`)
//! into internal types and applies them to `SessionState`.
//!
//! Shared by both the CLI SDK mode and the app-server WebSocket mode.

use std::collections::HashMap;
use std::sync::Arc;

use cocode_app_server_protocol::AgentDefinitionConfig;
use cocode_app_server_protocol::HookCallbackConfig;
use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_hooks::HookSdkCallbackFn;
use cocode_protocol::HookEventType;
use cocode_protocol::ModelSpec;
use cocode_protocol::PermissionMode;
use cocode_protocol::SubagentType;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_session::SessionState;
use cocode_subagent::AgentDefinition;
use cocode_subagent::AgentSource;
use cocode_subagent::IsolationMode;
use cocode_subagent::McpServerRef;
use cocode_subagent::MemoryScope;
use tokio::sync::Mutex;
use tokio::sync::oneshot;

/// Info about a pending hook callback that needs to be sent to the client.
pub struct HookCallbackRequestInfo {
    /// Unique request ID for response correlation.
    pub request_id: String,
    /// Pre-registered callback identifier.
    pub callback_id: String,
    /// Hook event type (e.g., "PreToolUse").
    pub event_type: String,
    /// Hook context input.
    pub input: serde_json::Value,
}

/// Bridge for routing SDK hook callbacks through the JSON-RPC protocol.
pub struct SdkHookBridge {
    /// Pending responses keyed by request_id → oneshot sender.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    /// Receiver for hook callback requests from the registry.
    request_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<HookCallbackRequestInfo>>,
}

impl SdkHookBridge {
    /// Receive the next pending hook callback request.
    pub async fn recv_request(&self) -> Option<HookCallbackRequestInfo> {
        self.request_rx.lock().await.recv().await
    }

    /// Resolve a pending hook callback with the client's response.
    pub async fn resolve(&self, request_id: &str, output: serde_json::Value) {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(request_id)
        };
        if let Some(tx) = tx {
            let _ = tx.send(output);
        } else {
            tracing::warn!(request_id, "Hook callback response for unknown request_id");
        }
    }

    /// Drain all pending hook callbacks on turn end.
    pub async fn drain_pending(&self) {
        let mut pending = self.pending.lock().await;
        for (id, tx) in pending.drain() {
            tracing::debug!(request_id = id, "Draining pending hook callback");
            let _ = tx.send(serde_json::Value::Null);
        }
    }
}

/// Apply all SDK session parameters to the given `SessionState`.
///
/// Returns an optional `SdkHookBridge` if SDK hook callbacks were registered.
pub async fn apply_sdk_params(
    state: &mut SessionState,
    params: &SessionStartRequestParams,
) -> anyhow::Result<Option<Arc<SdkHookBridge>>> {
    let mgr = state.subagent_manager();
    let mut mgr = mgr.lock().await;

    if params.disable_builtin_agents == Some(true) {
        mgr.retain_definitions(|d| d.source != AgentSource::BuiltIn);
    }

    let guide_type = SubagentType::Guide.as_str();
    mgr.retain_definitions(|d| d.agent_type != guide_type);

    if let Some(ref agents) = params.agents {
        for (name, config) in agents {
            let definition = convert_agent_config(name, config);
            mgr.register_agent_type(definition);
        }
    }

    drop(mgr);

    let hook_bridge = if let Some(ref hooks) = params.hooks {
        Some(register_sdk_hooks(&state.hook_registry, hooks))
    } else {
        None
    };

    if let Some(cents) = params.max_budget_cents {
        state.session.set_max_budget_cents(Some(cents));
    }

    if let Some(ref thinking) = params.thinking {
        use cocode_protocol::ThinkingLevel;
        let level = match thinking.mode {
            cocode_app_server_protocol::ThinkingMode::Enabled => ThinkingLevel::high(),
            cocode_app_server_protocol::ThinkingMode::Disabled => ThinkingLevel::none(),
            cocode_app_server_protocol::ThinkingMode::Adaptive => ThinkingLevel::medium(),
        };
        state.switch_thinking_level(cocode_protocol::ModelRole::Main, level);
    }

    if let Some(ref rules) = params.permission_rules {
        state.append_permission_rules_from_json(rules);
    }

    if let Some(ref tools) = params.tools {
        match tools {
            cocode_app_server_protocol::ToolsConfig::List(names) => {
                let registry = Arc::get_mut(&mut state.tool_registry);
                if let Some(registry) = registry {
                    registry.restrict_to(names);
                    tracing::info!(count = names.len(), "SDK tool restriction applied");
                } else {
                    tracing::warn!("Cannot restrict tools: registry has multiple owners");
                }
            }
            cocode_app_server_protocol::ToolsConfig::Preset { preset } => {
                tracing::debug!(preset, "SDK tools preset (no-op: using default tool set)");
            }
        }
    }

    if let Some(ref output_format) = params.output_format {
        state.set_structured_output_schema(output_format.schema.clone());
        tracing::info!("SDK structured output schema applied");
    }

    if params.sandbox.is_some() {
        tracing::debug!("SDK sandbox config applied at config level");
    }

    if params.mcp_servers.is_some() {
        tracing::info!("SDK MCP servers config received (wiring deferred)");
    }

    Ok(hook_bridge)
}

fn register_sdk_hooks(
    hook_registry: &Arc<cocode_hooks::HookRegistry>,
    hooks: &[HookCallbackConfig],
) -> Arc<SdkHookBridge> {
    let pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<HookCallbackRequestInfo>(64);

    let pending_clone = pending.clone();
    let sdk_callback_fn: HookSdkCallbackFn = Arc::new(move |callback_id, event_type, input| {
        let pending = pending_clone.clone();
        let tx = request_tx.clone();
        Box::pin(async move {
            let request_id = uuid::Uuid::new_v4().to_string();
            let (resp_tx, resp_rx) = oneshot::channel();

            {
                let mut map = pending.lock().await;
                map.insert(request_id.clone(), resp_tx);
            }

            if let Err(e) = tx
                .send(HookCallbackRequestInfo {
                    request_id: request_id.clone(),
                    callback_id,
                    event_type,
                    input,
                })
                .await
            {
                tracing::warn!("Failed to send hook callback request: {e}");
                let mut map = pending.lock().await;
                map.remove(&request_id);
                return Err("Hook callback request channel closed".to_string());
            }

            match resp_rx.await {
                Ok(output) => Ok(output),
                Err(_) => Err("Hook callback response channel closed".to_string()),
            }
        })
    });
    hook_registry.set_sdk_callback_fn(sdk_callback_fn);

    for hook_config in hooks {
        if hook_config.callback_id.is_empty() {
            tracing::warn!("SDK hook callback has empty callback_id, skipping");
            continue;
        }
        let event_type = match hook_config.event.parse::<HookEventType>() {
            Ok(et) => et,
            Err(_) => {
                tracing::warn!(
                    event = hook_config.event,
                    callback_id = hook_config.callback_id,
                    "Unknown hook event type, defaulting to PreToolUse"
                );
                HookEventType::PreToolUse
            }
        };
        let matcher = hook_config
            .matcher
            .as_ref()
            .map(|m| cocode_hooks::HookMatcher::Exact { value: m.clone() });

        let definition = cocode_hooks::HookDefinition {
            name: format!("sdk-callback-{}", hook_config.callback_id),
            event_type,
            matcher,
            handler: cocode_hooks::HookHandler::SdkCallback {
                callback_id: hook_config.callback_id.clone(),
            },
            enabled: true,
            source: cocode_hooks::HookSource::Session,
            once: false,
            is_async: false,
            force_sync_execution: false,
            timeout_secs: hook_config
                .timeout_ms
                .map(|ms| (ms / 1000).clamp(1, cocode_hooks::MAX_TIMEOUT_SECS))
                .unwrap_or(30),
            group_id: None,
            status_message: None,
        };
        hook_registry.register(definition);
    }

    Arc::new(SdkHookBridge {
        pending,
        request_rx: tokio::sync::Mutex::new(request_rx),
    })
}

fn convert_agent_config(name: &str, config: &AgentDefinitionConfig) -> AgentDefinition {
    let critical_reminder = config
        .prompt
        .clone()
        .or_else(|| config.critical_reminder.clone());

    AgentDefinition {
        name: name.to_string(),
        description: config.description.clone().unwrap_or_default(),
        agent_type: name.to_string(),
        tools: config.tools.clone().unwrap_or_default(),
        disallowed_tools: config.disallowed_tools.clone().unwrap_or_default(),
        identity: config.model.as_ref().map(|m| convert_model_identity(m)),
        max_turns: config.max_turns,
        permission_mode: config.permission_mode.as_deref().map(parse_permission_mode),
        fork_context: config.fork_context,
        color: config.color.clone(),
        critical_reminder,
        source: AgentSource::Sdk,
        skills: config.skills.clone().unwrap_or_default(),
        background: config.background,
        memory: config.memory.map(convert_memory_scope),
        hooks: config
            .hooks
            .as_ref()
            .map(|hooks| hooks.iter().map(convert_agent_hook).collect()),
        mcp_servers: config.mcp_servers.as_ref().map(|servers| {
            servers
                .iter()
                .map(|name| McpServerRef {
                    name: name.clone(),
                    transport: None,
                })
                .collect()
        }),
        isolation: config.isolation.map(convert_isolation_mode),
        use_custom_prompt: config.use_custom_prompt,
    }
}

fn parse_permission_mode(mode: &str) -> PermissionMode {
    match mode {
        "acceptEdits" => PermissionMode::AcceptEdits,
        "bypassPermissions" | "bypass" => PermissionMode::Bypass,
        "plan" => PermissionMode::Plan,
        "dontAsk" => PermissionMode::DontAsk,
        _ => PermissionMode::Default,
    }
}

fn convert_memory_scope(scope: cocode_app_server_protocol::AgentMemoryScope) -> MemoryScope {
    match scope {
        cocode_app_server_protocol::AgentMemoryScope::User => MemoryScope::User,
        cocode_app_server_protocol::AgentMemoryScope::Project => MemoryScope::Project,
        cocode_app_server_protocol::AgentMemoryScope::Local => MemoryScope::Local,
    }
}

fn convert_isolation_mode(mode: cocode_app_server_protocol::AgentIsolationMode) -> IsolationMode {
    match mode {
        cocode_app_server_protocol::AgentIsolationMode::None => IsolationMode::None,
        cocode_app_server_protocol::AgentIsolationMode::Worktree => IsolationMode::Worktree,
    }
}

fn convert_agent_hook(
    hook: &cocode_app_server_protocol::AgentHookConfig,
) -> cocode_subagent::AgentHookDefinition {
    cocode_subagent::AgentHookDefinition {
        event: hook.event.clone(),
        matcher: hook.matcher.clone(),
        command: hook.command.clone(),
        timeout: hook.timeout.and_then(|t| u32::try_from(t).ok()),
    }
}

fn convert_model_identity(model: &str) -> ExecutionIdentity {
    match model {
        "inherit" => ExecutionIdentity::Inherit,
        other => {
            let (provider, slug) = if let Some((p, m)) = other.split_once('/') {
                (p.to_string(), m.to_string())
            } else {
                ("anthropic".to_string(), other.to_string())
            };
            ExecutionIdentity::Spec(ModelSpec::new(provider, slug))
        }
    }
}
