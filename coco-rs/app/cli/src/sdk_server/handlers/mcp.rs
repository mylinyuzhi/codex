//! MCP lifecycle handlers — `mcp/status` / `mcp/setServers` /
//! `mcp/reconnect` / `mcp/toggle`.
//!
//! All reachable routes require an [`coco_mcp::McpConnectionManager`] to
//! be wired via `SdkServer::with_mcp_manager`; otherwise they return
//! `INVALID_REQUEST`.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use super::HandlerContext;
use super::HandlerResult;

/// `mcp/status` — report MCP server connection status.
///
/// If an `McpConnectionManager` is wired, returns the actual connection
/// state for every registered server. Otherwise returns an empty list
/// (persistence disabled).
///
/// TS reference: `SDKControlMcpStatusResponseSchema`
/// (controlSchemas.ts:165-173).
pub(super) async fn handle_mcp_status(ctx: &HandlerContext) -> HandlerResult {
    let manager_slot = ctx.state.mcp_manager.read().await;
    let Some(manager) = manager_slot.as_ref() else {
        info!("SdkServer: mcp/status (no MCP manager wired, returning empty)");
        return HandlerResult::ok(coco_types::McpStatusResult {
            mcp_servers: Vec::new(),
        });
    };
    let manager = manager.lock().await;
    let names = manager.registered_server_names();
    let mut statuses: Vec<coco_types::McpServerStatus> = Vec::new();
    for name in &names {
        let state = manager.get_state(name).await;
        let (status, error, tool_count) = match state {
            Some(coco_mcp::McpConnectionState::Connected(server)) => (
                coco_types::McpConnectionStatus::Connected,
                None,
                server.tools.len() as i32,
            ),
            Some(coco_mcp::McpConnectionState::Pending { .. }) => {
                (coco_types::McpConnectionStatus::Pending, None, 0)
            }
            Some(coco_mcp::McpConnectionState::Failed { error }) => {
                (coco_types::McpConnectionStatus::Failed, Some(error), 0)
            }
            Some(coco_mcp::McpConnectionState::NeedsAuth { .. }) => {
                (coco_types::McpConnectionStatus::NeedsAuth, None, 0)
            }
            Some(coco_mcp::McpConnectionState::Disabled) => {
                (coco_types::McpConnectionStatus::Disabled, None, 0)
            }
            None => (coco_types::McpConnectionStatus::Disconnected, None, 0),
        };
        statuses.push(coco_types::McpServerStatus {
            name: name.clone(),
            status,
            tool_count,
            error,
        });
    }
    info!(server_count = statuses.len(), "SdkServer: mcp/status");
    HandlerResult::ok(coco_types::McpStatusResult {
        mcp_servers: statuses,
    })
}

/// Convert the tools of a connected MCP server into `McpToolSchema` values
/// ready for `register_mcp_tools`.
///
/// The `McpConnectionManager::get_state` is awaited while the caller already
/// holds the manager lock, so this is a `&McpConnectionManager` call (not
/// `&mut`). Safety: the lock was acquired by the caller and we are still
/// inside that scope.
async fn collect_server_schemas(
    manager: &coco_mcp::McpConnectionManager,
    server_name: &str,
) -> Vec<coco_tool_runtime::McpToolSchema> {
    let Some(coco_mcp::McpConnectionState::Connected(server)) =
        manager.get_state(server_name).await
    else {
        return vec![];
    };
    server
        .tools
        .iter()
        .map(|t| coco_tool_runtime::McpToolSchema {
            server_name: server_name.to_string(),
            tool_name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
            annotations: coco_tool_runtime::McpToolAnnotations::default(),
        })
        .collect()
}

/// Register MCP server tools in the shared `ToolRegistry` via the session
/// runtime, if one is currently wired.
async fn register_server_tools(
    ctx: &HandlerContext,
    server_name: &str,
    schemas: Vec<coco_tool_runtime::McpToolSchema>,
) {
    let rt_guard = ctx.state.session_runtime.read().await;
    if let Some(rt) = rt_guard.as_ref() {
        coco_tools::register_mcp_tools(rt.tools(), server_name, schemas);
    }
}

/// Deregister all tools for an MCP server from the shared `ToolRegistry`.
async fn deregister_server_tools(ctx: &HandlerContext, server_name: &str) {
    let rt_guard = ctx.state.session_runtime.read().await;
    if let Some(rt) = rt_guard.as_ref() {
        coco_tools::deregister_mcp_server(rt.tools(), server_name);
    }
}

/// No-op `SendElicitation` callback used when the SDK server's MCP
/// lifecycle handlers trigger a connect that surfaces an elicitation
/// from the upstream server.
///
/// In the SDK design, elicitations from MCP servers should propagate
/// to the SDK client via a `ServerRequest::RequestElicitation` and
/// `elicitation/resolve` round-trip. Wiring that bridge is a future
/// follow-up — until then, this stub immediately rejects any
/// elicitation so connect either succeeds (no auth needed) or errors
/// out (auth required) without blocking forever.
fn no_op_send_elicitation() -> coco_mcp::SendElicitation {
    use std::future::Future;
    use std::pin::Pin;
    Box::new(
        |_request_id, _elicitation| -> Pin<
            Box<dyn Future<Output = anyhow::Result<coco_mcp::ElicitationResponse>> + Send>,
        > {
            Box::pin(async move {
                Err(anyhow::anyhow!(
                    "elicitation rejected: SDK server does not yet bridge elicitations to clients"
                ))
            })
        },
    )
}

/// Helper: borrow the wired MCP manager or return INVALID_REQUEST.
async fn require_mcp_manager(
    ctx: &HandlerContext,
) -> Result<Arc<Mutex<coco_mcp::McpConnectionManager>>, HandlerResult> {
    let slot = ctx.state.mcp_manager.read().await;
    match slot.as_ref() {
        Some(m) => Ok(m.clone()),
        None => Err(HandlerResult::Err {
            code: coco_types::error_codes::INVALID_REQUEST,
            message: "MCP manager not enabled on this server".into(),
            data: None,
        }),
    }
}

/// `mcp/setServers` — register or replace MCP server configurations.
///
/// For each `(name, config_json)` pair in `params.servers`, this
/// handler:
/// 1. Deserializes the JSON value into [`coco_mcp::McpServerConfig`]
///    (transport-tagged enum).
/// 2. Wraps it in a [`coco_mcp::ScopedMcpServerConfig`] with
///    `scope = ConfigScope::Dynamic` and no plugin source.
/// 3. Calls `register_server(...)` on the live manager.
///
/// Note that this only **registers** the configs — it does not
/// auto-connect. Use `mcp/reconnect` (or the existing tool layer's
/// connect-on-first-use logic) to actually establish connections.
///
/// Returns:
/// - `added`: names that were added or replaced
/// - `removed`: always empty in this implementation (no diff vs prior state)
/// - `errors`: per-name deserialization errors
pub(super) async fn handle_mcp_set_servers(
    params: coco_types::McpSetServersParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let mut manager = manager_arc.lock().await;
    let mut added: Vec<String> = Vec::new();
    let mut errors: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (name, config_json) in params.servers {
        match serde_json::from_value::<coco_mcp::McpServerConfig>(config_json) {
            Ok(config) => {
                let scoped = coco_mcp::ScopedMcpServerConfig {
                    name: name.clone(),
                    config,
                    scope: coco_mcp::ConfigScope::Dynamic,
                    plugin_source: None,
                };
                manager.register_server(scoped);
                added.push(name);
            }
            Err(e) => {
                errors.insert(name, format!("invalid mcp config: {e}"));
            }
        }
    }
    info!(
        added = added.len(),
        errors = errors.len(),
        "SdkServer: mcp/setServers"
    );
    HandlerResult::ok(coco_types::McpSetServersResult {
        added,
        removed: Vec::new(),
        errors,
    })
}

/// `mcp/reconnect` — disconnect + reconnect a specific MCP server.
///
/// Useful after a server's process has been restarted externally or
/// after a transient network failure. The handler unconditionally
/// disconnects (no-op if not connected) then attempts to connect
/// using a no-op elicitation callback.
///
/// Errors:
/// - `INVALID_REQUEST` if MCP manager not enabled
/// - `INTERNAL_ERROR` if the connect attempt fails (e.g. server
///   process refused, OAuth required without elicitation bridge)
pub(super) async fn handle_mcp_reconnect(
    params: coco_types::McpReconnectParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let manager = manager_arc.lock().await;
    manager.disconnect(&params.server_name).await;
    match manager
        .connect(&params.server_name, no_op_send_elicitation())
        .await
    {
        Ok(()) => {
            info!(server = %params.server_name, "SdkServer: mcp/reconnect ok");
            let schemas = collect_server_schemas(&manager, &params.server_name).await;
            drop(manager);
            register_server_tools(ctx, &params.server_name, schemas).await;
            HandlerResult::ok_empty()
        }
        Err(e) => HandlerResult::Err {
            code: coco_types::error_codes::INTERNAL_ERROR,
            message: format!("mcp/reconnect: {e}"),
            data: None,
        },
    }
}

/// `mcp/toggle` — enable or disable an MCP server.
///
/// `enabled = true`: ensures the server is connected (no-op if
/// already connected).
/// `enabled = false`: disconnects the server.
///
/// Errors:
/// - `INVALID_REQUEST` if MCP manager not enabled
/// - `INTERNAL_ERROR` if enabling and the connect attempt fails
pub(super) async fn handle_mcp_toggle(
    params: coco_types::McpToggleParams,
    ctx: &HandlerContext,
) -> HandlerResult {
    let manager_arc = match require_mcp_manager(ctx).await {
        Ok(m) => m,
        Err(e) => return e,
    };
    let manager = manager_arc.lock().await;
    if params.enabled {
        match manager
            .connect(&params.server_name, no_op_send_elicitation())
            .await
        {
            Ok(()) => {
                info!(server = %params.server_name, "SdkServer: mcp/toggle (enabled)");
                let schemas = collect_server_schemas(&manager, &params.server_name).await;
                drop(manager);
                register_server_tools(ctx, &params.server_name, schemas).await;
                HandlerResult::ok_empty()
            }
            Err(e) => HandlerResult::Err {
                code: coco_types::error_codes::INTERNAL_ERROR,
                message: format!("mcp/toggle enable: {e}"),
                data: None,
            },
        }
    } else {
        manager.disconnect(&params.server_name).await;
        info!(server = %params.server_name, "SdkServer: mcp/toggle (disabled)");
        drop(manager);
        deregister_server_tools(ctx, &params.server_name).await;
        HandlerResult::ok_empty()
    }
}
