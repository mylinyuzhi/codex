//! SDK MCP routing bridge for in-process tool execution.
//!
//! Routes tool calls for SDK-managed MCP servers through the JSON-RPC
//! protocol channel instead of direct MCP connections. This enables
//! the Python SDK's `@tool()` decorator to host tools in-process.
//!
//! Flow:
//! 1. Agent calls `SdkMcpToolWrapper.execute()` for an SDK-managed tool
//! 2. Wrapper sends request through `SdkMcpBridge.call_tool()`
//! 3. Bridge emits the request via internal channel
//! 4. Turn loop picks it up, sends `ServerRequest::McpRouteMessage` to client
//! 5. Client handles the call, responds with `McpRouteMessageResponse`
//! 6. Processor calls `bridge.resolve()` to unblock the caller

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use cocode_protocol::ToolResultContent;
use cocode_tools_api::Tool;
use cocode_tools_api::context::ToolContext;
use cocode_tools_api::error::Result;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// Info about a pending MCP route request (consumed by the turn loop).
pub struct McpRouteRequestInfo {
    pub request_id: String,
    pub server_name: String,
    pub message: Value,
}

/// Bridge between SDK MCP tool wrappers and the JSON-RPC protocol channel.
pub struct SdkMcpBridge {
    /// Pending responses keyed by request_id -> oneshot sender.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Sender for MCP route requests (cloned into tool wrappers).
    request_tx: mpsc::Sender<McpRouteRequestInfo>,
    /// Receiver for MCP route requests (polled by the turn loop).
    request_rx: tokio::sync::Mutex<mpsc::Receiver<McpRouteRequestInfo>>,
}

impl Default for SdkMcpBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl SdkMcpBridge {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(64);
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            request_tx: tx,
            request_rx: tokio::sync::Mutex::new(rx),
        }
    }

    /// Receive the next pending MCP route request (for the turn loop).
    pub async fn recv_request(&self) -> Option<McpRouteRequestInfo> {
        self.request_rx.lock().await.recv().await
    }

    /// Resolve a pending MCP route request with the client's response.
    pub async fn resolve(&self, request_id: &str, response: Value) {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(request_id)
        };
        if let Some(tx) = tx {
            let _ = tx.send(response);
        } else {
            tracing::warn!(request_id, "MCP route response for unknown request_id");
        }
    }

    /// Drain all pending MCP route requests on turn end.
    pub async fn drain_pending(&self) {
        let mut pending = self.pending.lock().await;
        for (id, tx) in pending.drain() {
            tracing::debug!(request_id = id, "Draining pending MCP route on turn end");
            let _ = tx.send(serde_json::json!({"error": "turn ended"}));
        }
    }

    /// Create a tool call function for use by `SdkMcpToolWrapper`.
    fn call_fn(&self) -> SdkMcpCallFn {
        SdkMcpCallFn {
            pending: self.pending.clone(),
            request_tx: self.request_tx.clone(),
        }
    }

    /// Create an `SdkMcpToolWrapper` for a tool on this bridge.
    pub fn create_tool_wrapper(
        &self,
        server_name: String,
        tool_name: String,
        description: String,
        input_schema: Value,
    ) -> SdkMcpToolWrapper {
        SdkMcpToolWrapper {
            server_name,
            tool_name,
            description,
            input_schema,
            call_fn: self.call_fn(),
        }
    }
}

/// Cloneable call function for SDK MCP tool invocations.
#[derive(Clone)]
struct SdkMcpCallFn {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    request_tx: mpsc::Sender<McpRouteRequestInfo>,
}

impl SdkMcpCallFn {
    async fn call(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> anyhow::Result<Value> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": tool_name, "arguments": arguments }
        });

        let (resp_tx, resp_rx) = oneshot::channel();

        {
            let mut map = self.pending.lock().await;
            map.insert(request_id.clone(), resp_tx);
        }

        if let Err(e) = self
            .request_tx
            .send(McpRouteRequestInfo {
                request_id: request_id.clone(),
                server_name: server_name.to_string(),
                message,
            })
            .await
        {
            let mut map = self.pending.lock().await;
            map.remove(&request_id);
            anyhow::bail!("MCP route request channel closed: {e}");
        }

        resp_rx
            .await
            .map_err(|_| anyhow::anyhow!("MCP route response channel closed"))
    }
}

/// Tool wrapper for SDK-managed MCP tools routed through the protocol channel.
pub struct SdkMcpToolWrapper {
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
    call_fn: SdkMcpCallFn,
}

impl SdkMcpToolWrapper {
    /// Qualified name following the `mcp__<server>__<tool>` convention.
    pub fn qualified_name(&self) -> String {
        format!(
            "{}{}{}{}",
            cocode_protocol::MCP_TOOL_PREFIX,
            self.server_name,
            cocode_protocol::MCP_TOOL_SEPARATOR,
            self.tool_name,
        )
    }
}

#[async_trait]
impl Tool for SdkMcpToolWrapper {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        tracing::debug!(
            server = %self.server_name,
            tool = %self.tool_name,
            "Executing SDK MCP tool via protocol channel"
        );

        let result = self
            .call_fn
            .call(&self.server_name, &self.tool_name, input)
            .await
            .map_err(|e| {
                cocode_tools_api::error::ToolError::execution_failed(format!(
                    "SDK MCP tool call failed: {e}"
                ))
            })?;

        // Check for error in response
        if let Some(err) = result.get("error") {
            let err_msg = err
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| err.to_string());
            return Ok(ToolOutput {
                content: ToolResultContent::Text(err_msg),
                is_error: true,
                modifiers: Vec::new(),
                images: Vec::new(),
            });
        }

        // Extract result content
        let content = match result.get("result") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => result.to_string(),
        };

        Ok(ToolOutput {
            content: ToolResultContent::Text(content),
            is_error: false,
            modifiers: Vec::new(),
            images: Vec::new(),
        })
    }
}

impl std::fmt::Debug for SdkMcpToolWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkMcpToolWrapper")
            .field("server_name", &self.server_name)
            .field("tool_name", &self.tool_name)
            .finish_non_exhaustive()
    }
}
