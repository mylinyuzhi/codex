//! Mid-turn command queue and query guard for steering.
//!
//! TS: utils/messageQueueManager.ts (547 LOC) + utils/QueryGuard.ts (122 LOC)
//!
//! CommandQueue enables mid-turn message injection: slash commands, teammate
//! messages, and task notifications are queued and drained between tool calls.
//! QueryGuard prevents race conditions between queue processing and query startup.

use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

/// Priority for queued commands.
/// Higher priority (lower numeric value) commands are dequeued first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePriority {
    /// Highest: urgent user input during streaming.
    Now = 0,
    /// Normal: standard queued commands.
    Next = 1,
    /// Lowest: background task notifications.
    Later = 2,
}

/// A command queued for mid-turn injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedCommand {
    /// The prompt text or slash command.
    pub prompt: String,
    /// Priority level.
    pub priority: QueuePriority,
    /// Agent ID filter (None = main thread).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Whether this is a slash command (starts with /).
    pub is_slash_command: bool,
    /// Source identifier for tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl QueuedCommand {
    pub fn new(prompt: String, priority: QueuePriority) -> Self {
        let is_slash = prompt.starts_with('/');
        Self {
            prompt,
            priority,
            agent_id: None,
            is_slash_command: is_slash,
            source: None,
        }
    }

    pub fn with_agent(mut self, agent_id: String) -> Self {
        self.agent_id = Some(agent_id);
        self
    }
}

/// Thread-safe mid-turn command queue with priority ordering.
///
/// Commands enqueued during tool execution are drained between turns.
/// Slash commands are excluded from mid-turn draining (processed post-turn).
#[derive(Debug, Clone)]
pub struct CommandQueue {
    inner: Arc<Mutex<Vec<QueuedCommand>>>,
    changed: Arc<Notify>,
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            changed: Arc::new(Notify::new()),
        }
    }

    /// Enqueue a command.
    pub async fn enqueue(&self, command: QueuedCommand) {
        let mut queue = self.inner.lock().await;
        queue.push(command);
        queue.sort_by_key(|c| c.priority);
        self.changed.notify_waiters();
    }

    /// Dequeue highest-priority non-slash command matching the agent filter.
    pub async fn dequeue(&self, agent_id: Option<&str>) -> Option<QueuedCommand> {
        let mut queue = self.inner.lock().await;
        let idx = queue
            .iter()
            .position(|c| !c.is_slash_command && c.agent_id.as_deref() == agent_id)?;
        Some(queue.remove(idx))
    }

    /// Get all commands at or above (lower numeric value) a max priority.
    /// Used for mid-turn draining.
    pub async fn get_commands_by_max_priority(
        &self,
        max_priority: QueuePriority,
        agent_id: Option<&str>,
    ) -> Vec<QueuedCommand> {
        let queue = self.inner.lock().await;
        queue
            .iter()
            .filter(|c| {
                c.priority <= max_priority
                    && !c.is_slash_command
                    && c.agent_id.as_deref() == agent_id
            })
            .cloned()
            .collect()
    }

    /// Remove specific commands from the queue.
    pub async fn remove(&self, prompts_to_remove: &[String]) {
        let mut queue = self.inner.lock().await;
        queue.retain(|c| !prompts_to_remove.contains(&c.prompt));
    }

    /// Check if the queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }

    /// Wait for a change to the queue.
    pub async fn wait_for_change(&self) {
        self.changed.notified().await;
    }

    /// Number of queued commands.
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Peek at the highest-priority command without removing it.
    pub async fn peek(&self, agent_id: Option<&str>) -> Option<QueuedCommand> {
        let queue = self.inner.lock().await;
        queue
            .iter()
            .find(|c| !c.is_slash_command && c.agent_id.as_deref() == agent_id)
            .cloned()
    }

    /// Remove all commands matching a predicate.
    pub async fn dequeue_matching<F>(&self, predicate: F) -> Vec<QueuedCommand>
    where
        F: Fn(&QueuedCommand) -> bool,
    {
        let mut queue = self.inner.lock().await;
        let mut matched = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            if predicate(&queue[i]) {
                matched.push(queue.remove(i));
            } else {
                i += 1;
            }
        }
        if !matched.is_empty() {
            self.changed.notify_waiters();
        }
        matched
    }

    /// Remove and return all non-slash commands for the given agent.
    pub async fn dequeue_all(&self, agent_id: Option<&str>) -> Vec<QueuedCommand> {
        let mut queue = self.inner.lock().await;
        let mut drained = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            if !queue[i].is_slash_command && queue[i].agent_id.as_deref() == agent_id {
                drained.push(queue.remove(i));
            } else {
                i += 1;
            }
        }
        if !drained.is_empty() {
            self.changed.notify_waiters();
        }
        drained
    }

    /// Clear the entire queue.
    pub async fn clear(&self) {
        let mut queue = self.inner.lock().await;
        queue.clear();
        self.changed.notify_waiters();
    }
}

/// State machine guarding query lifecycle to prevent race conditions.
///
/// TS: QueryGuard — three states with generation counter.
/// Prevents re-entry during async gaps between queue processing and query startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryGuardStatus {
    /// No query active or reserved.
    Idle,
    /// A queue processor reserved the slot but hasn't started yet.
    Dispatching,
    /// A query is actively running.
    Running,
}

#[derive(Debug)]
pub struct QueryGuard {
    status: Arc<Mutex<QueryGuardState>>,
    changed: Arc<Notify>,
}

#[derive(Debug)]
struct QueryGuardState {
    status: QueryGuardStatus,
    generation: i64,
}

impl Default for QueryGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryGuard {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(QueryGuardState {
                status: QueryGuardStatus::Idle,
                generation: 0,
            })),
            changed: Arc::new(Notify::new()),
        }
    }

    /// Reserve the query slot for queue processing (idle -> dispatching).
    /// Returns `false` if already active.
    pub async fn reserve(&self) -> bool {
        let mut state = self.status.lock().await;
        if state.status != QueryGuardStatus::Idle {
            return false;
        }
        state.status = QueryGuardStatus::Dispatching;
        self.changed.notify_waiters();
        true
    }

    /// Start the query. Returns a generation number for safe cleanup.
    /// Transitions: idle -> running, dispatching -> running.
    pub async fn try_start(&self) -> Option<i64> {
        let mut state = self.status.lock().await;
        match state.status {
            QueryGuardStatus::Idle | QueryGuardStatus::Dispatching => {
                state.generation += 1;
                state.status = QueryGuardStatus::Running;
                self.changed.notify_waiters();
                Some(state.generation)
            }
            QueryGuardStatus::Running => None,
        }
    }

    /// End the query if the generation matches (safe cleanup).
    pub async fn end(&self, generation: i64) -> bool {
        let mut state = self.status.lock().await;
        if state.generation == generation {
            state.status = QueryGuardStatus::Idle;
            self.changed.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Force end regardless of generation (for cancellation).
    pub async fn force_end(&self) {
        let mut state = self.status.lock().await;
        state.status = QueryGuardStatus::Idle;
        self.changed.notify_waiters();
    }

    /// Cancel a reservation (dispatching -> idle).
    pub async fn cancel_reservation(&self) {
        let mut state = self.status.lock().await;
        if state.status == QueryGuardStatus::Dispatching {
            state.status = QueryGuardStatus::Idle;
            self.changed.notify_waiters();
        }
    }

    /// Whether a query is active (dispatching or running).
    pub async fn is_active(&self) -> bool {
        let state = self.status.lock().await;
        state.status != QueryGuardStatus::Idle
    }

    /// Current status.
    pub async fn status(&self) -> QueryGuardStatus {
        self.status.lock().await.status
    }
}

/// Inbox message from a teammate agent.
///
/// TS: InboxMessage in QueryEngine — teammate → main thread message passing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    /// Sender agent ID.
    pub from_agent: String,
    /// Message content.
    pub content: String,
    /// Whether this message has been consumed.
    pub consumed: bool,
    /// Timestamp (epoch ms).
    pub timestamp: i64,
}

/// Thread-safe inbox for receiving teammate messages.
#[derive(Debug, Clone)]
pub struct Inbox {
    messages: Arc<Mutex<Vec<InboxMessage>>>,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbox {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a message to the inbox.
    pub async fn push(&self, msg: InboxMessage) {
        self.messages.lock().await.push(msg);
    }

    /// Drain unconsumed messages, marking them as consumed.
    pub async fn drain_unconsumed(&self) -> Vec<InboxMessage> {
        let mut msgs = self.messages.lock().await;
        let mut drained = Vec::new();
        for msg in msgs.iter_mut() {
            if !msg.consumed {
                msg.consumed = true;
                drained.push(msg.clone());
            }
        }
        drained
    }

    /// Count of unconsumed messages.
    pub async fn unconsumed_count(&self) -> usize {
        self.messages
            .lock()
            .await
            .iter()
            .filter(|m| !m.consumed)
            .count()
    }
}

#[cfg(test)]
#[path = "command_queue.test.rs"]
mod tests;
