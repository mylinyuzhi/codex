//! Lightweight JSON-RPC message types for SDK wire protocol.
//!
//! Follows JSON-RPC 2.0 semantics (request/notification/response/error)
//! without requiring the `"jsonrpc":"2.0"` field, matching the codex-rs
//! approach in `jsonrpc_lite.rs`.
//!
//! The key distinction: messages with an `id` field are requests that
//! expect a response; messages without `id` are fire-and-forget
//! notifications.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// A JSON-RPC request ID (string or integer).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::Integer(n) => write!(f, "{n}"),
        }
    }
}

/// Any valid JSON-RPC message that can be sent or received over the wire.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// A request that expects a response (has `id` + `method`).
    Request(JsonRpcRequest),
    /// A fire-and-forget notification (has `method` but no `id`).
    Notification(JsonRpcNotification),
    /// A successful response to a request (has `id` + `result`).
    Response(JsonRpcResponse),
    /// An error response to a request (has `id` + `error`).
    Error(JsonRpcError),
}

/// A request that expects a response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JsonRpcRequest {
    /// Unique request identifier for correlation.
    pub id: RequestId,
    /// Method name (e.g., "session/start", "turn/start").
    pub method: String,
    /// Method parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A notification which does not expect a response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JsonRpcNotification {
    /// Method name (e.g., "turn/started", "agentMessage/delta").
    pub method: String,
    /// Notification parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A successful response to a request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JsonRpcResponse {
    /// The request ID this response corresponds to.
    pub id: RequestId,
    /// The result value.
    pub result: Value,
}

/// An error response to a request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JsonRpcError {
    /// The request ID this error corresponds to.
    pub id: RequestId,
    /// Error details.
    pub error: JsonRpcErrorData,
}

/// Error detail within a JSON-RPC error response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JsonRpcErrorData {
    /// Error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}
