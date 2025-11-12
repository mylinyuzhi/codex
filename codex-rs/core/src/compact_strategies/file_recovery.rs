use crate::compact_strategy::CompactContext;
use crate::compact_strategy::CompactStrategy;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use tracing::warn;

const FILE_RECOVERY_PROMPT: &str = include_str!("../../templates/compact/file_recovery.md");

// Token budgets for file recovery
const MAX_FILES: usize = 5;
const MAX_TOKENS_PER_FILE: usize = 10_000;
const MAX_TOTAL_FILE_TOKENS: usize = 50_000;

/// File recovery strategy inspired by Kode-cli's auto-compact
///
/// This strategy:
/// - Uses a structured 8-section summary prompt
/// - Preserves recent user messages
/// - **Automatically recovers recently accessed files**
/// - Reads files from filesystem (current state, not historical output)
pub struct FileRecoveryStrategy;

impl FileRecoveryStrategy {
    pub fn new() -> Self {
        Self
    }

    /// Extract all read_file paths from conversation history
    ///
    /// Parses FunctionCall items to extract file_path arguments.
    /// Returns paths in reverse chronological order (most recent first).
    fn extract_read_files(&self, history: &[ResponseItem]) -> Vec<PathBuf> {
        let mut file_paths = Vec::new();
        let mut seen = HashSet::new();

        // Iterate in reverse to prioritize recent files
        for item in history.iter().rev() {
            if let ResponseItem::FunctionCall {
                name, arguments, ..
            } = item
                && name == "read_file"
                && let Ok(args) = self.parse_read_args(arguments)
            {
                let path = PathBuf::from(args.file_path);
                if seen.insert(path.clone()) && self.is_valid_for_recovery(&path) {
                    file_paths.push(path);
                    if file_paths.len() >= MAX_FILES {
                        break;
                    }
                }
            }
        }

        file_paths
    }

    /// Parse read_file function arguments
    fn parse_read_args(&self, arguments: &str) -> Result<ReadFileArgs, serde_json::Error> {
        serde_json::from_str(arguments)
    }

    /// Filter rules for file recovery
    ///
    /// Excludes:
    /// - node_modules, .git, dist, build (common build artifacts)
    /// - .cache directories
    /// - Files outside the workspace
    fn is_valid_for_recovery(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        !path_str.contains("node_modules")
            && !path_str.contains(".git/")
            && !path_str.contains("/dist/")
            && !path_str.contains("/build/")
            && !path_str.contains("/.cache/")
            && !path_str.starts_with("/tmp")
    }

    /// Read file and truncate if needed
    ///
    /// Returns None if file cannot be read.
    /// Truncates files exceeding MAX_TOKENS_PER_FILE.
    fn read_and_truncate(&self, path: &Path) -> Option<FileRecoveryContent> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "Failed to read file for recovery: {}: {}",
                    path.display(),
                    e
                );
                return None;
            }
        };

        // Estimate tokens (rough: 0.25 tokens per character)
        let estimated_tokens = (content.len() as f64 * 0.25) as usize;

        let (final_content, truncated) = if estimated_tokens > MAX_TOKENS_PER_FILE {
            let max_chars = (MAX_TOKENS_PER_FILE as f64 / 0.25) as usize;
            (content.chars().take(max_chars).collect(), true)
        } else {
            (content, false)
        };

        Some(FileRecoveryContent {
            path: path.to_path_buf(),
            content: final_content,
            tokens: estimated_tokens.min(MAX_TOKENS_PER_FILE),
            truncated,
        })
    }
}

impl CompactStrategy for FileRecoveryStrategy {
    fn name(&self) -> &str {
        "file-recovery"
    }

    fn generate_prompt(&self) -> &str {
        FILE_RECOVERY_PROMPT
    }

    fn build_compacted_history(
        &self,
        mut history: Vec<ResponseItem>,
        user_messages: &[String],
        summary_text: &str,
        context: &CompactContext,
    ) -> Vec<ResponseItem> {
        // 1. Add user messages (same as simple strategy)
        //    Limit total bytes to avoid bloating context
        const MAX_USER_MESSAGE_BYTES: usize = 20_000 * 4; // ~20k tokens
        let mut remaining_bytes = MAX_USER_MESSAGE_BYTES;

        for message in user_messages.iter().rev() {
            if remaining_bytes == 0 {
                break;
            }
            let message_to_add = if message.len() <= remaining_bytes {
                remaining_bytes = remaining_bytes.saturating_sub(message.len());
                message.clone()
            } else {
                // Truncate if too long
                let truncated = message.chars().take(remaining_bytes).collect::<String>();
                remaining_bytes = 0;
                format!(
                    "{}\n\n[... {} tokens truncated ...]",
                    truncated,
                    (message.len() - truncated.len()) / 4
                )
            };

            history.push(ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: message_to_add,
                }],
            });
        }

        // 2. Add summary message
        let summary = if summary_text.is_empty() {
            "(no summary available)".to_string()
        } else {
            summary_text.to_string()
        };

        history.push(ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: summary }],
        });

        // 3. **Key extension: Recover files**
        let file_paths = self.extract_read_files(&context.history);
        let mut total_tokens = 0;

        for path in file_paths {
            if total_tokens >= MAX_TOTAL_FILE_TOKENS {
                break;
            }

            if let Some(file) = self.read_and_truncate(&path) {
                total_tokens += file.tokens;

                let recovery_message = format!(
                    "**Recovered File: {}**\n\n```\n{}\n```\n\n*Automatically recovered ({} tokens){}*",
                    file.path.display(),
                    file.content,
                    file.tokens,
                    if file.truncated { " [truncated]" } else { "" }
                );

                history.push(ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: recovery_message,
                    }],
                });
            }
        }

        history
    }
}

/// Arguments for read_file function
#[derive(Deserialize)]
struct ReadFileArgs {
    file_path: String,
}

/// Recovered file content with metadata
struct FileRecoveryContent {
    path: PathBuf,
    content: String,
    tokens: usize,
    truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_read_args() {
        let strategy = FileRecoveryStrategy::new();
        let json = r#"{"file_path":"/path/to/file.rs","offset":1,"limit":2000}"#;

        let args = strategy.parse_read_args(json).unwrap();
        assert_eq!(args.file_path, "/path/to/file.rs");
    }

    #[test]
    fn test_is_valid_for_recovery() {
        let strategy = FileRecoveryStrategy::new();

        assert!(strategy.is_valid_for_recovery(Path::new("/workspace/src/main.rs")));
        assert!(strategy.is_valid_for_recovery(Path::new("relative/path/file.rs")));

        assert!(!strategy.is_valid_for_recovery(Path::new("/workspace/node_modules/pkg/index.js")));
        assert!(!strategy.is_valid_for_recovery(Path::new("/workspace/.git/config")));
        assert!(!strategy.is_valid_for_recovery(Path::new("/workspace/dist/bundle.js")));
        assert!(!strategy.is_valid_for_recovery(Path::new("/tmp/tempfile")));
    }

    #[test]
    fn test_extract_read_files() {
        let strategy = FileRecoveryStrategy::new();

        let history = vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"file_path":"/workspace/src/main.rs"}"#.to_string(),
                call_id: "call_1".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"file_path":"/workspace/src/lib.rs"}"#.to_string(),
                call_id: "call_2".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "bash".to_string(),
                arguments: r#"{"command":"ls"}"#.to_string(),
                call_id: "call_3".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"file_path":"/workspace/node_modules/foo/bar.js"}"#.to_string(),
                call_id: "call_4".to_string(),
            },
        ];

        let files = strategy.extract_read_files(&history);

        assert_eq!(files.len(), 2);
        // Most recent first
        assert_eq!(files[0], PathBuf::from("/workspace/src/lib.rs"));
        assert_eq!(files[1], PathBuf::from("/workspace/src/main.rs"));
    }

    #[test]
    fn test_extract_read_files_deduplication() {
        let strategy = FileRecoveryStrategy::new();

        let history = vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"file_path":"/workspace/src/main.rs"}"#.to_string(),
                call_id: "call_1".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"file_path":"/workspace/src/main.rs"}"#.to_string(),
                call_id: "call_2".to_string(),
            },
        ];

        let files = strategy.extract_read_files(&history);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0], PathBuf::from("/workspace/src/main.rs"));
    }
}
