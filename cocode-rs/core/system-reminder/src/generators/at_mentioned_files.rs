//! At-mentioned files generator.
//!
//! Injects file contents for @mentioned files in user prompts.
//! Aligns with Claude Code's Read tool limits.
//!
//! # Cache Integration (Claude Code v2.1.38 Alignment)
//!
//! When a file is @mentioned, the generator checks the FileTracker cache:
//! - If cached AND unchanged (exact mtime match): produce silent AlreadyReadFile reminder
//! - If not cached OR changed: read the file normally
//!
//! This reduces token usage by avoiding re-reading unchanged files.
//!
//! # FileTracker Updates
//!
//! When files are read via @mention (not cached or changed), the generator
//! records the file read info in the reminder's `file_reads` field. The driver
//! then applies these updates to FileTracker after generate_all() completes.

use std::fs;
use std::path::Path;

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::file_context_resolver::FileReadConfig;
use crate::file_context_resolver::MentionReadDecision;
use crate::file_context_resolver::ReadFileResult;
use crate::file_context_resolver::read_file_with_limits;
use crate::file_context_resolver::resolve_mentions;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::MentionReadRecord;
use crate::types::AttachmentType;
use crate::types::FileReadKind;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for @mentioned files.
///
/// Parses the user prompt for @file mentions and injects the file contents.
/// Supports line ranges via @file.txt:10-20 syntax.
#[derive(Debug)]
pub struct AtMentionedFilesGenerator;

#[async_trait]
impl AttachmentGenerator for AtMentionedFilesGenerator {
    fn name(&self) -> &str {
        "AtMentionedFilesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AtMentionedFiles
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::UserPrompt
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.at_mentioned_files
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Need user prompt to parse mentions
        let user_prompt = match ctx.user_prompt {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(None),
        };

        // Resolve all @mentions: parsing + normalization + dedup + decision
        let resolutions = resolve_mentions(user_prompt, &ctx.cwd, ctx.file_tracker);
        if resolutions.is_empty() {
            return Ok(None);
        }

        // Get config limits
        let file_config = &ctx.config.at_mentioned_files;

        let mut content = String::new();
        let mut already_read_files = Vec::new();

        for resolution in &resolutions {
            let resolved_path = &resolution.path;

            match resolution.decision {
                MentionReadDecision::AlreadyReadUnchanged => {
                    // ============================================================
                    // Already-Read Handling (Claude Code v2.1.38 Alignment)
                    // ============================================================
                    //
                    // The file was fully read before and is unchanged. We add it to
                    // the already-read list which will produce a SILENT reminder.
                    //
                    // # Why Silent?
                    //
                    // Silent reminders consume ZERO tokens in the API request while
                    // still being visible in UI logs. This is crucial for efficiency:
                    // - The model already has the file content from the prior read
                    // - Re-sending the content would waste context window space
                    // - The UI still shows "Read <filename>" notification
                    //
                    // # Why No Separate AlreadyReadFilesGenerator?
                    //
                    // We DON'T use a separate generator for already-read files because:
                    // 1. Avoids duplicate mention parsing overhead
                    // 2. Cleaner integration - already-read check happens during
                    //    mention resolution in this generator
                    // 3. Single source of truth for mention-driven file handling
                    // 4. The codex branch's separate already_read_files.rs generator
                    //    should NOT be adopted - it adds unnecessary complexity
                    //
                    // # MentionReadDecision Variants
                    //
                    // - AlreadyReadUnchanged: File is cached and unchanged -> silent
                    // - NeedsReadLineRange: Has line range -> force re-read with range
                    // - NeedsRead: File not cached or changed -> normal read
                    already_read_files.push(resolved_path.clone());
                    continue;
                }
                MentionReadDecision::NeedsReadLineRange | MentionReadDecision::NeedsRead => {
                    // File needs to be read - proceed below
                }
            }

            // Handle directories before attempting file read
            if resolved_path.is_dir() {
                let file_path_str = resolved_path.to_string_lossy();
                match list_directory(resolved_path) {
                    Ok(listing) => {
                        content.push_str(&format!(
                            "Called the Read tool with the following input: {{\"file_path\":\"{file_path_str}\"}}\n"
                        ));
                        content.push_str(&format!(
                            "Result of calling the Read tool (directory listing):\n{listing}\n\n"
                        ));
                    }
                    Err(dir_err) => {
                        content.push_str(&format!(
                            "Error reading directory {file_path_str}: {dir_err}\n\n"
                        ));
                    }
                }
                continue;
            }

            let file_path_str = resolved_path.to_string_lossy();
            let has_line_range = resolution.line_start.is_some() || resolution.line_end.is_some();

            let read_config = FileReadConfig {
                max_file_size: file_config.max_file_size,
                max_lines: file_config.max_lines,
                max_line_length: file_config.max_line_length,
            };

            match read_file_with_limits(
                resolved_path,
                resolution.line_start,
                resolution.line_end,
                &read_config,
            ) {
                ReadFileResult::Content(file_content) => {
                    // Get file mtime for FileTracker
                    let file_mtime = fs::metadata(resolved_path)
                        .ok()
                        .and_then(|m| m.modified().ok());

                    // Determine read kind based on line range
                    let read_kind = if has_line_range {
                        FileReadKind::PartialContent
                    } else {
                        FileReadKind::FullContent
                    };

                    // Push mention read record to shared buffer for FileTracker sync
                    ctx.mention_read_records
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(MentionReadRecord {
                            path: resolved_path.clone(),
                            content: file_content.clone(),
                            last_modified: file_mtime,
                            offset: resolution.line_start.map(|s| s as i64),
                            limit: resolution
                                .line_end
                                .map(|end| (end - resolution.line_start.unwrap_or(1)) as i64),
                            read_kind,
                            read_turn: ctx.turn_number,
                        });

                    // Format as tool result (Claude Code alignment)
                    content.push_str(&format!(
                        "Called the Read tool with the following input: {{\"file_path\":\"{file_path_str}\"}}\n"
                    ));
                    content.push_str(&format!(
                        "Result of calling the Read tool: \"{}\"\n\n",
                        escape_json_string(&file_content)
                    ));
                }
                ReadFileResult::TooLarge { size, max } => {
                    content.push_str(&format!(
                        "Called the Read tool with the following input: {{\"file_path\":\"{file_path_str}\"}}\n"
                    ));
                    content.push_str(&format!(
                        "Error: File too large ({size} bytes, max {max} bytes)\n\n"
                    ));
                }
                ReadFileResult::Error(e) => {
                    content.push_str(&format!("Error reading file {file_path_str}: {e}\n\n"));
                }
            }
        }

        // If all files were already-read, return a silent reminder with AlreadyReadFile type
        // This ensures zero token cost (silent) while UI still shows the notification
        if content.is_empty() && !already_read_files.is_empty() {
            // All mentioned files were already read - return empty silent reminder
            // The UI should still display "Read <filename>" notification based on the type
            // but no content is sent to the API (zero tokens)
            //
            // Claude Code v2.1.38 alignment: already_read_file type is SILENT.
            // It returns [] in the normalizer, meaning zero tokens to API.
            // The UI handles display separately via the attachment type and metadata.
            return Ok(Some(SystemReminder::already_read_files(already_read_files)));
        }

        // If we have new content (some files needed to be read), just return that.
        // DO NOT add text notification for already-read files - that wastes tokens.
        // The model already has those files in context from prior reads.
        if content.trim().is_empty() {
            return Ok(None);
        }

        // Return reminder (file reads already pushed to mention_read_records Arc)
        Ok(Some(SystemReminder::new(
            AttachmentType::AtMentionedFiles,
            content.trim(),
        )))
    }
}

/// List directory contents.
fn list_directory(path: &Path) -> std::io::Result<String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = if entry.file_type()?.is_dir() {
            "dir"
        } else {
            "file"
        };
        entries.push(format!("  {file_type}: {file_name}"));
    }
    entries.sort();
    Ok(entries.join("\n"))
}

/// Escape a string for JSON output.
fn escape_json_string(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
#[path = "at_mentioned_files.test.rs"]
mod tests;
