//! Stream processor state management.
//!
//! This module provides internal state tracking for the StreamProcessor,
//! including tool call management and snapshot accumulation.

use crate::stream::events::StreamEvent;
use crate::stream::snapshot::StreamSnapshot;
use crate::stream::snapshot::ThinkingSnapshot;
use crate::stream::snapshot::ToolCallSnapshot;
use std::collections::HashMap;

/// Tool call tracking with index mapping.
///
/// Manages the relationship between stream indices and vector positions,
/// allowing efficient updates as tool call events arrive.
#[derive(Debug, Default)]
pub(crate) struct ToolCallManager {
    /// Tool call snapshots in order of creation.
    calls: Vec<ToolCallSnapshot>,
    /// Map from stream index to vector index.
    index_map: HashMap<i64, usize>,
}

impl ToolCallManager {
    /// Create a new empty manager.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking a new tool call.
    pub fn start(&mut self, index: i64, id: String, name: String) {
        let vec_idx = self.calls.len();
        self.calls.push(ToolCallSnapshot::new(&id, &name));
        self.index_map.insert(index, vec_idx);
    }

    /// Append arguments delta to a tool call.
    pub fn append_delta(&mut self, index: i64, delta: &str) {
        if let Some(&vec_idx) = self.index_map.get(&index) {
            if let Some(tc) = self.calls.get_mut(vec_idx) {
                tc.append_arguments(delta);
            }
        }
    }

    /// Complete a tool call with final data.
    pub fn complete(&mut self, index: i64, id: &str, name: &str, arguments: String) {
        if let Some(&vec_idx) = self.index_map.get(&index) {
            if let Some(tc) = self.calls.get_mut(vec_idx) {
                tc.id = id.to_string();
                tc.name = name.to_string();
                tc.complete(arguments);
            }
        } else {
            // Tool call done without start - create it directly
            let mut tc = ToolCallSnapshot::new(id, name);
            tc.complete(arguments);
            self.calls.push(tc);
        }
    }

    /// Get a reference to the tool calls slice.
    pub fn as_slice(&self) -> &[ToolCallSnapshot] {
        &self.calls
    }

    /// Get mutable reference to tool calls.
    #[allow(dead_code)]
    pub fn as_mut_slice(&mut self) -> &mut Vec<ToolCallSnapshot> {
        &mut self.calls
    }
}

/// Internal processor state for accumulating stream events.
#[derive(Debug, Default)]
pub(crate) struct ProcessorState {
    /// Current accumulated snapshot.
    pub snapshot: StreamSnapshot,
    /// Tool call manager for tracking index mappings.
    pub tool_calls: ToolCallManager,
}

impl ProcessorState {
    /// Create a new empty state.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update state based on a stream event.
    pub fn update(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::ResponseCreated { id } => {
                self.snapshot.id = Some(id.clone());
            }

            StreamEvent::TextDelta { delta, .. } => {
                self.snapshot.text.push_str(delta);
            }

            StreamEvent::TextDone { .. } => {
                // Text done - we trust accumulated deltas
            }

            StreamEvent::ThinkingDelta { delta, .. } => {
                self.update_thinking_delta(delta);
            }

            StreamEvent::ThinkingDone {
                content, signature, ..
            } => {
                self.update_thinking_done(content, signature.clone());
            }

            StreamEvent::ToolCallStart { index, id, name } => {
                self.tool_calls.start(*index, id.clone(), name.clone());
                self.sync_tool_calls();
            }

            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
                ..
            } => {
                self.tool_calls.append_delta(*index, arguments_delta);
                self.sync_tool_calls();
            }

            StreamEvent::ToolCallDone { index, tool_call } => {
                self.tool_calls.complete(
                    *index,
                    &tool_call.id,
                    &tool_call.name,
                    tool_call.arguments.to_string(),
                );
                self.sync_tool_calls();
            }

            StreamEvent::ResponseDone {
                id,
                usage,
                finish_reason,
                model,
            } => {
                if self.snapshot.id.is_none() {
                    self.snapshot.id = Some(id.clone());
                }
                self.snapshot.usage = usage.clone();
                self.snapshot.finish_reason = Some(*finish_reason);
                self.snapshot.is_complete = true;
                if !model.is_empty() {
                    self.snapshot.model = model.clone();
                }
            }

            StreamEvent::Error(_) | StreamEvent::Ignored => {
                // These don't change snapshot state
            }
        }
    }

    /// Update thinking delta, creating snapshot if needed.
    fn update_thinking_delta(&mut self, delta: &str) {
        match &mut self.snapshot.thinking {
            Some(thinking) => {
                thinking.append(delta);
            }
            None => {
                self.snapshot.thinking = Some(ThinkingSnapshot::new());
                if let Some(thinking) = &mut self.snapshot.thinking {
                    thinking.append(delta);
                }
            }
        }
    }

    /// Update thinking done, preferring accumulated deltas.
    fn update_thinking_done(&mut self, content: &str, signature: Option<String>) {
        match &mut self.snapshot.thinking {
            Some(thinking) => {
                // Deltas were accumulated - just mark complete and add signature
                thinking.signature = signature;
                thinking.is_complete = true;
            }
            None => {
                // No deltas received - use the final content
                self.snapshot.thinking = Some(ThinkingSnapshot {
                    content: content.to_string(),
                    signature,
                    is_complete: true,
                });
            }
        }
    }

    /// Sync tool calls from manager to snapshot.
    fn sync_tool_calls(&mut self) {
        self.snapshot.tool_calls = self.tool_calls.as_slice().to_vec();
    }
}

#[cfg(test)]
#[path = "processor_state.test.rs"]
mod tests;
