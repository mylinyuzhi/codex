//! Background session memory extraction agent.
//!
//! This module implements a background agent that proactively extracts
//! conversation summaries during normal operation. This enables "zero API cost"
//! compaction at critical moments because a cached summary is already available.
//!
//! ## Architecture
//!
//! The extraction agent runs asynchronously and doesn't block the main
//! conversation loop. When trigger conditions are met:
//!
//! 1. Agent is spawned with current conversation state
//! 2. LLM summarization request is made
//! 3. Summary is written to `summary.md`
//! 4. Tracking is updated with the last summarized message ID
//!
//! ## Trigger Conditions
//!
//! Extraction is triggered when ALL of the following are true:
//! - Not currently compacting
//! - Not currently extracting
//! - Either:
//!   - First extraction: `min_tokens_to_init` tokens accumulated
//!   - Subsequent: `min_tokens_between` tokens AND `tool_calls_between` tool calls
//!     since last extraction, AND cooldown elapsed

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cocode_inference::ApiClient;
use cocode_inference::AssistantContentPart;
use cocode_inference::LanguageModel;
use cocode_inference::LanguageModelCallOptions;
use cocode_inference::LanguageModelMessage;
use cocode_inference::TextPart;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::LoopEvent;
use cocode_protocol::SessionMemoryExtractionConfig;
use snafu::ResultExt;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::compaction::write_session_memory;
use crate::error::AgentLoopError;
use crate::error::agent_loop_error;

/// Outcome sent from the background extraction task back to the main loop.
///
/// The main loop drains these via `try_recv()` at the top of each iteration
/// to update `AutoCompactTracking`, fixing the bug where `extraction_in_progress`
/// stayed `true` forever after the first extraction.
#[derive(Debug, Clone)]
pub enum ExtractionOutcome {
    /// Extraction completed successfully.
    Completed {
        /// Estimated token count of the summary.
        summary_tokens: i32,
        /// ID of the last message that was summarized.
        last_summarized_id: String,
    },
    /// Extraction failed.
    Failed,
}

/// Result of a session memory extraction operation.
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// The generated summary text.
    pub summary: String,
    /// Estimated token count of the summary.
    pub summary_tokens: i32,
    /// ID of the last message that was summarized.
    pub last_summarized_id: String,
    /// Number of messages that were summarized.
    pub messages_summarized: i32,
}

/// Background agent for session memory extraction.
///
/// This agent runs asynchronously during normal conversation to proactively
/// update the session memory (summary.md). It enables instant (zero API cost)
/// compaction when the context limit is reached.
pub struct SessionMemoryExtractionAgent {
    /// Configuration for extraction behavior.
    config: SessionMemoryExtractionConfig,
    /// API client for LLM requests.
    api_client: ApiClient,
    /// Model to use for summarization.
    model: Arc<dyn LanguageModel>,
    /// Event sender for emitting extraction events.
    event_tx: mpsc::Sender<LoopEvent>,
    /// Path to the summary.md file.
    summary_path: PathBuf,
}

impl SessionMemoryExtractionAgent {
    /// Create a new extraction agent.
    pub fn new(
        config: SessionMemoryExtractionConfig,
        api_client: ApiClient,
        model: Arc<dyn LanguageModel>,
        event_tx: mpsc::Sender<LoopEvent>,
        summary_path: PathBuf,
    ) -> Self {
        Self {
            config,
            api_client,
            model,
            event_tx,
            summary_path,
        }
    }

    /// Check if extraction should be triggered.
    ///
    /// Returns `true` if all trigger conditions are met:
    /// - Extraction is enabled
    /// - Not currently compacting
    /// - Not currently extracting
    /// - Token/tool call thresholds met
    /// - Cooldown elapsed (for subsequent extractions)
    pub fn should_trigger(
        &self,
        tracking: &AutoCompactTracking,
        current_tokens: i32,
        is_compacting: bool,
    ) -> bool {
        // Check basic conditions
        if !self.config.enabled {
            debug!("Extraction disabled");
            return false;
        }

        if is_compacting {
            debug!("Skipping extraction: compaction in progress");
            return false;
        }

        if tracking.extraction_in_progress {
            debug!("Skipping extraction: extraction already in progress");
            return false;
        }

        let tokens_since = tracking.tokens_since_extraction(current_tokens);
        let tool_calls_since = tracking.tool_calls_since_extraction();

        // First extraction: only need min_tokens_to_init
        if tracking.extraction_count == 0 {
            let should = tokens_since >= self.config.min_tokens_to_init;
            debug!(
                tokens_since,
                min_tokens = self.config.min_tokens_to_init,
                should,
                "First extraction check"
            );
            return should;
        }

        // Subsequent extractions: need all conditions
        let cooldown = Duration::from_secs(self.config.cooldown_secs as u64);
        let cooldown_elapsed = tracking.time_since_extraction() >= cooldown;

        let tokens_ok = tokens_since >= self.config.min_tokens_between;
        let tool_calls_ok = tool_calls_since >= self.config.tool_calls_between;

        let should = cooldown_elapsed && tokens_ok && tool_calls_ok;

        debug!(
            tokens_since,
            min_tokens = self.config.min_tokens_between,
            tokens_ok,
            tool_calls_since,
            min_tool_calls = self.config.tool_calls_between,
            tool_calls_ok,
            cooldown_elapsed,
            should,
            "Subsequent extraction check"
        );

        should
    }

    /// Run extraction asynchronously.
    ///
    /// This method:
    /// 1. Emits `SessionMemoryExtractionStarted` event
    /// 2. Builds summarization prompt
    /// 3. Calls LLM for summary
    /// 4. Writes to summary.md
    /// 5. Emits `SessionMemoryExtractionCompleted` or `SessionMemoryExtractionFailed`
    ///
    /// # Arguments
    /// * `conversation_text` - The conversation content to summarize
    /// * `current_tokens` - Current token count in the conversation
    /// * `tool_calls_since` - Tool calls since last extraction
    /// * `last_message_id` - ID of the last message being summarized
    /// * `message_count` - Total number of messages being summarized
    pub async fn run_extraction(
        &self,
        conversation_text: &str,
        current_tokens: i32,
        tool_calls_since: i32,
        last_message_id: &str,
        message_count: i32,
    ) -> Result<ExtractionResult, AgentLoopError> {
        // Emit started event
        self.emit(LoopEvent::SessionMemoryExtractionStarted {
            current_tokens,
            tool_calls_since,
        })
        .await;

        info!(
            current_tokens,
            tool_calls_since, message_count, "Starting session memory extraction"
        );

        // Build summarization prompt
        let system_prompt = self.build_extraction_prompt();
        let user_prompt =
            format!("Please summarize the following conversation:\n\n{conversation_text}");

        // Call LLM for summary
        let messages = vec![
            LanguageModelMessage::system(&system_prompt),
            LanguageModelMessage::user_text(&user_prompt),
        ];
        let mut request = LanguageModelCallOptions::new(messages);
        request.max_output_tokens = Some(self.config.max_summary_tokens as u64);

        let response = match self.api_client.generate(&*self.model, request).await {
            Ok(r) => r,
            Err(e) => {
                let error = format!("LLM request failed: {e}");
                warn!(error = %e, "Session memory extraction failed");
                self.emit(LoopEvent::SessionMemoryExtractionFailed { error, attempts: 1 })
                    .await;
                return Err(e).context(agent_loop_error::ExtractionLlmFailedSnafu);
            }
        };

        // Extract summary text
        let summary: String = response
            .content
            .iter()
            .filter_map(|b| match b {
                AssistantContentPart::Text(TextPart { text, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect();

        if summary.is_empty() {
            let error = "Empty summary generated";
            warn!("Session memory extraction produced empty summary");
            self.emit(LoopEvent::SessionMemoryExtractionFailed {
                error: error.to_string(),
                attempts: 1,
            })
            .await;
            return agent_loop_error::ExtractionEmptySummarySnafu.fail();
        }

        let summary_tokens = cocode_protocol::estimate_text_tokens(&summary);

        // Write to summary.md
        if let Err(e) = write_session_memory(&self.summary_path, &summary, last_message_id).await {
            let error = format!("Failed to write summary.md: {e}");
            warn!(error = %e, path = ?self.summary_path, "Failed to write session memory");
            self.emit(LoopEvent::SessionMemoryExtractionFailed {
                error: error.clone(),
                attempts: 1,
            })
            .await;
            return agent_loop_error::ExtractionWriteFailedSnafu { message: error }.fail();
        }

        info!(
            summary_tokens,
            last_message_id,
            message_count,
            path = ?self.summary_path,
            "Session memory extraction completed"
        );

        // Emit completed event
        self.emit(LoopEvent::SessionMemoryExtractionCompleted {
            summary_tokens,
            last_summarized_id: last_message_id.to_string(),
            messages_summarized: message_count,
        })
        .await;

        Ok(ExtractionResult {
            summary,
            summary_tokens,
            last_summarized_id: last_message_id.to_string(),
            messages_summarized: message_count,
        })
    }

    /// Build the extraction prompt for incremental summarization.
    ///
    /// Uses a 10-section template aligned with Claude Code's session memory format.
    /// Each section has a ~2,000 token limit with a total limit of ~12,000 tokens.
    fn build_extraction_prompt(&self) -> String {
        format!(
            r#"You are extracting key information from an ongoing conversation between a user and an AI coding assistant. Your job is to maintain a structured session memory that preserves essential context.

## Instructions

Maintain exact structure with all 10 sections below. Write DETAILED, INFO-DENSE content under each section. ONLY update content BELOW the italic descriptions — do not modify section headers or descriptions.

Maximum output: {} tokens. Each section should not exceed 2,000 tokens.

## Template

### Session Title
*A short, descriptive title for this session (max 10 words).*
(Summarize the main topic or goal of the conversation.)

### 1. Current State
*Where things stand right now — the latest status of the task/conversation.*
(What is the user currently working on? What was the most recent action or decision?)

### 2. Task Specification
*The original goal and requirements as stated or refined by the user.*
(What was requested? Include any constraints, preferences, or acceptance criteria.)

### 3. Files and Functions
*Key files, functions, classes, and code locations referenced or modified.*
(List file paths with brief notes on what was done or what's relevant in each.)

### 4. Workflow
*Steps taken so far and the overall approach being followed.*
(Outline the sequence of actions, including any iteration or backtracking.)

### 5. Errors & Corrections
*Mistakes made, bugs encountered, and how they were resolved.*
(Document what went wrong and the fix applied — this prevents repeating mistakes.)

### 6. Codebase and System Documentation
*Architectural patterns, conventions, and system details discovered during the session.*
(Note any non-obvious design patterns, configuration, or dependencies found.)

### 7. Learnings
*User preferences, project conventions, and insights discovered during the session.*
(Technical preferences, coding style, tool choices, or workflow preferences.)

### 8. Key Results
*Important outputs, decisions, or artifacts produced.*
(Final or intermediate results: code snippets, config changes, test results, etc.)

### 9. Worklog
*Chronological log of significant actions taken during the conversation.*
(Brief timestamped entries: "Implemented X", "Fixed Y", "User requested Z".)

## Format Rules

- Use bullet points within each section
- Be concise but information-dense — prefer specifics over generalities
- Include file paths, function names, and error messages verbatim when relevant
- If a section has no content, write "N/A" rather than omitting it
- Provide the summary directly without any preamble"#,
            self.config.max_summary_tokens
        )
    }

    /// Emit an event to the event channel.
    async fn emit(&self, event: LoopEvent) {
        if let Err(e) = self.event_tx.send(event).await {
            debug!("Failed to send extraction event: {e}");
        }
    }
}

#[cfg(test)]
#[path = "session_memory_agent.test.rs"]
mod tests;
