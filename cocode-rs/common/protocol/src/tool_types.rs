//! Tool-related types for the agent system.
//!
//! These types define tool execution characteristics and results.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use strum::Display;
use strum::IntoStaticStr;

use crate::event_types::ToolResultContent;

/// Kind of file read operation.
///
/// This enum distinguishes between different types of file reads for
/// proper already-read detection and state management.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileReadKind {
    /// Complete file read with full content.
    FullContent,
    /// Partial read with offset/limit (e.g., Read with line range).
    PartialContent,
    /// Metadata-only read (e.g., Glob/Grep path discovery).
    /// These should NOT be considered "already read" for @mention purposes.
    #[default]
    MetadataOnly,
}

impl FileReadKind {
    /// Check if this is a full content read.
    pub fn is_full(&self) -> bool {
        matches!(self, FileReadKind::FullContent)
    }

    /// Check if this is a partial read.
    pub fn is_partial(&self) -> bool {
        matches!(self, FileReadKind::PartialContent)
    }

    /// Check if this is metadata-only (path discovery).
    pub fn is_metadata_only(&self) -> bool {
        matches!(self, FileReadKind::MetadataOnly)
    }
}

/// Concurrency safety level for a tool.
///
/// Determines whether a tool can be executed concurrently with other tools.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ConcurrencySafety {
    /// Tool is safe to run concurrently with other tools.
    #[default]
    Safe,
    /// Tool must run exclusively (cannot run with other tools).
    Unsafe,
}

impl ConcurrencySafety {
    /// Check if concurrent execution is safe.
    pub fn is_safe(&self) -> bool {
        matches!(self, ConcurrencySafety::Safe)
    }

    /// Get the safety level as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// Image data for tool results (base64-encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    /// Base64-encoded image bytes.
    pub data: String,
    /// MIME type (e.g., "image/png").
    pub media_type: String,
}

/// Output from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolOutput {
    /// The content of the output.
    pub content: ToolResultContent,
    /// Whether this output represents an error.
    #[serde(default)]
    pub is_error: bool,
    /// Context modifiers to apply after this tool execution.
    #[serde(default)]
    pub modifiers: Vec<ContextModifier>,
    /// Images to include in the tool result (e.g., from AskUserQuestion).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageData>,
}

impl ToolOutput {
    /// Create a successful text output.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
            modifiers: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Create an error output.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(message.into()),
            is_error: true,
            modifiers: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Create a structured output.
    pub fn structured(value: Value) -> Self {
        Self {
            content: ToolResultContent::Structured(value),
            is_error: false,
            modifiers: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Add a context modifier.
    pub fn with_modifier(mut self, modifier: ContextModifier) -> Self {
        self.modifiers.push(modifier);
        self
    }

    /// Add multiple context modifiers.
    pub fn with_modifiers(mut self, modifiers: impl IntoIterator<Item = ContextModifier>) -> Self {
        self.modifiers.extend(modifiers);
        self
    }

    /// Truncate text content to at most `max_chars`, preserving start and end.
    pub fn truncate_to(&mut self, max_chars: usize) {
        if let ToolResultContent::Text(ref text) = self.content
            && text.len() > max_chars
        {
            let half = max_chars / 2;
            let start_end = text.floor_char_boundary(half);
            let suffix_start = text.ceil_char_boundary(text.len() - half);
            let start = &text[..start_end];
            let end = &text[suffix_start..];
            let omitted = text.len() - start_end - (text.len() - suffix_start);
            self.content = ToolResultContent::Text(format!(
                "{start}\n\n... (output truncated, {omitted} characters omitted) ...\n\n{end}"
            ));
        }
    }
}

/// A modifier that changes the conversation context after tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextModifier {
    /// A file was read, record its content.
    FileRead {
        /// Path to the file.
        path: PathBuf,
        /// Content that was read.
        content: String,
        /// File modification time at read time (Unix milliseconds).
        /// Used for change detection and already-read checks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_mtime_ms: Option<i64>,
        /// Line offset of the read (1-based, None if from start).
        /// Present for partial reads with line range.
        /// Uses i64 for large file support (>2 billion lines).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<i64>,
        /// Line limit of the read.
        /// Present for partial reads with line range.
        /// Uses i64 for large file support (>2 billion lines).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<i64>,
        /// Kind of read operation.
        /// Determines if this file should be considered "already read" for @mention purposes.
        #[serde(default)]
        read_kind: FileReadKind,
    },
    /// A permission was granted for future operations.
    PermissionGranted {
        /// Tool that received permission.
        tool: String,
        /// Pattern for allowed operations.
        pattern: String,
    },
    /// A skill restricts which tools can be used.
    SkillAllowedTools {
        /// The skill name that set the restriction.
        skill_name: String,
        /// Tools allowed by the skill. Only these tools (plus "Skill" itself)
        /// should be executable while the skill is active.
        allowed_tools: Vec<String>,
    },
    /// The task list was updated by a TodoWrite tool call.
    TodosUpdated {
        /// The full task list (as the raw JSON array from TodoWrite input).
        todos: Value,
    },
    /// Structured task state was updated (TaskCreate/TaskUpdate).
    StructuredTasksUpdated {
        /// The full structured tasks map (id → task object).
        tasks: Value,
    },
    /// Cron job state was updated (CronCreate/CronDelete).
    CronJobsUpdated {
        /// The full cron jobs map (id → job object).
        jobs: Value,
    },
    /// Team state was updated (TeamCreate/TeamDelete).
    TeamsUpdated {
        /// The full teams map (name → team object).
        teams: Value,
    },
    /// Delegate mode was toggled for the main agent.
    DelegateModeChanged {
        /// Whether delegate mode is now active.
        active: bool,
    },
    /// A teammate joined a team.
    TeammateJoined {
        /// The team name.
        team_name: String,
        /// The joining agent ID.
        agent_id: String,
    },
    /// A teammate left a team.
    TeammateLeft {
        /// The team name.
        team_name: String,
        /// The leaving agent ID.
        agent_id: String,
    },
    /// A skill requests a model override for inline execution.
    ModelOverride {
        /// The model slug to switch to (e.g., "sonnet", "opus", "haiku").
        model: String,
        /// The skill that requested the override.
        skill_name: String,
    },
    /// Signal that deferred MCP tools should be restored into the registry.
    /// Contains the qualified names of tools to restore.
    RestoreDeferredMcpTools {
        /// Qualified names of MCP tools to restore (e.g., `mcp__server__tool`).
        names: Vec<String>,
    },
    /// A file was modified — notify LSP servers for re-analysis.
    ///
    /// Emitted by Write, Edit, ApplyPatch, and NotebookEdit tools.
    /// Handled centrally in `apply_modifiers()` to sync with LSP servers.
    FileModified {
        /// Absolute path to the modified file.
        path: PathBuf,
        /// New file content after modification.
        content: String,
    },
}

impl ContextModifier {
    /// Create a FileRead modifier for a complete file read.
    pub fn file_read(path: PathBuf, content: String, file_mtime_ms: Option<i64>) -> Self {
        ContextModifier::FileRead {
            path,
            content,
            file_mtime_ms,
            offset: None,
            limit: None,
            read_kind: FileReadKind::FullContent,
        }
    }

    /// Create a FileRead modifier for a partial read with line range.
    pub fn file_read_partial(
        path: PathBuf,
        content: String,
        file_mtime_ms: Option<i64>,
        offset: i64,
        limit: i64,
    ) -> Self {
        ContextModifier::FileRead {
            path,
            content,
            file_mtime_ms,
            offset: Some(offset),
            limit: Some(limit),
            read_kind: FileReadKind::PartialContent,
        }
    }

    /// Create a FileRead modifier for metadata-only (path discovery).
    pub fn file_read_metadata(path: PathBuf) -> Self {
        ContextModifier::FileRead {
            path,
            content: String::new(),
            file_mtime_ms: None,
            offset: None,
            limit: None,
            read_kind: FileReadKind::MetadataOnly,
        }
    }

    /// Check if this is a FileRead modifier with full content.
    pub fn is_full_file_read(&self) -> bool {
        matches!(
            self,
            ContextModifier::FileRead {
                read_kind: FileReadKind::FullContent,
                ..
            }
        )
    }
}

/// Result of validating tool input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ValidationResult {
    /// Input is valid.
    Valid,
    /// Input is invalid.
    Invalid {
        /// List of validation errors.
        errors: Vec<ValidationError>,
    },
}

impl ValidationResult {
    /// Check if validation passed.
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    /// Create a valid result.
    pub fn valid() -> Self {
        ValidationResult::Valid
    }

    /// Create an invalid result with errors.
    pub fn invalid(errors: impl IntoIterator<Item = ValidationError>) -> Self {
        ValidationResult::Invalid {
            errors: errors.into_iter().collect(),
        }
    }

    /// Create an invalid result with a single error.
    pub fn error(message: impl Into<String>) -> Self {
        ValidationResult::Invalid {
            errors: vec![ValidationError {
                message: message.into(),
                path: None,
            }],
        }
    }
}

/// A validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    /// Error message.
    pub message: String,
    /// JSON path to the invalid field (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ValidationError {
    /// Create a new validation error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
        }
    }

    /// Create a validation error with a path.
    pub fn with_path(message: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: Some(path.into()),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{}: {}", path, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

#[cfg(test)]
#[path = "tool_types.test.rs"]
mod tests;
