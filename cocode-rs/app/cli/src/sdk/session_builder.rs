//! Wiring logic for SDK session parameters.
//!
//! Converts SDK-facing protocol types (from `cocode-app-server-protocol`)
//! into internal types and applies them to `SessionState`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use cocode_app_server_protocol::AgentDefinitionConfig;
use cocode_app_server_protocol::HookCallbackConfig;
use cocode_app_server_protocol::McpServerConfig;
use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_hooks::HookSdkCallbackFn;
use cocode_protocol::HookEventType;
use cocode_protocol::ModelSpec;
use cocode_protocol::PermissionMode;
use cocode_protocol::SubagentType;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_rmcp_client::RmcpClient;
use cocode_session::SessionState;
use cocode_subagent::AgentDefinition;
use cocode_subagent::AgentSource;
use cocode_subagent::IsolationMode;
use cocode_subagent::McpServerRef;
use cocode_subagent::MemoryScope;
use tokio::sync::Mutex;
use tokio::sync::oneshot;

use super::mcp_bridge::SdkMcpBridge;

/// Result of applying SDK session parameters.
pub struct SdkParamsResult {
    /// Hook bridge for routing SDK callbacks (if hooks were registered).
    pub hook_bridge: Option<Arc<SdkHookBridge>>,
    /// MCP bridge for routing SDK-managed tool calls (if SDK tools were registered).
    pub mcp_bridge: Option<Arc<SdkMcpBridge>>,
    /// Successfully connected MCP servers: (name, tool_count).
    pub mcp_servers: Vec<(String, i32)>,
    /// Failed MCP servers: (name, error_message).
    pub mcp_failures: Vec<(String, String)>,
}

/// Apply all SDK session parameters to the given `SessionState`.
///
/// This wires agents, hooks, MCP servers, budget, tools, sandbox, thinking,
/// and output format from the SDK's `SessionStartRequestParams`.
pub async fn apply_sdk_params(
    state: &mut SessionState,
    params: &SessionStartRequestParams,
) -> anyhow::Result<SdkParamsResult> {
    let mgr = state.subagent_manager();
    let mut mgr = mgr.lock().await;

    // ── Disable builtin agents ──────────────────────────────────
    if params.disable_builtin_agents == Some(true) {
        mgr.retain_definitions(|d| d.source != AgentSource::BuiltIn);
    }

    // ── Filter guide agent in SDK mode (matches Claude Code behavior) ──
    let guide_type = SubagentType::Guide.as_str();
    mgr.retain_definitions(|d| d.agent_type != guide_type);

    // ── Register SDK-provided agents ─────────────────────────────
    if let Some(ref agents) = params.agents {
        for (name, config) in agents {
            let definition = convert_agent_config(name, config);
            mgr.register_agent_type(definition);
        }
    }

    drop(mgr);

    // ── Register SDK hook callbacks ──────────────────────────────
    let hook_bridge = if let Some(ref hooks) = params.hooks {
        Some(register_sdk_hooks(&state.hook_registry, hooks))
    } else {
        None
    };

    // ── Budget ───────────────────────────────────────────────────
    if let Some(cents) = params.max_budget_cents {
        state.session.set_max_budget_cents(Some(cents));
    }

    // ── Thinking ─────────────────────────────────────────────────
    if let Some(ref thinking) = params.thinking {
        use cocode_protocol::ThinkingLevel;
        let level = match thinking.mode {
            cocode_app_server_protocol::ThinkingMode::Enabled => ThinkingLevel::high(),
            cocode_app_server_protocol::ThinkingMode::Disabled => ThinkingLevel::none(),
            cocode_app_server_protocol::ThinkingMode::Adaptive => ThinkingLevel::medium(),
        };
        state.switch_thinking_level(cocode_protocol::ModelRole::Main, level);
    }

    // ── Permission rules ─────────────────────────────────────────
    if let Some(ref rules) = params.permission_rules {
        state.append_permission_rules_from_json(rules);
    }

    // ── Tools ──────────────────────────────────────────────────────
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

    // ── Output format ─────────────────────────────────────────────
    if let Some(ref output_format) = params.output_format {
        state.set_structured_output_schema(output_format.schema.clone());
        tracing::info!("SDK structured output schema applied");
    }

    // ── Sandbox ─────────────────────────────────────────────────
    // Sandbox is wired at config level in build_session_state() (before
    // SessionState creation), so no action needed here.
    if params.sandbox.is_some() {
        tracing::debug!("SDK sandbox config applied at config level");
    }

    // ── MCP servers ─────────────────────────────────────────────
    let mut mcp_servers_result = Vec::new();
    let mut mcp_failures_result = Vec::new();
    let mut mcp_bridge: Option<Arc<SdkMcpBridge>> = None;
    if let Some(ref mcp_servers) = params.mcp_servers {
        let (clients, successes, failures, bridge) =
            connect_sdk_mcp_servers(state, mcp_servers).await?;
        mcp_servers_result = successes;
        mcp_failures_result = failures;
        mcp_bridge = bridge;
        if !clients.is_empty() {
            state.push_mcp_clients(clients);
        }
    }

    Ok(SdkParamsResult {
        hook_bridge,
        mcp_bridge,
        mcp_servers: mcp_servers_result,
        mcp_failures: mcp_failures_result,
    })
}

/// Convert SDK `ThinkingMode` to internal `ThinkingLevel`.
pub(crate) fn convert_thinking_mode(
    mode: cocode_app_server_protocol::ThinkingMode,
) -> cocode_protocol::ThinkingLevel {
    use cocode_protocol::ThinkingLevel;
    match mode {
        cocode_app_server_protocol::ThinkingMode::Enabled => ThinkingLevel::high(),
        cocode_app_server_protocol::ThinkingMode::Disabled => ThinkingLevel::none(),
        cocode_app_server_protocol::ThinkingMode::Adaptive => ThinkingLevel::medium(),
    }
}

/// Info about a pending hook callback that needs to be sent to the SDK client.
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
///
/// Flow:
/// 1. Hook fires → registry calls `sdk_callback_fn`
/// 2. Callback fn sends `HookCallbackRequestInfo` via `request_tx`
/// 3. Turn loop polls `recv_request()` → emits `ServerRequest::HookCallback`
/// 4. SDK client responds → turn loop calls `resolve()`
/// 5. Oneshot channel unblocks the waiting `sdk_callback_fn`
pub struct SdkHookBridge {
    /// Pending responses keyed by request_id → oneshot sender.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    /// Receiver for hook callback requests from the registry.
    request_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<HookCallbackRequestInfo>>,
}

impl SdkHookBridge {
    /// Receive the next pending hook callback request (for the turn loop).
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
    ///
    /// Unblocks any `sdk_callback_fn` futures still waiting for responses
    /// by sending a null value. This prevents resource leaks when a turn
    /// is interrupted or completes while hook callbacks are in-flight.
    pub async fn drain_pending(&self) {
        let mut pending = self.pending.lock().await;
        for (id, tx) in pending.drain() {
            tracing::debug!(
                request_id = id,
                "Draining pending hook callback on turn end"
            );
            let _ = tx.send(serde_json::Value::Null);
        }
    }
}

/// Register SDK hook callbacks and return a bridge for routing responses.
fn register_sdk_hooks(
    hook_registry: &Arc<cocode_hooks::HookRegistry>,
    hooks: &[HookCallbackConfig],
) -> Arc<SdkHookBridge> {
    let pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<HookCallbackRequestInfo>(64);

    // Set the SDK callback function on the hook registry
    let pending_clone = pending.clone();
    let sdk_callback_fn: HookSdkCallbackFn = Arc::new(move |callback_id, event_type, input| {
        let pending = pending_clone.clone();
        let tx = request_tx.clone();
        Box::pin(async move {
            let request_id = uuid::Uuid::new_v4().to_string();
            let (resp_tx, resp_rx) = oneshot::channel();

            // Store the response channel
            {
                let mut map = pending.lock().await;
                map.insert(request_id.clone(), resp_tx);
            }

            // Notify the turn loop to emit ServerRequest::HookCallback
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

            // Wait for the SDK client to respond (with safety timeout)
            let timeout_duration = Duration::from_secs(30);
            match tokio::time::timeout(timeout_duration, resp_rx).await {
                Ok(Ok(output)) => Ok(output),
                Ok(Err(_)) => Err("Hook callback response channel closed".to_string()),
                Err(_) => {
                    // Timeout — clean up pending entry
                    let mut map = pending.lock().await;
                    map.remove(&request_id);
                    tracing::warn!(
                        request_id = %request_id,
                        "Hook callback timed out after {timeout_duration:?}"
                    );
                    Err("Hook callback timed out".to_string())
                }
            }
        })
    });
    hook_registry.set_sdk_callback_fn(sdk_callback_fn);

    // Register hook definitions for each SDK callback
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
                .map(|ms| {
                    let raw = ms / 1000;
                    let clamped = raw.clamp(1, cocode_hooks::MAX_TIMEOUT_SECS);
                    if clamped != raw {
                        tracing::warn!(
                            callback_id = %hook_config.callback_id,
                            timeout_ms = ms,
                            clamped_secs = clamped,
                            "Hook timeout_ms clamped to {clamped}s (valid range: 1-{}s)",
                            cocode_hooks::MAX_TIMEOUT_SECS
                        );
                    }
                    clamped
                })
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

/// Convert an SDK `AgentDefinitionConfig` to an internal `AgentDefinition`.
fn convert_agent_config(name: &str, config: &AgentDefinitionConfig) -> AgentDefinition {
    // `prompt` is the primary system prompt content; `critical_reminder` is the
    // legacy name. Use `prompt` when set, fall back to `critical_reminder`.
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
        "auto" => PermissionMode::Auto,
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

/// Convert an SDK model string to an `ExecutionIdentity`.
///
/// Strings like "inherit" → `Inherit`, others are treated as model
/// slugs (e.g., "sonnet" → `Spec("anthropic", "sonnet")`).
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

// ── SDK MCP server connection ─────────────────────────────────────────

/// Connect SDK-provided MCP servers and register their tools.
///
/// Follows the same pattern as `cocode_plugin::connect_plugin_mcp_servers`:
/// spawn/connect → initialize → list tools → register in tool registry.
/// Failed servers are logged and skipped (don't fail the session).
///
/// For `McpServerConfig::Sdk` servers, tools are registered via
/// `SdkMcpBridge` and routed through the NDJSON control channel.
///
/// Returns `(clients, successes, failures, mcp_bridge)` where:
/// - `clients`: `Arc<RmcpClient>` to keep alive for session lifetime
/// - `successes`: `(name, tool_count)` for startup status reporting
/// - `failures`: `(name, error)` for startup status reporting
/// - `mcp_bridge`: bridge for SDK-managed servers (if any)
async fn connect_sdk_mcp_servers(
    state: &mut SessionState,
    servers: &HashMap<String, McpServerConfig>,
) -> anyhow::Result<(
    Vec<Arc<RmcpClient>>,
    Vec<(String, i32)>,
    Vec<(String, String)>,
    Option<Arc<SdkMcpBridge>>,
)> {
    let tool_timeout = Duration::from_secs(60);

    // Separate SDK-managed servers from real servers
    let (sdk_servers, real_servers): (Vec<_>, Vec<_>) = servers
        .iter()
        .partition(|(_, config)| matches!(config, McpServerConfig::Sdk { .. }));

    // Connect real servers concurrently
    let connection_futures: Vec<_> = real_servers
        .iter()
        .map(|(name, config)| {
            let name = (*name).clone();
            let config = (*config).clone();
            async move {
                let result = connect_single_mcp_server(&name, &config).await;
                (name, result)
            }
        })
        .collect();

    let results = futures::future::join_all(connection_futures).await;

    // Register tools sequentially (requires &mut ToolRegistry).
    // Arc::get_mut succeeds here because apply_sdk_params runs before
    // the executor clones the registry — assert this invariant.
    let mut clients = Vec::new();
    let mut successes: Vec<(String, i32)> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    let registry = Arc::get_mut(&mut state.tool_registry).ok_or_else(|| {
        anyhow::anyhow!(
            "SDK MCP tool registration failed: tool registry already shared. \
             Ensure apply_sdk_params is called before the executor clones the registry."
        )
    })?;

    for (name, result) in results {
        match result {
            Ok((client, tools_result)) => {
                let tools_count = tools_result.tools.len() as i32;
                registry.register_mcp_tools_executable(
                    &name,
                    tools_result.tools,
                    client.clone(),
                    tool_timeout,
                );
                tracing::info!(
                    server = %name,
                    tools = tools_count,
                    "Connected SDK MCP server"
                );
                successes.push((name, tools_count));
                clients.push(client);
            }
            Err(e) => {
                tracing::warn!(
                    server = %name,
                    error = %e,
                    "Failed to connect SDK MCP server"
                );
                failures.push((name, format!("{e:#}")));
            }
        }
    }

    // Register SDK-managed servers via bridge
    let mcp_bridge = if sdk_servers.is_empty() {
        None
    } else {
        let bridge = Arc::new(SdkMcpBridge::new());
        for (name, config) in &sdk_servers {
            if let McpServerConfig::Sdk { tools } = config {
                let tools_count = tools.len() as i32;
                for tool_def in tools {
                    let schema = tool_def
                        .input_schema
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({"type": "object"}));
                    let wrapper = bridge.create_tool_wrapper(
                        (*name).clone(),
                        tool_def.name.clone(),
                        tool_def.description.clone().unwrap_or_default(),
                        schema.clone(),
                    );
                    let qualified_name = wrapper.qualified_name();
                    registry.register_tool(qualified_name.clone(), Arc::new(wrapper));
                    registry.register_mcp_tool_info(
                        name,
                        &tool_def.name,
                        tool_def.description.as_deref(),
                        &schema,
                    );
                }
                tracing::info!(
                    server = %name,
                    tools = tools_count,
                    "Registered SDK-managed MCP server"
                );
                successes.push(((*name).clone(), tools_count));
            }
        }
        Some(bridge)
    };

    Ok((clients, successes, failures, mcp_bridge))
}

/// Connect a single SDK MCP server.
async fn connect_single_mcp_server(
    name: &str,
    config: &McpServerConfig,
) -> anyhow::Result<(Arc<RmcpClient>, cocode_mcp_types::ListToolsResult)> {
    use cocode_mcp_types::ClientCapabilities;
    use cocode_mcp_types::Implementation;
    use cocode_mcp_types::InitializeRequestParams;
    use cocode_mcp_types::MCP_SCHEMA_VERSION;
    use futures::FutureExt as _;

    let timeout = Duration::from_secs(30);

    let client = match config {
        McpServerConfig::Stdio { command, args, env } => Arc::new(
            RmcpClient::new_stdio_client(
                OsString::from(command),
                args.iter().map(OsString::from).collect(),
                env.clone(),
                &[],
                None,
            )
            .await?,
        ),
        McpServerConfig::Sdk { .. } => {
            anyhow::bail!("SDK-managed servers should be handled by connect_sdk_mcp_servers")
        }
        McpServerConfig::Http { url } | McpServerConfig::Sse { url } => {
            let cocode_home = dirs::home_dir()
                .map(|p| p.join(".cocode"))
                .unwrap_or_else(|| std::env::temp_dir().join(".cocode"));
            Arc::new(
                RmcpClient::new_streamable_http_client(
                    name,
                    url,
                    /*bearer_token*/ None,
                    /*http_headers*/ None,
                    /*env_http_headers*/ None,
                    Default::default(),
                    cocode_home,
                )
                .await?,
            )
        }
    };

    let init_params = InitializeRequestParams {
        capabilities: ClientCapabilities {
            elicitation: None,
            experimental: None,
            roots: None,
            sampling: None,
        },
        client_info: Implementation {
            name: "cocode-sdk".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some(format!("cocode SDK MCP: {name}")),
            user_agent: None,
        },
        protocol_version: MCP_SCHEMA_VERSION.to_string(),
    };

    let no_elicitation: cocode_rmcp_client::SendElicitation = Box::new(|_, _| {
        async {
            Err(anyhow::anyhow!(
                "Elicitation not supported for SDK MCP servers"
            ))
        }
        .boxed()
    });

    client
        .initialize(init_params, Some(timeout), no_elicitation)
        .await?;

    let tools_result = client.list_tools(None, Some(timeout)).await?;

    Ok((client, tools_result))
}
