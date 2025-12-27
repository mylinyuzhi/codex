//! LSP client for communicating with a single language server

use crate::diagnostics::DiagnosticsStore;
use crate::error::LspErr;
use crate::error::Result;
use crate::protocol::JsonRpcConnection;
use crate::protocol::TimeoutConfig;
use crate::symbols::ResolvedSymbol;
use crate::symbols::SymbolKind;
use crate::symbols::find_matching_symbols;
use crate::symbols::flatten_symbols;
use lsp_types::CallHierarchyIncomingCall;
use lsp_types::CallHierarchyIncomingCallsParams;
use lsp_types::CallHierarchyItem;
use lsp_types::CallHierarchyOutgoingCall;
use lsp_types::CallHierarchyOutgoingCallsParams;
use lsp_types::CallHierarchyPrepareParams;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::DocumentSymbolParams;
use lsp_types::DocumentSymbolResponse;
use lsp_types::GotoDefinitionParams;
use lsp_types::GotoDefinitionResponse;
use lsp_types::Hover;
use lsp_types::HoverParams;
use lsp_types::InitializeParams;
use lsp_types::InitializeResult;
use lsp_types::Location;
use lsp_types::PartialResultParams;
use lsp_types::Position;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::ReferenceContext;
use lsp_types::ReferenceParams;
use lsp_types::SymbolInformation;
use lsp_types::TextDocumentContentChangeEvent;
use lsp_types::TextDocumentIdentifier;
use lsp_types::TextDocumentItem;
use lsp_types::TextDocumentPositionParams;
use lsp_types::Url;
use lsp_types::VersionedTextDocumentIdentifier;
use lsp_types::WorkDoneProgressParams;
use lsp_types::WorkspaceSymbolParams;
use lsp_types::WorkspaceSymbolResponse;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::info;
use tracing::trace;
use tracing::warn;

/// Maximum number of files to track as opened (prevents unbounded memory growth)
const MAX_OPENED_FILES: usize = 500;

/// Health check timeout in seconds
const HEALTH_CHECK_TIMEOUT_SECS: i32 = 5;

/// Cached server capabilities from initialize response
#[derive(Debug, Default, Clone)]
pub struct CachedCapabilities {
    /// Server supports textDocument/implementation
    pub supports_implementation: bool,
    /// Server supports textDocument/typeDefinition
    pub supports_type_definition: bool,
    /// Server supports textDocument/declaration
    pub supports_declaration: bool,
    /// Server supports call hierarchy (prepare, incoming, outgoing)
    pub supports_call_hierarchy: bool,
    /// Server supports workspace/symbol
    pub supports_workspace_symbol: bool,
}

/// Percentage of files to evict when cache is full (25%)
const LRU_EVICTION_PERCENT: usize = 25;

/// LSP client for a single language server
pub struct LspClient {
    connection: Arc<JsonRpcConnection>,
    #[allow(dead_code)]
    diagnostics: Arc<DiagnosticsStore>,
    server_id: String,
    root_uri: Url,
    /// Tracks files that have been opened with textDocument/didOpen
    opened_files: Arc<Mutex<HashSet<PathBuf>>>,
    /// Tracks file versions for textDocument/didChange
    file_versions: Arc<Mutex<HashMap<PathBuf, i32>>>,
    /// Tracks last access time for LRU eviction
    file_access: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    /// Handle to notification handler task for cleanup
    notification_handle: Mutex<Option<JoinHandle<()>>>,
    /// Timeout configuration
    timeout_config: TimeoutConfig,
    /// Server capabilities cached from initialize response
    capabilities: Mutex<CachedCapabilities>,
}

impl LspClient {
    /// Create a new LSP client from stdio streams
    pub async fn new(
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
        server_id: String,
        root_path: &Path,
        diagnostics: Arc<DiagnosticsStore>,
        initialization_options: Option<serde_json::Value>,
        settings: Option<serde_json::Value>,
        timeout_config: TimeoutConfig,
    ) -> Result<Self> {
        info!(
            "Creating LSP client for {} at {}",
            server_id,
            root_path.display()
        );

        let (notification_tx, notification_rx) = mpsc::channel(100);

        let connection = Arc::new(JsonRpcConnection::new(
            stdin,
            stdout,
            notification_tx.clone(),
        ));

        let root_uri = Url::from_file_path(root_path)
            .map_err(|_| LspErr::Internal(format!("invalid root path: {}", root_path.display())))?;

        // Spawn notification handler
        let diag_store = Arc::clone(&diagnostics);
        let notification_rx = Arc::new(Mutex::new(notification_rx));
        let notification_rx_clone = Arc::clone(&notification_rx);
        let notification_handle = tokio::spawn(async move {
            Self::handle_notifications(notification_rx_clone, diag_store).await;
        });

        let client = Self {
            connection,
            diagnostics: Arc::clone(&diagnostics),
            server_id,
            root_uri: root_uri.clone(),
            opened_files: Arc::new(Mutex::new(HashSet::new())),
            file_versions: Arc::new(Mutex::new(HashMap::new())),
            file_access: Arc::new(Mutex::new(HashMap::new())),
            notification_handle: Mutex::new(Some(notification_handle)),
            timeout_config,
            capabilities: Mutex::new(CachedCapabilities::default()),
        };

        // Initialize the server with configurable timeout
        client.initialize(root_uri, initialization_options).await?;

        // Send workspace settings if provided
        if let Some(ref settings) = settings {
            client.send_configuration(settings).await?;
        }

        Ok(client)
    }

    /// Send workspace settings via workspace/didChangeConfiguration
    pub async fn send_configuration(&self, settings: &serde_json::Value) -> Result<()> {
        if settings.is_null() {
            return Ok(());
        }

        let params = lsp_types::DidChangeConfigurationParams {
            settings: settings.clone(),
        };

        self.connection
            .notify("workspace/didChangeConfiguration", params)
            .await?;

        debug!(
            "Sent workspace/didChangeConfiguration to {}",
            self.server_id
        );
        Ok(())
    }

    /// Lightweight health check with fallback methods
    ///
    /// Attempts to verify the server is still responsive using multiple methods:
    /// 1. workspace/symbol with empty query (widely supported)
    /// 2. textDocument/hover on any opened file (fallback)
    ///
    /// Returns true if the server responds (even with an error), false if timeout
    /// or connection is lost.
    pub async fn health_check(&self) -> bool {
        // Method 1: Try workspace/symbol (widely supported)
        let params = lsp_types::WorkspaceSymbolParams {
            query: String::new(),
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        };

        match self
            .connection
            .request_with_timeout("workspace/symbol", params, HEALTH_CHECK_TIMEOUT_SECS)
            .await
        {
            Ok(_) => {
                info!(
                    "LSP {} health check passed (workspace/symbol)",
                    self.server_id
                );
                return true;
            }
            Err(LspErr::JsonRpc { code, .. }) => {
                // Server responded with an error, but it's still alive
                if matches!(code, Some(-32601) | Some(-32602)) {
                    info!(
                        "LSP {} health check passed (method not supported but alive)",
                        self.server_id
                    );
                    return true;
                }
                // Other JSON-RPC errors also mean the server is responsive
                info!(
                    "LSP {} health check passed (JSON-RPC error code {:?})",
                    self.server_id, code
                );
                return true;
            }
            Err(LspErr::RequestTimeout { .. }) | Err(LspErr::ConnectionClosed) => {
                // Primary method failed, try fallback
                debug!(
                    "LSP {} workspace/symbol health check failed, trying fallback",
                    self.server_id
                );
            }
            Err(_) => {
                // Other errors mean server is responsive but had an issue
                return true;
            }
        }

        // Method 2: Fallback - try hover if we have an open file
        if let Some(opened_file) = self.get_any_opened_file().await {
            let uri = match Url::from_file_path(&opened_file) {
                Ok(u) => u,
                Err(_) => {
                    warn!("LSP {} health check failed (all methods)", self.server_id);
                    return false;
                }
            };

            let params = lsp_types::HoverParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: lsp_types::TextDocumentIdentifier { uri },
                    position: lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                },
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            };

            match self
                .connection
                .request_with_timeout("textDocument/hover", params, HEALTH_CHECK_TIMEOUT_SECS)
                .await
            {
                Ok(_) | Err(LspErr::JsonRpc { .. }) => {
                    info!(
                        "LSP {} health check passed (hover fallback)",
                        self.server_id
                    );
                    return true;
                }
                _ => {}
            }
        }

        warn!("LSP {} health check failed (all methods)", self.server_id);
        false
    }

    /// Get any opened file for fallback health check
    async fn get_any_opened_file(&self) -> Option<PathBuf> {
        let opened = self.opened_files.lock().await;
        opened.iter().next().cloned()
    }

    /// Get the timeout configuration
    pub fn timeout_config(&self) -> &TimeoutConfig {
        &self.timeout_config
    }

    /// Initialize the language server
    #[allow(deprecated)] // root_uri is deprecated but still widely supported
    async fn initialize(
        &self,
        root_uri: Url,
        initialization_options: Option<serde_json::Value>,
    ) -> Result<()> {
        let params = InitializeParams {
            root_uri: Some(root_uri),
            initialization_options,
            capabilities: lsp_types::ClientCapabilities {
                text_document: Some(lsp_types::TextDocumentClientCapabilities {
                    document_symbol: Some(lsp_types::DocumentSymbolClientCapabilities {
                        hierarchical_document_symbol_support: Some(true),
                        ..Default::default()
                    }),
                    definition: Some(lsp_types::GotoCapability {
                        link_support: Some(false),
                        ..Default::default()
                    }),
                    references: Some(lsp_types::DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    hover: Some(lsp_types::HoverClientCapabilities {
                        content_format: Some(vec![lsp_types::MarkupKind::PlainText]),
                        ..Default::default()
                    }),
                    publish_diagnostics: Some(
                        lsp_types::PublishDiagnosticsClientCapabilities::default(),
                    ),
                    implementation: Some(lsp_types::GotoCapability {
                        link_support: Some(false),
                        ..Default::default()
                    }),
                    type_definition: Some(lsp_types::GotoCapability {
                        link_support: Some(false),
                        ..Default::default()
                    }),
                    declaration: Some(lsp_types::GotoCapability {
                        link_support: Some(false),
                        ..Default::default()
                    }),
                    call_hierarchy: Some(lsp_types::CallHierarchyClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    ..Default::default()
                }),
                workspace: Some(lsp_types::WorkspaceClientCapabilities {
                    symbol: Some(lsp_types::WorkspaceSymbolClientCapabilities {
                        dynamic_registration: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        // Use configurable timeout
        let init_timeout_secs = self.timeout_config.init_timeout_secs();
        let result: InitializeResult = self
            .connection
            .request_with_timeout("initialize", params, init_timeout_secs)
            .await
            .and_then(|v| serde_json::from_value(v).map_err(Into::into))?;

        // Cache server capabilities
        let caps = {
            let mut caps = self.capabilities.lock().await;
            let server_caps = &result.capabilities;
            caps.supports_implementation = server_caps.implementation_provider.is_some();
            caps.supports_type_definition = server_caps.type_definition_provider.is_some();
            caps.supports_declaration = server_caps.declaration_provider.is_some();
            caps.supports_call_hierarchy = server_caps.call_hierarchy_provider.is_some();
            caps.supports_workspace_symbol = server_caps.workspace_symbol_provider.is_some();
            caps.clone()
        };

        info!(
            "LSP {} initialized: server={:?}, capabilities=[implementation={}, type_definition={}, declaration={}, call_hierarchy={}, workspace_symbol={}]",
            self.server_id,
            result.server_info.as_ref().map(|s| &s.name),
            caps.supports_implementation,
            caps.supports_type_definition,
            caps.supports_declaration,
            caps.supports_call_hierarchy,
            caps.supports_workspace_symbol
        );

        // Send initialized notification
        self.connection
            .notify("initialized", serde_json::json!({}))
            .await?;

        Ok(())
    }

    /// Handle incoming notifications
    async fn handle_notifications(
        notification_rx: Arc<Mutex<mpsc::Receiver<(String, serde_json::Value)>>>,
        diagnostics: Arc<DiagnosticsStore>,
    ) {
        let mut rx = notification_rx.lock().await;
        while let Some((method, params)) = rx.recv().await {
            match method.as_str() {
                "textDocument/publishDiagnostics" => {
                    if let Ok(diag_params) =
                        serde_json::from_value::<PublishDiagnosticsParams>(params)
                    {
                        info!(
                            "Received {} diagnostics for {}",
                            diag_params.diagnostics.len(),
                            diag_params.uri
                        );
                        diagnostics.update(diag_params).await;
                    }
                }
                "window/showMessage" => {
                    if let Ok(msg_params) =
                        serde_json::from_value::<lsp_types::ShowMessageParams>(params)
                    {
                        let level = match msg_params.typ {
                            lsp_types::MessageType::ERROR => "error",
                            lsp_types::MessageType::WARNING => "warn",
                            lsp_types::MessageType::INFO => "info",
                            lsp_types::MessageType::LOG => "debug",
                            _ => "trace",
                        };
                        info!("LSP server message [{}]: {}", level, msg_params.message);
                    }
                }
                "window/logMessage" => {
                    if let Ok(log_params) =
                        serde_json::from_value::<lsp_types::LogMessageParams>(params)
                    {
                        debug!("LSP server log: {}", log_params.message);
                    }
                }
                "$/progress" => {
                    trace!("LSP progress notification received");
                }
                _ => {
                    trace!("Unhandled notification: {}", method);
                }
            }
        }
    }

    /// Sync a file to the server (textDocument/didOpen)
    pub async fn sync_file(&self, path: &Path) -> Result<()> {
        let path = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    "Failed to canonicalize path {}: {}, using original path",
                    path.display(),
                    e
                );
                path.to_path_buf()
            }
        };

        // Check if already opened - if so, just update access time
        {
            let opened = self.opened_files.lock().await;
            if opened.contains(&path) {
                // Update access time for LRU tracking
                let mut access = self.file_access.lock().await;
                access.insert(path, Instant::now());
                return Ok(());
            }
        }

        // Check if we need to evict files before opening a new one
        if self.opened_files.lock().await.len() >= MAX_OPENED_FILES {
            self.evict_lru_files().await;
        }

        // Read file content
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|_| LspErr::FileNotFound {
                path: path.display().to_string(),
            })?;

        let uri = Url::from_file_path(&path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        // Detect language ID from extension
        let language_id = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext {
                "rs" => "rust",
                "go" => "go",
                "py" | "pyi" => "python",
                _ => "plaintext",
            })
            .unwrap_or("plaintext")
            .to_string();

        let language_id_for_log = language_id.clone();
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id,
                version: 1,
                text: content,
            },
        };

        self.connection
            .notify("textDocument/didOpen", params)
            .await?;

        info!(
            "Opened file in LSP {}: {} (language: {})",
            self.server_id,
            path.display(),
            language_id_for_log
        );

        // Mark as opened and track access time
        let now = Instant::now();
        {
            let mut opened = self.opened_files.lock().await;
            opened.insert(path.clone());
        }
        {
            let mut access = self.file_access.lock().await;
            access.insert(path.clone(), now);
        }
        {
            // Initialize version tracking
            let mut versions = self.file_versions.lock().await;
            versions.insert(path, 1);
        }

        Ok(())
    }

    /// Close a file (textDocument/didClose)
    ///
    /// Sends didClose notification to the server and removes from tracking.
    async fn close_file(&self, path: &PathBuf) -> Result<()> {
        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        };

        self.connection
            .notify("textDocument/didClose", params)
            .await?;

        trace!("Closed file: {}", path.display());
        Ok(())
    }

    /// Close all opened files and clear tracking
    async fn close_all_files(&self) {
        let paths: Vec<PathBuf> = {
            let opened = self.opened_files.lock().await;
            opened.iter().cloned().collect()
        };

        for path in &paths {
            if let Err(e) = self.close_file(path).await {
                debug!("Failed to close file {}: {}", path.display(), e);
            }
        }

        // Clear all tracking
        self.opened_files.lock().await.clear();
        self.file_versions.lock().await.clear();
        self.file_access.lock().await.clear();
    }

    /// Evict LRU_EVICTION_PERCENT of oldest files to make room for new ones
    async fn evict_lru_files(&self) {
        let access = self.file_access.lock().await;
        let mut files_by_access: Vec<_> = access.iter().collect();

        // Sort by access time (oldest first)
        files_by_access.sort_by(|a, b| a.1.cmp(b.1));

        // Calculate number of files to evict (25% of max)
        let evict_count = (MAX_OPENED_FILES * LRU_EVICTION_PERCENT) / 100;
        let evict_count = evict_count.max(1); // Evict at least 1

        let to_evict: Vec<PathBuf> = files_by_access
            .iter()
            .take(evict_count)
            .map(|(path, _)| (*path).clone())
            .collect();

        drop(access); // Release lock before closing files

        info!(
            "Evicting {} oldest files from {} cache (max: {})",
            to_evict.len(),
            self.server_id,
            MAX_OPENED_FILES
        );

        for path in &to_evict {
            if let Err(e) = self.close_file(path).await {
                debug!("Failed to close evicted file {}: {}", path.display(), e);
            }

            // Remove from tracking
            self.opened_files.lock().await.remove(path);
            self.file_versions.lock().await.remove(path);
            self.file_access.lock().await.remove(path);
        }
    }

    /// Update file content (textDocument/didChange)
    ///
    /// Call this after editing a file to sync changes with the LSP server.
    /// The file must have been previously synced with sync_file.
    pub async fn update_file(&self, path: &Path, content: &str) -> Result<()> {
        let path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };

        // Ensure file is opened first
        if !self.opened_files.lock().await.contains(&path) {
            self.sync_file(&path).await?;
        }

        let uri = Url::from_file_path(&path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        // Get and increment version
        let version = {
            let mut versions = self.file_versions.lock().await;
            let v = versions.entry(path).or_insert(1);
            *v += 1;
            *v
        };

        let uri_path_for_log = uri.path().to_string();
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None, // Full document sync
                range_length: None,
                text: content.to_string(),
            }],
        };

        self.connection
            .notify("textDocument/didChange", params)
            .await?;

        info!(
            "Updated file in LSP {}: {} (version {})",
            self.server_id, uri_path_for_log, version
        );

        Ok(())
    }

    /// Force re-sync a file (close and reopen)
    ///
    /// Useful when you want to refresh the file content from disk.
    pub async fn resync_file(&self, path: &Path) -> Result<()> {
        let path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };

        // Close the file first if it was opened
        if self.opened_files.lock().await.contains(&path) {
            let _ = self.close_file(&path).await;
        }

        // Remove from all tracking
        {
            let mut opened = self.opened_files.lock().await;
            opened.remove(&path);
        }
        {
            let mut versions = self.file_versions.lock().await;
            versions.remove(&path);
        }
        {
            let mut access = self.file_access.lock().await;
            access.remove(&path);
        }

        // Re-sync
        self.sync_file(&path).await
    }

    /// Get document symbols
    pub async fn document_symbols(&self, path: &Path) -> Result<Vec<ResolvedSymbol>> {
        self.sync_file(path).await?;

        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<DocumentSymbolResponse> = self
            .connection
            .request("textDocument/documentSymbol", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        let symbols = match result {
            Some(response) => flatten_symbols(&response),
            None => Vec::new(),
        };

        info!(
            "Retrieved {} symbols from {} via {}: {:?}",
            symbols.len(),
            path.display(),
            self.server_id,
            symbols.iter().take(5).map(|s| &s.name).collect::<Vec<_>>()
        );

        Ok(symbols)
    }

    /// Go to definition by symbol name
    pub async fn definition(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Vec<Location>> {
        info!(
            "Finding definition for '{}' (kind={:?}) in {} via {}",
            symbol_name,
            symbol_kind,
            path.display(),
            self.server_id
        );

        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        // Use the first (best) match
        let symbol = &matches[0].symbol;
        let locations = self.definition_at_position(path, symbol.position).await?;

        info!(
            "Definition result for '{}': {} locations {:?}",
            symbol_name,
            locations.len(),
            locations.iter().map(|l| l.uri.path()).collect::<Vec<_>>()
        );

        Ok(locations)
    }

    /// Go to definition at exact position
    pub async fn definition_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>> {
        self.goto_at_position("textDocument/definition", path, position)
            .await
    }

    /// Go to implementation by symbol name
    ///
    /// Finds implementations of traits/interfaces for the given symbol.
    pub async fn implementation(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Vec<Location>> {
        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        // Use the first (best) match
        let symbol = &matches[0].symbol;
        self.implementation_at_position(path, symbol.position).await
    }

    /// Go to implementation at exact position
    pub async fn implementation_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>> {
        self.goto_at_position("textDocument/implementation", path, position)
            .await
    }

    /// Go to type definition by symbol name
    ///
    /// Finds the type definition for a symbol (e.g., the struct definition for a variable).
    pub async fn type_definition(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Vec<Location>> {
        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        let symbol = &matches[0].symbol;
        self.type_definition_at_position(path, symbol.position)
            .await
    }

    /// Go to type definition at exact position
    pub async fn type_definition_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>> {
        self.goto_at_position("textDocument/typeDefinition", path, position)
            .await
    }

    /// Go to declaration by symbol name
    ///
    /// Finds the declaration of a symbol (useful in languages with separate declaration/definition).
    pub async fn declaration(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Vec<Location>> {
        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        let symbol = &matches[0].symbol;
        self.declaration_at_position(path, symbol.position).await
    }

    /// Go to declaration at exact position
    pub async fn declaration_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>> {
        self.goto_at_position("textDocument/declaration", path, position)
            .await
    }

    /// Generic goto request (internal helper)
    ///
    /// Handles definition, implementation, typeDefinition, declaration requests
    /// which all share the same parameter and response structure.
    async fn goto_at_position(
        &self,
        method: &str,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>> {
        self.sync_file(path).await?;

        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<GotoDefinitionResponse> = self
            .connection
            .request(method, params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        Ok(match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
            Some(GotoDefinitionResponse::Array(locs)) => locs,
            Some(GotoDefinitionResponse::Link(links)) => links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect(),
            None => Vec::new(),
        })
    }

    /// Find references by symbol name
    pub async fn references(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        info!(
            "Finding references for '{}' (kind={:?}, include_declaration={}) in {} via {}",
            symbol_name,
            symbol_kind,
            include_declaration,
            path.display(),
            self.server_id
        );

        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        let symbol = &matches[0].symbol;
        let locations = self
            .references_at_position(path, symbol.position, include_declaration)
            .await?;

        info!(
            "References result for '{}': {} locations",
            symbol_name,
            locations.len()
        );

        Ok(locations)
    }

    /// Find references at exact position
    pub async fn references_at_position(
        &self,
        path: &Path,
        position: Position,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        self.sync_file(path).await?;

        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            context: ReferenceContext {
                include_declaration,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<Vec<Location>> = self
            .connection
            .request("textDocument/references", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        Ok(result.unwrap_or_default())
    }

    /// Get hover information by symbol name
    pub async fn hover(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Option<String>> {
        info!(
            "Hover for '{}' (kind={:?}) in {} via {}",
            symbol_name,
            symbol_kind,
            path.display(),
            self.server_id
        );

        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        let symbol = &matches[0].symbol;
        let result = self.hover_at_position(path, symbol.position).await?;

        info!(
            "Hover result for '{}': {} chars",
            symbol_name,
            result.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        Ok(result)
    }

    /// Get hover information at exact position
    pub async fn hover_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Option<String>> {
        self.sync_file(path).await?;

        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result: Option<Hover> = self
            .connection
            .request("textDocument/hover", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        Ok(result.map(|hover| match hover.contents {
            lsp_types::HoverContents::Scalar(content) => Self::markup_to_string(content),
            lsp_types::HoverContents::Array(contents) => contents
                .into_iter()
                .map(Self::markup_to_string)
                .collect::<Vec<_>>()
                .join("\n\n"),
            lsp_types::HoverContents::Markup(markup) => markup.value,
        }))
    }

    fn markup_to_string(content: lsp_types::MarkedString) -> String {
        match content {
            lsp_types::MarkedString::String(s) => s,
            lsp_types::MarkedString::LanguageString(ls) => {
                format!("```{}\n{}\n```", ls.language, ls.value)
            }
        }
    }

    /// Search for symbols across the workspace
    ///
    /// This searches all files in the workspace for symbols matching the query.
    /// Useful for finding where a symbol is defined without knowing the file path.
    pub async fn workspace_symbol(&self, query: &str) -> Result<Vec<SymbolInformation>> {
        info!(
            "Workspace symbol search query='{}' via {}",
            query, self.server_id
        );

        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<WorkspaceSymbolResponse> = self
            .connection
            .request("workspace/symbol", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        let symbols = match result {
            Some(WorkspaceSymbolResponse::Flat(symbols)) => symbols,
            Some(WorkspaceSymbolResponse::Nested(symbols)) => {
                // Convert WorkspaceSymbol to SymbolInformation
                // WorkspaceSymbol has location as OneOf<Location, WorkspaceLocation>
                symbols
                    .into_iter()
                    .filter_map(|ws| {
                        // Extract location from the WorkspaceSymbol
                        let location = match ws.location {
                            lsp_types::OneOf::Left(loc) => loc,
                            lsp_types::OneOf::Right(workspace_loc) => Location {
                                uri: workspace_loc.uri,
                                range: lsp_types::Range::default(),
                            },
                        };

                        #[allow(deprecated)]
                        Some(SymbolInformation {
                            name: ws.name,
                            kind: ws.kind,
                            tags: ws.tags,
                            deprecated: None,
                            location,
                            container_name: ws.container_name,
                        })
                    })
                    .collect()
            }
            None => Vec::new(),
        };

        info!(
            "Workspace symbol result: {} symbols {:?}",
            symbols.len(),
            symbols.iter().take(5).map(|s| &s.name).collect::<Vec<_>>()
        );

        Ok(symbols)
    }

    /// Prepare call hierarchy for a symbol
    ///
    /// This is the first step of the call hierarchy protocol.
    /// Returns CallHierarchyItem(s) that can be used with incoming_calls/outgoing_calls.
    pub async fn prepare_call_hierarchy(
        &self,
        path: &Path,
        symbol_name: &str,
        symbol_kind: Option<SymbolKind>,
    ) -> Result<Vec<CallHierarchyItem>> {
        info!(
            "Prepare call hierarchy for '{}' (kind={:?}) in {}",
            symbol_name,
            symbol_kind,
            path.display()
        );

        let symbols = self.document_symbols(path).await?;
        let matches = find_matching_symbols(&symbols, symbol_name, symbol_kind);

        if matches.is_empty() {
            return Err(LspErr::SymbolNotFound {
                name: symbol_name.to_string(),
                file: path.display().to_string(),
            });
        }

        let symbol = &matches[0].symbol;
        let items = self
            .prepare_call_hierarchy_at_position(path, symbol.position)
            .await?;

        info!(
            "Call hierarchy prepared: {} items {:?}",
            items.len(),
            items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );

        Ok(items)
    }

    /// Prepare call hierarchy at exact position
    pub async fn prepare_call_hierarchy_at_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<CallHierarchyItem>> {
        self.sync_file(path).await?;

        let uri = Url::from_file_path(path)
            .map_err(|_| LspErr::Internal(format!("invalid file path: {}", path.display())))?;

        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result: Option<Vec<CallHierarchyItem>> = self
            .connection
            .request("textDocument/prepareCallHierarchy", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        Ok(result.unwrap_or_default())
    }

    /// Get incoming calls to a symbol
    ///
    /// Requires a CallHierarchyItem from prepare_call_hierarchy.
    pub async fn incoming_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>> {
        info!("Finding incoming calls for '{}'", item.name);

        let params = CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<Vec<CallHierarchyIncomingCall>> = self
            .connection
            .request("callHierarchy/incomingCalls", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        let calls = result.unwrap_or_default();

        info!(
            "Incoming calls result: {} callers {:?}",
            calls.len(),
            calls.iter().map(|c| &c.from.name).collect::<Vec<_>>()
        );

        Ok(calls)
    }

    /// Get outgoing calls from a symbol
    ///
    /// Requires a CallHierarchyItem from prepare_call_hierarchy.
    pub async fn outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>> {
        info!("Finding outgoing calls from '{}'", item.name);

        let params = CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<Vec<CallHierarchyOutgoingCall>> = self
            .connection
            .request("callHierarchy/outgoingCalls", params)
            .await
            .and_then(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v).map(Some).map_err(Into::into)
                }
            })?;

        let calls = result.unwrap_or_default();

        info!(
            "Outgoing calls result: {} callees {:?}",
            calls.len(),
            calls.iter().map(|c| &c.to.name).collect::<Vec<_>>()
        );

        Ok(calls)
    }

    /// Shutdown the server
    ///
    /// Performs a clean shutdown sequence:
    /// 1. Close all opened files (sends textDocument/didClose for each)
    /// 2. Send shutdown request
    /// 3. Send exit notification
    /// 4. Clean up notification handler task
    pub async fn shutdown(&self) -> Result<()> {
        let opened_files_count = self.opened_files.lock().await.len();
        info!(
            "Shutting down LSP server: {} (files: {})",
            self.server_id, opened_files_count
        );

        // Close all opened files first to free server-side resources
        self.close_all_files().await;

        let shutdown_timeout = self.timeout_config.shutdown_timeout();

        // Send shutdown request with timeout
        match tokio::time::timeout(
            shutdown_timeout,
            self.connection.request("shutdown", serde_json::Value::Null),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!("LSP server {} acknowledged shutdown", self.server_id);
            }
            Ok(Err(e)) => {
                warn!("LSP shutdown request failed: {}", e);
            }
            Err(_) => {
                warn!(
                    "LSP shutdown request timed out after {}ms",
                    self.timeout_config.shutdown_timeout_ms
                );
            }
        }

        // Send exit notification (best effort)
        let _ = self
            .connection
            .notify("exit", serde_json::Value::Null)
            .await;

        // Abort notification handler task to prevent resource leak
        if let Some(handle) = self.notification_handle.lock().await.take() {
            handle.abort();
            // Wait briefly for task to complete, ignore result
            let _ = tokio::time::timeout(std::time::Duration::from_millis(100), handle).await;
        }

        info!("LSP {} shutdown complete", self.server_id);
        Ok(())
    }

    /// Get the server ID
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Get the root URI
    pub fn root_uri(&self) -> &Url {
        &self.root_uri
    }

    // =========================================================================
    // Server Capability Queries
    // =========================================================================

    /// Check if server supports textDocument/implementation
    pub async fn supports_implementation(&self) -> bool {
        self.capabilities.lock().await.supports_implementation
    }

    /// Check if server supports textDocument/typeDefinition
    pub async fn supports_type_definition(&self) -> bool {
        self.capabilities.lock().await.supports_type_definition
    }

    /// Check if server supports textDocument/declaration
    pub async fn supports_declaration(&self) -> bool {
        self.capabilities.lock().await.supports_declaration
    }

    /// Check if server supports call hierarchy operations
    pub async fn supports_call_hierarchy(&self) -> bool {
        self.capabilities.lock().await.supports_call_hierarchy
    }

    /// Check if server supports workspace/symbol
    pub async fn supports_workspace_symbol(&self) -> bool {
        self.capabilities.lock().await.supports_workspace_symbol
    }

    /// Get all cached capabilities
    pub async fn capabilities(&self) -> CachedCapabilities {
        self.capabilities.lock().await.clone()
    }
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("server_id", &self.server_id)
            .field("root_uri", &self.root_uri)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markup_to_string_plain() {
        let content = lsp_types::MarkedString::String("Hello world".to_string());
        assert_eq!(LspClient::markup_to_string(content), "Hello world");
    }

    #[test]
    fn test_markup_to_string_language() {
        let content = lsp_types::MarkedString::LanguageString(lsp_types::LanguageString {
            language: "rust".to_string(),
            value: "fn main() {}".to_string(),
        });
        assert_eq!(
            LspClient::markup_to_string(content),
            "```rust\nfn main() {}\n```"
        );
    }

    #[test]
    fn test_language_id_from_extension() {
        // Test the language detection logic used in sync_file
        let test_cases = vec![
            ("rs", "rust"),
            ("go", "go"),
            ("py", "python"),
            ("pyi", "python"),
            ("txt", "plaintext"),
            ("unknown", "plaintext"),
        ];

        for (ext, expected) in test_cases {
            let language_id = match ext {
                "rs" => "rust",
                "go" => "go",
                "py" | "pyi" => "python",
                _ => "plaintext",
            };
            assert_eq!(
                language_id, expected,
                "Extension '{}' should map to '{}'",
                ext, expected
            );
        }
    }
}
