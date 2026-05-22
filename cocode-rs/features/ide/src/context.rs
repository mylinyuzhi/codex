//! IDE context aggregate.
//!
//! Combines all IDE state into a single struct that is attached to
//! `SessionState` and threaded through the agent loop.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::diagnostics_manager::IdeDiagnosticsManager;
use crate::diff_handler::IdeDiffHandler;
use crate::lockfile::discover_ide_lockfile;
use crate::mcp_bridge::IdeMcpBridge;
use crate::selection::IdeSelectionState;

/// Aggregate IDE context holding all IDE integration state.
///
/// Created once per session when an IDE is detected and `Feature::Ide`
/// is enabled. Stored in `SessionState` as `Option<Arc<IdeContext>>`.
///
/// # Notification delivery (codex-rs pattern)
///
/// IDE events fall into two categories for the session's event loop:
/// - **Critical** (must be delivered): diff resolution responses
///   (FILE_SAVED, DIFF_REJECTED) — use `.send().await` in channels
/// - **Optional** (may be dropped under backpressure): `selection_changed`,
///   diagnostic updates — use `.try_send()` in channels
#[derive(Debug)]
pub struct IdeContext {
    /// MCP bridge for communicating with the IDE.
    pub bridge: Arc<IdeMcpBridge>,
    /// Current text selection in the IDE.
    pub selection: IdeSelectionState,
    /// Diagnostic baseline/delta manager.
    pub diagnostics_manager: IdeDiagnosticsManager,
    /// IDE diff handler for routing edits to IDE preview.
    pub diff_handler: IdeDiffHandler,
    /// Workspace folders reported by the IDE.
    pub workspace_folders: Arc<RwLock<Vec<PathBuf>>>,
}

impl IdeContext {
    /// Attempt to discover and connect to an IDE.
    ///
    /// Returns `None` if no IDE is detected or connection fails.
    pub async fn try_connect(cwd: &Path) -> Option<Arc<Self>> {
        // Discover IDE lockfile
        let lockfile = match discover_ide_lockfile(cwd).await {
            Some(lf) => lf,
            None => {
                debug!("No IDE lockfile found for {}", cwd.display());
                return None;
            }
        };

        info!(
            "Found IDE: {} (port {}, transport {})",
            lockfile.ide_type.display_name,
            lockfile.port,
            if lockfile.is_websocket() { "ws" } else { "sse" }
        );

        // Connect to IDE MCP server
        let bridge = match IdeMcpBridge::connect(&lockfile).await {
            Ok(b) => Arc::new(b),
            Err(e) => {
                warn!("Failed to connect to IDE MCP server: {e}");
                return None;
            }
        };

        // Initialize MCP handshake
        if let Err(e) = bridge.initialize().await {
            warn!("IDE MCP initialization failed: {e}");
            return None;
        }

        let diff_handler = IdeDiffHandler::new(Arc::clone(&bridge));

        // Fetch initial workspace folders
        let workspace_folders = match bridge.get_workspace_folders().await {
            Ok(folders) => folders,
            Err(e) => {
                warn!("Failed to fetch workspace folders from IDE, using lockfile: {e}");
                lockfile
                    .lockfile
                    .workspace_folders
                    .iter()
                    .map(PathBuf::from)
                    .collect()
            }
        };

        info!(
            "IDE integration active: {} ({} workspace folders)",
            bridge.ide_type().display_name,
            workspace_folders.len()
        );

        Some(Arc::new(Self {
            bridge,
            selection: IdeSelectionState::new(),
            diagnostics_manager: IdeDiagnosticsManager::new(),
            diff_handler,
            workspace_folders: Arc::new(RwLock::new(workspace_folders)),
        }))
    }

    /// Whether the IDE connection is currently active.
    pub async fn is_connected(&self) -> bool {
        self.bridge.is_connected().await
    }

    /// Get the IDE display name.
    pub fn ide_name(&self) -> &str {
        self.bridge.ide_type().display_name
    }

    /// Clean up IDE resources on session shutdown.
    ///
    /// Closes all open diff tabs and marks the bridge as disconnected.
    /// Should be called when the session ends.
    pub async fn shutdown(&self) {
        info!(
            "Shutting down IDE integration ({})",
            self.bridge.ide_type().display_name
        );
        self.bridge.shutdown().await;
    }
}
