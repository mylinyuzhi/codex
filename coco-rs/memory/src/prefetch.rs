//! Relevant memory prefetch and selection.
//!
//! TS: memdir/findRelevantMemories.ts — findRelevantMemories,
//!     selectRelevantMemories.
//! TS: utils/attachments.ts — startRelevantMemoryPrefetch.
//!
//! Uses a fast LLM to select the most relevant memories for the current query.
//! Runs asynchronously concurrent with the main turn.

use std::collections::HashSet;
use std::path::Path;

use crate::scan::ScannedMemory;
use crate::staleness;

/// Maximum content bytes per memory for the selection prompt.
const MAX_SELECTION_CONTENT_BYTES: usize = 2048;

/// A memory selected as relevant to the current query.
#[derive(Debug, Clone)]
pub struct RelevantMemory {
    /// Absolute path to the memory file.
    pub path: String,
    /// File content (may be truncated).
    pub content: String,
    /// Modification time in milliseconds since epoch.
    pub mtime_ms: i64,
    /// Pre-computed header: "[age] filename".
    pub header: String,
}

/// State for tracking which memories have already been surfaced in a session.
///
/// Prevents re-picking the same memory across turns.
#[derive(Debug, Default)]
pub struct PrefetchState {
    /// Set of memory file paths already surfaced this session.
    pub already_surfaced: HashSet<String>,
    /// Cumulative bytes of relevant memories surfaced this session.
    pub total_bytes: i64,
}

/// Session-level cap on cumulative relevant-memory bytes.
const MAX_SESSION_MEMORY_BYTES: i64 = 60 * 1024;

impl PrefetchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a memory has already been surfaced this session.
    pub fn is_surfaced(&self, path: &str) -> bool {
        self.already_surfaced.contains(path)
    }

    /// Mark a memory as surfaced and track its byte cost.
    pub fn mark_surfaced(&mut self, path: &str, bytes: i64) {
        self.already_surfaced.insert(path.to_string());
        self.total_bytes += bytes;
    }

    /// Check if the session memory budget is exhausted.
    pub fn is_budget_exhausted(&self) -> bool {
        self.total_bytes >= MAX_SESSION_MEMORY_BYTES
    }
}

/// The system prompt for the relevance-ranking LLM.
///
/// TS: findRelevantMemories.ts SELECT_MEMORIES_SYSTEM_PROMPT.
const SELECT_MEMORIES_SYSTEM_PROMPT: &str = "\
You are selecting memories that will be useful to Claude Code as it processes a user's query. \
You will be given the user's query and a list of available memory files with their filenames \
and descriptions.\n\
\n\
Return a list of filenames for the memories that will clearly be useful to Claude Code as it \
processes the user's query (up to 5). Only include memories that you are certain will be \
helpful based on their name and description.\n\
- If you are unsure if a memory will be useful in processing the user's query, then do not \
include it in your list. Be selective and discerning.\n\
- If there are no memories in the list that would clearly be useful, feel free to return an \
empty list.\n\
- If a list of recently-used tools is provided, do not select memories that are usage \
reference or API documentation for those tools (Claude Code is already exercising them). \
DO still select memories containing warnings, gotchas, or known issues about those tools — \
active use is exactly when those matter.";

/// Build the selection prompt for the relevance-ranking LLM.
///
/// TS: findRelevantMemories.ts — selectRelevantMemories builds the user prompt.
pub fn build_selection_prompt(
    scanned: &[ScannedMemory],
    prefetch_state: &PrefetchState,
    max_relevant: usize,
    recent_tools: &[String],
) -> String {
    let mut lines = Vec::new();
    lines.push(SELECT_MEMORIES_SYSTEM_PROMPT.to_string());
    lines.push(String::new());
    lines.push(format!(
        "Select up to {max_relevant} memories. Return JSON: {{\"selected_memories\": [\"filename.md\", ...]}}"
    ));
    lines.push(String::new());

    if !recent_tools.is_empty() {
        lines.push(format!("Recently-used tools: {}", recent_tools.join(", "),));
        lines.push(String::new());
    }

    lines.push("## Available Memories".to_string());
    lines.push(String::new());

    for mem in scanned {
        let path_str = mem.path.to_string_lossy();
        if prefetch_state.is_surfaced(&path_str) {
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
        let mem_type = mem
            .frontmatter
            .as_ref()
            .map_or("unknown", |fm| fm.memory_type.as_str());
        let age = staleness::memory_age(mem.mtime_ms);

        lines.push(format!(
            "- `{path_str}` — {name} ({mem_type}): {desc} [{age}]"
        ));
    }

    lines.join("\n")
}

/// Parse the selection response (JSON array of file paths).
pub fn parse_selection_response(response: &str) -> Vec<String> {
    // Extract JSON array from response
    let start = response.find('[').unwrap_or(0);
    let end = response.rfind(']').map(|i| i + 1).unwrap_or(response.len());
    let json_str = &response[start..end];

    serde_json::from_str::<Vec<String>>(json_str).unwrap_or_default()
}

/// Load selected memories from disk, applying truncation and freshness headers.
pub fn load_relevant_memories(
    selected_paths: &[String],
    prefetch_state: &mut PrefetchState,
    max_relevant: usize,
) -> Vec<RelevantMemory> {
    let mut memories = Vec::new();

    for path_str in selected_paths {
        if memories.len() >= max_relevant {
            break;
        }
        if prefetch_state.is_budget_exhausted() {
            break;
        }
        if prefetch_state.is_surfaced(path_str) {
            continue;
        }

        let path = Path::new(path_str);
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Truncate content if too large
        let truncated = if content.len() > MAX_SELECTION_CONTENT_BYTES {
            let mut s = content[..MAX_SELECTION_CONTENT_BYTES].to_string();
            s.push_str("\n... (truncated)");
            s
        } else {
            content
        };

        let mtime_ms = staleness::file_mtime_ms(path).unwrap_or(0);
        let age = staleness::memory_age(mtime_ms);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let header = format!("[{age}] {filename}");

        let byte_cost = truncated.len() as i64;
        prefetch_state.mark_surfaced(path_str, byte_cost);

        memories.push(RelevantMemory {
            path: path_str.clone(),
            content: truncated,
            mtime_ms,
            header,
        });
    }

    memories
}

/// Select relevant memories without LLM (heuristic fallback).
///
/// Uses recency + type matching as a simple relevance heuristic when
/// no LLM is available for selection.
pub fn select_heuristic(
    scanned: &[ScannedMemory],
    prefetch_state: &PrefetchState,
    max_relevant: usize,
) -> Vec<String> {
    scanned
        .iter()
        .filter(|m| !prefetch_state.is_surfaced(&m.path.to_string_lossy()))
        .take(max_relevant)
        .map(|m| m.path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
#[path = "prefetch.test.rs"]
mod tests;
