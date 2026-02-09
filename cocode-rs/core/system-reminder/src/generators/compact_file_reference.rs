//! Compact file reference generator.
//!
//! This generator reports large files that were read before compaction
//! but are too large to include in the restored context.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::COMPACTED_LARGE_FILES_KEY;
use crate::generator::CompactedLargeFile;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for compact file references.
///
/// Reports large files that were compacted but not fully restored.
/// This helps the model know which files it has seen before but
/// cannot access without re-reading.
#[derive(Debug)]
pub struct CompactFileReferenceGenerator;

#[async_trait]
impl AttachmentGenerator for CompactFileReferenceGenerator {
    fn name(&self) -> &str {
        "CompactFileReferenceGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CompactFileReference
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.compact_file_reference
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Always check after compaction
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Get large files from extension_data
        let large_files = ctx.get_extension::<Vec<CompactedLargeFile>>(COMPACTED_LARGE_FILES_KEY);

        let Some(files) = large_files else {
            return Ok(None);
        };

        if files.is_empty() {
            return Ok(None);
        }

        // Build the reference message
        let mut lines = vec![
            "The following files were read before compaction but are too large to include:"
                .to_string(),
        ];

        for file in files {
            lines.push(format!(
                "- {} ({} lines, {} bytes) - use Read tool to access",
                file.path.display(),
                file.line_count,
                file.byte_size
            ));
        }

        Ok(Some(SystemReminder::text(
            AttachmentType::CompactFileReference,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "compact_file_reference.test.rs"]
mod tests;
