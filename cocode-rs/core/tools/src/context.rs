//! Tool execution context.
//!
//! This module provides [`ToolContext`] which contains all the context
//! needed for tool execution, including permissions, event channels,
//! and cancellation support.

use async_trait::async_trait;
use cocode_hooks::HookRegistry;
use cocode_lsp::LspServerManager;
use cocode_policy::PermissionRuleEvaluator;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::CoreEvent;
use cocode_protocol::Features;
use cocode_protocol::PermissionMode;
use cocode_protocol::RoleSelections;
use cocode_protocol::TuiEvent;
use cocode_protocol::WebFetchConfig;
use cocode_protocol::WebSearchConfig;
use cocode_shell::ShellExecutor;
use cocode_skill::SkillManager;
use cocode_skill::SkillUsageTracker;
use lru::LruCache;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use std::time::SystemTime;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// Responder for AskUserQuestion tool.
///
/// Manages pending question requests with oneshot channels. The tool emits
/// a `QuestionAsked` event and waits on the receiver. The main loop calls
/// `respond()` when the user answers, unblocking the tool.
pub struct QuestionResponder {
    /// Pending question requests: request_id → oneshot sender.
    pending: std::sync::Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>,
}

impl QuestionResponder {
    /// Create a new question responder.
    pub fn new() -> Self {
        Self {
            pending: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Register a pending question and return the receiver to await.
    pub fn register(&self, request_id: String) -> oneshot::Receiver<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| {
                tracing::error!("QuestionResponder lock poisoned — concurrent bug detected");
                e.into_inner()
            })
            .insert(request_id, tx);
        rx
    }

    /// Send the user's response for a pending question.
    ///
    /// Returns `true` if the response was delivered.
    pub fn respond(&self, request_id: &str, answers: serde_json::Value) -> bool {
        if let Some(tx) = self
            .pending
            .lock()
            .unwrap_or_else(|e| {
                tracing::error!("QuestionResponder lock poisoned — concurrent bug detected");
                e.into_inner()
            })
            .remove(request_id)
        {
            tx.send(answers).is_ok()
        } else {
            false
        }
    }
}

impl Default for QuestionResponder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for QuestionResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuestionResponder").finish()
    }
}

/// Input for spawning a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentInput {
    /// The agent type to spawn.
    pub agent_type: String,
    /// The task prompt for the agent.
    pub prompt: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Optional turn limit override.
    pub max_turns: Option<i32>,
    /// Whether to run in background.
    ///
    /// - `Some(true/false)`: Explicitly set by the model.
    /// - `None`: Deferred to the agent definition's `background` default.
    pub run_in_background: Option<bool>,
    /// Optional tool filter override.
    pub allowed_tools: Option<Vec<String>>,
    /// Parent's role selections (snapshot at spawn time for isolation).
    ///
    /// When present, the spawned subagent will use these selections,
    /// ensuring it's unaffected by subsequent changes to the parent's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_selections: Option<RoleSelections>,
    /// Permission mode override for the subagent.
    ///
    /// When set (from `AgentDefinition.permission_mode`), the subagent uses
    /// this mode instead of inheriting from the parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<cocode_protocol::PermissionMode>,
    /// Agent ID to resume from a previous invocation.
    ///
    /// When set, the agent continues from the previous execution's output,
    /// prepending the prior context to the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<String>,

    /// Isolation mode for the spawned agent.
    ///
    /// When set to `"worktree"`, a temporary git worktree is created and the
    /// agent's CWD is set to the worktree path. Auto-cleanup on completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,

    /// Display name for the spawned agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Team to auto-join the agent to after spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,

    /// Agent execution mode (normal, plan, auto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,

    /// Working directory for the spawned agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Short description of what the agent will do (for TUI display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Result of spawning a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentResult {
    /// The unique agent ID.
    pub agent_id: String,
    /// The agent output (foreground only).
    pub output: Option<String>,
    /// Background agent output file path.
    pub output_file: Option<PathBuf>,
    /// Cancellation token for the spawned agent.
    ///
    /// Present for background agents so the caller can register it
    /// in `agent_cancel_tokens` for TaskStop to cancel by ID.
    #[serde(skip)]
    pub cancel_token: Option<CancellationToken>,
    /// Display color from agent definition (for TUI rendering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Input for a single-shot model call (no agent loop).
#[derive(Debug, Clone)]
pub struct ModelCallInput {
    /// The call options (messages + JSON response format).
    pub request: cocode_inference::LanguageModelCallOptions,
}

/// Result of a single-shot model call.
#[derive(Debug, Clone)]
pub struct ModelCallResult {
    /// The generate result.
    pub response: cocode_inference::LanguageModelGenerateResult,
}

/// Lightweight model call callback — single request/response, no agent loop.
/// Used by SmartEdit for LLM-assisted edit correction.
pub type ModelCallFn = Arc<
    dyn Fn(
            ModelCallInput,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<ModelCallResult, cocode_error::BoxedError>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// Shared registry of cancellation tokens for background agents.
///
/// When a subagent is spawned, its `CancellationToken` is registered here.
/// TaskStop can look up the token by agent ID and cancel it directly,
/// without needing a callback to the SubagentManager.
pub type AgentCancelTokens = Arc<Mutex<HashMap<String, CancellationToken>>>;

/// Shared set of agent IDs that have been explicitly killed via TaskStop.
///
/// When TaskStop cancels an agent, its ID is recorded here. The session
/// layer checks this set when building background task info so the agent's
/// status is reported as `Killed` rather than `Failed`.
pub type KilledAgents = Arc<Mutex<HashSet<String>>>;

/// Type alias for the agent spawn callback function.
///
/// This callback is provided by the executor layer to enable tools
/// to spawn subagents without creating circular dependencies.
pub type SpawnAgentFn = Arc<
    dyn Fn(
            SpawnAgentInput,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<SpawnAgentResult, cocode_error::BoxedError>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// Trait for requesting user permission approval.
///
/// This trait decouples the tools crate from the executor crate,
/// allowing `WorkerPermissionQueue` (in cocode-executor) to be used
/// without creating a circular dependency.
#[async_trait]
pub trait PermissionRequester: Send + Sync {
    /// Request permission for an operation.
    ///
    /// Returns the user's three-way decision: approve once, approve similar
    /// commands (with prefix pattern), or deny.
    async fn request_permission(
        &self,
        request: ApprovalRequest,
        worker_id: &str,
    ) -> ApprovalDecision;
}

/// Information about an invoked skill.
///
/// Tracks skills that have been invoked during the session for hook cleanup
/// and system reminder injection.
#[derive(Debug, Clone)]
pub struct InvokedSkill {
    /// The skill name.
    pub name: String,
    /// When the skill was invoked.
    pub started_at: Instant,
    /// The skill's prompt content (after argument substitution).
    pub prompt_content: String,
    /// Base directory of the skill (for relative path resolution).
    pub path: Option<PathBuf>,
}

pub use cocode_policy::ApprovalStore;

/// State of a file that has been read.
///
/// Tracks content, timestamps, and access patterns for read-before-edit validation
/// and change detection.
///
/// # Serialization
///
/// The `timestamp` field is not serialized - it's reconstructed to `UNIX_EPOCH`
/// on load. This simplifies persistence while still providing in-memory tracking
/// for LRU ordering.
///
/// # Claude Code Alignment
///
/// Uses `i64` for offset/limit to support large files (>2 billion lines).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileReadState {
    /// File content at time of read (None if partial or too large).
    pub content: Option<String>,
    /// When this read state was recorded (not serialized - in-memory only).
    #[serde(skip)]
    pub timestamp: SystemTime,
    /// File modification time at time of read.
    pub file_mtime: Option<SystemTime>,
    /// SHA256 hex hash of content at time of read (None if partial or too large).
    pub content_hash: Option<String>,
    /// Line offset of the read (None if from start).
    /// Uses i64 for large file support (>2 billion lines).
    pub offset: Option<i64>,
    /// Line limit of the read (None if no limit).
    /// Uses i64 for large file support (>2 billion lines).
    pub limit: Option<i64>,
    /// Kind of read operation.
    pub kind: FileReadKind,
    /// Number of times this file has been accessed.
    pub access_count: i32,
    /// Turn number when the file was read (for compaction cleanup).
    pub read_turn: i32,
}

impl Default for FileReadState {
    fn default() -> Self {
        Self {
            content: None,
            timestamp: SystemTime::UNIX_EPOCH,
            file_mtime: None,
            content_hash: None,
            offset: None,
            limit: None,
            kind: FileReadKind::MetadataOnly,
            access_count: 0,
            read_turn: 0,
        }
    }
}

/// Re-export FileReadKind from protocol.
pub use cocode_protocol::FileReadKind;

impl FileReadState {
    /// Compute SHA256 hex hash of content.
    pub fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Create a new read state for a complete file read.
    pub fn complete(content: String, file_mtime: Option<SystemTime>) -> Self {
        Self::complete_with_turn(content, file_mtime, 0)
    }

    /// Create a new read state for a complete file read with turn number.
    pub fn complete_with_turn(
        content: String,
        file_mtime: Option<SystemTime>,
        read_turn: i32,
    ) -> Self {
        let hash = Self::compute_hash(&content);
        Self {
            content: Some(content),
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: Some(hash),
            offset: None,
            limit: None,
            kind: FileReadKind::FullContent,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a new read state for a partial file read.
    pub fn partial(offset: i64, limit: i64, file_mtime: Option<SystemTime>) -> Self {
        Self::partial_with_turn(offset, limit, file_mtime, 0)
    }

    /// Create a new read state for a partial file read with turn number.
    pub fn partial_with_turn(
        offset: i64,
        limit: i64,
        file_mtime: Option<SystemTime>,
        read_turn: i32,
    ) -> Self {
        Self {
            content: None,
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: None,
            offset: Some(offset),
            limit: Some(limit),
            kind: FileReadKind::PartialContent,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a new read state for metadata-only (path discovery).
    pub fn metadata_only(file_mtime: Option<SystemTime>, read_turn: i32) -> Self {
        Self {
            content: None,
            timestamp: SystemTime::now(),
            file_mtime,
            content_hash: None,
            offset: None,
            limit: None,
            kind: FileReadKind::MetadataOnly,
            access_count: 1,
            read_turn,
        }
    }

    /// Create a read state with content and full metadata.
    ///
    /// This is the primary constructor for system-reminder compatibility,
    /// used when rebuilding state from ContextModifier::FileRead.
    ///
    /// # Arguments
    ///
    /// * `content` - File content
    /// * `last_modified` - File modification time at read time
    /// * `read_turn` - Turn number when the file was read
    /// * `offset` - Line offset (0 if from start)
    /// * `limit` - Line limit (0 if no limit)
    ///
    /// # Claude Code Alignment
    ///
    /// This matches the constructor pattern used in Claude Code v2.1.38's
    /// file read state reconstruction.
    pub fn with_content(
        content: String,
        last_modified: Option<SystemTime>,
        read_turn: i32,
        offset: i64,
        limit: i64,
    ) -> Self {
        let has_content = !content.is_empty();
        let is_full = offset == 0 && limit == 0;
        let content_hash = if has_content {
            Some(Self::compute_hash(&content))
        } else {
            None
        };

        Self {
            content: if has_content { Some(content) } else { None },
            timestamp: SystemTime::now(),
            file_mtime: last_modified,
            content_hash,
            offset: if offset > 0 { Some(offset) } else { None },
            limit: if limit > 0 { Some(limit) } else { None },
            kind: if is_full {
                FileReadKind::FullContent
            } else {
                FileReadKind::PartialContent
            },
            access_count: 1,
            read_turn,
        }
    }

    /// Return a normalized copy of this state.
    ///
    /// Normalization ensures consistency between the `kind` field and
    /// the `offset`/`limit`/`content_hash` fields:
    ///
    /// - `FullContent`: offset/limit are cleared, content_hash preserved
    /// - `PartialContent`: content_hash is cleared (partial reads can't verify)
    /// - `MetadataOnly`: content cleared, offset/limit set to None
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's state normalization behavior.
    pub fn normalized(mut self) -> Self {
        self.normalize_in_place();
        self
    }

    /// Normalize this state in place.
    fn normalize_in_place(&mut self) {
        match self.kind {
            FileReadKind::FullContent => {
                // Full reads should not have offset/limit
                self.offset = None;
                self.limit = None;
            }
            FileReadKind::PartialContent => {
                // Partial reads should not have content hash
                // (can't verify content hasn't changed)
                self.content_hash = None;
            }
            FileReadKind::MetadataOnly => {
                // Metadata-only has no content or range
                self.content = None;
                self.content_hash = None;
                self.offset = None;
                self.limit = None;
            }
        }
    }

    /// Check if this was a full content read.
    pub fn is_full(&self) -> bool {
        self.kind.is_full()
    }

    /// Check if this was a partial read.
    pub fn is_partial(&self) -> bool {
        self.kind.is_partial()
    }

    /// Check if this was a metadata-only read.
    pub fn is_metadata_only(&self) -> bool {
        self.kind.is_metadata_only()
    }

    /// Check if this is a cacheable read (full content only).
    /// Used for already-read detection.
    pub fn is_cacheable(&self) -> bool {
        matches!(self.kind, FileReadKind::FullContent)
    }
}

/// Configuration for FileTracker limits.
///
/// Provides clear, named configuration for the file tracker's LRU cache
/// behavior. This matches Claude Code v2.1.38's limits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FileTrackerConfig {
    /// Maximum number of entries in the LRU cache.
    ///
    /// When this limit is reached, the oldest (least recently used) entries
    /// are evicted to make room for new ones.
    ///
    /// Default: 100 (Claude Code v2.1.38 alignment)
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Maximum total content size in bytes across all tracked files.
    ///
    /// When this limit is approached, older entries are evicted to stay
    /// under the budget. This prevents unbounded memory growth.
    ///
    /// Default: ~25MB (26,214,400 bytes - Claude Code v2.1.38 alignment)
    #[serde(default = "default_max_size_bytes")]
    pub max_total_bytes: usize,
}

fn default_max_entries() -> usize {
    100
}

fn default_max_size_bytes() -> usize {
    26_214_400 // ~25MB
}

impl Default for FileTrackerConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            max_total_bytes: default_max_size_bytes(),
        }
    }
}

impl FileTrackerConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config with custom limits.
    pub fn with_limits(max_entries: usize, max_total_bytes: usize) -> Self {
        Self {
            max_entries,
            max_total_bytes,
        }
    }
}

/// Internal state for FileTracker.
///
/// Separated from the outer struct to enable RwLock-based interior mutability.
#[derive(Debug)]
struct TrackerState {
    /// Files that have been read, with their read state (LRU cache).
    read_files: LruCache<PathBuf, FileReadState>,
    /// Files that have been modified.
    modified_files: HashSet<PathBuf>,
    /// Paths that trigger nested memory lookup (CLAUDE.md, AGENTS.md, etc.).
    nested_memory_triggers: HashSet<PathBuf>,
    /// Mapping from tool call IDs to file paths (for cleanup during compaction).
    tool_id_to_path: HashMap<String, PathBuf>,
    /// Current total content size in bytes.
    current_size_bytes: usize,
}

/// Tracks files that have been read or modified.
///
/// This is the unified file tracker for the agent system, handling:
/// - Read state tracking (content, mtime, access patterns)
/// - Modification tracking
/// - Change detection (comparing current mtime to read-time mtime)
/// - Nested memory triggers (CLAUDE.md, AGENTS.md, etc.)
/// - Tool call ID to file path mapping (for compaction cleanup)
/// - LRU eviction with size limits
///
/// # Interior Mutability
///
/// Uses `RwLock` internally to allow shared access (`&self`) for all operations.
/// This enables:
/// - Concurrent reads via `read()` guard
/// - Exclusive writes via `write()` guard
/// - Snapshot generation without blocking writes for long
///
/// # LRU Eviction
///
/// The tracker uses an LRU cache with configurable limits:
/// - Maximum 100 entries (configurable via `with_limits`)
/// - Maximum ~25MB total content size (configurable via `with_max_size_bytes`)
///
/// When limits are exceeded, oldest entries are evicted automatically.
#[derive(Debug)]
pub struct FileTracker {
    /// Internal state protected by RwLock for interior mutability.
    state: std::sync::RwLock<TrackerState>,
    /// Maximum total content size in bytes (default: ~25MB).
    max_size_bytes: usize,
}

impl Default for FileTracker {
    fn default() -> Self {
        Self::with_config(FileTrackerConfig::default())
    }
}

impl FileTracker {
    /// Acquire a read guard, recovering from lock poisoning.
    fn read_guard(&self) -> std::sync::RwLockReadGuard<'_, TrackerState> {
        self.state.read().unwrap_or_else(|e| {
            tracing::warn!("Lock poisoned — concurrent bug detected");
            e.into_inner()
        })
    }

    /// Acquire a write guard, recovering from lock poisoning.
    fn write_guard(&self) -> std::sync::RwLockWriteGuard<'_, TrackerState> {
        self.state.write().unwrap_or_else(|e| {
            tracing::warn!("Lock poisoned — concurrent bug detected");
            e.into_inner()
        })
    }

    /// Create a new file tracker with default limits (100 entries, ~25MB).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a file tracker with a configuration.
    ///
    /// This is the preferred constructor for explicit control over limits.
    pub fn with_config(config: FileTrackerConfig) -> Self {
        Self::with_limits(config.max_entries, config.max_total_bytes)
    }

    /// Create a file tracker with custom limits.
    ///
    /// # Arguments
    /// * `max_entries` - Maximum number of files to track (sets LRU capacity)
    /// * `max_size_bytes` - Maximum total content size in bytes
    pub fn with_limits(max_entries: usize, max_size_bytes: usize) -> Self {
        // SAFETY: max(1) guarantees the value is at least 1
        let capacity = NonZeroUsize::new(max_entries.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            state: std::sync::RwLock::new(TrackerState {
                read_files: LruCache::new(capacity),
                modified_files: HashSet::new(),
                nested_memory_triggers: HashSet::new(),
                tool_id_to_path: HashMap::new(),
                current_size_bytes: 0,
            }),
            max_size_bytes,
        }
    }

    /// Create a file tracker with capacity (for pre-allocation).
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_limits(capacity, 26_214_400)
    }

    /// Get the current number of tracked files.
    pub fn len(&self) -> usize {
        self.read_guard().read_files.len()
    }

    /// Check if the tracker is empty.
    pub fn is_empty(&self) -> bool {
        self.read_guard().read_files.is_empty()
    }

    /// Get current total content size in bytes.
    pub fn current_size(&self) -> usize {
        self.read_guard().current_size_bytes
    }

    /// Get all read files with their state for syncing to another tracker.
    ///
    /// This is used to sync file read state to the system-reminder's FileTracker
    /// for change detection.
    ///
    /// Returns owned data (cloned) to avoid holding the read lock.
    pub fn read_files_with_state(&self) -> Vec<(PathBuf, FileReadState)> {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Record a file read (simple — backward-compatible).
    pub fn record_read(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        // Skip internal files (session memory, plan files, etc.)
        if Self::is_internal_file(&path) {
            return;
        }
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.get_mut(&path) {
            read_state.access_count += 1;
            read_state.timestamp = SystemTime::now();
        } else {
            drop(state); // Release write lock before re-acquiring with eviction
            self.insert_with_eviction(
                path,
                FileReadState {
                    content: None,
                    timestamp: SystemTime::now(),
                    file_mtime: None,
                    content_hash: None,
                    offset: None,
                    limit: None,
                    kind: FileReadKind::MetadataOnly,
                    access_count: 1,
                    read_turn: 0,
                },
            );
        }
    }

    /// Record a file read with full state.
    pub fn record_read_with_state(&self, path: impl Into<PathBuf>, read_state: FileReadState) {
        let path = path.into();
        // Skip internal files (session memory, plan files, etc.)
        if Self::is_internal_file(&path) {
            return;
        }
        self.insert_with_eviction(path, read_state);
    }

    /// Insert a file with state, handling LRU eviction.
    fn insert_with_eviction(&self, path: PathBuf, read_state: FileReadState) {
        let content_size = read_state.content.as_ref().map(String::len).unwrap_or(0);
        let max_size = self.max_size_bytes;

        let mut state = self.write_guard();

        // Check if we need to evict entries for size
        while state.current_size_bytes + content_size > max_size && !state.read_files.is_empty() {
            // Evict oldest entry
            if let Some((_, old_state)) = state.read_files.pop_lru() {
                let old_size = old_state.content.as_ref().map(String::len).unwrap_or(0);
                state.current_size_bytes = state.current_size_bytes.saturating_sub(old_size);
            }
        }

        // If this path already exists, update the size accounting
        if let Some(old_state) = state.read_files.peek(&path) {
            let old_size = old_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes = state.current_size_bytes.saturating_sub(old_size);
        }

        // Add the new size
        state.current_size_bytes += content_size;

        // Insert the entry
        state.read_files.put(path, read_state);
    }

    /// Record a file modification.
    pub fn record_modified(&self, path: impl Into<PathBuf>) {
        let mut state = self.write_guard();
        state.modified_files.insert(path.into());
    }

    /// Check if a file has been read.
    pub fn was_read(&self, path: &Path) -> bool {
        let state = self.read_guard();
        state.read_files.contains(path)
    }

    /// Get the read state for a file (cloned to avoid holding lock).
    pub fn read_state(&self, path: &Path) -> Option<FileReadState> {
        let state = self.read_guard();
        state.read_files.peek(path).cloned()
    }

    /// Check if a file has been modified.
    pub fn was_modified(&self, path: &Path) -> bool {
        let state = self.read_guard();
        state.modified_files.contains(path)
    }

    /// Get all read file paths.
    pub fn read_files(&self) -> Vec<PathBuf> {
        let state = self.read_guard();
        state.read_files.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Get all modified files.
    pub fn modified_files(&self) -> HashSet<PathBuf> {
        let state = self.read_guard();
        state.modified_files.clone()
    }

    /// Track a file read with full state.
    ///
    /// Returns `true` if this file triggers nested memory lookup
    /// (e.g., CLAUDE.md, AGENTS.md files).
    pub fn track_read(&self, path: impl Into<PathBuf>, read_state: FileReadState) -> bool {
        let path = path.into();
        let is_memory_trigger = Self::is_nested_memory_trigger(&path);

        self.insert_with_eviction(path.clone(), read_state);

        if is_memory_trigger {
            let mut state = self.state.write().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            state.nested_memory_triggers.insert(path);
            true
        } else {
            false
        }
    }

    /// Check if a file has changed since it was last read.
    ///
    /// Returns `None` if the file isn't tracked.
    /// Skips change detection for partial reads.
    ///
    /// A file is considered changed if its current mtime differs from the stored mtime.
    /// This uses exact comparison (not just `new > old`) to detect any modification.
    pub fn has_file_changed(&self, path: &Path) -> Option<bool> {
        // Get state under read lock, then release lock before filesystem access
        let (file_mtime, content_hash, is_partial) = {
            let state = self.state.read().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            let read_state = state.read_files.peek(path)?;
            let is_partial = read_state.is_partial();
            (
                read_state.file_mtime,
                read_state.content_hash.clone(),
                is_partial,
            )
        };

        // Skip partial reads - can't reliably detect changes
        if is_partial {
            return Some(false);
        }

        // Check modification time - exact match means unchanged
        let current_mtime = std::fs::metadata(path).ok()?.modified().ok();

        match (file_mtime, current_mtime) {
            (Some(old), Some(new)) => Some(new != old), // Changed if mtime differs
            (None, Some(_)) => Some(true),              // File now has mtime
            (Some(_), None) => Some(true),              // File lost mtime (weird but changed)
            (None, None) => {
                // Fall back to content comparison
                let current_content = std::fs::read_to_string(path).ok()?;
                let current_hash = FileReadState::compute_hash(&current_content);
                Some(Some(&current_hash) != content_hash.as_ref())
            }
        }
    }

    /// Check if a file is unchanged since it was last read.
    ///
    /// Returns `None` if the file isn't tracked or can't be checked.
    /// Returns `Some(true)` if the file's current mtime exactly matches the stored mtime.
    /// This is used for already-read-files detection to skip re-reading unchanged files.
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's behavior: an exact mtime match indicates
    /// the file hasn't been modified since it was read. This is more precise than
    /// just checking `new > old` because it catches any change, not just newer.
    pub fn is_unchanged(&self, path: &Path) -> Option<bool> {
        // Get state under read lock, then release lock before filesystem access
        let (file_mtime, content_hash, is_partial) = {
            let state = self.state.read().unwrap_or_else(|e| {
                tracing::warn!("Lock poisoned — concurrent bug detected");
                e.into_inner()
            });
            let read_state = state.read_files.peek(path)?;
            let is_partial = read_state.is_partial();
            (
                read_state.file_mtime,
                read_state.content_hash.clone(),
                is_partial,
            )
        };

        // Partial reads are NOT cacheable - return None
        // This ensures @mentioned files with partial reads are always re-read
        if is_partial {
            return None;
        }

        // Check modification time - exact match means unchanged
        let current_mtime = std::fs::metadata(path).ok()?.modified().ok();

        match (file_mtime, current_mtime) {
            (Some(old), Some(new)) => Some(new == old), // Unchanged only if exact match
            (None, None) => {
                // No mtime available, fall back to content hash comparison
                let current_content = std::fs::read_to_string(path).ok()?;
                let current_hash = FileReadState::compute_hash(&current_content);
                Some(Some(&current_hash) == content_hash.as_ref())
            }
            // If we had no mtime before but do now, or vice versa, consider it potentially changed
            _ => None,
        }
    }

    /// Get all tracked file paths.
    pub fn tracked_files(&self) -> Vec<PathBuf> {
        let state = self.read_guard();
        state.read_files.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Get files that have changed since last read.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.tracked_files()
            .into_iter()
            .filter(|p| self.has_file_changed(p) == Some(true))
            .collect()
    }

    /// Update the modification time for a file after editing.
    pub fn update_modified_time(&self, path: &Path) {
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.get_mut(path)
            && let Ok(meta) = std::fs::metadata(path)
        {
            read_state.file_mtime = meta.modified().ok();
        }
    }

    /// Remove tracking for a file.
    pub fn remove(&self, path: &Path) {
        let mut state = self.write_guard();
        if let Some(read_state) = state.read_files.pop(path) {
            let size = read_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes = state.current_size_bytes.saturating_sub(size);
        }
        state.nested_memory_triggers.remove(path);
    }

    /// Enforce an entry limit by evicting the least-recently-used entries.
    ///
    /// Pops LRU entries until the count is at most `max_entries`.
    pub fn enforce_entry_limit(&self, max_entries: usize) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.read_files.len() > max_entries {
            if let Some((_path, evicted)) = state.read_files.pop_lru() {
                if let Some(content) = &evicted.content {
                    state.current_size_bytes =
                        state.current_size_bytes.saturating_sub(content.len());
                }
            } else {
                break;
            }
        }
    }

    /// Clear all tracked files.
    pub fn clear(&self) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.modified_files.clear();
        state.nested_memory_triggers.clear();
        state.current_size_bytes = 0;
    }

    /// Get and clear nested memory trigger paths.
    ///
    /// Returns paths that need nested memory lookup, then clears them.
    pub fn drain_nested_memory_triggers(&self) -> HashSet<PathBuf> {
        let mut state = self.write_guard();
        std::mem::take(&mut state.nested_memory_triggers)
    }

    /// Check if there are pending nested memory triggers.
    pub fn has_nested_memory_triggers(&self) -> bool {
        let state = self.read_guard();
        !state.nested_memory_triggers.is_empty()
    }

    /// Check if a path triggers nested memory lookup.
    fn is_nested_memory_trigger(path: &Path) -> bool {
        let filename = path.file_name().and_then(|n| n.to_str());
        matches!(
            filename,
            Some("CLAUDE.md" | "AGENTS.md" | "settings.json" | ".cursorrules" | ".aider.conf.yml")
        )
    }

    /// Check if a path is an internal file that shouldn't be tracked for compaction.
    ///
    /// Internal files include session memory files, plan files, and other system files
    /// that shouldn't be preserved during compaction restoration.
    fn is_internal_file(path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Session memory file
        if path_str.contains("session-memory") && path_str.contains("summary.md") {
            return true;
        }

        // Plan files (in ~/.cocode/plans/)
        if path_str.contains(".cocode/plans/") {
            return true;
        }

        // Auto memory files (MEMORY.md or project memory)
        if let Some(filename) = path.file_name().and_then(|n| n.to_str())
            && (filename == "MEMORY.md" || filename.starts_with("memory-"))
        {
            return true;
        }

        // Tool result persistence files
        if path_str.contains("tool-results/") {
            return true;
        }

        false
    }

    /// Register a file read with its tool call ID for compaction cleanup.
    ///
    /// When micro-compact removes tool results, this mapping allows
    /// cleaning up the corresponding FileTracker entries.
    pub fn register_tool_read(&self, tool_call_id: String, path: PathBuf) {
        let mut state = self.write_guard();
        state.tool_id_to_path.insert(tool_call_id, path);
    }

    /// Clean up file tracker entries for compacted tool call IDs.
    ///
    /// Called after micro-compaction to remove entries for compacted reads.
    pub fn cleanup_compacted(&self, compacted_ids: &[String]) {
        let mut state = self.write_guard();
        for id in compacted_ids {
            if let Some(path) = state.tool_id_to_path.remove(id) {
                // Remove from read_files if present
                if let Some(read_state) = state.read_files.pop(&path) {
                    let size = read_state.content.as_ref().map(String::len).unwrap_or(0);
                    state.current_size_bytes = state.current_size_bytes.saturating_sub(size);
                }
                state.nested_memory_triggers.remove(&path);
            }
        }
    }

    /// Get the mapping of tool call IDs to paths (for testing/debugging).
    pub fn tool_id_paths(&self) -> HashMap<String, PathBuf> {
        let state = self.read_guard();
        state.tool_id_to_path.clone()
    }

    /// Get the most recent files sorted by timestamp (for compaction restoration).
    ///
    /// Returns up to `limit` file paths sorted by most recent access.
    pub fn most_recent_files(&self, limit: usize) -> Vec<PathBuf> {
        let state = self.read_guard();
        let mut files: Vec<_> = state
            .read_files
            .iter()
            .filter(|(_, read_state)| read_state.content.is_some())
            .collect::<Vec<_>>();

        // Sort by timestamp (most recent first)
        files.sort_by(|a, b| {
            b.1.timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .cmp(
                    &a.1.timestamp
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default(),
                )
        });

        files
            .into_iter()
            .take(limit)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Check if a file has been fully read and is unchanged.
    ///
    /// This is the key method for already-read detection:
    /// - Returns `false` if file not tracked
    /// - Returns `false` if file was partially read (offset/limit)
    /// - Returns `false` if file was metadata-only (Glob/Grep)
    /// - Returns `true` only if full content was read AND file is unchanged
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's `is_already_read_unchanged` behavior:
    /// Only `FullContent` reads are considered "already read" for @mention purposes.
    /// Partial reads and metadata-only entries (from Glob/Grep) are NOT cacheable.
    pub fn is_already_read_unchanged(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let state = self.read_guard();
        let Some(read_state) = state.read_files.peek(path) else {
            return false;
        };

        // Only full content reads are cacheable
        if !read_state.is_full() {
            return false;
        }

        drop(state); // Release lock before filesystem access in has_file_changed
        self.has_file_changed(path) == Some(false)
    }

    /// Get the read state for a file.
    ///
    /// Returns `None` if the file is not tracked.
    pub fn get_state(&self, path: impl AsRef<Path>) -> Option<FileReadState> {
        let state = self.read_guard();
        state.read_files.peek(path.as_ref()).cloned()
    }

    /// Create a snapshot of all tracked files.
    ///
    /// Used for rewind recovery - captures all file read states.
    pub fn snapshot(&self) -> Vec<(PathBuf, FileReadState)> {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect()
    }

    /// Create a read-only snapshot of all tracked files.
    ///
    /// Identical to [`snapshot()`] but named to clarify intent: this is a
    /// point-in-time copy used for building derived tracker views.
    /// No LRU promotion occurs.
    pub fn read_files_snapshot(&self) -> Vec<(PathBuf, FileReadState)> {
        self.snapshot()
    }

    /// Replace all tracked files from a snapshot.
    ///
    /// Used for rewind recovery - restores file read states.
    pub fn replace_snapshot(&self, entries: Vec<(PathBuf, FileReadState)>) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.current_size_bytes = 0;

        for (path, read_state) in entries {
            let content_size = read_state.content.as_ref().map(String::len).unwrap_or(0);
            state.current_size_bytes += content_size;
            state.read_files.put(path, read_state);
        }
    }

    /// Remove tracking for multiple paths.
    ///
    /// Used for compaction cleanup - removes entries for compacted reads.
    pub fn remove_paths(&self, paths: &[PathBuf]) {
        for path in paths {
            self.remove(path);
        }
    }

    /// Clear all read file tracking (keep modified files).
    ///
    /// Used for full reset during rewind.
    pub fn clear_reads(&self) {
        let mut state = self.write_guard();
        state.read_files.clear();
        state.nested_memory_triggers.clear();
        state.current_size_bytes = 0;
    }

    /// Get the number of tracked files.
    ///
    /// Returns the count of files currently in the read cache.
    pub fn read_count(&self) -> usize {
        let state = self.read_guard();
        state.read_files.len()
    }

    // ========================================================================
    // Token Estimation (Claude Code v2.1.38 alignment)
    // ========================================================================

    /// Estimate token count for content using the canonical formula.
    ///
    /// Delegates to `cocode_protocol::estimate_text_tokens` which uses
    /// `ceil(len / 3.0)` (~3 characters per token).
    pub fn estimate_tokens(content: &str) -> usize {
        cocode_protocol::estimate_text_tokens(content) as usize
    }

    /// Estimate token count for a tracked file.
    ///
    /// Returns the estimated tokens for a file's content, or 0 if not tracked
    /// or if content is not available.
    pub fn estimate_file_tokens(&self, path: &Path) -> usize {
        let state = self.read_guard();
        state
            .read_files
            .peek(path)
            .and_then(|read_state| read_state.content.as_ref())
            .map(|c| Self::estimate_tokens(c))
            .unwrap_or(0)
    }

    /// Get total estimated tokens for all tracked files.
    ///
    /// Sums up the estimated tokens for all files with content in the tracker.
    pub fn total_estimated_tokens(&self) -> usize {
        let state = self.read_guard();
        state
            .read_files
            .iter()
            .filter_map(|(_, read_state)| read_state.content.as_ref())
            .map(|c| Self::estimate_tokens(c))
            .sum()
    }
}

/// Context for tool execution.
///
/// This provides everything a tool needs during execution:
/// - Call identification (call_id, turn_id, session_id, agent_id)
/// - Working directory and additional directories
/// - Permission mode and approvals
/// - Event channel for progress updates
/// - Cancellation support
/// - File tracking with content/timestamp validation
/// - Subagent spawning capability
/// - Plan mode state for Write/Edit permission checks
/// - Background task registry for Bash background execution
/// - LSP server manager for language intelligence
/// - Session directory for persisting large tool results
#[derive(Clone)]
pub struct ToolContext {
    /// Unique call ID for this execution.
    pub call_id: String,
    /// Session ID.
    pub session_id: String,
    /// Turn ID for the current conversation turn.
    pub turn_id: String,
    /// Turn number for the current conversation turn (1-indexed).
    pub turn_number: i32,
    /// Agent ID (set when running inside a sub-agent).
    pub agent_id: Option<String>,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Additional working directories (e.g., for multi-root workspaces).
    pub additional_working_directories: Vec<PathBuf>,
    /// Permission mode for this execution.
    pub permission_mode: PermissionMode,
    /// Channel for emitting core events.
    pub event_tx: Option<mpsc::Sender<CoreEvent>>,
    /// Cancellation token for aborting execution.
    pub cancel_token: CancellationToken,
    /// Stored approvals.
    pub approval_store: Arc<Mutex<ApprovalStore>>,
    /// File tracker.
    pub file_tracker: Arc<Mutex<FileTracker>>,
    /// Optional callback for spawning subagents.
    pub spawn_agent_fn: Option<SpawnAgentFn>,
    /// Shared registry of cancellation tokens for background agents.
    ///
    /// TaskStop uses this to cancel agents by ID. Tokens are registered
    /// by the executor when spawning subagents.
    pub agent_cancel_tokens: AgentCancelTokens,
    /// Shared set of agent IDs killed via TaskStop.
    ///
    /// Populated by `kill_shell.rs` after cancelling an agent so the session
    /// layer can report the agent's status as `Killed` instead of `Failed`.
    pub killed_agents: KilledAgents,
    /// Base directory for background agent output files.
    ///
    /// Used by TaskOutput to find agent output JSONL files. When set, this
    /// takes precedence over the fallback session_dir and temp_dir checks.
    pub agent_output_dir: Option<PathBuf>,
    /// Optional lightweight model call function (for SmartEdit correction).
    pub model_call_fn: Option<ModelCallFn>,
    /// Whether plan mode is currently active.
    pub is_plan_mode: bool,
    /// Whether this is an ultraplan session (plan pre-written by a remote session).
    pub is_ultraplan: bool,
    /// Path to the current plan file (if in plan mode).
    pub plan_file_path: Option<PathBuf>,
    /// Auto memory directory path (for write permission bypass).
    pub auto_memory_dir: Option<PathBuf>,
    /// Shell executor for command execution and background task management.
    pub shell_executor: ShellExecutor,
    /// Sandbox state for platform-level command isolation.
    pub sandbox_state: Option<Arc<cocode_sandbox::SandboxState>>,
    /// Optional LSP server manager for language intelligence tools.
    pub lsp_manager: Option<Arc<LspServerManager>>,
    /// Optional skill manager for executing named skills.
    pub skill_manager: Option<Arc<SkillManager>>,
    /// Optional skill usage tracker for recording invocations.
    pub skill_usage_tracker: Option<Arc<SkillUsageTracker>>,
    /// Optional hook registry for skill hook integration.
    pub hook_registry: Option<Arc<HookRegistry>>,
    /// Skills that have been invoked (for hook cleanup).
    pub invoked_skills: Arc<Mutex<Vec<InvokedSkill>>>,
    /// Session directory for storing tool results.
    ///
    /// Large tool results (>400K chars by default) are persisted here with only
    /// a preview kept in context. Typical path: `~/.cocode/sessions/{session_id}/`
    pub session_dir: Option<PathBuf>,
    /// Parent's role selections (snapshot for subagent isolation).
    ///
    /// When set, spawned subagents will inherit these selections,
    /// ensuring they're unaffected by subsequent changes to the parent's settings.
    pub parent_selections: Option<RoleSelections>,
    /// Optional permission requester for interactive approval flow.
    ///
    /// When set, the executor can route `NeedsApproval` results to the
    /// UI/TUI for user confirmation instead of denying immediately.
    pub permission_requester: Option<Arc<dyn PermissionRequester>>,
    /// Optional permission rule evaluator for pre-configured rules.
    ///
    /// When set, rules are evaluated before the tool's own `check_permission()`
    /// to allow, deny, or delegate based on project/user/policy configuration.
    pub permission_evaluator: Option<PermissionRuleEvaluator>,
    /// Feature flags for tool enablement checks.
    pub features: Features,
    /// Web search configuration.
    pub web_search_config: WebSearchConfig,
    /// Web fetch configuration.
    pub web_fetch_config: WebFetchConfig,
    /// Allowed subagent types for the Task tool.
    ///
    /// When set (from `Task(type1, type2)` syntax in the agent's tools list),
    /// only the specified subagent types can be spawned. `None` means no
    /// restriction — all agent types are available.
    pub task_type_restrictions: Option<Vec<String>>,
    /// Optional file backup store for pre-modify snapshots (Tier 1 rewind).
    pub file_backup_store: Option<Arc<cocode_file_backup::FileBackupStore>>,
    /// Optional question responder for AskUserQuestion tool.
    ///
    /// When set, the AskUserQuestion tool can emit a `QuestionAsked` event
    /// and wait for the user's structured response via a oneshot channel.
    pub question_responder: Option<Arc<QuestionResponder>>,
    /// Path to the cocode home directory (e.g. `~/.cocode`).
    ///
    /// Used for durable cron persistence and other session-scoped file operations.
    pub cocode_home: Option<PathBuf>,
    /// Per-task byte offsets for incremental (delta) output reading.
    ///
    /// TaskOutput stores the last-read byte offset per task_id so subsequent
    /// reads only return new entries, matching CC's `readOutputFileDelta`.
    pub output_offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
}

impl ToolContext {
    /// Create a new tool context.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>, cwd: PathBuf) -> Self {
        let shell_executor = ShellExecutor::new(cwd.clone());
        Self {
            call_id: call_id.into(),
            session_id: session_id.into(),
            turn_id: String::new(),
            turn_number: 0,
            agent_id: None,
            cwd,
            additional_working_directories: Vec::new(),
            permission_mode: PermissionMode::Default,
            event_tx: None,
            cancel_token: CancellationToken::new(),
            approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
            file_tracker: Arc::new(Mutex::new(FileTracker::new())),
            spawn_agent_fn: None,
            agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            killed_agents: Arc::new(Mutex::new(HashSet::new())),
            agent_output_dir: None,
            model_call_fn: None,
            is_plan_mode: false,
            is_ultraplan: false,
            plan_file_path: None,
            auto_memory_dir: None,
            shell_executor,
            sandbox_state: None,
            lsp_manager: None,
            skill_manager: None,
            skill_usage_tracker: None,
            hook_registry: None,
            invoked_skills: Arc::new(Mutex::new(Vec::new())),
            session_dir: None,
            parent_selections: None,
            permission_requester: None,
            permission_evaluator: None,
            features: Features::with_defaults(),
            web_search_config: WebSearchConfig::default(),
            web_fetch_config: WebFetchConfig::default(),
            task_type_restrictions: None,
            file_backup_store: None,
            question_responder: None,
            cocode_home: None,
            output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the permission mode.
    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Set the event channel.
    pub fn with_event_tx(mut self, tx: mpsc::Sender<CoreEvent>) -> Self {
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

    /// Set the turn ID.
    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = turn_id.into();
        self
    }

    /// Set the turn number.
    pub fn with_turn_number(mut self, turn_number: i32) -> Self {
        self.turn_number = turn_number;
        self
    }

    /// Set the agent ID.
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Set additional working directories.
    pub fn with_additional_working_directories(mut self, dirs: Vec<PathBuf>) -> Self {
        self.additional_working_directories = dirs;
        self
    }

    /// Set the spawn agent callback.
    pub fn with_spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn with_agent_cancel_tokens(mut self, tokens: AgentCancelTokens) -> Self {
        self.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn with_killed_agents(mut self, killed: KilledAgents) -> Self {
        self.killed_agents = killed;
        self
    }

    /// Set the agent output directory.
    pub fn with_agent_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.agent_output_dir = Some(dir.into());
        self
    }

    /// Set the model call function for single-shot LLM calls.
    pub fn with_model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.model_call_fn = Some(f);
        self
    }

    /// Set plan mode state.
    pub fn with_plan_mode(mut self, is_active: bool, plan_file_path: Option<PathBuf>) -> Self {
        self.is_plan_mode = is_active;
        self.plan_file_path = plan_file_path;
        self
    }

    /// Set the auto memory directory for write permission bypass.
    pub fn with_auto_memory_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.auto_memory_dir = dir;
        self
    }

    /// Set the shell executor.
    pub fn with_shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor = executor;
        self
    }

    /// Set the sandbox state.
    pub fn with_sandbox_state(mut self, state: Arc<cocode_sandbox::SandboxState>) -> Self {
        self.sandbox_state = Some(state);
        self
    }

    /// Set the LSP server manager.
    pub fn with_lsp_manager(mut self, manager: Arc<LspServerManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Set the skill manager.
    pub fn with_skill_manager(mut self, manager: Arc<SkillManager>) -> Self {
        self.skill_manager = Some(manager);
        self
    }

    /// Set the skill usage tracker.
    pub fn with_skill_usage_tracker(mut self, tracker: Arc<SkillUsageTracker>) -> Self {
        self.skill_usage_tracker = Some(tracker);
        self
    }

    /// Set the hook registry.
    pub fn with_hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.hook_registry = Some(registry);
        self
    }

    /// Set the session directory for persisting large tool results.
    pub fn with_session_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.session_dir = Some(dir.into());
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn with_permission_requester(mut self, requester: Arc<dyn PermissionRequester>) -> Self {
        self.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn with_permission_evaluator(mut self, evaluator: PermissionRuleEvaluator) -> Self {
        self.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn with_question_responder(mut self, responder: Arc<QuestionResponder>) -> Self {
        self.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory path.
    pub fn with_cocode_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.cocode_home = Some(path.into());
        self
    }

    /// Check if a write to the given path is allowed in plan mode.
    ///
    /// Returns `true` if not in plan mode, or if the path is the plan file.
    /// Returns `false` if in plan mode and the path is not the plan file.
    pub fn plan_mode_allows_write(&self, path: &Path) -> bool {
        if !self.is_plan_mode {
            return true;
        }
        cocode_plan_mode::is_safe_file(path, self.plan_file_path.as_deref())
    }

    /// Check if a write to the given path should be auto-allowed
    /// because it's within the auto memory directory.
    pub fn auto_memory_allows_write(&self, path: &Path) -> bool {
        self.auto_memory_dir
            .as_deref()
            .is_some_and(|dir| cocode_auto_memory::is_auto_memory_path(path, dir))
    }

    /// Spawn a subagent using the configured callback.
    ///
    /// Returns an error if no spawn callback is configured.
    pub async fn spawn_agent(
        &self,
        input: SpawnAgentInput,
    ) -> std::result::Result<SpawnAgentResult, cocode_error::BoxedError> {
        let spawn_fn = self.spawn_agent_fn.as_ref().ok_or_else(|| {
            cocode_error::boxed_err(
                crate::error::tool_error::InternalSnafu {
                    message: "No spawn_agent_fn configured".to_string(),
                }
                .build(),
            )
        })?;
        spawn_fn(input).await
    }

    /// Check if agent spawning is available.
    pub fn can_spawn_agent(&self) -> bool {
        self.spawn_agent_fn.is_some()
    }

    /// Emit a core event.
    pub async fn emit_event(&self, event: CoreEvent) {
        if let Some(tx) = &self.event_tx
            && let Err(e) = tx.send(event).await
        {
            debug!("Failed to emit event: {e}");
        }
    }

    /// Emit tool progress.
    pub async fn emit_progress(&self, message: impl Into<String>) {
        self.emit_event(CoreEvent::Tui(TuiEvent::ToolProgress {
            call_id: self.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: None,
                bytes_processed: None,
                total_bytes: None,
            },
        }))
        .await;
    }

    /// Emit tool progress with percentage.
    pub async fn emit_progress_percent(&self, message: impl Into<String>, percentage: i32) {
        self.emit_event(CoreEvent::Tui(TuiEvent::ToolProgress {
            call_id: self.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: Some(percentage),
                bytes_processed: None,
                total_bytes: None,
            },
        }))
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

    /// Record a file read (simple — backward-compatible).
    pub async fn record_file_read(&self, path: impl Into<PathBuf>) {
        self.file_tracker.lock().await.record_read(path);
    }

    /// Record a file read with full state tracking.
    pub async fn record_file_read_with_state(
        &self,
        path: impl Into<PathBuf>,
        state: FileReadState,
    ) {
        self.file_tracker
            .lock()
            .await
            .record_read_with_state(path, state);
    }

    /// Register a file read with tool call ID for compaction cleanup.
    pub async fn register_file_read_id(&self, path: &Path) {
        let tracker = self.file_tracker.lock().await;
        tracker.register_tool_read(self.call_id.clone(), path.to_path_buf());
    }

    /// Record a file modification.
    pub async fn record_file_modified(&self, path: impl Into<PathBuf>) {
        self.file_tracker.lock().await.record_modified(path);
    }

    /// Check if a file was read.
    pub async fn was_file_read(&self, path: &Path) -> bool {
        self.file_tracker.lock().await.was_read(path)
    }

    /// Get the read state for a file.
    pub async fn file_read_state(&self, path: &Path) -> Option<FileReadState> {
        self.file_tracker.lock().await.read_state(path)
    }

    /// Check if a file was modified.
    pub async fn was_file_modified(&self, path: &Path) -> bool {
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

    /// Persist a permission rule to `~/.cocode/settings.local.json`.
    ///
    /// Called when the user selects "Allow always" — writes the pattern
    /// into `permissions.allow` so it's remembered across sessions.
    pub async fn persist_permission_rule(&self, tool_name: &str, pattern: &str) {
        let config_dir = cocode_config::default_config_dir();
        if let Err(e) = cocode_policy::persist_rule(&config_dir, tool_name, pattern).await {
            tracing::warn!("Failed to persist permission rule: {e}");
        }
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
            .field("turn_id", &self.turn_id)
            .field("agent_id", &self.agent_id)
            .field("cwd", &self.cwd)
            .field("permission_mode", &self.permission_mode)
            .field("is_cancelled", &self.is_cancelled())
            .field("is_plan_mode", &self.is_plan_mode)
            .field("plan_file_path", &self.plan_file_path)
            .field("auto_memory_dir", &self.auto_memory_dir)
            .field("lsp_manager", &self.lsp_manager.is_some())
            .field("skill_manager", &self.skill_manager.is_some())
            .field("session_dir", &self.session_dir)
            .field("permission_requester", &self.permission_requester.is_some())
            .field("permission_evaluator", &self.permission_evaluator.is_some())
            .finish_non_exhaustive()
    }
}

/// Builder for creating tool contexts.
pub struct ToolContextBuilder {
    call_id: String,
    session_id: String,
    turn_id: String,
    turn_number: i32,
    agent_id: Option<String>,
    cwd: PathBuf,
    additional_working_directories: Vec<PathBuf>,
    permission_mode: PermissionMode,
    event_tx: Option<mpsc::Sender<CoreEvent>>,
    cancel_token: CancellationToken,
    approval_store: Arc<Mutex<ApprovalStore>>,
    file_tracker: Arc<Mutex<FileTracker>>,
    spawn_agent_fn: Option<SpawnAgentFn>,
    agent_cancel_tokens: AgentCancelTokens,
    killed_agents: KilledAgents,
    agent_output_dir: Option<PathBuf>,
    model_call_fn: Option<ModelCallFn>,
    is_plan_mode: bool,
    is_ultraplan: bool,
    plan_file_path: Option<PathBuf>,
    auto_memory_dir: Option<PathBuf>,
    shell_executor: Option<ShellExecutor>,
    sandbox_state: Option<Arc<cocode_sandbox::SandboxState>>,
    lsp_manager: Option<Arc<LspServerManager>>,
    skill_manager: Option<Arc<SkillManager>>,
    skill_usage_tracker: Option<Arc<SkillUsageTracker>>,
    hook_registry: Option<Arc<HookRegistry>>,
    invoked_skills: Arc<Mutex<Vec<InvokedSkill>>>,
    session_dir: Option<PathBuf>,
    parent_selections: Option<RoleSelections>,
    permission_requester: Option<Arc<dyn PermissionRequester>>,
    permission_evaluator: Option<PermissionRuleEvaluator>,
    features: Features,
    web_search_config: WebSearchConfig,
    web_fetch_config: WebFetchConfig,
    task_type_restrictions: Option<Vec<String>>,
    file_backup_store: Option<Arc<cocode_file_backup::FileBackupStore>>,
    question_responder: Option<Arc<QuestionResponder>>,
    cocode_home: Option<PathBuf>,
    output_offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
}

impl ToolContextBuilder {
    /// Create a new builder.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            session_id: session_id.into(),
            turn_id: String::new(),
            turn_number: 0,
            agent_id: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            additional_working_directories: Vec::new(),
            permission_mode: PermissionMode::Default,
            event_tx: None,
            cancel_token: CancellationToken::new(),
            approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
            file_tracker: Arc::new(Mutex::new(FileTracker::new())),
            spawn_agent_fn: None,
            agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            killed_agents: Arc::new(Mutex::new(HashSet::new())),
            agent_output_dir: None,
            model_call_fn: None,
            is_plan_mode: false,
            is_ultraplan: false,
            plan_file_path: None,
            auto_memory_dir: None,
            shell_executor: None,
            sandbox_state: None,
            lsp_manager: None,
            skill_manager: None,
            skill_usage_tracker: None,
            hook_registry: None,
            invoked_skills: Arc::new(Mutex::new(Vec::new())),
            session_dir: None,
            parent_selections: None,
            permission_requester: None,
            permission_evaluator: None,
            features: Features::with_defaults(),
            web_search_config: WebSearchConfig::default(),
            web_fetch_config: WebFetchConfig::default(),
            task_type_restrictions: None,
            file_backup_store: None,
            question_responder: None,
            cocode_home: None,
            output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Set the turn ID.
    pub fn turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = turn_id.into();
        self
    }

    /// Set the turn number.
    pub fn turn_number(mut self, turn_number: i32) -> Self {
        self.turn_number = turn_number;
        self
    }

    /// Set the agent ID.
    pub fn agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Set additional working directories.
    pub fn additional_working_directories(mut self, dirs: Vec<PathBuf>) -> Self {
        self.additional_working_directories = dirs;
        self
    }

    /// Set the permission mode.
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Set the event channel.
    pub fn event_tx(mut self, tx: mpsc::Sender<CoreEvent>) -> Self {
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

    /// Set the spawn agent callback.
    pub fn spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn agent_cancel_tokens(mut self, tokens: AgentCancelTokens) -> Self {
        self.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn killed_agents(mut self, killed: KilledAgents) -> Self {
        self.killed_agents = killed;
        self
    }

    /// Set the agent output directory.
    pub fn agent_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.agent_output_dir = Some(dir.into());
        self
    }

    /// Set the model call function for single-shot LLM calls.
    pub fn model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.model_call_fn = Some(f);
        self
    }

    /// Set plan mode state.
    pub fn plan_mode(mut self, is_active: bool, plan_file_path: Option<PathBuf>) -> Self {
        self.is_plan_mode = is_active;
        self.plan_file_path = plan_file_path;
        self
    }

    /// Set whether this is an ultraplan session.
    pub fn is_ultraplan(mut self, is_ultraplan: bool) -> Self {
        self.is_ultraplan = is_ultraplan;
        self
    }

    /// Set the auto memory directory for write permission bypass.
    pub fn auto_memory_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.auto_memory_dir = dir;
        self
    }

    /// Set the shell executor.
    pub fn shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor = Some(executor);
        self
    }

    /// Set the sandbox state.
    pub fn sandbox_state(mut self, state: Arc<cocode_sandbox::SandboxState>) -> Self {
        self.sandbox_state = Some(state);
        self
    }

    /// Set the sandbox state from an Option (no-op if None).
    pub fn maybe_sandbox_state(mut self, state: Option<Arc<cocode_sandbox::SandboxState>>) -> Self {
        self.sandbox_state = state;
        self
    }

    /// Set the LSP server manager.
    pub fn lsp_manager(mut self, manager: Arc<LspServerManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Set the skill manager.
    pub fn skill_manager(mut self, manager: Arc<SkillManager>) -> Self {
        self.skill_manager = Some(manager);
        self
    }

    /// Set the hook registry.
    pub fn hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.hook_registry = Some(registry);
        self
    }

    /// Set a shared invoked skills tracker.
    ///
    /// When set, all tool contexts share the same invoked skills list,
    /// allowing the driver to read invoked skills after tool execution.
    pub fn invoked_skills(mut self, skills: Arc<Mutex<Vec<InvokedSkill>>>) -> Self {
        self.invoked_skills = skills;
        self
    }

    /// Set the session directory for persisting large tool results.
    pub fn session_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.session_dir = Some(dir.into());
        self
    }

    /// Set parent selections for subagent isolation.
    ///
    /// When spawning subagents via the Task tool, these selections will be
    /// passed to the subagent, ensuring it's unaffected by subsequent
    /// changes to the parent's model settings.
    pub fn parent_selections(mut self, selections: RoleSelections) -> Self {
        self.parent_selections = Some(selections);
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn permission_requester(mut self, requester: Arc<dyn PermissionRequester>) -> Self {
        self.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn permission_evaluator(mut self, evaluator: PermissionRuleEvaluator) -> Self {
        self.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the feature flags.
    pub fn features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    /// Set the web search configuration.
    pub fn web_search_config(mut self, config: WebSearchConfig) -> Self {
        self.web_search_config = config;
        self
    }

    /// Set the web fetch configuration.
    pub fn web_fetch_config(mut self, config: WebFetchConfig) -> Self {
        self.web_fetch_config = config;
        self
    }

    /// Set the file backup store for pre-modify snapshots.
    pub fn file_backup_store(mut self, store: Arc<cocode_file_backup::FileBackupStore>) -> Self {
        self.file_backup_store = Some(store);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn question_responder(mut self, responder: Arc<QuestionResponder>) -> Self {
        self.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory path.
    pub fn cocode_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.cocode_home = Some(path.into());
        self
    }

    /// Set the shared output offsets for delta reads.
    pub fn output_offsets(
        mut self,
        offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
    ) -> Self {
        self.output_offsets = offsets;
        self
    }

    /// Set allowed subagent types for the Task tool.
    pub fn task_type_restrictions(mut self, restrictions: Vec<String>) -> Self {
        self.task_type_restrictions = Some(restrictions);
        self
    }

    /// Build the context.
    pub fn build(self) -> ToolContext {
        let shell_executor = self
            .shell_executor
            .unwrap_or_else(|| ShellExecutor::new(self.cwd.clone()));
        ToolContext {
            call_id: self.call_id,
            session_id: self.session_id,
            turn_id: self.turn_id,
            turn_number: self.turn_number,
            agent_id: self.agent_id,
            cwd: self.cwd,
            additional_working_directories: self.additional_working_directories,
            permission_mode: self.permission_mode,
            event_tx: self.event_tx,
            cancel_token: self.cancel_token,
            approval_store: self.approval_store,
            file_tracker: self.file_tracker,
            spawn_agent_fn: self.spawn_agent_fn,
            agent_cancel_tokens: self.agent_cancel_tokens,
            killed_agents: self.killed_agents,
            agent_output_dir: self.agent_output_dir,
            model_call_fn: self.model_call_fn,
            is_plan_mode: self.is_plan_mode,
            is_ultraplan: self.is_ultraplan,
            plan_file_path: self.plan_file_path,
            auto_memory_dir: self.auto_memory_dir,
            shell_executor,
            sandbox_state: self.sandbox_state,
            lsp_manager: self.lsp_manager,
            skill_manager: self.skill_manager,
            skill_usage_tracker: self.skill_usage_tracker,
            hook_registry: self.hook_registry,
            invoked_skills: self.invoked_skills,
            session_dir: self.session_dir,
            parent_selections: self.parent_selections,
            permission_requester: self.permission_requester,
            permission_evaluator: self.permission_evaluator,
            features: self.features,
            web_search_config: self.web_search_config,
            web_fetch_config: self.web_fetch_config,
            task_type_restrictions: self.task_type_restrictions,
            file_backup_store: self.file_backup_store,
            question_responder: self.question_responder,
            cocode_home: self.cocode_home,
            output_offsets: self.output_offsets,
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
