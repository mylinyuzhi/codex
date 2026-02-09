//! Worker permissions queue for background tool execution.
//!
//! This module provides a permissions queue system that allows background workers
//! to request permissions from the main thread. The main thread processes the queue
//! and responds to permission requests.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────┐          ┌───────────────────┐          ┌─────────────────┐
//! │  Background      │   queue  │  Permission       │   wait   │   Main Thread   │
//! │  Worker          │─────────▶│  Queue            │◀─────────│   (UI/TUI)      │
//! │                  │          │                   │          │                 │
//! │  await permit()  │◀─────────│  response channel │──────────│  respond()      │
//! └──────────────────┘          └───────────────────┘          └─────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! // Create the queue
//! let queue = WorkerPermissionQueue::new();
//!
//! // Background worker requests permission
//! let result = queue.request_permission(ApprovalRequest { ... }).await;
//!
//! // Main thread processes requests
//! while let Some(request) = queue.next_pending().await {
//!     // Show UI and get user response
//!     let decision = show_approval_dialog(&request);
//!     queue.respond(&request.request_id, decision).await;
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::LoopEvent;
use cocode_tools::PermissionRequester;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::info;
use tracing::warn;

// ============================================================================
// Constants
// ============================================================================

/// Default timeout for permission requests (seconds).
pub const DEFAULT_PERMISSION_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Maximum pending permission requests.
pub const MAX_PENDING_REQUESTS: usize = 100;

// ============================================================================
// PermissionRequestStatus
// ============================================================================

/// Status of a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRequestStatus {
    /// Request is pending user response.
    Pending,
    /// Request was approved.
    Approved,
    /// Request was denied.
    Denied,
    /// Request timed out.
    TimedOut,
    /// Request was cancelled.
    Cancelled,
}

impl PermissionRequestStatus {
    /// Check if the request is still pending.
    pub fn is_pending(&self) -> bool {
        matches!(self, PermissionRequestStatus::Pending)
    }

    /// Check if the request was approved.
    pub fn is_approved(&self) -> bool {
        matches!(self, PermissionRequestStatus::Approved)
    }

    /// Check if the request was resolved (approved, denied, or timed out).
    pub fn is_resolved(&self) -> bool {
        !self.is_pending()
    }

    /// Get the status as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionRequestStatus::Pending => "pending",
            PermissionRequestStatus::Approved => "approved",
            PermissionRequestStatus::Denied => "denied",
            PermissionRequestStatus::TimedOut => "timed_out",
            PermissionRequestStatus::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for PermissionRequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// QueuedPermissionRequest
// ============================================================================

/// A permission request in the queue.
#[derive(Debug)]
pub struct QueuedPermissionRequest {
    /// The approval request.
    pub request: ApprovalRequest,
    /// When the request was queued.
    pub queued_at: Instant,
    /// Timeout for this request.
    pub timeout: Duration,
    /// Worker ID that submitted the request.
    pub worker_id: String,
    /// Response channel.
    response_tx: Option<oneshot::Sender<ApprovalDecision>>,
}

impl QueuedPermissionRequest {
    /// Check if the request has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.queued_at.elapsed() > self.timeout
    }

    /// Get remaining time before timeout.
    pub fn remaining_time(&self) -> Duration {
        self.timeout.saturating_sub(self.queued_at.elapsed())
    }
}

// ============================================================================
// PermissionResponse
// ============================================================================

/// Response to a permission request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    /// Request ID.
    pub request_id: String,
    /// Whether the request was approved.
    pub approved: bool,
    /// Optional message from the approver.
    pub message: Option<String>,
    /// Whether to remember this decision for similar requests.
    pub remember: bool,
}

// ============================================================================
// WorkerPermissionQueue
// ============================================================================

/// Queue for background worker permission requests.
///
/// This queue allows background workers to request permissions from the main
/// thread. The main thread processes requests and sends responses back to
/// waiting workers.
#[derive(Debug)]
pub struct WorkerPermissionQueue {
    /// Pending requests keyed by request ID.
    requests: Arc<Mutex<HashMap<String, QueuedPermissionRequest>>>,
    /// Channel for notifying the main thread of new requests.
    notify_tx: broadcast::Sender<String>,
    /// Event sender for emitting events.
    event_tx: Option<mpsc::Sender<LoopEvent>>,
    /// Default timeout for requests.
    default_timeout: Duration,
    /// Counter for generating request IDs.
    next_id: Arc<Mutex<u64>>,
}

impl WorkerPermissionQueue {
    /// Create a new permission queue.
    pub fn new() -> Self {
        let (notify_tx, _) = broadcast::channel(MAX_PENDING_REQUESTS);

        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            notify_tx,
            event_tx: None,
            default_timeout: Duration::from_secs(DEFAULT_PERMISSION_TIMEOUT_SECS),
            next_id: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a queue with an event sender.
    pub fn with_event_tx(mut self, tx: mpsc::Sender<LoopEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Set the default timeout for requests.
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Generate a new request ID.
    async fn next_request_id(&self) -> String {
        let mut counter = self.next_id.lock().await;
        *counter += 1;
        format!("perm-{}", *counter)
    }

    /// Request permission from the main thread.
    ///
    /// This method queues the request and waits for a response.
    /// Returns the user's three-way decision, or `Denied` on timeout/cancel.
    pub async fn request_permission(
        &self,
        mut request: ApprovalRequest,
        worker_id: &str,
    ) -> ApprovalDecision {
        // Generate request ID if not set
        if request.request_id.is_empty() {
            request.request_id = self.next_request_id().await;
        }

        let request_id = request.request_id.clone();
        let (response_tx, response_rx) = oneshot::channel();

        // Queue the request
        {
            let mut requests = self.requests.lock().await;

            // Check capacity
            if requests.len() >= MAX_PENDING_REQUESTS {
                warn!(
                    request_id = %request_id,
                    "Permission queue full, denying request"
                );
                return ApprovalDecision::Denied;
            }

            let queued = QueuedPermissionRequest {
                request: request.clone(),
                queued_at: Instant::now(),
                timeout: self.default_timeout,
                worker_id: worker_id.to_string(),
                response_tx: Some(response_tx),
            };

            requests.insert(request_id.clone(), queued);
        }

        info!(
            request_id = %request_id,
            tool = %request.tool_name,
            worker = %worker_id,
            "Permission request queued"
        );

        // Emit event
        self.emit_event(LoopEvent::ApprovalRequired { request })
            .await;

        // Notify the main thread
        let _ = self.notify_tx.send(request_id.clone());

        // Wait for response with timeout
        match tokio::time::timeout(self.default_timeout, response_rx).await {
            Ok(Ok(decision)) => {
                debug!(request_id = %request_id, ?decision, "Permission response received");
                decision
            }
            Ok(Err(_)) => {
                // Channel closed - request was cancelled
                warn!(request_id = %request_id, "Permission request cancelled");
                self.cleanup_request(&request_id).await;
                ApprovalDecision::Denied
            }
            Err(_) => {
                // Timeout
                warn!(request_id = %request_id, "Permission request timed out");
                self.cleanup_request(&request_id).await;
                ApprovalDecision::Denied
            }
        }
    }

    /// Request permission with a custom timeout.
    pub async fn request_permission_with_timeout(
        &self,
        mut request: ApprovalRequest,
        worker_id: &str,
        timeout: Duration,
    ) -> ApprovalDecision {
        // Generate request ID if not set
        if request.request_id.is_empty() {
            request.request_id = self.next_request_id().await;
        }

        let request_id = request.request_id.clone();
        let (response_tx, response_rx) = oneshot::channel();

        // Queue the request with custom timeout
        {
            let mut requests = self.requests.lock().await;

            if requests.len() >= MAX_PENDING_REQUESTS {
                warn!(
                    request_id = %request_id,
                    "Permission queue full, denying request"
                );
                return ApprovalDecision::Denied;
            }

            let queued = QueuedPermissionRequest {
                request: request.clone(),
                queued_at: Instant::now(),
                timeout,
                worker_id: worker_id.to_string(),
                response_tx: Some(response_tx),
            };

            requests.insert(request_id.clone(), queued);
        }

        info!(
            request_id = %request_id,
            tool = %request.tool_name,
            worker = %worker_id,
            timeout_secs = timeout.as_secs(),
            "Permission request queued with custom timeout"
        );

        // Emit event
        self.emit_event(LoopEvent::ApprovalRequired { request })
            .await;

        // Notify the main thread
        let _ = self.notify_tx.send(request_id.clone());

        // Wait for response with custom timeout
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(decision)) => {
                debug!(request_id = %request_id, decision = ?decision, "Permission response received");
                decision
            }
            Ok(Err(_)) => {
                warn!(request_id = %request_id, "Permission request cancelled");
                self.cleanup_request(&request_id).await;
                ApprovalDecision::Denied
            }
            Err(_) => {
                warn!(request_id = %request_id, "Permission request timed out");
                self.cleanup_request(&request_id).await;
                ApprovalDecision::Denied
            }
        }
    }

    /// Respond to a pending permission request.
    ///
    /// Returns `true` if the request was found and responded to.
    pub async fn respond(&self, request_id: &str, decision: ApprovalDecision) -> bool {
        let response_tx = {
            let mut requests = self.requests.lock().await;
            if let Some(mut queued) = requests.remove(request_id) {
                queued.response_tx.take()
            } else {
                None
            }
        };

        if let Some(tx) = response_tx {
            let _ = tx.send(decision.clone());

            info!(
                request_id = %request_id,
                decision = ?decision,
                "Permission response sent"
            );

            // Emit event
            self.emit_event(LoopEvent::ApprovalResponse {
                request_id: request_id.to_string(),
                decision,
            })
            .await;

            true
        } else {
            warn!(
                request_id = %request_id,
                "Permission request not found or already responded"
            );
            false
        }
    }

    /// Get the next pending request (non-blocking).
    pub async fn next_pending(&self) -> Option<ApprovalRequest> {
        let requests = self.requests.lock().await;

        // Find first non-timed-out pending request
        for queued in requests.values() {
            if !queued.is_timed_out() {
                return Some(queued.request.clone());
            }
        }

        None
    }

    /// Wait for the next pending request.
    pub async fn wait_for_request(&self) -> Option<ApprovalRequest> {
        let mut rx = self.notify_tx.subscribe();

        loop {
            // Check for existing pending requests first
            if let Some(request) = self.next_pending().await {
                return Some(request);
            }

            // Wait for notification of new request
            match rx.recv().await {
                Ok(request_id) => {
                    let requests = self.requests.lock().await;
                    if let Some(queued) = requests.get(&request_id) {
                        if !queued.is_timed_out() {
                            return Some(queued.request.clone());
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return None;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed some notifications, check queue again
                    continue;
                }
            }
        }
    }

    /// Get all pending requests.
    pub async fn pending_requests(&self) -> Vec<ApprovalRequest> {
        let requests = self.requests.lock().await;
        requests
            .values()
            .filter(|q| !q.is_timed_out())
            .map(|q| q.request.clone())
            .collect()
    }

    /// Get the count of pending requests.
    pub async fn pending_count(&self) -> usize {
        let requests = self.requests.lock().await;
        requests.values().filter(|q| !q.is_timed_out()).count()
    }

    /// Cancel all pending requests for a worker.
    pub async fn cancel_worker_requests(&self, worker_id: &str) -> i32 {
        let mut cancelled = 0;
        let mut requests = self.requests.lock().await;

        let to_cancel: Vec<_> = requests
            .iter()
            .filter(|(_, q)| q.worker_id == worker_id)
            .map(|(id, _)| id.clone())
            .collect();

        for request_id in to_cancel {
            if let Some(mut queued) = requests.remove(&request_id) {
                if let Some(tx) = queued.response_tx.take() {
                    let _ = tx.send(ApprovalDecision::Denied); // Deny cancelled requests
                }
                cancelled += 1;
            }
        }

        if cancelled > 0 {
            info!(
                worker_id = %worker_id,
                cancelled,
                "Cancelled worker permission requests"
            );
        }

        cancelled
    }

    /// Cancel all pending requests.
    pub async fn cancel_all(&self) -> i32 {
        let mut cancelled = 0;
        let mut requests = self.requests.lock().await;

        for (_, mut queued) in requests.drain() {
            if let Some(tx) = queued.response_tx.take() {
                let _ = tx.send(ApprovalDecision::Denied);
            }
            cancelled += 1;
        }

        if cancelled > 0 {
            info!(cancelled, "Cancelled all permission requests");
        }

        cancelled
    }

    /// Clean up timed out requests.
    pub async fn cleanup_timed_out(&self) -> i32 {
        let mut cleaned = 0;
        let mut requests = self.requests.lock().await;

        let timed_out: Vec<_> = requests
            .iter()
            .filter(|(_, q)| q.is_timed_out())
            .map(|(id, _)| id.clone())
            .collect();

        for request_id in timed_out {
            if let Some(mut queued) = requests.remove(&request_id) {
                if let Some(tx) = queued.response_tx.take() {
                    let _ = tx.send(ApprovalDecision::Denied);
                }
                cleaned += 1;
            }
        }

        if cleaned > 0 {
            debug!(cleaned, "Cleaned up timed out permission requests");
        }

        cleaned
    }

    /// Get queue statistics.
    pub async fn stats(&self) -> PermissionQueueStats {
        let requests = self.requests.lock().await;
        let pending = requests.values().filter(|q| !q.is_timed_out()).count();
        let timed_out = requests.values().filter(|q| q.is_timed_out()).count();

        PermissionQueueStats {
            pending,
            timed_out,
            total: requests.len(),
        }
    }

    /// Clean up a specific request.
    async fn cleanup_request(&self, request_id: &str) {
        let mut requests = self.requests.lock().await;
        requests.remove(request_id);
    }

    /// Emit a loop event.
    async fn emit_event(&self, event: LoopEvent) {
        if let Some(tx) = &self.event_tx {
            if let Err(e) = tx.send(event).await {
                debug!("Failed to send permission event: {e}");
            }
        }
    }
}

impl Default for WorkerPermissionQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PermissionRequester impl
// ============================================================================

#[async_trait]
impl PermissionRequester for WorkerPermissionQueue {
    async fn request_permission(
        &self,
        request: ApprovalRequest,
        worker_id: &str,
    ) -> ApprovalDecision {
        self.request_permission(request, worker_id).await
    }
}

// ============================================================================
// PermissionQueueStats
// ============================================================================

/// Statistics about the permission queue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionQueueStats {
    /// Number of pending requests.
    pub pending: usize,
    /// Number of timed out requests.
    pub timed_out: usize,
    /// Total number of requests in queue.
    pub total: usize,
}

impl PermissionQueueStats {
    /// Check if there are any pending requests.
    pub fn has_pending(&self) -> bool {
        self.pending > 0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[path = "worker_permissions.test.rs"]
mod tests;
