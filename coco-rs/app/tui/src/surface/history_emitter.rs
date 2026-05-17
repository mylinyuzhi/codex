//! Exactly-once finalized-history emission tracking.
// S2 lands before production native scrollback wiring; keep this scoped while
// `terminal::Tui` still owns the live UI.
#![allow(dead_code)]

use crate::state::session::ChatMessage;
use crate::surface::terminal::SurfaceBackend;
use crate::surface::terminal::SurfaceTerminal;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionPlan {
    Noop,
    Append { start: usize },
    ReplayRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionOutcome {
    Noop,
    Appended {
        start: usize,
        message_count: usize,
        rows: u16,
    },
    Replayed {
        message_count: usize,
        rows: u16,
    },
    ReplayRequired,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct HistoryEmissionTracker {
    emitted_message_ids: Vec<String>,
}

impl HistoryEmissionTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn emitted_count(&self) -> usize {
        self.emitted_message_ids.len()
    }

    pub(crate) fn plan(&self, messages: &[ChatMessage]) -> HistoryEmissionPlan {
        if self.emitted_message_ids.is_empty() {
            return if messages.is_empty() {
                HistoryEmissionPlan::Noop
            } else {
                HistoryEmissionPlan::Append { start: 0 }
            };
        }

        if self.emitted_message_ids.len() > messages.len() {
            return HistoryEmissionPlan::ReplayRequired;
        }

        let prefix_matches = self
            .emitted_message_ids
            .iter()
            .zip(messages.iter())
            .all(|(emitted_id, message)| emitted_id == &message.id);
        if !prefix_matches {
            return HistoryEmissionPlan::ReplayRequired;
        }

        if self.emitted_message_ids.len() == messages.len() {
            HistoryEmissionPlan::Noop
        } else {
            HistoryEmissionPlan::Append {
                start: self.emitted_message_ids.len(),
            }
        }
    }

    pub(crate) fn mark_emitted_through(&mut self, messages: &[ChatMessage], end: usize) {
        let end = end.min(messages.len());
        self.emitted_message_ids = messages
            .iter()
            .take(end)
            .map(|message| message.id.clone())
            .collect();
    }

    pub(crate) fn reset(&mut self) {
        self.emitted_message_ids.clear();
    }

    pub(crate) fn emit_append_only<B, F>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        messages: &[ChatMessage],
        render_tail: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
        F: FnOnce(&[ChatMessage]) -> Vec<Line<'static>>,
    {
        let start = match self.plan(messages) {
            HistoryEmissionPlan::Noop => return Ok(HistoryEmissionOutcome::Noop),
            HistoryEmissionPlan::ReplayRequired => {
                return Ok(HistoryEmissionOutcome::ReplayRequired);
            }
            HistoryEmissionPlan::Append { start } => start,
        };

        let rows = terminal.insert_history_lines(render_tail(&messages[start..]))?;
        self.mark_emitted_through(messages, messages.len());
        Ok(HistoryEmissionOutcome::Appended {
            start,
            message_count: messages.len() - start,
            rows,
        })
    }

    pub(crate) fn replay_all<B, F>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        messages: &[ChatMessage],
        render_all: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
        F: FnOnce(&[ChatMessage]) -> Vec<Line<'static>>,
    {
        terminal.clear_owned_scrollback()?;
        let rows = terminal.insert_history_lines(render_all(messages))?;
        self.mark_emitted_through(messages, messages.len());
        Ok(HistoryEmissionOutcome::Replayed {
            message_count: messages.len(),
            rows,
        })
    }
}

#[cfg(test)]
#[path = "history_emitter.test.rs"]
mod tests;
