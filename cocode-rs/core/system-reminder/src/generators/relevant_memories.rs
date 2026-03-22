//! Relevant memories generator.
//!
//! Searches the auto memory directory for topic files relevant to the
//! current user prompt and injects them as system reminders with
//! staleness information.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use tracing::debug;
use tracing::warn;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for relevant memory file injection.
#[derive(Debug)]
pub struct RelevantMemoriesGenerator;

#[async_trait]
impl AttachmentGenerator for RelevantMemoriesGenerator {
    fn name(&self) -> &str {
        "RelevantMemoriesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::RelevantMemories
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        // Enabled when auto memory state is present (feature-gated at initialization)
        true
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Static fallback when no context is available.
        ThrottleConfig {
            min_turns_between: cocode_protocol::DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS,
            ..ThrottleConfig::default()
        }
    }

    fn throttle_config_for_context(&self, ctx: &GeneratorContext<'_>) -> ThrottleConfig {
        // Use the user-configurable throttle value from auto_memory_state
        // instead of the hardcoded default.
        let min_turns = ctx
            .auto_memory_state
            .as_ref()
            .map(|s| s.config.relevant_memories_throttle_turns)
            .unwrap_or(cocode_protocol::DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS);
        ThrottleConfig {
            min_turns_between: min_turns,
            ..ThrottleConfig::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let state = match ctx.auto_memory_state.as_ref() {
            Some(s) if s.is_enabled() => s,
            _ => return Ok(None),
        };

        // Gate on the RelevantMemories feature flag (independent of AutoMemory).
        if !state.config.relevant_memories_enabled {
            return Ok(None);
        }

        // Need user prompt to determine relevance
        let user_prompt = match ctx.user_prompt {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(None),
        };

        let timeout_ms = state.config.relevant_search_timeout_ms;
        let timeout = Duration::from_millis(timeout_ms as u64);

        // Wrap the entire search in a timeout to bound latency.
        match tokio::time::timeout(timeout, search_relevant_memories(state, user_prompt)).await {
            Ok(result) => result,
            Err(_) => {
                warn!(timeout_ms, "Relevant memories search timed out");
                Ok(None)
            }
        }
    }
}

/// Perform the actual memory file search and scoring.
async fn search_relevant_memories(
    state: &cocode_auto_memory::AutoMemoryState,
    user_prompt: &str,
) -> Result<Option<SystemReminder>> {
    let config = &state.config;
    let memory_dir = &config.directory;
    let max_files = config.max_relevant_files;
    let max_lines = config.max_lines_per_file;
    let max_files_to_scan = config.max_files_to_scan;
    let max_frontmatter_lines = config.max_frontmatter_lines;
    let min_keyword_length = config.min_keyword_length as usize;
    let staleness_warning_days = config.staleness_warning_days as i64;

    // List available memory files
    let files = match cocode_auto_memory::list_memory_files(memory_dir) {
        Ok(f) => f,
        Err(e) => {
            warn!(error = %e, "Failed to list memory files");
            return Ok(None);
        }
    };

    let files_scanned = files.len();
    if files.is_empty() {
        debug!(files_scanned = 0, "No memory files to search");
        return Ok(None);
    }

    // Deduplicate: skip files already referenced in MEMORY.md (they're
    // already in context via the AutoMemoryPrompt generator).
    let index_referenced = extract_index_filenames(state).await;

    // Load files concurrently and score by keyword relevance
    let prompt_lower = user_prompt.to_lowercase();

    let load_futures: Vec<_> = files
        .into_iter()
        .take(max_files_to_scan as usize)
        .map(|path| {
            tokio::task::spawn_blocking(move || {
                cocode_auto_memory::load_memory_file(&path, max_lines, max_frontmatter_lines)
            })
        })
        .collect();

    let results = futures::future::join_all(load_futures).await;

    let mut scored_entries: Vec<(i32, cocode_auto_memory::AutoMemoryEntry)> = Vec::new();
    for result in results {
        let entry = match result {
            Ok(Ok(entry)) => entry,
            _ => continue,
        };

        // Skip files already referenced in MEMORY.md index
        if let Some(name) = entry.path.file_name().and_then(|n| n.to_str())
            && index_referenced.contains(name)
        {
            continue;
        }

        let score = compute_relevance_score(&entry, &prompt_lower, min_keyword_length);
        if score > 0 {
            scored_entries.push((score, entry));
        }
    }

    if scored_entries.is_empty() {
        return Ok(None);
    }

    // Sort by score descending, take top N
    scored_entries.sort_by(|a, b| b.0.cmp(&a.0));
    scored_entries.truncate(max_files as usize);

    debug!(
        files_scanned,
        files_matched = scored_entries.len(),
        top_score = scored_entries.first().map(|(s, _)| *s).unwrap_or(0),
        "Relevant memories search complete"
    );

    // Format as system reminder content
    let mut parts = Vec::new();
    for (_, entry) in &scored_entries {
        let mut header = String::new();

        // Add staleness info
        if let Some(mtime) = entry.last_modified {
            let staleness = cocode_auto_memory::staleness_info(mtime, staleness_warning_days);
            header.push_str(&format!(
                "Memory (saved {}): {}:",
                staleness.relative_time,
                entry.path.display()
            ));
            if staleness.needs_warning {
                header.push_str(&format!("\n{}", staleness.warning));
            }
        } else {
            header.push_str(&format!("Memory: {}:", entry.path.display()));
        }

        parts.push(format!("{header}\n\n{}", entry.content));
    }

    let content = parts.join("\n\n---\n\n");
    Ok(Some(SystemReminder::text(
        AttachmentType::RelevantMemories,
        content,
    )))
}

/// Compute a simple keyword-based relevance score.
///
/// Checks how many words from the user prompt appear in the memory
/// entry's description, type, and filename. Uses word-boundary matching
/// to avoid false positives (e.g., "go" should not match "going").
fn compute_relevance_score(
    entry: &cocode_auto_memory::AutoMemoryEntry,
    prompt_lower: &str,
    min_keyword_len: usize,
) -> i32 {
    let mut score = 0;

    // Score based on description match (+2 per keyword)
    if let Some(desc) = entry.description() {
        let desc_lower = desc.to_lowercase();
        for word in prompt_lower.split_whitespace() {
            if word.len() >= min_keyword_len && contains_word(&desc_lower, word) {
                score += 2;
            }
        }
    }

    // Boost score for matching memory type keywords (+1 per keyword)
    if let Some(mem_type) = entry.memory_type() {
        let type_lower = mem_type.to_lowercase();
        for word in prompt_lower.split_whitespace() {
            if word.len() >= min_keyword_len && contains_word(&type_lower, word) {
                score += 1;
            }
        }
    }

    // Score based on filename match (+1 per keyword)
    if let Some(filename) = entry.path.file_stem() {
        let filename_lower = filename.to_string_lossy().to_lowercase();
        for word in prompt_lower.split_whitespace() {
            if word.len() >= min_keyword_len && contains_word(&filename_lower, word) {
                score += 1;
            }
        }
    }

    score
}

/// Check if `haystack` contains `needle` as a whole word.
///
/// Splits the haystack on non-alphanumeric boundaries and checks for
/// an exact token match. This avoids false positives like "go" matching
/// "going" or "google".
fn contains_word(haystack: &str, needle: &str) -> bool {
    tokenize(haystack).any(|token| token == needle)
}

/// Iterate over alphanumeric tokens in a string.
fn tokenize(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
}

/// Extract filenames referenced in the MEMORY.md index.
///
/// MEMORY.md is an index containing links like `[topic](topic_file.md)`.
/// Files listed there are already injected by `AutoMemoryPromptGenerator`,
/// so the relevant memories search should skip them to avoid duplication.
async fn extract_index_filenames(state: &cocode_auto_memory::AutoMemoryState) -> HashSet<String> {
    let index = match state.index().await {
        Some(idx) => idx,
        None => return HashSet::new(),
    };

    // Extract .md filenames from markdown links and bare references.
    // Matches patterns like: `(filename.md)`, `[...](filename.md)`, bare `filename.md`
    index
        .raw_content
        .split(|c: char| c == '(' || c == ')' || c == '[' || c == ']' || c.is_whitespace())
        .filter(|s| s.ends_with(".md") && !s.is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

#[cfg(test)]
#[path = "relevant_memories.test.rs"]
mod tests;
