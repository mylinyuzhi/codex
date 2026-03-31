//! Relevant memories generator.
//!
//! Searches the auto memory directory for topic files relevant to the
//! current user prompt and injects them as system reminders with
//! staleness information.
//!
//! Two search strategies are supported:
//! - **LLM-based** (primary): sends file metadata to a fast model for
//!   semantic relevance selection.
//! - **Keyword-based** (fallback): scores files by keyword overlap with
//!   the user prompt when no LLM selector is available or the LLM
//!   returns no results.

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::SystemTime;

use async_trait::async_trait;
use cocode_tools_api::ModelCallFn;
use cocode_tools_api::ModelCallInput;
use cocode_tools_api::ModelCallResult;
use tracing::debug;
use tracing::warn;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// A memory file candidate for LLM selection.
#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    /// Filename of the memory file.
    pub filename: String,
    /// Memory type from frontmatter (e.g. "user", "feedback", "project").
    pub memory_type: Option<String>,
    /// One-line description from frontmatter.
    pub description: Option<String>,
    /// ISO-8601 timestamp of last modification.
    pub timestamp_iso: String,
}

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
        match tokio::time::timeout(timeout, search_relevant_memories(state, user_prompt, ctx)).await
        {
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
    ctx: &GeneratorContext<'_>,
) -> Result<Option<SystemReminder>> {
    let config = &state.config;
    let memory_dir = &config.directory;
    let max_files = config.max_relevant_files;
    let max_lines = config.max_lines_per_file;
    let max_files_to_scan = config.max_files_to_scan;
    let max_frontmatter_lines = config.max_frontmatter_lines;
    let min_keyword_length = config.min_keyword_length as usize;
    let staleness_warning_days = config.staleness_warning_days as i64;

    // Collect memory files from the primary directory.
    let mut files = match cocode_auto_memory::list_memory_files(memory_dir) {
        Ok(f) => f,
        Err(e) => {
            warn!(error = %e, "Failed to list memory files");
            return Ok(None);
        }
    };

    // Also search agent-specific memory directories when the
    // user prompt mentions @agent-name.
    let agent_mentions: Vec<String> = crate::parsing::parse_agent_mentions(user_prompt)
        .into_iter()
        .map(|m| m.agent_type)
        .collect();
    for agent_name in &agent_mentions {
        if let Some(agent_dir) = ctx.agent_memory_dirs.get(agent_name) {
            match cocode_auto_memory::list_memory_files(agent_dir) {
                Ok(agent_files) => files.extend(agent_files),
                Err(e) => {
                    warn!(agent = %agent_name, error = %e, "Failed to list agent memory files");
                }
            }
        }
    }

    let files_scanned = files.len();
    if files.is_empty() {
        debug!(files_scanned = 0, "No memory files to search");
        return Ok(None);
    }

    // Deduplicate: skip files already referenced in MEMORY.md (they're
    // already in context via the AutoMemoryPrompt generator).
    let index_referenced = extract_index_filenames(state).await;

    // Load files concurrently
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

    // Collect loaded entries, filtering out index-referenced and already-read files.
    let mut entries: Vec<cocode_auto_memory::AutoMemoryEntry> = Vec::new();
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

        // Skip files already read via Read tool this turn.
        if ctx.read_file_paths.contains(&entry.path) {
            debug!(path = %entry.path.display(), "Skipping memory file already read this turn");
            continue;
        }

        entries.push(entry);
    }

    if entries.is_empty() {
        return Ok(None);
    }

    // Try LLM-based selection first; fall back to keyword scoring.
    let selected_entries = match ctx.model_call_fn.as_ref() {
        Some(model_call_fn) => {
            let selected = select_memories_with_llm(
                user_prompt,
                &entries,
                &ctx.recent_tool_names,
                model_call_fn,
            )
            .await;

            if selected.is_empty() {
                debug!("LLM selection returned no results, falling back to keyword scoring");
                select_memories_by_keywords(&entries, user_prompt, min_keyword_length, max_files)
            } else {
                // Map selected filenames back to entries, preserving LLM order.
                let entry_map: HashMap<&str, &cocode_auto_memory::AutoMemoryEntry> = entries
                    .iter()
                    .filter_map(|e| {
                        e.path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|name| (name, e))
                    })
                    .collect();

                selected
                    .iter()
                    .filter_map(|name| entry_map.get(name.as_str()).copied())
                    .take(max_files as usize)
                    .cloned()
                    .collect()
            }
        }
        None => select_memories_by_keywords(&entries, user_prompt, min_keyword_length, max_files),
    };

    if selected_entries.is_empty() {
        return Ok(None);
    }

    let used_llm = ctx.model_call_fn.is_some();
    debug!(
        files_scanned,
        files_matched = selected_entries.len(),
        used_llm,
        "Relevant memories search complete"
    );

    format_memory_reminder(&selected_entries, staleness_warning_days)
}

/// Maximum tokens for the memory selection response.
const MAX_OUTPUT_TOKENS: u64 = 256;

/// Timeout for the LLM selection call.
const SELECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// JSON response schema for memory selection.
#[derive(serde::Deserialize)]
struct MemorySelectionResponse {
    #[serde(default)]
    selected_memories: Vec<String>,
}

/// Select relevant memories using an LLM for semantic understanding.
///
/// Builds a `ModelCallInput` with file metadata and calls the `ModelCallFn`
/// callback to get the filenames most relevant to the user's query.
/// Returns empty vec on any error (timeout, parse, etc.), triggering
/// keyword fallback.
async fn select_memories_with_llm(
    query: &str,
    entries: &[cocode_auto_memory::AutoMemoryEntry],
    recent_tools: &[String],
    model_call_fn: &ModelCallFn,
) -> Vec<String> {
    let candidates: Vec<MemoryCandidate> = entries
        .iter()
        .filter_map(|entry| {
            let filename = entry.path.file_name()?.to_str()?.to_string();
            let timestamp_iso = entry
                .last_modified
                .map(|time| {
                    let secs = time
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    match chrono::DateTime::from_timestamp(secs, 0) {
                        Some(dt) => dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        None => "unknown".to_string(),
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());
            Some(MemoryCandidate {
                filename,
                memory_type: entry.memory_type().map(str::to_string),
                description: entry.description().map(str::to_string),
                timestamp_iso,
            })
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    // Build the set of valid filenames for filtering the LLM response.
    let valid_names: HashSet<&str> = candidates.iter().map(|c| c.filename.as_str()).collect();

    // Build the LLM request.
    let system_prompt = build_memory_selection_system_prompt();
    let user_prompt = build_memory_selection_user_prompt(query, &candidates, recent_tools);

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "selected_memories": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "required": ["selected_memories"],
        "additionalProperties": false
    });

    let messages = vec![
        cocode_inference::LanguageModelMessage::system(system_prompt),
        cocode_inference::LanguageModelMessage::user_text(&user_prompt),
    ];

    let mut request = cocode_inference::LanguageModelCallOptions::new(messages);
    request.max_output_tokens = Some(MAX_OUTPUT_TOKENS);
    request.response_format = Some(
        cocode_inference::ResponseFormat::json_with_schema(schema).with_name("memory_selection"),
    );

    let input = ModelCallInput { request };

    // Call with timeout.
    let result = tokio::time::timeout(SELECTION_TIMEOUT, (model_call_fn)(input)).await;

    let response = match result {
        Ok(Ok(ModelCallResult { response })) => response,
        Ok(Err(e)) => {
            warn!(error = %e, "LLM memory selection request failed");
            return Vec::new();
        }
        Err(_) => {
            warn!("LLM memory selection timed out after {SELECTION_TIMEOUT:?}");
            return Vec::new();
        }
    };

    // Extract text from the response.
    let text: String = response
        .content
        .iter()
        .filter_map(|part| match part {
            cocode_inference::AssistantContentPart::Text(cocode_inference::TextPart {
                text,
                ..
            }) => Some(text.as_str()),
            _ => None,
        })
        .collect();

    if text.is_empty() {
        warn!("LLM memory selection returned empty response");
        return Vec::new();
    }

    // Parse the JSON response.
    let parsed: MemorySelectionResponse = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, text = %text, "Failed to parse LLM memory selection response");
            return Vec::new();
        }
    };

    debug!(
        selected = ?parsed.selected_memories,
        "LLM memory selection complete"
    );

    // Filter against valid set (LLM may hallucinate filenames).
    parsed
        .selected_memories
        .into_iter()
        .filter(|name| valid_names.contains(name.as_str()))
        .collect()
}

/// Select memories by keyword scoring (fallback path).
fn select_memories_by_keywords(
    entries: &[cocode_auto_memory::AutoMemoryEntry],
    user_prompt: &str,
    min_keyword_length: usize,
    max_files: i32,
) -> Vec<cocode_auto_memory::AutoMemoryEntry> {
    let prompt_lower = user_prompt.to_lowercase();

    let mut scored: Vec<(i32, &cocode_auto_memory::AutoMemoryEntry)> = entries
        .iter()
        .filter_map(|entry| {
            let score = compute_relevance_score(entry, &prompt_lower, min_keyword_length);
            if score > 0 {
                Some((score, entry))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(max_files as usize)
        .map(|(_, entry)| entry.clone())
        .collect()
}

/// Format selected memory entries into a system reminder.
fn format_memory_reminder(
    entries: &[cocode_auto_memory::AutoMemoryEntry],
    staleness_warning_days: i64,
) -> Result<Option<SystemReminder>> {
    let mut parts = Vec::new();
    for entry in entries {
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

/// Build the LLM system prompt for memory selection.
fn build_memory_selection_system_prompt() -> &'static str {
    "You are selecting memories that will be useful to Claude Code as it processes a user's \
     query. You will be given the user's query and a list of available memory files with their \
     filenames and descriptions.\n\n\
     Return a list of filenames for the memories that will clearly be useful to Claude Code as it \
     processes the user's query (up to 5). Only include memories that you are certain will be \
     helpful based on their name and description.\n\
     - If you are unsure if a memory will be useful in processing the user's query, then do not \
     include it in your list. Be selective and discerning.\n\
     - If there are no memories in the list that would clearly be useful, feel free to return an \
     empty list.\n\
     - If a list of recently-used tools is provided, do not select memories that are usage \
     reference or API documentation for those tools (Claude Code is already exercising them). DO \
     still select memories containing warnings, gotchas, or known issues about those tools — \
     active use is exactly when those matter."
}

/// Build the LLM user prompt for memory selection.
fn build_memory_selection_user_prompt(
    query: &str,
    candidates: &[MemoryCandidate],
    recent_tools: &[String],
) -> String {
    let mut prompt = format!("Query: {query}\n\nAvailable memories:\n");
    for c in candidates {
        let mem_type = c.memory_type.as_deref().unwrap_or("unknown");
        let desc = c.description.as_deref().unwrap_or("(no description)");
        prompt.push_str(&format!(
            "- [{mem_type}] {} ({}): {desc}\n",
            c.filename, c.timestamp_iso
        ));
    }

    if !recent_tools.is_empty() {
        prompt.push_str(&format!(
            "\nRecently used tools: {}",
            recent_tools.join(", ")
        ));
    }

    prompt
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
