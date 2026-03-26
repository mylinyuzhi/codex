//! JSON-RPC transport over stdin/stdout.
//!
//! Split into [`StdinReader`] and [`StdoutWriter`] so the SDK turn loop
//! can read client requests and write notifications concurrently within
//! a `tokio::select!` loop.
//!
//! Wire format follows JSON-RPC 2.0 semantics:
//! - Messages with `id` are requests (expect a response)
//! - Messages without `id` are notifications (fire-and-forget)
//! - Backward compatible: accepts both `ClientRequest` and `JsonRpcRequest`

use anyhow::Context;
use cocode_app_server_protocol::ApprovalResolveRequestParams;
use cocode_app_server_protocol::ClientRequest;
use cocode_app_server_protocol::HookCallbackResponseParams;
use cocode_app_server_protocol::JsonRpcNotification;
use cocode_app_server_protocol::JsonRpcRequest;
use cocode_app_server_protocol::JsonRpcResponse;
use cocode_app_server_protocol::KeepAliveRequestParams;
use cocode_app_server_protocol::RequestId;
use cocode_app_server_protocol::RewindFilesRequestParams;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::ServerRequest;
use cocode_app_server_protocol::SessionResumeRequestParams;
use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_app_server_protocol::SetModelRequestParams;
use cocode_app_server_protocol::SetPermissionModeRequestParams;
use cocode_app_server_protocol::SetThinkingRequestParams;
use cocode_app_server_protocol::StopTaskRequestParams;
use cocode_app_server_protocol::TurnInterruptRequestParams;
use cocode_app_server_protocol::TurnStartRequestParams;
use cocode_app_server_protocol::UpdateEnvRequestParams;
use cocode_app_server_protocol::UserInputResolveRequestParams;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

/// Parsed inbound message from stdin.
#[derive(Debug)]
pub enum InboundMessage {
    SessionStart(Box<SessionStartRequestParams>),
    SessionResume(SessionResumeRequestParams),
    TurnStart(TurnStartRequestParams),
    TurnInterrupt(#[allow(dead_code)] TurnInterruptRequestParams),
    ApprovalResolve(ApprovalResolveRequestParams),
    UserInputResolve(UserInputResolveRequestParams),
    SetModel(SetModelRequestParams),
    SetPermissionMode(SetPermissionModeRequestParams),
    StopTask(StopTaskRequestParams),
    HookCallbackResponse(HookCallbackResponseParams),
    SetThinking(SetThinkingRequestParams),
    RewindFiles(RewindFilesRequestParams),
    UpdateEnv(UpdateEnvRequestParams),
    KeepAlive(#[allow(dead_code)] KeepAliveRequestParams),
}

/// Reads client requests from stdin as NDJSON (JSON-RPC or legacy format).
pub struct StdinReader {
    reader: BufReader<tokio::io::Stdin>,
}

impl StdinReader {
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
        }
    }

    /// Read the next JSON line from stdin and parse as a client request.
    ///
    /// Accepts both JSON-RPC format (`{"id":..,"method":..,"params":..}`)
    /// and legacy format (`{"method":..,"params":..}`).
    pub async fn read_message(&mut self) -> anyhow::Result<InboundMessage> {
        let mut line = String::new();
        let bytes_read = self
            .reader
            .read_line(&mut line)
            .await
            .context("failed to read from stdin")?;

        if bytes_read == 0 {
            anyhow::bail!("stdin closed (EOF)");
        }

        let line = line.trim();

        // Parse as ClientRequest (handles both JSON-RPC and legacy formats
        // since ClientRequest uses tag="method" which matches both)
        let request: ClientRequest =
            serde_json::from_str(line).context("failed to parse client request")?;

        match request {
            ClientRequest::SessionStart(params) => Ok(InboundMessage::SessionStart(params)),
            ClientRequest::TurnStart(params) => Ok(InboundMessage::TurnStart(params)),
            ClientRequest::TurnInterrupt(params) => Ok(InboundMessage::TurnInterrupt(params)),
            ClientRequest::ApprovalResolve(params) => Ok(InboundMessage::ApprovalResolve(params)),
            ClientRequest::UserInputResolve(params) => Ok(InboundMessage::UserInputResolve(params)),
            ClientRequest::SetModel(params) => Ok(InboundMessage::SetModel(params)),
            ClientRequest::SetPermissionMode(params) => {
                Ok(InboundMessage::SetPermissionMode(params))
            }
            ClientRequest::StopTask(params) => Ok(InboundMessage::StopTask(params)),
            ClientRequest::HookCallbackResponse(params) => {
                Ok(InboundMessage::HookCallbackResponse(params))
            }
            ClientRequest::SessionResume(params) => Ok(InboundMessage::SessionResume(params)),
            ClientRequest::SetThinking(params) => Ok(InboundMessage::SetThinking(params)),
            ClientRequest::RewindFiles(params) => Ok(InboundMessage::RewindFiles(params)),
            ClientRequest::UpdateEnv(params) => Ok(InboundMessage::UpdateEnv(params)),
            ClientRequest::KeepAlive(params) => Ok(InboundMessage::KeepAlive(params)),
        }
    }
}

/// Writes server messages to stdout as NDJSON using JSON-RPC format.
pub struct StdoutWriter {
    stdout: tokio::io::Stdout,
    request_counter: i64,
}

impl StdoutWriter {
    pub fn new() -> Self {
        Self {
            stdout: tokio::io::stdout(),
            request_counter: 0,
        }
    }

    /// Write a `ServerNotification` as a JSON-RPC notification (no `id`).
    pub async fn write_notification(&mut self, notif: &ServerNotification) -> anyhow::Result<()> {
        let value = serde_json::to_value(notif).context("failed to serialize notification")?;
        let rpc = JsonRpcNotification {
            method: value["method"].as_str().unwrap_or("unknown").to_string(),
            params: value.get("params").cloned(),
        };
        self.write_json(&rpc).await
    }

    /// Write a `ServerRequest` as a JSON-RPC request (with `id`).
    pub async fn write_server_request(&mut self, req: &ServerRequest) -> anyhow::Result<()> {
        self.request_counter += 1;
        let value = serde_json::to_value(req).context("failed to serialize server request")?;
        let rpc = JsonRpcRequest {
            id: RequestId::Integer(self.request_counter),
            method: value["method"].as_str().unwrap_or("unknown").to_string(),
            params: value.get("params").cloned(),
        };
        self.write_json(&rpc).await
    }

    /// Write a JSON-RPC response (for client-initiated requests).
    #[allow(dead_code)]
    pub async fn write_response(
        &mut self,
        id: RequestId,
        result: serde_json::Value,
    ) -> anyhow::Result<()> {
        let rpc = JsonRpcResponse { id, result };
        self.write_json(&rpc).await
    }

    async fn write_json(&mut self, value: &impl serde::Serialize) -> anyhow::Result<()> {
        let json = serde_json::to_string(value).context("failed to serialize message")?;
        self.stdout
            .write_all(json.as_bytes())
            .await
            .context("failed to write to stdout")?;
        self.stdout
            .write_all(b"\n")
            .await
            .context("failed to write newline")?;
        self.stdout
            .flush()
            .await
            .context("failed to flush stdout")?;
        Ok(())
    }
}
