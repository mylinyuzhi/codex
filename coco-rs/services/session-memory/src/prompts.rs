//! Prompt templates for the forked-agent extraction call.
//!
//! TS: `services/SessionMemory/prompts.ts` — system + user templates
//! describing what the extractor should produce. We mirror the shape
//! (sectioned markdown, bounded length) without copying the prose
//! verbatim; the caller can override.

/// Default forked-agent extraction prompt.
///
/// The agent receives a normalized transcript and is asked to emit a
/// markdown summary of long-lived facts (decisions, file edits, open
/// questions). Hosts may override per-session via
/// [`crate::SessionMemoryService::set_extraction_prompt`].
#[must_use]
pub fn default_extraction_prompt() -> String {
    r#"You are a session-memory extractor. Read the conversation below and produce
a concise markdown summary that captures only durable, cross-turn context:

  ## Goals
  - one short paragraph describing the user's overall intent.

  ## Decisions
  - bullets of architectural / API / naming decisions made so far.

  ## Files touched
  - bullets of files with one-line summaries of *why* they were changed.

  ## Open questions
  - bullets of unresolved questions or follow-ups.

Rules:
  - Skip trivial chit-chat, error reproductions, and tool noise.
  - Never invent content not in the transcript.
  - Keep total length under 800 words.

Output the markdown directly — no preamble, no closing remarks.
"#
    .to_string()
}
