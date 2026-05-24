//! `/btw <question>` — by-the-way side-channel question.
//!
//! Asks a quick side question that shares the parent session's prompt
//! cache via [`coco_query::forked_agent`]. Output goes back to the
//! user as a single assistant message; the parent conversation is
//! unaffected.
//!
//! TS: `commands/btw/btw.tsx` + `utils/sideQuestion.ts`. The TS path
//! renders into a modal overlay; coco-rs surfaces the answer inline
//! (TUI shows it as a regular assistant message; SDK consumers see
//! it on the existing message stream).
//!
//! ## Sentinel pattern
//!
//! Slash-command handlers in this crate are pure `fn(&str) -> String` —
//! they don't hold a `QueryEngine` reference, so the actual fork has
//! to happen in the runner. The handler emits:
//!
//! ```text
//! __COCO_BTW_NOW__ <question>
//! <status text shown to the user>
//! ```
//!
//! Runners parse the first line via [`parse_btw_sentinel`], drop it
//! from displayed output, and drive
//! `coco_query::forked_agent::build_query_config` against the engine's
//! `last_cache_safe_params`. If a runner doesn't recognise the
//! sentinel it falls back to displaying both lines — no crash, just a
//! no-op (the runner that doesn't know the protocol simply renders
//! the verbatim text).

/// Sentinel prefix runners recognise on the handler's first output
/// line. Text after the prefix (until newline) is the user's question.
pub const BTW_SENTINEL: &str = "__COCO_BTW_NOW__";

/// Parsed `/btw` request extracted from handler output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtwRequest {
    /// The user's `/btw <question>` argument, trimmed.
    pub question: String,
    /// Remaining lines of the handler output (status text shown
    /// verbatim while the fork is running).
    pub display_text: String,
}

/// Parse a [`BTW_SENTINEL`]-prefixed handler output. Returns `None`
/// when the input does not begin with the sentinel.
#[must_use]
pub fn parse_btw_sentinel(handler_output: &str) -> Option<BtwRequest> {
    let mut lines = handler_output.splitn(2, '\n');
    let first = lines.next()?;
    let rest = lines.next().unwrap_or("");
    let after = first.strip_prefix(BTW_SENTINEL)?;
    let question = after.trim().to_string();
    if question.is_empty() {
        return None;
    }
    Some(BtwRequest {
        question,
        display_text: rest.to_string(),
    })
}

/// Sync handler — emits the sentinel + a one-line status. The runner
/// picks up the sentinel and drives the actual fork.
pub fn handler(args: &str) -> String {
    let question = args.trim();
    if question.is_empty() {
        return "Usage: /btw <question> — Ask a quick side question without interrupting the main conversation.".to_string();
    }
    format!("{BTW_SENTINEL} {question}\nAsking… (this won't affect the main conversation)")
}

#[cfg(test)]
#[path = "btw.test.rs"]
mod tests;
