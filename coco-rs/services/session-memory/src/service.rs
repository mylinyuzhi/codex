//! Forked-agent extraction service.
//!
//! `SessionMemoryService` owns the decision-and-execute loop: every
//! caller invokes [`SessionMemoryService::maybe_extract`] after a turn
//! completes. We consult [`coco_compact::should_extract_memory`], and
//! when it fires we run the caller-supplied summarizer (the "forked
//! agent" in TS terms — typically a `Fast`-role LLM call) and write
//! the result back to disk atomically.
//!
//! TS: `services/SessionMemory/sessionMemory.ts:374 setupSessionMemoryFile`
//! + the `extractSessionMemory` post-sampling hook. We collapse those
//!   two steps into one method so the host doesn't have to wire a hook
//!   object — the hook becomes "call this after every assistant turn".

use std::path::PathBuf;
use std::sync::Arc;

use coco_compact::SessionMemoryExtractionInputs;
use coco_compact::SessionMemoryExtractionThresholds;
use coco_compact::should_extract_memory;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

use crate::path::session_memory_path;
use crate::prompts::default_extraction_prompt;

/// Async summarizer signature. Caller supplies a closure wrapping a
/// forked-agent LLM call. Receives the full extraction prompt
/// (system + transcript) and returns the produced markdown body.
///
/// Errors propagate to [`SessionMemoryService::maybe_extract`] which
/// logs and skips the write.
pub type SummarizerFn = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, anyhow::Error>> + Send>,
        > + Send
        + Sync,
>;

/// Outcome of a single [`SessionMemoryService::maybe_extract`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionOutcome {
    /// Threshold not met — no work done.
    Skipped(ExtractionDecision),
    /// Summarizer produced new content; written to disk.
    Extracted { written_to: PathBuf, bytes: usize },
    /// Threshold met but summarizer returned an empty body — no write.
    EmptySummary,
    /// Disabled (no summarizer installed). No work attempted.
    Disabled,
}

/// Why the extractor was skipped — captured for telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionDecision {
    BelowInitThreshold,
    InsufficientDelta,
    InsufficientToolActivity,
}

/// Service holding session-memory state. Cheap to clone (`Arc` interior).
pub struct SessionMemoryService {
    inner: Arc<Inner>,
}

struct Inner {
    config_home: PathBuf,
    /// Mutable session id so `/clear` regen propagates to disk paths
    /// (`path()` reads it on every operation). Pre-fix this was a
    /// plain `String` baked at construction — post-clear writes would
    /// land in the OLD session's memory directory, mixing two
    /// sessions' data.
    session_id: std::sync::RwLock<String>,
    /// In-memory cache of the latest summary body. Empty string ⇒
    /// nothing extracted yet (or file missing).
    text: RwLock<String>,
    /// Tokens at the most recent successful extract. 0 = never.
    tokens_at_last_extract: RwLock<i64>,
    /// `lastSummarizedMessageId` — UUID of the assistant message that
    /// triggered the most recent extract. Used by the SM-first compact
    /// path to anchor the keep-tail. TS: sessionMemoryUtils.ts:44.
    last_summarized_message_id: RwLock<Option<Uuid>>,
    /// Tunable thresholds.
    thresholds: SessionMemoryExtractionThresholds,
    /// Optional override of the default extraction prompt.
    prompt_override: RwLock<Option<String>>,
    /// Caller-installed summarizer. None ⇒ service is inert.
    summarizer: RwLock<Option<SummarizerFn>>,
    /// Held for the duration of an in-flight extraction. Allows
    /// SM-first compact to wait for any pending extraction so it
    /// reads the freshly-written memory rather than a stale snapshot.
    /// TS: waitForSessionMemoryExtraction (sessionMemoryCompact.ts:527).
    extraction_lock: Mutex<()>,
}

impl SessionMemoryService {
    pub fn new(config_home: PathBuf, session_id: String) -> Self {
        Self {
            inner: Arc::new(Inner {
                config_home,
                session_id: std::sync::RwLock::new(session_id),
                text: RwLock::new(String::new()),
                tokens_at_last_extract: RwLock::new(0),
                last_summarized_message_id: RwLock::new(None),
                thresholds: SessionMemoryExtractionThresholds::default(),
                prompt_override: RwLock::new(None),
                summarizer: RwLock::new(None),
                extraction_lock: Mutex::new(()),
            }),
        }
    }

    /// Update the session id used for disk paths. Called by
    /// `SessionRuntime::clear_conversation` after `regenerateSessionId`
    /// so subsequent `current_text()` reads / `maybe_extract` writes
    /// hit the new session's memory file. Also clears the in-memory
    /// text cache — the new session has no extracted memory yet.
    pub async fn set_session_id(&self, new_id: String) {
        if let Ok(mut g) = self.inner.session_id.write() {
            *g = new_id;
        }
        let mut t = self.inner.text.write().await;
        t.clear();
        let mut tok = self.inner.tokens_at_last_extract.write().await;
        *tok = 0;
        let mut last = self.inner.last_summarized_message_id.write().await;
        *last = None;
    }

    /// Override the threshold tuning (e.g. from settings.json).
    pub fn with_thresholds(self, thresholds: SessionMemoryExtractionThresholds) -> Self {
        // SAFETY: we own this Arc exclusively at construction time.
        // If a caller already cloned this, the override is silently
        // dropped — that's the intended "set once at startup" pattern.
        if Arc::strong_count(&self.inner) == 1 {
            // Use a dance: build new inner with new thresholds.
            let prev = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| {
                // Single-owner check above can race in theory; fall
                // back to leaving thresholds at default if it does.
                let id = arc.session_id.read().map(|g| g.clone()).unwrap_or_default();
                Inner {
                    config_home: arc.config_home.clone(),
                    session_id: std::sync::RwLock::new(id),
                    text: RwLock::new(String::new()),
                    tokens_at_last_extract: RwLock::new(0),
                    last_summarized_message_id: RwLock::new(None),
                    thresholds: SessionMemoryExtractionThresholds::default(),
                    prompt_override: RwLock::new(None),
                    summarizer: RwLock::new(None),
                    extraction_lock: Mutex::new(()),
                }
            });
            return Self {
                inner: Arc::new(Inner { thresholds, ..prev }),
            };
        }
        self
    }

    /// Install the forked-agent summarizer. Without this, the service
    /// is inert (every `maybe_extract` returns [`ExtractionOutcome::Disabled`]).
    pub async fn set_summarizer(&self, summarizer: SummarizerFn) {
        let mut g = self.inner.summarizer.write().await;
        *g = Some(summarizer);
    }

    pub async fn set_extraction_prompt(&self, prompt: String) {
        let mut g = self.inner.prompt_override.write().await;
        *g = Some(prompt);
    }

    /// Best-effort load of any existing on-disk summary into the
    /// in-memory cache. Call once at startup. Missing file ⇒ no-op.
    pub async fn load_from_disk(&self) -> anyhow::Result<()> {
        let path = self.path();
        match fs::read_to_string(&path).await {
            Ok(body) => {
                let mut g = self.inner.text.write().await;
                *g = body;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Read the cached summary body. SM-first compact uses this.
    pub async fn current_text(&self) -> String {
        self.inner.text.read().await.clone()
    }

    pub async fn last_summarized_message_id(&self) -> Option<Uuid> {
        *self.inner.last_summarized_message_id.read().await
    }

    /// Push a new boundary anchor. SM-compact calls this after writing
    /// the post-compact history so the next extraction has the same
    /// view of "where summary coverage ends" as the compactor.
    pub async fn set_last_summarized_message_id(&self, uuid: Option<Uuid>) {
        let mut g = self.inner.last_summarized_message_id.write().await;
        *g = uuid;
    }

    /// Resolved on-disk path of the session-memory file. Reads
    /// `session_id` under a brief read lock so `/clear` regen is
    /// picked up immediately.
    pub fn path(&self) -> PathBuf {
        let id = self
            .inner
            .session_id
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        session_memory_path(&self.inner.config_home, &id)
    }

    /// Decide whether to run extraction this turn and execute when so.
    ///
    /// Inputs map to [`SessionMemoryExtractionInputs`]:
    ///   - `current_tokens` = full conversation token estimate now.
    ///   - `tool_calls_in_last_turn` = tool_use blocks in the just-completed
    ///     assistant turn (caller computes via `coco_messages` predicates).
    ///   - `latest_assistant_uuid` = the assistant message UUID that
    ///     would become the new `lastSummarizedMessageId` on success.
    ///
    /// `transcript_for_extractor` is the full conversation text used as
    /// the user portion of the extraction prompt. Caller can pre-trim
    /// (e.g. drop binary blobs) — this service does no transformation.
    /// Block until any in-flight extraction has completed. Returns
    /// immediately when no extraction is running. Used by SM-first
    /// compact to read freshly-written memory.
    pub async fn wait_for_extraction(&self) {
        // Acquiring + dropping the mutex returns once whoever holds it
        // (a `maybe_extract` in flight) lets go.
        let _guard = self.inner.extraction_lock.lock().await;
    }

    pub async fn maybe_extract(
        &self,
        current_tokens: i64,
        tool_calls_in_last_turn: i32,
        latest_assistant_uuid: Option<Uuid>,
        transcript_for_extractor: &str,
    ) -> ExtractionOutcome {
        let _extraction_guard = self.inner.extraction_lock.lock().await;
        let summarizer = {
            let g = self.inner.summarizer.read().await;
            g.clone()
        };
        let Some(summarizer) = summarizer else {
            return ExtractionOutcome::Disabled;
        };

        let tokens_at_last = *self.inner.tokens_at_last_extract.read().await;
        let inputs = SessionMemoryExtractionInputs {
            current_tokens,
            tokens_at_last_extract: tokens_at_last,
            tool_calls_in_last_turn,
        };
        if !should_extract_memory(inputs, &self.inner.thresholds) {
            let decision = if tokens_at_last <= 0 {
                ExtractionDecision::BelowInitThreshold
            } else if (current_tokens - tokens_at_last)
                < self.inner.thresholds.minimum_tokens_between_update
            {
                ExtractionDecision::InsufficientDelta
            } else {
                ExtractionDecision::InsufficientToolActivity
            };
            return ExtractionOutcome::Skipped(decision);
        }

        let prompt = self.compose_prompt(transcript_for_extractor).await;
        let body = match summarizer(prompt).await {
            Ok(s) => s,
            Err(e) => {
                warn!("session-memory extractor failed: {e}");
                return ExtractionOutcome::EmptySummary;
            }
        };
        let body = body.trim().to_string();
        if body.is_empty() {
            return ExtractionOutcome::EmptySummary;
        }

        if let Err(e) = self.write_atomic(&body).await {
            warn!("session-memory write failed: {e}");
            return ExtractionOutcome::EmptySummary;
        }

        let bytes = body.len();
        {
            let mut g = self.inner.text.write().await;
            *g = body;
        }
        {
            let mut g = self.inner.tokens_at_last_extract.write().await;
            *g = current_tokens;
        }
        {
            let mut g = self.inner.last_summarized_message_id.write().await;
            *g = latest_assistant_uuid;
        }
        info!(bytes, "session-memory extracted");
        ExtractionOutcome::Extracted {
            written_to: self.path(),
            bytes,
        }
    }

    /// Wipe the cached body and reset extraction state. Called from a
    /// `CompactionObserver::on_compaction_complete` handler so the next
    /// extraction starts from a clean baseline.
    pub async fn clear_after_compact(&self) {
        let mut t = self.inner.text.write().await;
        t.clear();
        let mut tokens = self.inner.tokens_at_last_extract.write().await;
        *tokens = 0;
        let mut id = self.inner.last_summarized_message_id.write().await;
        *id = None;
    }

    async fn compose_prompt(&self, transcript: &str) -> String {
        let template = {
            let g = self.inner.prompt_override.read().await;
            g.clone().unwrap_or_else(default_extraction_prompt)
        };
        format!("{template}\n\n--- transcript ---\n{transcript}\n")
    }

    async fn write_atomic(&self, body: &str) -> anyhow::Result<()> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        // Tempfile-and-rename for atomic on-disk swap. Mirrors TS
        // `await fs.writeFile(tmp, body); await fs.rename(tmp, path)`.
        let tmp = path.with_extension("md.tmp");
        fs::write(&tmp, body).await?;
        fs::rename(&tmp, &path).await?;
        Ok(())
    }
}

impl Clone for SessionMemoryService {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Help callers count tool_use blocks in the last assistant turn so they
/// can pass `tool_calls_in_last_turn` to [`SessionMemoryService::maybe_extract`].
///
/// Walks `messages` from the tail looking for the most recent assistant
/// message and counts tool_use content blocks within it.
#[must_use]
pub fn count_tool_calls_in_last_assistant_turn(messages: &[coco_messages::Message]) -> i32 {
    use coco_messages::AssistantContent;
    use coco_messages::LlmMessage;
    use coco_messages::Message;

    for m in messages.iter().rev() {
        let Message::Assistant(asst) = m else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &asst.message else {
            return 0;
        };
        let count = content
            .iter()
            .filter(|p| matches!(p, AssistantContent::ToolCall(_)))
            .count() as i32;
        return count;
    }
    0
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
