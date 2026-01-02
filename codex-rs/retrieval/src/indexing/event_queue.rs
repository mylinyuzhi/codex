//! Generic event queue with key-based deduplication.
//!
//! Provides a queue for events that automatically deduplicates
//! and merges events for the same key (e.g., file path).
//!
//! ## Architecture
//!
//! Events are wrapped in `TrackedEvent` for batch tracking and observability:
//! - `batch_id`: Links event to a SessionStart batch (for completion tracking)
//! - `seq`: Sequence number for lag tracking (watermark mechanism)
//! - `trace_id`: For distributed tracing across the pipeline
//!
//! ## Merge Strategy
//!
//! When multiple events arrive for the same key, they are merged:
//! - Deleted has highest priority, always overwrites
//! - Modified overwrites Created (file created then immediately modified)
//! - Created is kept if file was deleted then recreated
//! - batch_id is preserved (not lost during merge)

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;
use tokio::sync::broadcast;

use super::BatchId;
use super::WatchEventKind;

/// Tracked event with batch and trace information.
///
/// Wraps the actual event data with metadata for:
/// - Batch tracking (SessionStart completion)
/// - Lag tracking (watermark mechanism)
/// - Distributed tracing (observability)
#[derive(Debug, Clone)]
pub struct TrackedEvent<T: Clone> {
    /// The actual event data.
    pub data: T,
    /// Batch ID for SessionStart events (None for Timer/Watcher).
    pub batch_id: Option<BatchId>,
    /// Sequence number for lag tracking (assigned by LagTracker).
    pub seq: i64,
    /// Trace ID for distributed tracing.
    pub trace_id: String,
    /// Timestamp when the event was created.
    pub timestamp: Instant,
}

impl<T: Clone> TrackedEvent<T> {
    /// Create a new tracked event.
    pub fn new(data: T, batch_id: Option<BatchId>, seq: i64, trace_id: String) -> Self {
        Self {
            data,
            batch_id,
            seq,
            trace_id,
            timestamp: Instant::now(),
        }
    }

    /// Create a simple event without batch tracking (for backward compatibility).
    pub fn simple(data: T) -> Self {
        Self {
            data,
            batch_id: None,
            seq: 0,
            trace_id: String::new(),
            timestamp: Instant::now(),
        }
    }
}

/// Merge function type for combining events with the same key.
pub type MergeFn<V> = Arc<dyn Fn(&V, &V) -> V + Send + Sync>;

/// Generic event queue with key-based deduplication.
///
/// When multiple events occur for the same key, they are merged using
/// the provided merge function. Events are processed in FIFO order
/// based on timestamp.
///
/// # Type Parameters
/// - `K`: Key type (e.g., `PathBuf` for file events)
/// - `V`: Value type (e.g., `WatchEventKind` for file change events)
pub struct EventQueue<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Key -> TrackedEvent mapping (deduplication by key).
    pending: RwLock<HashMap<K, TrackedEvent<V>>>,
    /// Notify channel to wake up workers.
    notify_tx: broadcast::Sender<()>,
    /// Function to merge two events with the same key.
    merge_fn: MergeFn<V>,
}

impl<K, V> EventQueue<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Create a new event queue with custom merge function.
    ///
    /// # Arguments
    /// * `capacity` - Broadcast channel capacity for notifications
    /// * `merge_fn` - Function to merge two events with the same key
    pub fn new(capacity: usize, merge_fn: MergeFn<V>) -> Self {
        let (notify_tx, _) = broadcast::channel(capacity);
        Self {
            pending: RwLock::new(HashMap::new()),
            notify_tx,
            merge_fn,
        }
    }

    /// Add an event to the queue (automatically deduplicates/merges).
    ///
    /// If an event for the same key already exists, merges using the merge function.
    /// The batch_id from the new event is preserved if present.
    pub async fn push(&self, key: K, event: TrackedEvent<V>) {
        let mut pending = self.pending.write().await;

        if let Some(existing) = pending.get(&key) {
            // Merge the data using the merge function
            let merged_data = (self.merge_fn)(&existing.data, &event.data);

            // Preserve batch_id: prefer new event's batch_id if present
            let batch_id = event.batch_id.or_else(|| existing.batch_id.clone());

            // Use the newer seq and trace_id
            let merged = TrackedEvent {
                data: merged_data,
                batch_id,
                seq: event.seq,
                trace_id: event.trace_id,
                timestamp: Instant::now(),
            };
            pending.insert(key, merged);
        } else {
            pending.insert(key, event);
        }

        // Notify workers that there's a new event
        let _ = self.notify_tx.send(());
    }

    /// Add a simple event without tracking info (backward compatibility).
    pub async fn push_simple(&self, key: K, data: V) {
        self.push(key, TrackedEvent::simple(data)).await;
    }

    /// Pop the oldest event from the queue (FIFO by timestamp).
    ///
    /// Returns `None` if the queue is empty.
    pub async fn pop(&self) -> Option<(K, TrackedEvent<V>)> {
        let mut pending = self.pending.write().await;
        let oldest = pending
            .iter()
            .min_by_key(|(_, event)| event.timestamp)
            .map(|(k, e)| (k.clone(), e.clone()));

        if let Some((key, event)) = oldest {
            pending.remove(&key);
            Some((key, event))
        } else {
            None
        }
    }

    /// Requeue an event (used when lock conflict occurs).
    ///
    /// The event will be merged with any existing event for the same key.
    pub async fn requeue(&self, key: K, event: TrackedEvent<V>) {
        self.push(key, event).await;
    }

    /// Subscribe to notifications for new events.
    ///
    /// Workers should call this to get a receiver that wakes them
    /// when new events are pushed.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.notify_tx.subscribe()
    }

    /// Get the current number of pending events.
    pub async fn len(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Check if the queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.pending.read().await.is_empty()
    }

    /// Clear all pending events.
    pub async fn clear(&self) {
        self.pending.write().await.clear();
    }

    /// Get all pending keys (for diagnostics).
    pub async fn pending_keys(&self) -> Vec<K> {
        self.pending.read().await.keys().cloned().collect()
    }
}

// ============================================================================
// Type aliases for specific event types
// ============================================================================

/// Default merge function for WatchEventKind.
///
/// Merge strategy:
/// - Deleted has highest priority, overwrites any
/// - Modified overwrites Created (file created then immediately modified)
/// - Created kept (file doesn't exist -> create)
pub fn watch_event_merge(existing: &WatchEventKind, new: &WatchEventKind) -> WatchEventKind {
    match (existing, new) {
        (_, WatchEventKind::Deleted) => WatchEventKind::Deleted,
        (WatchEventKind::Deleted, _) => new.clone(), // Deleted then recreated
        (WatchEventKind::Created, WatchEventKind::Modified) => WatchEventKind::Created,
        _ => new.clone(),
    }
}

/// Watch event queue with filepath-based deduplication.
///
/// This is the standard queue for file watch events.
pub type WatchEventQueue = EventQueue<PathBuf, WatchEventKind>;

/// Create a new watch event queue with default settings.
pub fn new_watch_event_queue(capacity: usize) -> WatchEventQueue {
    EventQueue::new(capacity, Arc::new(watch_event_merge))
}

/// Shared event queue wrapped in Arc for use across threads.
pub type SharedEventQueue = Arc<WatchEventQueue>;

// ============================================================================
// Tag event types for RepoMap pipeline
// ============================================================================

/// Tag extraction event kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagEventKind {
    /// Extract tags from newly created file.
    Created,
    /// Re-extract tags from modified file.
    Modified,
    /// Remove tags for deleted file.
    Deleted,
}

/// Default merge function for TagEventKind (same as WatchEventKind).
pub fn tag_event_merge(existing: &TagEventKind, new: &TagEventKind) -> TagEventKind {
    match (existing, new) {
        (_, TagEventKind::Deleted) => TagEventKind::Deleted,
        (TagEventKind::Deleted, _) => new.clone(),
        (TagEventKind::Created, TagEventKind::Modified) => TagEventKind::Created,
        _ => new.clone(),
    }
}

/// Tag event queue for RepoMap pipeline.
pub type TagEventQueue = EventQueue<PathBuf, TagEventKind>;

/// Create a new tag event queue with default settings.
pub fn new_tag_event_queue(capacity: usize) -> TagEventQueue {
    EventQueue::new(capacity, Arc::new(tag_event_merge))
}

/// Shared tag event queue wrapped in Arc.
pub type SharedTagEventQueue = Arc<TagEventQueue>;

// ============================================================================
// Backward compatibility wrapper
// ============================================================================

impl WatchEventQueue {
    /// Create a new watch event queue (backward compatible).
    pub fn new_compat(capacity: usize) -> Self {
        new_watch_event_queue(capacity)
    }
}

impl Default for WatchEventQueue {
    fn default() -> Self {
        new_watch_event_queue(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_push_and_pop() {
        let queue = new_watch_event_queue(16);

        queue
            .push_simple(PathBuf::from("file1.rs"), WatchEventKind::Created)
            .await;
        queue
            .push_simple(PathBuf::from("file2.rs"), WatchEventKind::Modified)
            .await;

        assert_eq!(queue.len().await, 2);

        let (path, event) = queue.pop().await.unwrap();
        assert!(path == PathBuf::from("file1.rs") || path == PathBuf::from("file2.rs"));
        assert!(event.data == WatchEventKind::Created || event.data == WatchEventKind::Modified);

        assert_eq!(queue.len().await, 1);
    }

    #[tokio::test]
    async fn test_dedup_same_path() {
        let queue = new_watch_event_queue(16);

        // Same path, multiple events
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Created)
            .await;
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Modified)
            .await;

        // Should only have one event (Created wins over Modified)
        assert_eq!(queue.len().await, 1);

        let (path, event) = queue.pop().await.unwrap();
        assert_eq!(path, PathBuf::from("file.rs"));
        assert_eq!(event.data, WatchEventKind::Created);
    }

    #[tokio::test]
    async fn test_deleted_wins() {
        let queue = new_watch_event_queue(16);

        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Created)
            .await;
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Deleted)
            .await;

        assert_eq!(queue.len().await, 1);

        let (_, event) = queue.pop().await.unwrap();
        assert_eq!(event.data, WatchEventKind::Deleted);
    }

    #[tokio::test]
    async fn test_deleted_then_created() {
        let queue = new_watch_event_queue(16);

        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Deleted)
            .await;
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Created)
            .await;

        assert_eq!(queue.len().await, 1);

        let (_, event) = queue.pop().await.unwrap();
        // Created overwrites Deleted (file was deleted then recreated)
        assert_eq!(event.data, WatchEventKind::Created);
    }

    #[tokio::test]
    async fn test_tracked_event_with_batch_id() {
        let queue = new_watch_event_queue(16);
        let batch_id = BatchId::new();

        let event = TrackedEvent::new(
            WatchEventKind::Modified,
            Some(batch_id.clone()),
            42,
            "trace-123".to_string(),
        );

        queue.push(PathBuf::from("file.rs"), event).await;

        let (_, popped) = queue.pop().await.unwrap();
        assert_eq!(popped.batch_id.unwrap().as_str(), batch_id.as_str());
        assert_eq!(popped.seq, 42);
        assert_eq!(popped.trace_id, "trace-123");
    }

    #[tokio::test]
    async fn test_batch_id_preserved_on_merge() {
        let queue = new_watch_event_queue(16);
        let batch_id = BatchId::new();

        // First event without batch_id
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Created)
            .await;

        // Second event with batch_id
        let event = TrackedEvent::new(
            WatchEventKind::Modified,
            Some(batch_id.clone()),
            1,
            "trace".to_string(),
        );
        queue.push(PathBuf::from("file.rs"), event).await;

        let (_, popped) = queue.pop().await.unwrap();
        // batch_id should be preserved
        assert!(popped.batch_id.is_some());
        assert_eq!(popped.batch_id.unwrap().as_str(), batch_id.as_str());
    }

    #[tokio::test]
    async fn test_requeue() {
        let queue = new_watch_event_queue(16);

        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Modified)
            .await;
        let (path, event) = queue.pop().await.unwrap();

        // Requeue the event
        queue.requeue(path.clone(), event).await;

        assert_eq!(queue.len().await, 1);
        let (p, e) = queue.pop().await.unwrap();
        assert_eq!(p, path);
        assert_eq!(e.data, WatchEventKind::Modified);
    }

    #[tokio::test]
    async fn test_empty_queue() {
        let queue = new_watch_event_queue(16);
        assert!(queue.is_empty().await);
        assert!(queue.pop().await.is_none());
    }

    #[tokio::test]
    async fn test_subscribe_notification() {
        let queue = new_watch_event_queue(16);
        let mut rx = queue.subscribe();

        // Push should notify
        queue
            .push_simple(PathBuf::from("file.rs"), WatchEventKind::Created)
            .await;

        // Should receive notification
        let result = rx.try_recv();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tag_event_queue() {
        let queue = new_tag_event_queue(16);

        queue
            .push_simple(PathBuf::from("file.rs"), TagEventKind::Created)
            .await;
        queue
            .push_simple(PathBuf::from("file.rs"), TagEventKind::Deleted)
            .await;

        let (_, event) = queue.pop().await.unwrap();
        assert_eq!(event.data, TagEventKind::Deleted);
    }

    #[tokio::test]
    async fn test_pending_keys() {
        let queue = new_watch_event_queue(16);

        queue
            .push_simple(PathBuf::from("a.rs"), WatchEventKind::Created)
            .await;
        queue
            .push_simple(PathBuf::from("b.rs"), WatchEventKind::Modified)
            .await;

        let keys = queue.pending_keys().await;
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&PathBuf::from("a.rs")));
        assert!(keys.contains(&PathBuf::from("b.rs")));
    }
}
