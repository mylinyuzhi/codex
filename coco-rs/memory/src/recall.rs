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
use crate::scan::memory_age_string;

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

    pub fn is_surfaced(&self, path: &str) -> bool {
        self.inner
            .lock()
            .map(|s| s.already_surfaced.contains(path))
            .unwrap_or(false)
    }

    pub fn mark_surfaced(&self, path: &str, bytes: i64) {
        if let Ok(mut s) = self.inner.lock() {
            s.already_surfaced.insert(path.to_string());
            s.total_bytes += bytes;
        }
    }

    pub fn is_budget_exhausted(&self) -> bool {
        self.inner
            .lock()
            .map(|s| s.total_bytes >= MAX_SESSION_BYTES)
            .unwrap_or(true)
    }

    /// Wipe the surfaced set + byte counter — call from
    /// [`crate::MemoryRuntime::reset`] on `/clear` so the next
    /// conversation starts fresh.
    pub fn reset(&self) {
        if let Ok(mut s) = self.inner.lock() {
            s.already_surfaced.clear();
            s.total_bytes = 0;
        }
    }
}

/// System prompt for the relevance-ranker side-query.
///
/// TS: `findRelevantMemories.ts:SELECT_MEMORIES_SYSTEM_PROMPT`.
pub const SELECT_MEMORIES_SYSTEM_PROMPT: &str = "\
You are selecting memories that will be useful to Claude Code as it processes a user's query. \
You will be given the user's query and a list of available memory files with their filenames \
and descriptions.\n\
\n\
Return a JSON object: {\"selected_memories\": [\"filename.md\", ...]} listing up to 5 filenames \
for memories that will clearly be useful. Only include memories you are certain will help.\n\
- If unsure, leave it out. Be selective.\n\
- If none clearly help, return an empty list.\n\
- If a list of recently-used tools is provided, do NOT select API/usage references for those \
tools (Claude is already exercising them). DO still select warnings, gotchas, or known issues \
about those tools — active use is exactly when those matter.";

/// Build the user-side prompt for the recall ranker.
pub fn build_selection_prompt(
    query: &str,
    scanned: &[ScannedMemory],
    state: &PrefetchState,
    recent_tools: &[String],
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("User query: {query}"));
    lines.push(String::new());
    if !recent_tools.is_empty() {
        lines.push(format!("Recently-used tools: {}", recent_tools.join(", ")));
        lines.push(String::new());
    }
    lines.push("## Available Memories".to_string());
    if scanned.is_empty() {
        lines.push("_(none)_".to_string());
        return lines.join("\n");
    }
    for mem in scanned {
        let path_str = mem.path.to_string_lossy();
        if state.is_surfaced(&path_str) {
            continue;
        }
        let name = mem
            .frontmatter
            .as_ref()
            .map_or("unknown", |fm| fm.name.as_str());
        let desc = mem
            .frontmatter
            .as_ref()
            .map_or("", |fm| fm.description.as_str());
        let ty = mem
            .frontmatter
            .as_ref()
            .map_or("unknown", |fm| fm.memory_type.as_str());
        let age = memory_age_string(mem.mtime_ms);
        lines.push(format!(
            "- `{}` — {name} ({ty}): {desc} [{age}]",
            mem.filename
        ));
    }
    lines.join("\n")
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
            format!("{}\n... (truncated)", &content[..cut])
        } else {
            content
        };
        let mtime_ms = file_mtime_ms(path).unwrap_or(0);
        let age = memory_age_string(mtime_ms);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let header = format!("[{age}] {filename}");
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

/// Recency-only fallback when no LLM is available — returns up to
/// `MAX_RELEVANT` filenames newest-first, skipping already-surfaced.
pub fn select_heuristic(scanned: &[ScannedMemory], state: &PrefetchState) -> Vec<String> {
    scanned
        .iter()
        .filter(|m| !state.is_surfaced(&m.path.to_string_lossy()))
        .take(MAX_RELEVANT)
        .map(|m| m.path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
#[path = "recall.test.rs"]
mod tests;
