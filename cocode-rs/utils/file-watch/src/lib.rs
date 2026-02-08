//! Generic, reusable file-watch infrastructure.
//!
//! Provides [`FileWatcher`] — a throttled, event-coalescing file watcher that
//! bridges OS filesystem events into typed domain events via caller-supplied
//! `classify` and `merge` closures.
//!
//! # Example
//!
//! ```no_run
//! use std::path::PathBuf;
//! use std::time::Duration;
//! use cocode_file_watch::{FileWatcherBuilder, RecursiveMode};
//!
//! #[derive(Debug, Clone)]
//! struct ConfigChanged(Vec<PathBuf>);
//!
//! let watcher = FileWatcherBuilder::new()
//!     .throttle_interval(Duration::from_millis(500))
//!     .build(
//!         |event| {
//!             let paths: Vec<PathBuf> = event.paths.clone();
//!             if paths.is_empty() { None } else { Some(ConfigChanged(paths)) }
//!         },
//!         |mut acc, new| { acc.0.extend(new.0); acc },
//!     )
//!     .unwrap();
//!
//! let mut rx = watcher.subscribe();
//! watcher.watch("/tmp/config".into(), RecursiveMode::Recursive);
//! ```

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::sleep_until;
use tracing::info;
use tracing::warn;

pub use notify::RecursiveMode;
use notify::Watcher;

const DEFAULT_THROTTLE_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_CHANNEL_CAPACITY: usize = 128;

// ---------------------------------------------------------------------------
// ThrottledPaths
// ---------------------------------------------------------------------------

/// Coalesces burst filesystem events, emitting at most once per interval.
pub struct ThrottledPaths {
    pending: HashSet<PathBuf>,
    next_allowed_at: Instant,
    interval: Duration,
}

impl ThrottledPaths {
    pub fn new(interval: Duration) -> Self {
        Self {
            pending: HashSet::new(),
            next_allowed_at: Instant::now(),
            interval,
        }
    }

    /// Add paths to the pending set.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.pending.extend(paths);
    }

    /// Return accumulated paths if the throttle window has elapsed.
    pub fn take_ready(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        if self.pending.is_empty() || now < self.next_allowed_at {
            return None;
        }
        Some(self.drain(now))
    }

    /// Return accumulated paths regardless of the throttle window (e.g. on shutdown).
    pub fn take_pending(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        if self.pending.is_empty() {
            return None;
        }
        Some(self.drain(now))
    }

    /// Deadline at which the next emission is allowed, or `None` if there is
    /// nothing pending or the window has already elapsed.
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        (!self.pending.is_empty() && now < self.next_allowed_at).then_some(self.next_allowed_at)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn drain(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.pending.drain().collect();
        paths.sort_unstable_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
        self.next_allowed_at = now + self.interval;
        paths
    }
}

// ---------------------------------------------------------------------------
// WatcherInner
// ---------------------------------------------------------------------------

struct WatcherInner {
    watcher: notify::RecommendedWatcher,
    watched_paths: HashMap<PathBuf, RecursiveMode>,
}

// ---------------------------------------------------------------------------
// FileWatcher<E>
// ---------------------------------------------------------------------------

/// Generic file watcher: bridges OS events into typed, throttled domain events.
///
/// `E` is the domain event type produced by the caller-supplied `classify`
/// closure and coalesced via `merge`.
pub struct FileWatcher<E> {
    inner: Option<Mutex<WatcherInner>>,
    tx: broadcast::Sender<E>,
}

impl<E: Clone + Send + 'static> FileWatcher<E> {
    /// Subscribe to domain events emitted by this watcher.
    pub fn subscribe(&self) -> broadcast::Receiver<E> {
        self.tx.subscribe()
    }

    /// Start watching `path` for filesystem changes.
    pub fn watch(&self, path: PathBuf, mode: RecursiveMode) {
        let Some(inner) = &self.inner else {
            return;
        };
        if !path.exists() {
            return;
        }
        let mut guard = match inner.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if let Some(existing) = guard.watched_paths.get(&path) {
            if *existing == RecursiveMode::Recursive || *existing == mode {
                return;
            }
            // Upgrading from NonRecursive → Recursive: unwatch first.
            if let Err(err) = guard.watcher.unwatch(&path) {
                warn!("failed to unwatch {}: {err}", path.display());
            }
        }
        if let Err(err) = guard.watcher.watch(&path, mode) {
            warn!("failed to watch {}: {err}", path.display());
            return;
        }
        guard.watched_paths.insert(path, mode);
    }

    /// Stop watching `path`.
    pub fn unwatch(&self, path: &Path) {
        let Some(inner) = &self.inner else {
            return;
        };
        let mut guard = match inner.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if guard.watched_paths.remove(path).is_some() {
            if let Err(err) = guard.watcher.unwatch(path) {
                warn!("failed to unwatch {}: {err}", path.display());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FileWatcherBuilder<E>
// ---------------------------------------------------------------------------

/// Builder for [`FileWatcher`].
pub struct FileWatcherBuilder<E> {
    throttle_interval: Duration,
    channel_capacity: usize,
    _marker: std::marker::PhantomData<E>,
}

impl<E: Clone + Send + 'static> Default for FileWatcherBuilder<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Clone + Send + 'static> FileWatcherBuilder<E> {
    pub fn new() -> Self {
        Self {
            throttle_interval: DEFAULT_THROTTLE_INTERVAL,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            _marker: std::marker::PhantomData,
        }
    }

    /// Set the throttle interval (default: 1 second).
    pub fn throttle_interval(mut self, interval: Duration) -> Self {
        self.throttle_interval = interval;
        self
    }

    /// Set the broadcast channel capacity (default: 128).
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Build a live watcher.
    ///
    /// - `classify`: maps a raw [`notify::Event`] to an optional domain event.
    /// - `merge`: combines two domain events accumulated during a throttle window.
    pub fn build<C, M>(self, classify: C, merge: M) -> notify::Result<FileWatcher<E>>
    where
        C: Fn(&notify::Event) -> Option<E> + Send + 'static,
        M: Fn(E, E) -> E + Send + 'static,
    {
        let (raw_tx, raw_rx) = mpsc::unbounded_channel();
        let raw_tx_clone = raw_tx;
        let watcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx_clone.send(res);
        })?;
        let (tx, _) = broadcast::channel(self.channel_capacity);
        let file_watcher = FileWatcher {
            inner: Some(Mutex::new(WatcherInner {
                watcher,
                watched_paths: HashMap::new(),
            })),
            tx: tx.clone(),
        };
        spawn_event_loop(raw_rx, tx, self.throttle_interval, classify, merge);
        Ok(file_watcher)
    }

    /// Build a no-op watcher (for tests). Subscribe returns a receiver that
    /// never fires; `watch`/`unwatch` are safe no-ops.
    pub fn build_noop(self) -> FileWatcher<E> {
        let (tx, _) = broadcast::channel(1);
        FileWatcher { inner: None, tx }
    }
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

fn spawn_event_loop<E, C, M>(
    mut raw_rx: mpsc::UnboundedReceiver<notify::Result<notify::Event>>,
    tx: broadcast::Sender<E>,
    throttle_interval: Duration,
    classify: C,
    merge: M,
) where
    E: Clone + Send + 'static,
    C: Fn(&notify::Event) -> Option<E> + Send + 'static,
    M: Fn(E, E) -> E + Send + 'static,
{
    let Ok(handle) = Handle::try_current() else {
        warn!("file watcher loop skipped: no Tokio runtime available");
        return;
    };
    handle.spawn(async move {
        let mut pending: Option<E> = None;
        let mut next_allowed_at = Instant::now();

        loop {
            let now = Instant::now();
            let has_pending = pending.is_some();
            let timer_deadline = if has_pending && now < next_allowed_at {
                next_allowed_at
            } else {
                // Far future — only wake on channel activity.
                now + Duration::from_secs(60 * 60 * 24 * 365)
            };
            let timer = sleep_until(timer_deadline);
            tokio::pin!(timer);

            tokio::select! {
                res = raw_rx.recv() => {
                    match res {
                        Some(Ok(event)) => {
                            info!(
                                event_kind = ?event.kind,
                                event_paths = ?event.paths,
                                "file watcher received filesystem event"
                            );
                            if let Some(classified) = classify(&event) {
                                pending = Some(match pending.take() {
                                    Some(acc) => merge(acc, classified),
                                    None => classified,
                                });
                            }
                            let now = Instant::now();
                            if now >= next_allowed_at {
                                if let Some(e) = pending.take() {
                                    let _ = tx.send(e);
                                    next_allowed_at = now + throttle_interval;
                                }
                            }
                        }
                        Some(Err(err)) => {
                            warn!("file watcher error: {err}");
                        }
                        None => {
                            // Channel closed — flush any pending event.
                            if let Some(e) = pending.take() {
                                let _ = tx.send(e);
                            }
                            break;
                        }
                    }
                }
                _ = &mut timer => {
                    let now = Instant::now();
                    if now >= next_allowed_at {
                        if let Some(e) = pending.take() {
                            let _ = tx.send(e);
                            next_allowed_at = now + throttle_interval;
                        }
                    }
                }
            }
        }
    });
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use pretty_assertions::assert_eq;
    use tokio::time::timeout;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    // -----------------------------------------------------------------------
    // ThrottledPaths
    // -----------------------------------------------------------------------

    #[test]
    fn throttle_first_emit_immediate() {
        let mut tp = ThrottledPaths::new(Duration::from_secs(1));
        let now = Instant::now();
        tp.add(vec![path("a")]);
        let result = tp.take_ready(now);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), vec![path("a")]);
    }

    #[test]
    fn throttle_coalesces_within_interval() {
        let mut tp = ThrottledPaths::new(Duration::from_secs(1));
        let start = Instant::now();

        tp.add(vec![path("a")]);
        let _ = tp.take_ready(start).unwrap();

        // Within the throttle window — should not emit.
        tp.add(vec![path("b"), path("c")]);
        assert!(tp.take_ready(start).is_none());

        // After the window — should emit coalesced.
        let later = start + Duration::from_secs(1);
        let result = tp.take_ready(later).unwrap();
        assert_eq!(result, vec![path("b"), path("c")]);
    }

    #[test]
    fn throttle_flushes_on_shutdown() {
        let mut tp = ThrottledPaths::new(Duration::from_secs(1));
        let start = Instant::now();

        tp.add(vec![path("a")]);
        let _ = tp.take_ready(start).unwrap();

        tp.add(vec![path("b")]);
        assert!(tp.take_ready(start).is_none());

        let flushed = tp.take_pending(start).unwrap();
        assert_eq!(flushed, vec![path("b")]);
    }

    #[test]
    fn throttle_configurable_interval() {
        let mut tp = ThrottledPaths::new(Duration::from_millis(200));
        let start = Instant::now();

        tp.add(vec![path("a")]);
        let _ = tp.take_ready(start).unwrap();

        tp.add(vec![path("b")]);
        // 100ms < 200ms interval — still throttled.
        assert!(tp.take_ready(start + Duration::from_millis(100)).is_none());
        // 200ms = interval — should emit.
        assert!(tp.take_ready(start + Duration::from_millis(200)).is_some());
    }

    #[test]
    fn throttle_is_empty() {
        let mut tp = ThrottledPaths::new(Duration::from_secs(1));
        assert!(tp.is_empty());
        tp.add(vec![path("a")]);
        assert!(!tp.is_empty());
        let _ = tp.take_ready(Instant::now());
        assert!(tp.is_empty());
    }

    #[test]
    fn throttle_next_deadline_none_when_empty() {
        let tp = ThrottledPaths::new(Duration::from_secs(1));
        assert!(tp.next_deadline(Instant::now()).is_none());
    }

    // -----------------------------------------------------------------------
    // FileWatcher (noop mode)
    // -----------------------------------------------------------------------

    #[test]
    fn noop_watch_unwatch_no_panic() {
        let watcher: FileWatcher<String> = FileWatcherBuilder::new().build_noop();
        watcher.watch(path("/nonexistent"), RecursiveMode::Recursive);
        watcher.unwatch(Path::new("/nonexistent"));
        let _rx = watcher.subscribe();
    }

    // -----------------------------------------------------------------------
    // FileWatcher (watch deduplication / upgrade)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn watch_deduplicates_paths() {
        let watcher: FileWatcher<Vec<PathBuf>> = FileWatcherBuilder::new()
            .build(
                |event| Some(event.paths.clone()),
                |mut acc, new| {
                    acc.extend(new);
                    acc
                },
            )
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Watching the same path twice should not error.
        watcher.watch(dir_path.clone(), RecursiveMode::NonRecursive);
        watcher.watch(dir_path.clone(), RecursiveMode::NonRecursive);

        // Verify internal state: only one entry.
        let inner = watcher.inner.as_ref().unwrap().lock().unwrap();
        assert_eq!(inner.watched_paths.len(), 1);
    }

    #[tokio::test]
    async fn watch_upgrades_recursive_mode() {
        let watcher: FileWatcher<Vec<PathBuf>> = FileWatcherBuilder::new()
            .build(
                |event| Some(event.paths.clone()),
                |mut acc, new| {
                    acc.extend(new);
                    acc
                },
            )
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        watcher.watch(dir_path.clone(), RecursiveMode::NonRecursive);
        watcher.watch(dir_path.clone(), RecursiveMode::Recursive);

        let inner = watcher.inner.as_ref().unwrap().lock().unwrap();
        assert_eq!(
            inner.watched_paths.get(&dir_path),
            Some(&RecursiveMode::Recursive)
        );
    }

    // -----------------------------------------------------------------------
    // Event loop integration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn event_loop_classifies_and_broadcasts() {
        // Build a watcher that classifies events by collecting paths.
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let (tx, mut rx) = broadcast::channel::<Vec<PathBuf>>(8);

        spawn_event_loop(
            raw_rx,
            tx,
            Duration::from_secs(1),
            |event| {
                let paths: Vec<PathBuf> = event.paths.clone();
                if paths.is_empty() { None } else { Some(paths) }
            },
            |mut acc, new| {
                acc.extend(new);
                acc
            },
        );

        let mut event = notify::Event::new(EventKind::Any);
        event = event.add_path(path("/tmp/a"));

        raw_tx.send(Ok(event)).unwrap();

        let received = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, vec![path("/tmp/a")]);
    }

    #[tokio::test]
    async fn event_loop_flushes_on_close() {
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let (tx, mut rx) = broadcast::channel::<Vec<PathBuf>>(8);

        spawn_event_loop(
            raw_rx,
            tx,
            Duration::from_secs(1),
            |event| {
                let paths: Vec<PathBuf> = event.paths.clone();
                if paths.is_empty() { None } else { Some(paths) }
            },
            |mut acc, new| {
                acc.extend(new);
                acc
            },
        );

        // First event is emitted immediately.
        let mut event1 = notify::Event::new(EventKind::Any);
        event1 = event1.add_path(path("/tmp/a"));
        raw_tx.send(Ok(event1)).unwrap();

        let first = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, vec![path("/tmp/a")]);

        // Second event is within the throttle window, so it's pending.
        let mut event2 = notify::Event::new(EventKind::Any);
        event2 = event2.add_path(path("/tmp/b"));
        raw_tx.send(Ok(event2)).unwrap();

        // Give the loop time to receive the event before dropping the sender.
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(raw_tx);

        // The pending event should be flushed on close.
        let second = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second, vec![path("/tmp/b")]);
    }
}
