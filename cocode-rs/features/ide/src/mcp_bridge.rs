//! IDE MCP bridge.
//!
//! Wraps `RmcpClient` to connect to an IDE extension's MCP server,
//! providing typed methods for IDE-specific tool calls and notifications.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cocode_rmcp_client::ElicitationResponse;
use cocode_rmcp_client::RmcpClient;
use futures::FutureExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::debug;
use tracing::warn;

use crate::detection::IdeType;
use crate::error::Error;
use crate::error::Result;
use crate::lockfile::ResolvedLockfile;

/// Default timeout for IDE tool calls.
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for diff resolution (user interaction).
const DIFF_TIMEOUT: Duration = Duration::from_secs(60);

/// Connection status of the IDE MCP bridge.
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    /// Not connected.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Connection failed.
    Failed { error: String, retry_count: i32 },
}

/// Resolution from an IDE diff operation.
#[derive(Debug, Clone)]
pub enum DiffResolution {
    /// User saved the edited version in the IDE.
    FileSaved { content: String },
    /// User closed the diff tab (treated as acceptance).
    TabClosed,
    /// User explicitly rejected the diff.
    DiffRejected,
}

/// Maximum consecutive errors before marking connection as failed.
/// Matches Claude Code's `N = 3` threshold.
const MAX_CONSECUTIVE_ERRORS: i32 = 3;

/// IDE extension MCP tool names (defined by the IDE extension protocol).
mod ide_tools {
    pub const OPEN_DIFF: &str = "openDiff";
    pub const CLOSE_ALL_DIFF_TABS: &str = "closeAllDiffTabs";
    pub const GET_DIAGNOSTICS: &str = "getDiagnostics";
    pub const GET_WORKSPACE_FOLDERS: &str = "getWorkspaceFolders";
}

/// IDE extension diff resolution status strings.
mod diff_status {
    pub const FILE_SAVED: &str = "FILE_SAVED";
    pub const TAB_CLOSED: &str = "TAB_CLOSED";
    pub const DIFF_REJECTED: &str = "DIFF_REJECTED";
}

/// IDE MCP bridge wrapping `RmcpClient` for IDE-specific communication.
///
/// # Notification handling
///
/// The IDE sends `selection_changed` notifications via MCP. These are handled
/// by the `RmcpClient`'s notification handler internally. The caller (typically
/// `IdeContext`) should call `selection.update_from_notification()` when
/// receiving these notifications through the session's event loop.
pub struct IdeMcpBridge {
    client: RmcpClient,
    ide_type: &'static IdeType,
    status: Arc<RwLock<ConnectionStatus>>,
    /// Consecutive error counter for connection health tracking (lock-free).
    consecutive_errors: AtomicI32,
}

impl std::fmt::Debug for IdeMcpBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdeMcpBridge")
            .field("ide_type", &self.ide_type.display_name)
            .finish()
    }
}

impl IdeMcpBridge {
    /// Create and connect to an IDE MCP server from a resolved lockfile.
    pub(crate) async fn connect(lockfile: &ResolvedLockfile) -> Result<Self> {
        let url = lockfile.mcp_url();
        debug!(
            "Connecting to IDE MCP server at {url} ({})",
            lockfile.ide_type.display_name
        );

        let mut http_headers = HashMap::new();
        if !lockfile.lockfile.auth_token.is_empty() {
            http_headers.insert(
                "X-Claude-Code-Ide-Authorization".to_string(),
                lockfile.lockfile.auth_token.clone(),
            );
        }

        let headers = if http_headers.is_empty() {
            None
        } else {
            Some(http_headers)
        };

        let cocode_home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".cocode");

        let client = RmcpClient::new_streamable_http_client(
            "ide",
            &url,
            /*bearer_token*/ None,
            headers,
            /*env_http_headers*/ None,
            cocode_rmcp_client::OAuthCredentialsStoreMode::Auto,
            cocode_home,
        )
        .await
        .map_err(|e| Error::Connection {
            message: format!("failed to create MCP client: {e}"),
        })?;

        let bridge = Self {
            client,
            ide_type: lockfile.ide_type,
            status: Arc::new(RwLock::new(ConnectionStatus::Connecting)),
            consecutive_errors: AtomicI32::new(0),
        };

        Ok(bridge)
    }

    /// Perform the MCP initialization handshake and send `ide_connected`.
    pub async fn initialize(&self) -> Result<()> {
        use cocode_mcp_types::ClientCapabilities;
        use cocode_mcp_types::Implementation;
        use cocode_mcp_types::InitializeRequestParams;

        let params = InitializeRequestParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities {
                elicitation: None,
                experimental: None,
                roots: None,
                sampling: None,
            },
            client_info: Implementation {
                name: "cocode".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                user_agent: None,
            },
        };

        // No-op elicitation handler (IDE MCP doesn't need OAuth elicitation)
        let noop_elicitation: cocode_rmcp_client::SendElicitation = Box::new(|_id, _el| {
            async {
                Ok(ElicitationResponse {
                    action: cocode_rmcp_client::ElicitationAction::Decline,
                    content: None,
                })
            }
            .boxed()
        });

        self.client
            .initialize(params, Some(Duration::from_secs(10)), noop_elicitation)
            .await
            .map_err(|e| Error::Connection {
                message: format!("MCP initialize failed: {e}"),
            })?;

        // Send ide_connected notification
        self.client
            .send_custom_notification(
                "ide_connected",
                Some(json!({
                    "pid": std::process::id()
                })),
            )
            .await
            .map_err(|e| Error::Connection {
                message: format!("failed to send ide_connected: {e}"),
            })?;

        {
            let mut status = self.status.write().await;
            *status = ConnectionStatus::Connected;
        }

        debug!(
            "IDE MCP connection established ({})",
            self.ide_type.display_name
        );
        Ok(())
    }

    /// Whether the bridge is currently connected.
    pub async fn is_connected(&self) -> bool {
        matches!(*self.status.read().await, ConnectionStatus::Connected)
    }

    /// The IDE type this bridge is connected to.
    pub fn ide_type(&self) -> &'static IdeType {
        self.ide_type
    }

    /// Clean up IDE resources on session shutdown.
    ///
    /// Closes all open diff tabs and marks the connection as disconnected.
    pub async fn shutdown(&self) {
        let _ = self.close_all_diff_tabs().await;
        let mut status = self.status.write().await;
        *status = ConnectionStatus::Disconnected;
    }

    // ========================================================================
    // IDE tool methods (internal, not exposed to AI model)
    // ========================================================================

    /// Open a diff view in the IDE.
    ///
    /// Returns the user's resolution: accept, reject, or tab closed.
    pub async fn open_diff(
        &self,
        old_file_path: &Path,
        new_file_path: &Path,
        new_content: &str,
        tab_name: &str,
    ) -> Result<DiffResolution> {
        let args = json!({
            "old_file_path": old_file_path.to_string_lossy(),
            "new_file_path": new_file_path.to_string_lossy(),
            "new_file_contents": new_content,
            "tab_name": tab_name,
        });

        // openDiff returns multiple text content blocks:
        // [TextContent("FILE_SAVED"), TextContent("<edited content>")]
        // or [TextContent("TAB_CLOSED")] or [TextContent("DIFF_REJECTED")]
        let texts = self
            .call_tool_texts(ide_tools::OPEN_DIFF, args, Some(DIFF_TIMEOUT))
            .await?;

        parse_diff_resolution(&texts)
    }

    /// Close all open diff tabs.
    pub async fn close_all_diff_tabs(&self) -> Result<()> {
        let _ = self
            .call_tool(
                ide_tools::CLOSE_ALL_DIFF_TABS,
                json!({}),
                Some(TOOL_TIMEOUT),
            )
            .await;
        Ok(())
    }

    /// Get LSP diagnostics from the IDE.
    pub(crate) async fn get_diagnostics(&self, uri: Option<&str>) -> Result<Vec<IdeDiagnosticRaw>> {
        let args = match uri {
            Some(u) => json!({ "uri": u }),
            None => json!({}),
        };

        let result = self
            .call_tool(ide_tools::GET_DIAGNOSTICS, args, Some(TOOL_TIMEOUT))
            .await?;

        serde_json::from_value(result).map_err(|e| Error::ToolCall {
            tool: ide_tools::GET_DIAGNOSTICS.into(),
            message: format!("failed to parse response: {e}"),
        })
    }

    /// Get workspace folders from the IDE.
    pub async fn get_workspace_folders(&self) -> Result<Vec<PathBuf>> {
        let result = self
            .call_tool(
                ide_tools::GET_WORKSPACE_FOLDERS,
                json!({}),
                Some(TOOL_TIMEOUT),
            )
            .await?;

        #[derive(Deserialize)]
        struct FoldersResponse {
            #[serde(default)]
            folders: Vec<String>,
        }

        let response: FoldersResponse =
            serde_json::from_value(result).map_err(|e| Error::ToolCall {
                tool: ide_tools::GET_WORKSPACE_FOLDERS.into(),
                message: format!("failed to parse response: {e}"),
            })?;

        Ok(response.folders.into_iter().map(PathBuf::from).collect())
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Call an IDE MCP tool and return the first text content as JSON.
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        timeout: Option<Duration>,
    ) -> Result<serde_json::Value> {
        let texts = self.call_tool_texts(name, arguments, timeout).await?;

        if let Some(first) = texts.first() {
            return serde_json::from_str(first).or_else(|parse_err| {
                debug!("IDE tool response is not JSON, treating as string: {parse_err}");
                Ok(json!(first))
            });
        }

        Ok(json!(null))
    }

    /// Call an IDE MCP tool and return all text content blocks.
    ///
    /// MCP tool results are a list of `ContentBlock`. This extracts the `text`
    /// field from each `TextContent` block, preserving order.
    ///
    /// Tracks consecutive errors: after [`MAX_CONSECUTIVE_ERRORS`] failures,
    /// marks the connection as [`ConnectionStatus::Failed`].
    async fn call_tool_texts(
        &self,
        name: &str,
        arguments: serde_json::Value,
        timeout: Option<Duration>,
    ) -> Result<Vec<String>> {
        let result = self
            .client
            .call_tool(name.to_string(), Some(arguments), timeout)
            .await;

        match result {
            Ok(result) => {
                self.consecutive_errors.store(0, Ordering::Relaxed);

                let texts = result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        cocode_mcp_types::ContentBlock::TextContent(tc) => Some(tc.text.clone()),
                        _ => None,
                    })
                    .collect();

                Ok(texts)
            }
            Err(e) => {
                let errors = self.consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;

                if errors >= MAX_CONSECUTIVE_ERRORS {
                    warn!("IDE tool '{name}' failed {errors} times, marking disconnected: {e}");
                    let mut status = self.status.write().await;
                    *status = ConnectionStatus::Failed {
                        error: e.to_string(),
                        retry_count: errors,
                    };
                } else {
                    warn!("IDE tool call '{name}' failed ({errors}/{MAX_CONSECUTIVE_ERRORS}): {e}");
                }

                Err(Error::ToolCall {
                    tool: name.into(),
                    message: e.to_string(),
                })
            }
        }
    }
}

/// Raw diagnostic from the IDE's getDiagnostics response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdeDiagnosticRaw {
    /// Diagnostic message.
    pub message: String,
    /// Severity (1=Error, 2=Warning, 3=Information, 4=Hint).
    #[serde(default = "default_severity")]
    pub severity: i32,
    /// Source server name.
    #[serde(default)]
    pub source: Option<String>,
    /// Diagnostic code.
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    /// Range start.
    #[serde(default)]
    pub range: DiagnosticRange,
}

fn default_severity() -> i32 {
    1
}

/// Diagnostic range.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DiagnosticRange {
    /// Start position.
    #[serde(default)]
    pub start: DiagnosticPosition,
    /// End position.
    #[serde(default)]
    pub end: DiagnosticPosition,
}

/// Diagnostic position (line/character).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DiagnosticPosition {
    /// 0-based line number.
    #[serde(default)]
    pub line: i32,
    /// 0-based character offset.
    #[serde(default)]
    pub character: i32,
}

/// Parse diff resolution from openDiff MCP text content blocks.
///
/// Claude Code's IDE extension responds with:
/// - `["FILE_SAVED", "<edited content>"]` — user accepted
/// - `["TAB_CLOSED"]` — user closed tab (treated as accept)
/// - `["DIFF_REJECTED"]` — user rejected
fn parse_diff_resolution(texts: &[String]) -> Result<DiffResolution> {
    let status = texts.first().map(String::as_str).unwrap_or("");

    match status {
        diff_status::FILE_SAVED => {
            let content = texts.get(1).cloned().unwrap_or_default();
            Ok(DiffResolution::FileSaved { content })
        }
        diff_status::TAB_CLOSED => Ok(DiffResolution::TabClosed),
        diff_status::DIFF_REJECTED => Ok(DiffResolution::DiffRejected),
        other => Err(Error::ToolCall {
            tool: ide_tools::OPEN_DIFF.into(),
            message: format!("unexpected resolution: {other}"),
        }),
    }
}

#[cfg(test)]
#[path = "mcp_bridge.test.rs"]
mod tests;
