//! CLAUDE.md management, auto-extraction, session memory.
//!
//! TS: memdir/ + services/extractMemories/ + services/SessionMemory/ + services/autoDream/

pub mod auto_dream;
pub mod classify;
pub mod config;
pub mod hooks;
pub mod kairos;
pub mod memdir;
pub mod permissions;
pub mod prefetch;
pub mod prompt;
pub mod scan;
pub mod security;
pub mod session_memory;
pub mod staleness;
pub mod team_paths;
pub mod team_prompts;
pub mod team_sync;
pub mod telemetry;

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

/// Memory entry with frontmatter metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryEntryType,
    pub content: String,
    pub file_path: PathBuf,
}

/// Type of memory entry (from frontmatter).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEntryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryEntryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }
}

/// Frontmatter metadata parsed from a memory file's YAML header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFrontmatter {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryEntryType,
}

/// Memory index (MEMORY.md) — pointers to memory files.
#[derive(Debug, Clone, Default)]
pub struct MemoryIndex {
    pub entries: Vec<MemoryIndexEntry>,
}

/// A single entry in MEMORY.md.
#[derive(Debug, Clone)]
pub struct MemoryIndexEntry {
    pub title: String,
    pub file: String,
    pub description: String,
}

/// Parse frontmatter delimited by `---` from the beginning of content.
/// Returns the parsed frontmatter (if present) and the remaining body.
pub fn parse_frontmatter(content: &str) -> (Option<MemoryFrontmatter>, &str) {
    let Some(stripped) = content.strip_prefix("---") else {
        return (None, content);
    };
    let Some(end) = stripped.find("---") else {
        return (None, content);
    };

    let frontmatter_str = &stripped[..end];
    let body = stripped[end + 3..].trim_start_matches(['\r', '\n']);

    let mut name = String::new();
    let mut description = String::new();
    let mut memory_type = MemoryEntryType::User;

    for line in frontmatter_str.lines() {
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("type:") {
            memory_type = match val.trim() {
                "feedback" => MemoryEntryType::Feedback,
                "project" => MemoryEntryType::Project,
                "reference" => MemoryEntryType::Reference,
                _ => MemoryEntryType::User,
            };
        }
    }

    let fm = MemoryFrontmatter {
        name,
        description,
        memory_type,
    };
    (Some(fm), body)
}

/// Format a memory entry as a markdown file with YAML frontmatter.
pub fn format_entry_as_markdown(entry: &MemoryEntry) -> String {
    format!(
        "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}",
        entry.name,
        entry.description,
        entry.memory_type.as_str(),
        entry.content,
    )
}

/// Memory manager — CRUD operations on memory files.
pub struct MemoryManager {
    pub memory_dir: PathBuf,
}

impl MemoryManager {
    pub fn new(project_dir: &Path) -> Self {
        Self {
            memory_dir: project_dir.join(".claude/memory"),
        }
    }

    /// Load the memory index (MEMORY.md).
    pub fn load_index(&self) -> anyhow::Result<MemoryIndex> {
        let index_path = self.memory_dir.join("MEMORY.md");
        if !index_path.exists() {
            return Ok(MemoryIndex::default());
        }
        let content = std::fs::read_to_string(&index_path)?;
        Ok(parse_memory_index(&content))
    }

    /// List all memory files.
    pub fn list_entries(&self) -> anyhow::Result<Vec<MemoryEntry>> {
        let mut entries = Vec::new();
        if !self.memory_dir.exists() {
            return Ok(entries);
        }
        for entry in std::fs::read_dir(&self.memory_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md")
                && path.file_name().is_some_and(|n| n != "MEMORY.md")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Some(mem) = parse_memory_entry(&path, &content)
            {
                entries.push(mem);
            }
        }
        Ok(entries)
    }

    /// Save a memory entry to disk as a markdown file with frontmatter.
    /// Creates the memory directory if it does not exist.
    pub fn save_entry(&self, entry: &MemoryEntry) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.memory_dir)?;
        let file_path = if entry.file_path.is_absolute() {
            entry.file_path.clone()
        } else {
            self.memory_dir.join(&entry.file_path)
        };
        let markdown = format_entry_as_markdown(entry);
        std::fs::write(&file_path, markdown)?;
        Ok(())
    }

    /// Delete a memory entry by file name (e.g. "my_entry.md").
    /// Removes the file from disk. Returns an error if the file does not exist.
    pub fn delete_entry(&self, name: &str) -> anyhow::Result<()> {
        let file_path = self.memory_dir.join(name);
        if !file_path.exists() {
            anyhow::bail!("memory entry not found: {name}");
        }
        std::fs::remove_file(&file_path)?;
        Ok(())
    }

    /// Regenerate MEMORY.md from all .md files in the memory directory.
    pub fn update_index(&self) -> anyhow::Result<()> {
        let entries = self.list_entries()?;
        let mut lines = vec!["# Memory Index".to_string(), String::new()];
        for entry in &entries {
            let file_name = entry
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.md");
            lines.push(format!(
                "- [{}]({file_name}) — {}",
                entry.name, entry.description,
            ));
        }
        // Ensure trailing newline
        lines.push(String::new());
        let index_content = lines.join("\n");
        std::fs::create_dir_all(&self.memory_dir)?;
        std::fs::write(self.memory_dir.join("MEMORY.md"), index_content)?;
        Ok(())
    }
}

fn parse_memory_index(content: &str) -> MemoryIndex {
    let entries = content
        .lines()
        .filter(|l| l.starts_with("- ["))
        .filter_map(|line| {
            let title_start = line.find('[')? + 1;
            let title_end = line.find(']')?;
            let file_start = line.find('(')? + 1;
            let file_end = line.find(')')?;
            let desc = line
                .get(file_end + 1..)?
                .trim()
                .trim_start_matches("— ")
                .to_string();
            Some(MemoryIndexEntry {
                title: line[title_start..title_end].to_string(),
                file: line[file_start..file_end].to_string(),
                description: desc,
            })
        })
        .collect();
    MemoryIndex { entries }
}

fn parse_memory_entry(path: &Path, content: &str) -> Option<MemoryEntry> {
    let (frontmatter, body) = parse_frontmatter(content);
    let fm = frontmatter?;
    Some(MemoryEntry {
        name: fm.name,
        description: fm.description,
        memory_type: fm.memory_type,
        content: body.to_string(),
        file_path: path.to_path_buf(),
    })
}

/// Two-phase memory extraction from conversations.
///
/// TS: services/extractMemories/ (769 LOC) + services/SessionMemory/ (1K LOC)
///
/// Phase 1: Fast extraction — uses a fast model to identify potential memories
/// from the conversation. Low reasoning effort.
///
/// Phase 2: Consolidation — uses a capable model with thinking to merge
/// new memories with existing ones, resolving conflicts and deduplication.
pub mod extraction {
    use super::*;

    /// Extracted memory candidate from Phase 1.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MemoryCandidate {
        pub name: String,
        pub description: String,
        pub memory_type: MemoryEntryType,
        pub content: String,
        /// Confidence score (0.0 to 1.0).
        pub confidence: f64,
    }

    /// Configuration for memory extraction.
    #[derive(Debug, Clone)]
    pub struct ExtractionConfig {
        /// Maximum candidates to extract per conversation.
        pub max_candidates: i32,
        /// Minimum confidence to keep a candidate.
        pub min_confidence: f64,
    }

    impl Default for ExtractionConfig {
        fn default() -> Self {
            Self {
                max_candidates: 10,
                min_confidence: 0.5,
            }
        }
    }

    /// Phase 1: Extract memory candidates from a conversation.
    ///
    /// The `extract_fn` callback calls a fast LLM to identify memories.
    /// Input: system prompt + conversation text → JSON array of candidates.
    pub async fn extract_memories<F, Fut>(
        conversation_text: &str,
        config: &ExtractionConfig,
        extract_fn: F,
    ) -> anyhow::Result<Vec<MemoryCandidate>>
    where
        F: FnOnce(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let system_prompt = build_extraction_prompt();
        let user_prompt =
            format!("Extract memories from this conversation:\n\n{conversation_text}");

        let response = extract_fn(system_prompt, user_prompt)
            .await
            .map_err(|e| anyhow::anyhow!("memory extraction failed: {e}"))?;

        let candidates = parse_extraction_response(&response);

        Ok(candidates
            .into_iter()
            .filter(|c| c.confidence >= config.min_confidence)
            .take(config.max_candidates as usize)
            .collect())
    }

    /// Phase 2: Consolidate new candidates with existing memories.
    ///
    /// The `consolidate_fn` callback calls a capable LLM with thinking.
    /// Returns the merged set of memory entries to save.
    pub async fn consolidate_memories<F, Fut>(
        candidates: &[MemoryCandidate],
        existing: &[MemoryEntry],
        consolidate_fn: F,
    ) -> anyhow::Result<Vec<MemoryEntry>>
    where
        F: FnOnce(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let system_prompt = build_consolidation_prompt();
        let user_prompt = build_consolidation_user_prompt(candidates, existing);

        let response = consolidate_fn(system_prompt, user_prompt)
            .await
            .map_err(|e| anyhow::anyhow!("memory consolidation failed: {e}"))?;

        Ok(parse_consolidation_response(&response))
    }

    fn build_extraction_prompt() -> String {
        "You are a memory extraction system. Analyze the conversation and identify \
         information worth remembering across sessions.\n\n\
         Extract memories in these categories:\n\
         - user: Information about the user's role, preferences, expertise\n\
         - feedback: Corrections or confirmations the user gave about approach\n\
         - project: Non-obvious project context, decisions, constraints\n\
         - reference: Pointers to external resources\n\n\
         Return a JSON array of objects with: name, description, type, content, confidence.\n\
         Only include items with confidence > 0.5.\n\
         Do NOT include code patterns, architecture, or anything derivable from the codebase."
            .to_string()
    }

    fn build_consolidation_prompt() -> String {
        "You are a memory consolidation system. Merge new memory candidates with \
         existing memories. For each candidate:\n\
         - If it updates an existing memory, merge the content\n\
         - If it's new information, create a new entry\n\
         - If it contradicts an existing memory, prefer the newer information\n\
         - Remove duplicates\n\n\
         Return a JSON array of objects with: name, description, type, content, file_path."
            .to_string()
    }

    fn build_consolidation_user_prompt(
        candidates: &[MemoryCandidate],
        existing: &[MemoryEntry],
    ) -> String {
        let mut prompt = String::from("## Existing Memories\n\n");
        for entry in existing {
            prompt.push_str(&format!(
                "- {} ({}): {}\n  {}\n\n",
                entry.name,
                entry.memory_type.as_str(),
                entry.description,
                entry.content,
            ));
        }

        prompt.push_str("## New Candidates\n\n");
        for candidate in candidates {
            prompt.push_str(&format!(
                "- {} ({}, confidence={:.2}): {}\n  {}\n\n",
                candidate.name,
                candidate.memory_type.as_str(),
                candidate.confidence,
                candidate.description,
                candidate.content,
            ));
        }

        prompt
    }

    fn parse_extraction_response(response: &str) -> Vec<MemoryCandidate> {
        // Try to parse JSON array from response
        let json_str = extract_json_array(response);
        serde_json::from_str::<Vec<MemoryCandidate>>(json_str).unwrap_or_default()
    }

    fn parse_consolidation_response(response: &str) -> Vec<MemoryEntry> {
        let json_str = extract_json_array(response);
        serde_json::from_str::<Vec<MemoryEntry>>(json_str).unwrap_or_default()
    }

    fn extract_json_array(text: &str) -> &str {
        if let Some(start) = text.find('[')
            && let Some(end) = text.rfind(']')
        {
            return &text[start..=end];
        }
        "[]"
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
