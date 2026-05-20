//! Relevant-memory selection (LLM side-query + heuristic fallback).
//!
//! TS: `memdir/findRelevantMemories.ts`. Sends the manifest and the
//! current user query to a fast model; receives a JSON list of up to
//! five filenames the model thinks are most relevant. We track which
//! files have already been surfaced this session so the recall doesn't
//! re-inject the same memory turn after turn.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use crate::scan::ScannedMemory;
use crate::scan::file_mtime_ms;
use crate::scan::format_memory_manifest;
use crate::scan::memory_age_string;
use crate::scan::memory_freshness_text;

/// Hard cap on relevant memories returned per turn.
pub const MAX_RELEVANT: usize = 5;

/// Per-memory body cap when injecting content into the recall prompt or
/// the user-context attachment. Mirrors TS `MAX_MEMORY_BYTES` used by
/// `readMemoriesForSurfacing` (`attachments.ts:277, 2298`).
pub const MAX_BODY_BYTES: usize = 4_096;

/// Cumulative cap on bytes injected per session so a single chatty
/// memory directory can't blow the context budget.
pub const MAX_SESSION_BYTES: i64 = 60 * 1024;

/// One memory selected as relevant to the current query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantMemory {
    pub path: String,
    pub content: String,
    pub mtime_ms: i64,
    /// Pre-computed `[age] filename` header for the attachment block.
    pub header: String,
}

/// Cross-turn state for recall: tracks which memories have already
/// been surfaced and the cumulative byte cost.
#[derive(Debug, Default)]
pub struct PrefetchState {
    inner: Mutex<PrefetchInner>,
}

#[derive(Debug, Default)]
struct PrefetchInner {
    already_surfaced: HashSet<String>,
    total_bytes: i64,
}

impl PrefetchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `expect` semantics on poison: a poisoned `Mutex` means a panic
    /// happened mid-write on `already_surfaced` or `total_bytes`,
    /// leaving the inner state inconsistent. The previous silent-recovery
    /// (`unwrap_or(false)` / `unwrap_or(true)`) masked the bug AND
    /// produced wrong answers downstream (recall surfaces duplicates
    /// or starves the budget). Library-internal invariants should
    /// panic loudly so the bug surfaces in tests rather than
    /// silently rotting recall behavior for the rest of the session.
    fn lock(&self) -> std::sync::MutexGuard<'_, PrefetchInner> {
        self.inner
            .lock()
            .expect("PrefetchState mutex poisoned — invariant broken")
    }

    pub fn is_surfaced(&self, path: &str) -> bool {
        self.lock().already_surfaced.contains(path)
    }

    pub fn mark_surfaced(&self, path: &str, bytes: i64) {
        let mut s = self.lock();
        s.already_surfaced.insert(path.to_string());
        s.total_bytes += bytes;
    }

    pub fn is_budget_exhausted(&self) -> bool {
        self.lock().total_bytes >= MAX_SESSION_BYTES
    }

    /// Wipe the surfaced set + byte counter — call from
    /// [`crate::MemoryRuntime::reset`] on `/clear` and from the
    /// compact post-step so a fresh transcript can re-surface memory
    /// without inheriting the prior session's dedup + 60 KB cap.
    pub fn reset(&self) {
        let mut s = self.lock();
        s.already_surfaced.clear();
        s.total_bytes = 0;
    }
}

/// System prompt for the relevance-ranker side-query.
///
/// TS: `findRelevantMemories.ts:SELECT_MEMORIES_SYSTEM_PROMPT`.
/// Multi-LLM neutral wording (no "Claude Code" reference); the
/// `with_skip_system_prefix(true)` on the request keeps the agent's
/// preamble out of the ranker so this stays the only system context.
pub const SELECT_MEMORIES_SYSTEM_PROMPT: &str = "\
You are selecting memories that will be useful to an AI coding assistant as it processes a \
user's query. You will be given the user's query and a list of available memory files with \
their filenames and descriptions.\n\
\n\
Return a JSON object: {\"selected_memories\": [\"filename.md\", ...]} listing up to 5 filenames \
for memories that will clearly be useful. Only include memories you are certain will help.\n\
- If unsure, leave it out. Be selective.\n\
- If none clearly help, return an empty list.\n\
- If a list of recently-used tools is provided, do NOT select API/usage references for those \
tools (the assistant is already exercising them). DO still select warnings, gotchas, or known \
issues about those tools — active use is exactly when those matter.";

/// Build the user-side prompt for the recall ranker.
///
/// Layout (TS `findRelevantMemories.ts:105`):
///
/// ```text
/// Query: <query>
///
/// Available memories:
/// - [<type>] <filename> (<iso-ts>): <description>
/// ...
///
/// Recently used tools: <tool>, <tool>
/// ```
///
/// Uses [`format_memory_manifest`] for the bullet body so the manifest
/// stays byte-equivalent with what TS sends — calibration of the
/// ranker depends on this exact format. Already-surfaced files are
/// filtered before formatting (the ranker wastes turns if it returns
/// names the loader will then drop).
pub fn build_selection_prompt(
    query: &str,
    scanned: &[ScannedMemory],
    state: &PrefetchState,
    recent_tools: &[String],
) -> String {
    let visible: Vec<ScannedMemory> = scanned
        .iter()
        .filter(|m| !state.is_surfaced(&m.path.to_string_lossy()))
        .cloned()
        .collect();

    let mut out = String::new();
    out.push_str(&format!("Query: {query}\n\nAvailable memories:\n"));
    if visible.is_empty() {
        out.push_str("(none)");
    } else {
        out.push_str(&format_memory_manifest(&visible));
    }
    if !recent_tools.is_empty() {
        out.push_str(&format!(
            "\n\nRecently used tools: {}",
            recent_tools.join(", ")
        ));
    }
    out
}

/// Parse the selection response — a JSON object with a
/// `selected_memories` array, with permissive fallback to a bare array.
pub fn parse_selection_response(response: &str) -> Vec<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(arr) = v.get("selected_memories").and_then(|x| x.as_array())
    {
        return arr
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
    }
    // Fallback: a bare array embedded somewhere in the response.
    if let (Some(start), Some(end)) = (response.find('['), response.rfind(']'))
        && start < end
    {
        let slice = &response[start..=end];
        if let Ok(v) = serde_json::from_str::<Vec<String>>(slice) {
            return v;
        }
    }
    Vec::new()
}

/// Load selected memories from disk, applying truncation, freshness
/// headers, and the per-session byte budget. Marks each loaded path as
/// "already surfaced" so it isn't picked again later in the session.
pub fn load_relevant_memories(
    selected_paths: &[String],
    state: &PrefetchState,
) -> Vec<RelevantMemory> {
    let mut out = Vec::new();
    for path_str in selected_paths {
        if out.len() >= MAX_RELEVANT {
            break;
        }
        if state.is_budget_exhausted() {
            break;
        }
        if state.is_surfaced(path_str) {
            continue;
        }
        let path = Path::new(path_str);
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let truncated = if content.len() > MAX_BODY_BYTES {
            let cut = content[..MAX_BODY_BYTES]
                .rfind('\n')
                .unwrap_or(MAX_BODY_BYTES);
            format!(
                "{}\n\nThis memory file was truncated ({MAX_BODY_BYTES} byte limit). Use the \
                 Read tool to view the complete file at: {path_str}",
                &content[..cut]
            )
        } else {
            content
        };
        let mtime_ms = file_mtime_ms(path).unwrap_or(0);
        let age = memory_age_string(mtime_ms);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        // Freshness caveat (TS `memoryAge.ts:memoryFreshnessText`) is
        // prepended for memories older than one day so the model
        // doesn't blindly trust stale file/line references.
        let freshness = memory_freshness_text(mtime_ms);
        let header = format!("{freshness}[{age}] {filename}");
        let bytes = truncated.len() as i64;
        state.mark_surfaced(path_str, bytes);
        out.push(RelevantMemory {
            path: path_str.clone(),
            content: truncated,
            mtime_ms,
            header,
        });
    }
    out
}

// Note: a previous `select_heuristic` fallback was deliberately
// removed to mirror TS `findRelevantMemories.ts:131-140`. When the
// ranker errors or no LLM handle is installed, recall stays silent
// (returns empty) — surfacing arbitrarily-recent memories that
// occupy attention budget and the 60 KB session byte cap is worse
// than surfacing none. The runtime treats "ranker unavailable" the
// same as "ranker returned no matches."

#[cfg(test)]
#[path = "recall.test.rs"]
mod tests;
