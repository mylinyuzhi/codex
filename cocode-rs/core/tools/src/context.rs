//! Tool execution context.
//!
//! This module provides [`ToolContext`] which contains all the context
//! needed for tool execution, including permissions, event channels,
//! and cancellation support.

use cocode_protocol::{LoopEvent, PermissionMode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

/// Stored approvals for tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalStore {
    /// Approved tool patterns.
    approved_patterns: HashSet<String>,
    /// Session-wide approvals.
    session_approvals: HashSet<String>,
}

impl ApprovalStore {
    /// Create a new empty approval store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a tool action is approved.
    pub fn is_approved(&self, tool_name: &str, pattern: &str) -> bool {
        let key = format!("{tool_name}:{pattern}");
        self.approved_patterns.contains(&key) || self.session_approvals.contains(tool_name)
    }

    /// Add an approval for a specific pattern.
    pub fn approve_pattern(&mut self, tool_name: &str, pattern: &str) {
        let key = format!("{tool_name}:{pattern}");
        self.approved_patterns.insert(key);
    }

    /// Add a session-wide approval for a tool.
    pub fn approve_session(&mut self, tool_name: &str) {
        self.session_approvals.insert(tool_name.to_string());
    }

    /// Clear all approvals.
    pub fn clear(&mut self) {
        self.approved_patterns.clear();
        self.session_approvals.clear();
    }
}

/// Tracks files that have been read or modified.
#[derive(Debug, Clone, Default)]
pub struct FileTracker {
    /// Files that have been read.
    read_files: HashSet<PathBuf>,
    /// Files that have been modified.
    modified_files: HashSet<PathBuf>,
}

impl FileTracker {
    /// Create a new file tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a file read.
    pub fn record_read(&mut self, path: impl Into<PathBuf>) {
        self.read_files.insert(path.into());
    }

    /// Record a file modification.
    pub fn record_modified(&mut self, path: impl Into<PathBuf>) {
        self.modified_files.insert(path.into());
    }

    /// Check if a file has been read.
    pub fn was_read(&self, path: &PathBuf) -> bool {
        self.read_files.contains(path)
    }

    /// Check if a file has been modified.
    pub fn was_modified(&self, path: &PathBuf) -> bool {
        self.modified_files.contains(path)
    }

    /// Get all read files.
    pub fn read_files(&self) -> &HashSet<PathBuf> {
        &self.read_files
    }

    /// Get all modified files.
    pub fn modified_files(&self) -> &HashSet<PathBuf> {
        &self.modified_files
    }
}

/// Context for tool execution.
///
/// This provides everything a tool needs during execution:
/// - Call identification
/// - Working directory
/// - Permission mode and approvals
/// - Event channel for progress updates
/// - Cancellation support
/// - File tracking
#[derive(Clone)]
pub struct ToolContext {
    /// Unique call ID for this execution.
    pub call_id: String,
    /// Session ID.
    pub session_id: String,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Permission mode for this execution.
    pub permission_mode: PermissionMode,
    /// Channel for emitting loop events.
    pub event_tx: Option<mpsc::Sender<LoopEvent>>,
    /// Cancellation token for aborting execution.
    pub cancel_token: CancellationToken,
    /// Stored approvals.
    pub approval_store: Arc<Mutex<ApprovalStore>>,
    /// File tracker.
    pub file_tracker: Arc<Mutex<FileTracker>>,
}

impl ToolContext {
    /// Create a new tool context.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            call_id: call_id.into(),
            session_id: session_id.into(),
            cwd,
            permission_mode: PermissionMode::Default,
            event_tx: None,
            cancel_token: CancellationToken::new(),
            approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
            file_tracker: Arc::new(Mutex::new(FileTracker::new())),
        }
    }

    /// Set the permission mode.
    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Set the event channel.
    pub fn with_event_tx(mut self, tx: mpsc::Sender<LoopEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Set the cancellation token.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Set the approval store.
    pub fn with_approval_store(mut self, store: Arc<Mutex<ApprovalStore>>) -> Self {
        self.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn with_file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.file_tracker = tracker;
        self
    }

    /// Emit a loop event.
    pub async fn emit_event(&self, event: LoopEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// Emit tool progress.
    pub async fn emit_progress(&self, message: impl Into<String>) {
        self.emit_event(LoopEvent::ToolProgress {
            call_id: self.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: None,
                bytes_processed: None,
                total_bytes: None,
            },
        })
        .await;
    }

    /// Emit tool progress with percentage.
    pub async fn emit_progress_percent(&self, message: impl Into<String>, percentage: i32) {
        self.emit_event(LoopEvent::ToolProgress {
            call_id: self.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: Some(percentage),
                bytes_processed: None,
                total_bytes: None,
            },
        })
        .await;
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Wait for cancellation or completion.
    pub async fn cancelled(&self) {
        self.cancel_token.cancelled().await
    }

    /// Record a file read.
    pub async fn record_file_read(&self, path: impl Into<PathBuf>) {
        self.file_tracker.lock().await.record_read(path);
    }

    /// Record a file modification.
    pub async fn record_file_modified(&self, path: impl Into<PathBuf>) {
        self.file_tracker.lock().await.record_modified(path);
    }

    /// Check if a file was read.
    pub async fn was_file_read(&self, path: &PathBuf) -> bool {
        self.file_tracker.lock().await.was_read(path)
    }

    /// Check if a file was modified.
    pub async fn was_file_modified(&self, path: &PathBuf) -> bool {
        self.file_tracker.lock().await.was_modified(path)
    }

    /// Check if an action is approved.
    pub async fn is_approved(&self, tool_name: &str, pattern: &str) -> bool {
        self.approval_store
            .lock()
            .await
            .is_approved(tool_name, pattern)
    }

    /// Approve a specific pattern.
    pub async fn approve_pattern(&self, tool_name: &str, pattern: &str) {
        self.approval_store
            .lock()
            .await
            .approve_pattern(tool_name, pattern);
    }

    /// Approve a tool for the session.
    pub async fn approve_session(&self, tool_name: &str) {
        self.approval_store.lock().await.approve_session(tool_name);
    }

    /// Resolve a path relative to the working directory.
    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("call_id", &self.call_id)
            .field("session_id", &self.session_id)
            .field("cwd", &self.cwd)
            .field("permission_mode", &self.permission_mode)
            .field("is_cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// Builder for creating tool contexts.
pub struct ToolContextBuilder {
    call_id: String,
    session_id: String,
    cwd: PathBuf,
    permission_mode: PermissionMode,
    event_tx: Option<mpsc::Sender<LoopEvent>>,
    cancel_token: CancellationToken,
    approval_store: Arc<Mutex<ApprovalStore>>,
    file_tracker: Arc<Mutex<FileTracker>>,
}

impl ToolContextBuilder {
    /// Create a new builder.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            session_id: session_id.into(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            permission_mode: PermissionMode::Default,
            event_tx: None,
            cancel_token: CancellationToken::new(),
            approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
            file_tracker: Arc::new(Mutex::new(FileTracker::new())),
        }
    }

    /// Set the working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Set the permission mode.
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Set the event channel.
    pub fn event_tx(mut self, tx: mpsc::Sender<LoopEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Set the cancellation token.
    pub fn cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Set the approval store.
    pub fn approval_store(mut self, store: Arc<Mutex<ApprovalStore>>) -> Self {
        self.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.file_tracker = tracker;
        self
    }

    /// Build the context.
    pub fn build(self) -> ToolContext {
        ToolContext {
            call_id: self.call_id,
            session_id: self.session_id,
            cwd: self.cwd,
            permission_mode: self.permission_mode,
            event_tx: self.event_tx,
            cancel_token: self.cancel_token,
            approval_store: self.approval_store,
            file_tracker: self.file_tracker,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_store() {
        let mut store = ApprovalStore::new();

        assert!(!store.is_approved("Bash", "git status"));
        store.approve_pattern("Bash", "git status");
        assert!(store.is_approved("Bash", "git status"));

        store.approve_session("Read");
        assert!(store.is_approved("Read", "any_pattern"));
    }

    #[test]
    fn test_file_tracker() {
        let mut tracker = FileTracker::new();

        let path = PathBuf::from("/test/file.txt");
        assert!(!tracker.was_read(&path));

        tracker.record_read(&path);
        assert!(tracker.was_read(&path));
        assert!(!tracker.was_modified(&path));

        tracker.record_modified(&path);
        assert!(tracker.was_modified(&path));
    }

    #[tokio::test]
    async fn test_tool_context() {
        let ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

        assert_eq!(ctx.call_id, "call-1");
        assert_eq!(ctx.session_id, "session-1");
        assert!(!ctx.is_cancelled());
    }

    #[test]
    fn test_resolve_path() {
        let ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/home/user/project"));

        // Relative path
        assert_eq!(
            ctx.resolve_path("src/main.rs"),
            PathBuf::from("/home/user/project/src/main.rs")
        );

        // Absolute path
        assert_eq!(
            ctx.resolve_path("/etc/passwd"),
            PathBuf::from("/etc/passwd")
        );
    }

    #[tokio::test]
    async fn test_context_builder() {
        let ctx = ToolContextBuilder::new("call-1", "session-1")
            .cwd("/tmp")
            .permission_mode(PermissionMode::Plan)
            .build();

        assert_eq!(ctx.cwd, PathBuf::from("/tmp"));
        assert_eq!(ctx.permission_mode, PermissionMode::Plan);
    }
}
