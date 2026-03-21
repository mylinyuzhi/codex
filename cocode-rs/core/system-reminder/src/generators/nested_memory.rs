//! Nested memory generator.
//!
//! Auto-discovers and includes CLAUDE.md, AGENTS.md, and other
//! project configuration files.

use std::path::Path;

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for nested memory (CLAUDE.md auto-discovery).
#[derive(Debug)]
pub struct NestedMemoryGenerator;

#[async_trait]
impl AttachmentGenerator for NestedMemoryGenerator {
    fn name(&self) -> &str {
        "NestedMemoryGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::NestedMemory
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.nested_memory && config.nested_memory.enabled
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - check for new triggers each turn
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.nested_memory_triggers.is_empty() {
            return Ok(None);
        }

        let nested_config = &ctx.config.nested_memory;
        let mut content_parts = Vec::new();
        let mut total_bytes: i64 = 0;

        for trigger_path in &ctx.nested_memory_triggers {
            // Check if we've exceeded the byte limit
            if total_bytes >= nested_config.max_content_bytes {
                break;
            }

            // Try to read the file
            let file_content = match read_memory_file(trigger_path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            // Apply limits
            let truncated = truncate_content(
                &file_content,
                nested_config.max_content_bytes - total_bytes,
                nested_config.max_lines,
            );

            total_bytes += truncated.len() as i64;

            let display_path = trigger_path.display();
            content_parts.push(format!("## Memory File: {display_path}\n\n{truncated}"));
        }

        if content_parts.is_empty() {
            return Ok(None);
        }

        let content = format!(
            "The following project configuration files were discovered:\n\n{}",
            content_parts.join("\n\n---\n\n")
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::NestedMemory,
            content,
        )))
    }
}

/// Read a memory file, returning its content.
fn read_memory_file(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// Truncate content to fit within byte and line limits.
fn truncate_content(content: &str, max_bytes: i64, max_lines: i32) -> String {
    let mut result = String::new();
    let mut line_count = 0;
    let mut byte_count: i64 = 0;

    for line in content.lines() {
        if line_count >= max_lines {
            result.push_str("\n... (truncated, line limit reached)");
            break;
        }

        let line_with_newline = if result.is_empty() {
            line.to_string()
        } else {
            format!("\n{line}")
        };

        let line_bytes = line_with_newline.len() as i64;

        if byte_count + line_bytes > max_bytes {
            result.push_str("\n... (truncated, byte limit reached)");
            break;
        }

        result.push_str(&line_with_newline);
        byte_count += line_bytes;
        line_count += 1;
    }

    result
}

#[cfg(test)]
#[path = "nested_memory.test.rs"]
mod tests;
