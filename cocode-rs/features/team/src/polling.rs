//! Priority-based polling loop for teammate agents.
//!
//! In-process teammates need a continuous loop to discover work between
//! agent turns. This module implements a 4-level priority system aligned
//! with Claude Code's `pollForNextMessage` (DNY):
//!
//! 1. **Fast-path messages** (in-process channel) — sub-millisecond
//! 2. **Shutdown requests** — highest mailbox priority
//! 3. **Lead/peer messages** — from mailbox
//! 4. **Task claiming** — from task ledger
//!
//! The poller runs as a background tokio task and yields [`PollResult`]
//! values to the agent loop.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::fast_path::FastPath;
use crate::mailbox::Mailbox;
use crate::task_ledger::TaskLedger;
use crate::task_ledger::TeamTask;
use crate::types::AgentMessage;
use crate::types::MessageType;

// ============================================================================
// Types
// ============================================================================

/// Result of a single poll cycle.
#[derive(Debug, Clone)]
pub enum PollResult {
    /// Shutdown requested — agent must stop.
    Shutdown(AgentMessage),
    /// Message from the team lead.
    LeadMessage(AgentMessage),
    /// Message from a peer teammate.
    PeerMessage(AgentMessage),
    /// A claimable task is available.
    TaskAvailable(TeamTask),
    /// Nothing to do this cycle.
    Idle,
}

/// Configuration for the polling loop.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// Milliseconds between mailbox/ledger polls.
    pub poll_interval_ms: u64,
    /// Agent ID that should be treated as the "lead" for priority 2.
    pub leader_agent_id: Option<String>,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 500,
            leader_agent_id: None,
        }
    }
}

// ============================================================================
// TeamPoller
// ============================================================================

/// Priority-based poller for teammate agents.
///
/// Call [`TeamPoller::run`] to start the polling loop. It yields
/// [`PollResult`] values on the returned receiver.
pub struct TeamPoller {
    team_name: String,
    agent_id: String,
    config: PollConfig,
    mailbox: Arc<Mailbox>,
    task_ledger: Option<Arc<TaskLedger>>,
    fast_path: Option<Arc<FastPath>>,
    cancel: CancellationToken,
}

impl TeamPoller {
    pub fn new(
        team_name: impl Into<String>,
        agent_id: impl Into<String>,
        config: PollConfig,
        mailbox: Arc<Mailbox>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            team_name: team_name.into(),
            agent_id: agent_id.into(),
            config,
            mailbox,
            task_ledger: None,
            fast_path: None,
            cancel,
        }
    }

    pub fn with_task_ledger(mut self, ledger: Arc<TaskLedger>) -> Self {
        self.task_ledger = Some(ledger);
        self
    }

    pub fn with_fast_path(mut self, fp: Arc<FastPath>) -> Self {
        self.fast_path = Some(fp);
        self
    }

    /// Start the polling loop and return a receiver for poll results.
    ///
    /// The loop runs until the cancellation token is triggered.
    /// The channel has a buffer of 16 to avoid blocking the poller
    /// while the agent processes messages.
    pub fn run(self) -> mpsc::Receiver<PollResult> {
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(self.poll_loop(tx));
        rx
    }

    async fn poll_loop(self, tx: mpsc::Sender<PollResult>) {
        // If a fast-path channel exists, register and use it for priority 1.
        let mut fast_rx = if let Some(ref fp) = self.fast_path {
            Some(fp.register(&self.team_name, &self.agent_id).await)
        } else {
            None
        };

        let interval = Duration::from_millis(self.config.poll_interval_ms);

        loop {
            if self.cancel.is_cancelled() {
                break;
            }

            // Priority 1: Drain fast-path channel (non-blocking).
            if let Some(ref mut frx) = fast_rx {
                while let Ok(msg) = frx.try_recv() {
                    let result = self.classify_message(msg);
                    if tx.send(result).await.is_err() {
                        return; // receiver dropped
                    }
                }
            }

            // Priority 2-3: Check mailbox for unread messages.
            match self
                .mailbox
                .take_unread(&self.team_name, &self.agent_id)
                .await
            {
                Ok(messages) => {
                    // Sort: shutdown first, then lead, then peers.
                    let (shutdown, rest): (Vec<_>, Vec<_>) = messages
                        .into_iter()
                        .partition(|m| m.message_type == MessageType::ShutdownRequest);

                    for msg in shutdown {
                        if tx.send(PollResult::Shutdown(msg)).await.is_err() {
                            return;
                        }
                    }

                    let (lead, peer): (Vec<_>, Vec<_>) = rest.into_iter().partition(|m| {
                        self.config
                            .leader_agent_id
                            .as_ref()
                            .is_some_and(|lid| m.from == *lid)
                    });

                    for msg in lead {
                        if tx.send(PollResult::LeadMessage(msg)).await.is_err() {
                            return;
                        }
                    }
                    for msg in peer {
                        if tx.send(PollResult::PeerMessage(msg)).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        team = %self.team_name,
                        agent = %self.agent_id,
                        error = %e,
                        "Mailbox poll error"
                    );
                }
            }

            // Priority 4: Check task ledger for claimable work.
            if let Some(ref ledger) = self.task_ledger
                && let Some(task) = ledger.next_claimable(&self.team_name).await
                && tx.send(PollResult::TaskAvailable(task)).await.is_err()
            {
                return;
            }

            // Wait before next poll cycle.
            tokio::select! {
                () = self.cancel.cancelled() => break,
                () = tokio::time::sleep(interval) => {}
                // Also wake on fast-path message if available.
                msg = async {
                    match fast_rx {
                        Some(ref mut frx) => frx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(msg) = msg {
                        let result = self.classify_message(msg);
                        if tx.send(result).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }

        // Cleanup fast-path registration.
        if let Some(ref fp) = self.fast_path {
            fp.unregister(&self.team_name, &self.agent_id).await;
        }
    }

    fn classify_message(&self, msg: AgentMessage) -> PollResult {
        match msg.message_type {
            MessageType::ShutdownRequest => PollResult::Shutdown(msg),
            _ if self
                .config
                .leader_agent_id
                .as_ref()
                .is_some_and(|lid| msg.from == *lid) =>
            {
                PollResult::LeadMessage(msg)
            }
            _ => PollResult::PeerMessage(msg),
        }
    }
}

#[cfg(test)]
#[path = "polling.test.rs"]
mod tests;
