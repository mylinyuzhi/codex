//! At-mentioned files generator.
//!
//! Injects file contents for @mentioned files in user prompts.
//! Aligns with Claude Code's Read tool limits.

use std::fs;
use std::path::Path;

use async_trait::async_trait;

use crate::Result;
use crate::config::AtMentionedFilesConfig;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::parsing::parse_file_mentions;
use crate::types::AttachmentType;
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

        // Parse @file mentions from prompt
        let mentions = parse_file_mentions(user_prompt);
        if mentions.is_empty() {
            return Ok(None);
        }

        // Get config limits
        let file_config = &ctx.config.at_mentioned_files;

        let mut content = String::new();

        for mention in &mentions {
            let resolved_path = mention.resolve(&ctx.cwd);

            // Format matches Claude Code's Read tool output format
            let file_path_str = resolved_path.to_string_lossy();

            // Read file content with limits
            match read_file_content(
                &resolved_path,
                mention.line_start,
                mention.line_end,
                file_config,
            ) {
                Ok(ReadResult::Content(file_content)) => {
                    // Format as tool result (Claude Code alignment)
                    content.push_str(&format!(
                        "Called the Read tool with the following input: {{\"file_path\":\"{file_path_str}\"}}\n"
                    ));
                    content.push_str(&format!(
                        "Result of calling the Read tool: \"{}\"\n\n",
                        escape_json_string(&file_content)
                    ));
                }
                Ok(ReadResult::TooLarge { size, max }) => {
                    // File too large - show error message
                    content.push_str(&format!(
                        "Called the Read tool with the following input: {{\"file_path\":\"{file_path_str}\"}}\n"
                    ));
                    content.push_str(&format!(
                        "Error: File too large ({size} bytes, max {max} bytes)\n\n"
                    ));
                }
                Err(e) => {
                    // Handle directories
                    if resolved_path.is_dir() {
                        match list_directory(&resolved_path) {
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
                    } else {
                        content.push_str(&format!("Error reading file {file_path_str}: {e}\n\n"));
                    }
                }
            }
        }

        if content.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::AtMentionedFiles,
            content.trim(),
        )))
    }
}

/// Result of reading a file with limits applied.
enum ReadResult {
    /// File content successfully read.
    Content(String),
    /// File is too large.
    TooLarge { size: i64, max: i64 },
}

/// Read file content, optionally with line range, respecting config limits.
fn read_file_content(
    path: &Path,
    line_start: Option<i32>,
    line_end: Option<i32>,
    config: &AtMentionedFilesConfig,
) -> std::io::Result<ReadResult> {
    // Check file size first
    let metadata = fs::metadata(path)?;
    let file_size = metadata.len() as i64;
    if file_size > config.max_file_size {
        return Ok(ReadResult::TooLarge {
            size: file_size,
            max: config.max_file_size,
        });
    }

    let content = fs::read_to_string(path)?;

    match (line_start, line_end) {
        (Some(start), Some(end)) => {
            // Extract line range (1-indexed)
            let lines: Vec<&str> = content.lines().collect();
            let start_idx = (start - 1).max(0) as usize;
            let end_idx = (end as usize).min(lines.len());

            if start_idx >= lines.len() {
                return Ok(ReadResult::Content(String::new()));
            }

            // Format with line numbers, applying line length limit
            let mut result = String::new();
            for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
                let line_num = start_idx + i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
            }
            Ok(ReadResult::Content(result))
        }
        (Some(start), None) => {
            // From line start to EOF (with max_lines limit)
            let lines: Vec<&str> = content.lines().collect();
            let start_idx = (start - 1).max(0) as usize;
            if start_idx >= lines.len() {
                return Ok(ReadResult::Content(String::new()));
            }
            let mut result = String::new();
            let mut count = 0;
            for (i, line) in lines[start_idx..].iter().enumerate() {
                if count >= config.max_lines {
                    let remaining = lines.len() - start_idx - count as usize;
                    result.push_str(&format!("\n... truncated ({remaining} more lines)\n"));
                    break;
                }
                let line_num = start_idx + i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
                count += 1;
            }
            Ok(ReadResult::Content(result))
        }
        _ => {
            // Full file with line numbers, respecting max_lines
            let mut result = String::new();
            let mut line_count = 0;
            for (i, line) in content.lines().enumerate() {
                if line_count >= config.max_lines {
                    result.push_str(&format!(
                        "\n... truncated ({} more lines)\n",
                        content.lines().count() as i32 - config.max_lines
                    ));
                    break;
                }
                let line_num = i + 1;
                let truncated = truncate_line(line, config.max_line_length);
                result.push_str(&format!("{line_num:>6}\t{truncated}\n"));
                line_count += 1;
            }
            Ok(ReadResult::Content(result))
        }
    }
}

/// Truncate a line if it exceeds the max length.
fn truncate_line(line: &str, max_length: i32) -> String {
    if line.len() > max_length as usize {
        format!("{}...", &line[..max_length as usize])
    } else {
        line.to_string()
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
