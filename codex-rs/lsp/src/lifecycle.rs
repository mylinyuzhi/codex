//! LSP Server Lifecycle Manager
//!
//! Handles server health monitoring, restart logic, and graceful degradation.

use crate::config::LifecycleConfig;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Maximum number of health check retries before declaring failure
const MAX_HEALTH_CHECK_RETRIES: i32 = 3;

/// Delay between health check retries in milliseconds
const HEALTH_CHECK_RETRY_DELAY_MS: u64 = 1000;

/// Server health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerHealth {
    /// Server is running and responding
    Healthy,
    /// Server is starting up
    Starting,
    /// Server crashed, restart pending
    Crashed,
    /// Server failed to restart, giving up
    Failed,
    /// Server is shutting down
    Stopping,
}

impl std::fmt::Display for ServerHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerHealth::Healthy => write!(f, "healthy"),
            ServerHealth::Starting => write!(f, "starting"),
            ServerHealth::Crashed => write!(f, "crashed"),
            ServerHealth::Failed => write!(f, "failed"),
            ServerHealth::Stopping => write!(f, "stopping"),
        }
    }
}

/// Server lifecycle statistics
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    /// Total restart attempts since manager creation
    pub restart_count: i32,
    /// Consecutive crashes without successful requests
    pub consecutive_crashes: i32,
    /// Last successful request timestamp
    pub last_healthy: Option<Instant>,
    /// Server start time
    pub started_at: Option<Instant>,
}

/// Lifecycle manager for a single LSP server
pub struct ServerLifecycle {
    server_id: String,
    config: LifecycleConfig,

    // Current state
    health: RwLock<ServerHealth>,
    stats: RwLock<ServerStats>,

    // Restart tracking
    restart_count: AtomicI32,
    is_restarting: AtomicBool,

    // Health check task handle
    health_check_handle: Mutex<Option<JoinHandle<()>>>,

    // Shutdown signal
    shutdown_tx: watch::Sender<bool>,
    #[allow(dead_code)]
    shutdown_rx: watch::Receiver<bool>,
}

impl ServerLifecycle {
    /// Create a new lifecycle manager for a server
    pub fn new(server_id: String, config: LifecycleConfig) -> Self {
        info!(
            "Created lifecycle manager for {} (max_restarts: {}, restart_on_crash: {})",
            server_id, config.max_restarts, config.restart_on_crash
        );

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Self {
            server_id,
            config,
            health: RwLock::new(ServerHealth::Starting),
            stats: RwLock::new(ServerStats::default()),
            restart_count: AtomicI32::new(0),
            is_restarting: AtomicBool::new(false),
            health_check_handle: Mutex::new(None),
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Get the server ID
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Get the lifecycle configuration
    pub fn config(&self) -> &LifecycleConfig {
        &self.config
    }

    /// Check if restart should be attempted
    pub fn should_restart(&self) -> bool {
        if !self.config.restart_on_crash {
            return false;
        }

        let restarts = self.restart_count.load(Ordering::SeqCst);
        restarts < self.config.max_restarts
    }

    /// Check if server is currently restarting
    pub fn is_restarting(&self) -> bool {
        self.is_restarting.load(Ordering::SeqCst)
    }

    /// Set restarting flag
    pub fn set_restarting(&self, value: bool) {
        self.is_restarting.store(value, Ordering::SeqCst);
    }

    /// Record a crash and return true if restart should proceed
    pub async fn record_crash(&self) -> bool {
        let mut stats = self.stats.write().await;
        stats.consecutive_crashes += 1;

        if self.should_restart() {
            *self.health.write().await = ServerHealth::Crashed;
            warn!(
                "LSP server {} crashed (attempt {}/{}), will restart",
                self.server_id,
                self.restart_count.load(Ordering::SeqCst) + 1,
                self.config.max_restarts
            );
            true
        } else {
            *self.health.write().await = ServerHealth::Failed;
            info!(
                "LSP {} failed permanently - exceeded max restarts ({})",
                self.server_id, self.config.max_restarts
            );
            error!(
                "LSP server {} exceeded max restarts ({}), giving up",
                self.server_id, self.config.max_restarts
            );
            false
        }
    }

    /// Record successful server start
    pub async fn record_started(&self) {
        let mut stats = self.stats.write().await;
        stats.started_at = Some(Instant::now());
        stats.consecutive_crashes = 0;
        stats.last_healthy = Some(Instant::now());
        *self.health.write().await = ServerHealth::Healthy;

        info!(
            "LSP server {} started successfully (restart count: {})",
            self.server_id,
            self.restart_count.load(Ordering::SeqCst)
        );
    }

    /// Record a successful request (reset crash counter)
    pub async fn record_healthy(&self) {
        let mut stats = self.stats.write().await;
        stats.last_healthy = Some(Instant::now());
        stats.consecutive_crashes = 0;

        let mut health = self.health.write().await;
        if *health == ServerHealth::Crashed {
            *health = ServerHealth::Healthy;
        }
    }

    /// Increment restart counter and return new value
    pub fn increment_restart_count(&self) -> i32 {
        self.restart_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Get current restart count
    pub fn get_restart_count(&self) -> i32 {
        self.restart_count.load(Ordering::SeqCst)
    }

    /// Reset restart count (e.g., after successful period)
    pub fn reset_restart_count(&self) {
        self.restart_count.store(0, Ordering::SeqCst);
    }

    /// Get current health status
    pub async fn health(&self) -> ServerHealth {
        *self.health.read().await
    }

    /// Set health status
    pub async fn set_health(&self, health: ServerHealth) {
        *self.health.write().await = health;
    }

    /// Get server statistics
    pub async fn stats(&self) -> ServerStats {
        self.stats.read().await.clone()
    }

    /// Start background health check task
    ///
    /// The `check_fn` should return true if the server is healthy, false if unhealthy.
    /// Health checks will retry up to MAX_HEALTH_CHECK_RETRIES times with
    /// HEALTH_CHECK_RETRY_DELAY_MS delay between attempts before declaring failure.
    pub fn start_health_check<F, Fut>(&self, check_fn: F) -> JoinHandle<()>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = bool> + Send,
    {
        let interval = Duration::from_millis(self.config.health_check_interval_ms as u64);
        let server_id = self.server_id.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            // Skip the first immediate tick
            interval_timer.tick().await;

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        // Perform health check with retry logic
                        let healthy = Self::check_health_with_retry(&check_fn, &server_id).await;
                        if healthy {
                            debug!("LSP server {} health check passed", server_id);
                        } else {
                            warn!("LSP server {} health check failed after {} retries",
                                server_id, MAX_HEALTH_CHECK_RETRIES);
                            // The caller should handle the unhealthy state
                            break;
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        debug!("LSP server {} health check stopping (shutdown signal)", server_id);
                        break;
                    }
                }
            }
        });

        handle
    }

    /// Check health with retry logic
    async fn check_health_with_retry<F, Fut>(check_fn: &F, server_id: &str) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for attempt in 0..MAX_HEALTH_CHECK_RETRIES {
            if check_fn().await {
                if attempt > 0 {
                    info!(
                        "LSP {} recovered after {} health check retry(s)",
                        server_id, attempt
                    );
                }
                return true;
            }

            if attempt < MAX_HEALTH_CHECK_RETRIES - 1 {
                debug!(
                    "LSP server {} health check attempt {}/{} failed, retrying...",
                    server_id,
                    attempt + 1,
                    MAX_HEALTH_CHECK_RETRIES
                );
                tokio::time::sleep(Duration::from_millis(HEALTH_CHECK_RETRY_DELAY_MS)).await;
            }
        }
        false
    }

    /// Store health check handle for later cleanup
    pub async fn set_health_check_handle(&self, handle: JoinHandle<()>) {
        let mut guard = self.health_check_handle.lock().await;
        // Abort previous handle if exists
        if let Some(old_handle) = guard.take() {
            old_handle.abort();
        }
        *guard = Some(handle);
    }

    /// Signal shutdown
    pub fn signal_shutdown(&self) {
        info!("Shutdown signal received for LSP {}", self.server_id);
        let _ = self.shutdown_tx.send(true);
        *self.health.blocking_write() = ServerHealth::Stopping;
    }

    /// Check if shutdown was signaled
    pub fn is_shutdown(&self) -> bool {
        *self.shutdown_tx.borrow()
    }

    /// Abort health check task
    pub async fn abort_health_check(&self) {
        if let Some(handle) = self.health_check_handle.lock().await.take() {
            handle.abort();
            let _ = tokio::time::timeout(Duration::from_millis(100), handle).await;
        }
    }
}

impl std::fmt::Debug for ServerLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerLifecycle")
            .field("server_id", &self.server_id)
            .field("restart_count", &self.restart_count.load(Ordering::SeqCst))
            .field("is_restarting", &self.is_restarting.load(Ordering::SeqCst))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_health_display() {
        assert_eq!(format!("{}", ServerHealth::Healthy), "healthy");
        assert_eq!(format!("{}", ServerHealth::Crashed), "crashed");
        assert_eq!(format!("{}", ServerHealth::Failed), "failed");
    }

    #[test]
    fn test_should_restart_enabled() {
        let config = LifecycleConfig {
            max_restarts: 3,
            restart_on_crash: true,
            ..Default::default()
        };
        let lifecycle = ServerLifecycle::new("test".to_string(), config);

        assert!(lifecycle.should_restart());
        lifecycle.increment_restart_count();
        assert!(lifecycle.should_restart());
        lifecycle.increment_restart_count();
        assert!(lifecycle.should_restart());
        lifecycle.increment_restart_count();
        assert!(!lifecycle.should_restart()); // Exceeded limit
    }

    #[test]
    fn test_should_restart_disabled() {
        let config = LifecycleConfig {
            restart_on_crash: false,
            ..Default::default()
        };
        let lifecycle = ServerLifecycle::new("test".to_string(), config);

        assert!(!lifecycle.should_restart());
    }

    #[tokio::test]
    async fn test_record_crash_with_restart() {
        let config = LifecycleConfig {
            max_restarts: 2,
            restart_on_crash: true,
            ..Default::default()
        };
        let lifecycle = ServerLifecycle::new("test".to_string(), config);

        // First crash - should restart
        assert!(lifecycle.record_crash().await);
        assert_eq!(lifecycle.health().await, ServerHealth::Crashed);

        // Simulate restart
        lifecycle.increment_restart_count();

        // Second crash - should restart
        assert!(lifecycle.record_crash().await);

        // Simulate restart
        lifecycle.increment_restart_count();

        // Third crash - should NOT restart (exceeded max)
        assert!(!lifecycle.record_crash().await);
        assert_eq!(lifecycle.health().await, ServerHealth::Failed);
    }

    #[tokio::test]
    async fn test_record_started() {
        let lifecycle = ServerLifecycle::new("test".to_string(), LifecycleConfig::default());

        lifecycle.record_started().await;

        assert_eq!(lifecycle.health().await, ServerHealth::Healthy);
        let stats = lifecycle.stats().await;
        assert!(stats.started_at.is_some());
        assert!(stats.last_healthy.is_some());
        assert_eq!(stats.consecutive_crashes, 0);
    }

    #[tokio::test]
    async fn test_record_healthy() {
        let lifecycle = ServerLifecycle::new("test".to_string(), LifecycleConfig::default());

        // Set to crashed first
        lifecycle.set_health(ServerHealth::Crashed).await;
        assert_eq!(lifecycle.health().await, ServerHealth::Crashed);

        // Record healthy
        lifecycle.record_healthy().await;
        assert_eq!(lifecycle.health().await, ServerHealth::Healthy);
    }

    #[test]
    fn test_restart_count() {
        let lifecycle = ServerLifecycle::new("test".to_string(), LifecycleConfig::default());

        assert_eq!(lifecycle.get_restart_count(), 0);
        assert_eq!(lifecycle.increment_restart_count(), 1);
        assert_eq!(lifecycle.increment_restart_count(), 2);
        assert_eq!(lifecycle.get_restart_count(), 2);

        lifecycle.reset_restart_count();
        assert_eq!(lifecycle.get_restart_count(), 0);
    }
}
