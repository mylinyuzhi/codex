//! NDJSON transport over stdin/stdout.
//!
//! Each line is a complete JSON object. Stdout is used for server→client
//! messages; stdin for client→server messages.

use anyhow::Context;
use cocode_app_server_protocol::ApprovalResolveRequestParams;
use cocode_app_server_protocol::ClientRequest;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::ServerRequest;
use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_app_server_protocol::TurnInterruptRequestParams;
use cocode_app_server_protocol::TurnStartRequestParams;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

/// Parsed inbound message from stdin.
#[derive(Debug)]
pub enum InboundMessage {
    SessionStart(SessionStartRequestParams),
    TurnStart(TurnStartRequestParams),
    TurnInterrupt(#[allow(dead_code)] TurnInterruptRequestParams),
    ApprovalResolve(ApprovalResolveRequestParams),
}

/// NDJSON transport reading from stdin and writing to stdout.
pub struct NdjsonTransport {
    reader: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl NdjsonTransport {
    /// Create a new transport using process stdin/stdout.
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    /// Read the next JSON line from stdin and parse it as a `ClientRequest`.
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
        let request: ClientRequest =
            serde_json::from_str(line).context("failed to parse client request")?;

        match request {
            ClientRequest::SessionStart(params) => Ok(InboundMessage::SessionStart(params)),
            ClientRequest::TurnStart(params) => Ok(InboundMessage::TurnStart(params)),
            ClientRequest::TurnInterrupt(params) => Ok(InboundMessage::TurnInterrupt(params)),
            ClientRequest::ApprovalResolve(params) => Ok(InboundMessage::ApprovalResolve(params)),
            ClientRequest::SessionResume(_) => {
                anyhow::bail!("session/resume not yet supported in SDK mode")
            }
        }
    }

    /// Write a `ServerNotification` as a single NDJSON line to stdout.
    pub async fn write_notification(&mut self, notif: &ServerNotification) -> anyhow::Result<()> {
        let json = serde_json::to_string(notif).context("failed to serialize notification")?;
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

    /// Write a `ServerRequest` as a single NDJSON line to stdout.
    pub async fn write_server_request(&mut self, req: &ServerRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(req).context("failed to serialize server request")?;
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
