//! Correlation types for request-response tracking.
//!
//! These types enable tracking which events correspond to which commands,
//! providing better observability and debugging capabilities.

use serde::Deserialize;
use serde::Serialize;

use crate::LoopEvent;

/// A unique identifier for a command submission.
///
/// Used to correlate events back to the command that triggered them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubmissionId(pub String);

impl SubmissionId {
    /// Create a new submission ID with a random UUID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Create a submission ID from an existing string.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Default for SubmissionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SubmissionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SubmissionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SubmissionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for SubmissionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A loop event with optional correlation information.
///
/// Wraps a [`LoopEvent`] with an optional [`SubmissionId`] to enable
/// tracking which command triggered this event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelatedEvent {
    /// The correlation ID linking this event to its originating command.
    ///
    /// This is `None` for events that are not triggered by a specific command,
    /// such as background task completions or system-initiated events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<SubmissionId>,

    /// The underlying loop event.
    pub event: LoopEvent,
}

impl CorrelatedEvent {
    /// Create a new correlated event with the given correlation ID.
    pub fn new(event: LoopEvent, correlation_id: Option<SubmissionId>) -> Self {
        Self {
            correlation_id,
            event,
        }
    }

    /// Create a correlated event without a correlation ID.
    pub fn uncorrelated(event: LoopEvent) -> Self {
        Self {
            correlation_id: None,
            event,
        }
    }

    /// Create a correlated event with a correlation ID.
    pub fn correlated(event: LoopEvent, id: SubmissionId) -> Self {
        Self {
            correlation_id: Some(id),
            event,
        }
    }

    /// Check if this event has a correlation ID.
    pub fn has_correlation(&self) -> bool {
        self.correlation_id.is_some()
    }

    /// Get the correlation ID if present.
    pub fn correlation_id(&self) -> Option<&SubmissionId> {
        self.correlation_id.as_ref()
    }

    /// Get a reference to the underlying event.
    pub fn event(&self) -> &LoopEvent {
        &self.event
    }

    /// Consume self and return the underlying event.
    pub fn into_event(self) -> LoopEvent {
        self.event
    }

    /// Consume self and return both the correlation ID and event.
    pub fn into_parts(self) -> (Option<SubmissionId>, LoopEvent) {
        (self.correlation_id, self.event)
    }
}

impl From<LoopEvent> for CorrelatedEvent {
    fn from(event: LoopEvent) -> Self {
        Self::uncorrelated(event)
    }
}

impl From<(LoopEvent, SubmissionId)> for CorrelatedEvent {
    fn from((event, id): (LoopEvent, SubmissionId)) -> Self {
        Self::correlated(event, id)
    }
}

impl From<(LoopEvent, Option<SubmissionId>)> for CorrelatedEvent {
    fn from((event, id): (LoopEvent, Option<SubmissionId>)) -> Self {
        Self::new(event, id)
    }
}

#[cfg(test)]
#[path = "correlation.test.rs"]
mod tests;
