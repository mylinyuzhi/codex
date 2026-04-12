//! Session memory persistence: save/load per-session conversation insights.
//!
//! TS: services/SessionMemory/ (1K LOC) — session memory extraction, storage,
//! and retrieval across session resume.
//!
//! Each session accumulates insights (decisions, corrections, context) that
//! are persisted alongside the session transcript. On resume, these insights
//! are loaded back to provide continuity. They can also be merged into the
//! project-level MEMORY.md for cross-session benefit.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::MemoryEntry;
use crate::MemoryEntryType;
use crate::MemoryManager;
use crate::parse_frontmatter;

/// Session memory: tracks conversation insights for a single session.
///
/// Stored as JSON in the session directory alongside the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemory {
    /// Session identifier.
    pub session_id: String,
    /// Extracted insights from the conversation.
    pub insights: Vec<SessionInsight>,
    /// UUID of the last message that was summarized into this memory.
    /// Used by session memory compaction to know the boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_summarized_message_id: Option<String>,
    /// Timestamp of last update (ISO 8601).
    #[serde(default)]
    pub last_updated: String,
}

/// A single insight extracted from the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInsight {
    /// Category of insight.
    pub category: InsightCategory,
    /// Brief title.
    pub title: String,
    /// Full content of the insight.
    pub content: String,
    /// Confidence score (0.0 to 1.0) from extraction.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

fn default_confidence() -> f64 {
    1.0
}

/// Categories of session insights.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightCategory {
    /// Key decisions made during the session.
    Decision,
    /// Corrections or feedback from the user.
    Correction,
    /// Project context discovered during work.
    Context,
    /// Preferences expressed by the user.
    Preference,
    /// Technical discoveries (API behavior, gotchas).
    Discovery,
}

impl SessionMemory {
    /// Create a new empty session memory.
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            insights: Vec::new(),
            last_summarized_message_id: None,
            last_updated: String::new(),
        }
    }

    /// Add an insight to this session memory.
    pub fn add_insight(&mut self, insight: SessionInsight) {
        self.insights.push(insight);
    }

    /// Check if this session memory has any meaningful content.
    pub fn is_empty(&self) -> bool {
        self.insights.is_empty()
    }

    /// Format as a markdown string suitable for use as a compact summary.
    pub fn to_markdown(&self) -> String {
        if self.insights.is_empty() {
            return String::new();
        }

        let mut sections: std::collections::HashMap<&str, Vec<&SessionInsight>> =
            std::collections::HashMap::new();

        for insight in &self.insights {
            let key = match insight.category {
                InsightCategory::Decision => "Key Decisions",
                InsightCategory::Correction => "Corrections & Feedback",
                InsightCategory::Context => "Project Context",
                InsightCategory::Preference => "User Preferences",
                InsightCategory::Discovery => "Technical Discoveries",
            };
            sections.entry(key).or_default().push(insight);
        }

        let section_order = [
            "Key Decisions",
            "Corrections & Feedback",
            "Project Context",
            "User Preferences",
            "Technical Discoveries",
        ];

        let mut md = String::with_capacity(1024);
        for section_name in &section_order {
            if let Some(insights) = sections.get(section_name) {
                md.push_str(&format!("## {section_name}\n\n"));
                for insight in insights {
                    md.push_str(&format!("### {}\n", insight.title));
                    md.push_str(&insight.content);
                    md.push_str("\n\n");
                }
            }
        }

        md
    }
}

/// File name for session memory within a session directory.
const SESSION_MEMORY_FILENAME: &str = "session_memory.json";

/// Save session memory to the session directory.
///
/// Creates the directory if it does not exist. Overwrites any existing file.
pub fn save_session_memory(session_dir: &Path, memory: &SessionMemory) -> anyhow::Result<()> {
    std::fs::create_dir_all(session_dir)?;
    let path = session_dir.join(SESSION_MEMORY_FILENAME);
    let json = serde_json::to_string_pretty(memory)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Load session memory from the session directory.
///
/// Returns `None` if the file does not exist (new session).
/// Returns an error if the file exists but cannot be parsed.
pub fn load_session_memory(session_dir: &Path) -> anyhow::Result<Option<SessionMemory>> {
    let path = session_dir.join(SESSION_MEMORY_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let memory: SessionMemory = serde_json::from_str(&content)?;
    Ok(Some(memory))
}

/// Merge session insights into the project-level memory.
///
/// For each session insight, this function:
/// 1. Checks if a similar memory entry already exists (by title match)
/// 2. If yes, appends the new content (avoiding duplicates)
/// 3. If no, creates a new memory entry
///
/// Returns the list of memory entries that were created or updated.
pub fn merge_with_project_memory(
    session_memory: &SessionMemory,
    memory_manager: &MemoryManager,
) -> anyhow::Result<Vec<PathBuf>> {
    if session_memory.is_empty() {
        return Ok(Vec::new());
    }

    let existing_entries = memory_manager.list_entries()?;
    let mut affected_paths = Vec::new();

    for insight in &session_memory.insights {
        let memory_type = match insight.category {
            InsightCategory::Decision | InsightCategory::Context => MemoryEntryType::Project,
            InsightCategory::Correction | InsightCategory::Preference => MemoryEntryType::Feedback,
            InsightCategory::Discovery => MemoryEntryType::Reference,
        };

        // Check for existing entry with matching title
        let existing = existing_entries
            .iter()
            .find(|e| e.name == insight.title || e.description.contains(&insight.title));

        if let Some(existing_entry) = existing {
            // Merge: append content if not already present
            let new_content = if existing_entry.content.contains(&insight.content) {
                existing_entry.content.clone()
            } else {
                format!("{}\n\n{}", existing_entry.content.trim(), insight.content)
            };

            let updated = MemoryEntry {
                name: existing_entry.name.clone(),
                description: existing_entry.description.clone(),
                memory_type: existing_entry.memory_type.clone(),
                content: new_content,
                file_path: existing_entry.file_path.clone(),
            };
            memory_manager.save_entry(&updated)?;
            affected_paths.push(existing_entry.file_path.clone());
        } else {
            // Create new entry
            let safe_name = sanitize_filename(&insight.title);
            let file_path = PathBuf::from(format!("{safe_name}.md"));
            let entry = MemoryEntry {
                name: insight.title.clone(),
                description: insight.content.lines().next().unwrap_or("").to_string(),
                memory_type,
                content: insight.content.clone(),
                file_path: file_path.clone(),
            };
            memory_manager.save_entry(&entry)?;
            affected_paths.push(memory_manager.memory_dir.join(&file_path));
        }
    }

    // Regenerate the index after all changes
    if !affected_paths.is_empty() {
        memory_manager.update_index()?;
    }

    Ok(affected_paths)
}

/// Convert a title string to a safe file name.
fn sanitize_filename(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Parse session memory content from a markdown string.
///
/// This is the inverse of `SessionMemory::to_markdown()` — it reads the
/// markdown sections and reconstructs insights. Used when loading session
/// memory from legacy markdown-format files.
pub fn parse_session_memory_markdown(session_id: &str, content: &str) -> SessionMemory {
    let (_, body) = parse_frontmatter(content);
    let mut insights = Vec::new();
    let mut current_category: Option<InsightCategory> = None;
    let mut current_title: Option<String> = None;
    let mut current_content = String::new();

    for line in body.lines() {
        if let Some(section) = line.strip_prefix("## ") {
            // Flush previous insight
            flush_insight(
                &mut insights,
                &current_category,
                &current_title,
                &current_content,
            );

            current_category = match section.trim() {
                "Key Decisions" => Some(InsightCategory::Decision),
                "Corrections & Feedback" => Some(InsightCategory::Correction),
                "Project Context" => Some(InsightCategory::Context),
                "User Preferences" => Some(InsightCategory::Preference),
                "Technical Discoveries" => Some(InsightCategory::Discovery),
                _ => None,
            };
            current_title = None;
            current_content.clear();
        } else if let Some(title) = line.strip_prefix("### ") {
            // Flush previous insight within this section
            flush_insight(
                &mut insights,
                &current_category,
                &current_title,
                &current_content,
            );

            current_title = Some(title.trim().to_string());
            current_content.clear();
        } else if current_title.is_some()
            && (!current_content.is_empty() || !line.trim().is_empty())
        {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Flush last insight
    flush_insight(
        &mut insights,
        &current_category,
        &current_title,
        &current_content,
    );

    SessionMemory {
        session_id: session_id.to_string(),
        insights,
        last_summarized_message_id: None,
        last_updated: String::new(),
    }
}

fn flush_insight(
    insights: &mut Vec<SessionInsight>,
    category: &Option<InsightCategory>,
    title: &Option<String>,
    content: &str,
) {
    if let (Some(cat), Some(title)) = (category, title) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            insights.push(SessionInsight {
                category: cat.clone(),
                title: title.clone(),
                content: trimmed.to_string(),
                confidence: 1.0,
            });
        }
    }
}

#[cfg(test)]
#[path = "session_memory.test.rs"]
mod tests;
