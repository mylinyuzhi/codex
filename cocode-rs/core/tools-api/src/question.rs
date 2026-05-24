//! Question responder for AskUserQuestion tool.
//!
//! Manages pending question requests with oneshot channels. The tool emits
//! a `QuestionAsked` event and waits on the receiver. The main loop calls
//! `respond()` when the user answers, unblocking the tool.

use std::collections::HashMap;
use tokio::sync::oneshot;

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
