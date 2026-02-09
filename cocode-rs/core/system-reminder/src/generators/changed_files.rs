//! Changed files generator.
//!
//! Detects and reports files that have been modified since they were last read,
//! including unified diffs showing what changed.

use std::path::Path;

use async_trait::async_trait;
use similar::ChangeTag;
use similar::TextDiff;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Maximum diff lines to include per file (to avoid overwhelming the context).
const MAX_DIFF_LINES_PER_FILE: usize = 50;

/// Maximum total diff content size in characters.
const MAX_TOTAL_DIFF_SIZE: usize = 4000;

/// Generator for detecting changed files.
#[derive(Debug)]
pub struct ChangedFilesGenerator;

impl ChangedFilesGenerator {
    /// Generate a unified diff between old and new content.
    ///
    /// Returns a compact diff format showing only changed lines with context.
    fn generate_diff(old_content: &str, new_content: &str, path: &Path) -> String {
        let diff = TextDiff::from_lines(old_content, new_content);

        let mut result = String::new();
        let mut line_count = 0;

        for change in diff.iter_all_changes() {
            if line_count >= MAX_DIFF_LINES_PER_FILE {
                result.push_str("... (diff truncated)\n");
                break;
            }

            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };

            // Skip some equal lines if we have too many changes to show
            if change.tag() == ChangeTag::Equal && line_count > MAX_DIFF_LINES_PER_FILE / 2 {
                continue;
            }

            result.push_str(sign);
            result.push_str(change.value());
            if change.missing_newline() {
                result.push_str("\n\\ No newline at end of file\n");
            }
            line_count += 1;
        }

        if result.is_empty() {
            format!("(no textual changes detected for {})\n", path.display())
        } else {
            result
        }
    }

    /// Try to read the current content of a file.
    async fn read_current_content(path: &Path) -> Option<String> {
        tokio::fs::read_to_string(path).await.ok()
    }
}

#[async_trait]
impl AttachmentGenerator for ChangedFilesGenerator {
    fn name(&self) -> &str {
        "ChangedFilesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::ChangedFiles
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.changed_files
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - always check for changes
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(tracker) = ctx.file_tracker else {
            return Ok(None);
        };

        let changed = tracker.changed_files();

        if changed.is_empty() {
            return Ok(None);
        }

        // Format the changed files message with diffs
        let mut content =
            String::from("The following files have been modified since you last read them:\n\n");
        let mut total_diff_size = 0;

        for path in &changed {
            let display_path = path.display();
            content.push_str(&format!("### {display_path}\n"));

            // Try to generate diff if we have the old content and can read the new content
            if let Some(old_state) = tracker.get_state(path) {
                // Skip partial reads - can't generate meaningful diff
                if old_state.is_partial() {
                    content.push_str("(partial read - cannot show diff)\n\n");
                    continue;
                }

                if let Some(new_content) = Self::read_current_content(path).await {
                    // Check if we have room for more diff content
                    if total_diff_size < MAX_TOTAL_DIFF_SIZE {
                        let diff = Self::generate_diff(&old_state.content, &new_content, path);
                        let diff_size = diff.len();

                        // Truncate if needed
                        if total_diff_size + diff_size > MAX_TOTAL_DIFF_SIZE {
                            let remaining = MAX_TOTAL_DIFF_SIZE.saturating_sub(total_diff_size);
                            if remaining > 100 {
                                content.push_str("```diff\n");
                                content.push_str(&diff[..remaining.min(diff.len())]);
                                content.push_str("\n... (diff truncated)\n```\n\n");
                            } else {
                                content.push_str("(diff omitted - size limit reached)\n\n");
                            }
                        } else {
                            content.push_str("```diff\n");
                            content.push_str(&diff);
                            content.push_str("```\n\n");
                        }
                        total_diff_size += diff_size;
                    } else {
                        content.push_str("(diff omitted - size limit reached)\n\n");
                    }
                } else {
                    content.push_str("(unable to read current content)\n\n");
                }
            } else {
                content.push_str("(no previous content available for diff)\n\n");
            }
        }

        content.push_str(
            "You may want to re-read these files before making changes to ensure you have the latest content.",
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::ChangedFiles,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "changed_files.test.rs"]
mod tests;
