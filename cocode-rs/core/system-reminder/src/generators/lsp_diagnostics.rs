//! LSP diagnostics generator.
//!
//! Injects diagnostic information from language servers to help
//! the agent identify and fix issues.

use async_trait::async_trait;

use crate::Result;
use crate::config::DiagnosticSeverity;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::DiagnosticInfo;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;
use crate::types::XmlTag;

/// Generator for LSP diagnostics.
#[derive(Debug)]
pub struct LspDiagnosticsGenerator;

#[async_trait]
impl AttachmentGenerator for LspDiagnosticsGenerator {
    fn name(&self) -> &str {
        "LspDiagnosticsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::LspDiagnostics
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.lsp_diagnostics
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - always show new diagnostics
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.has_diagnostics() {
            return Ok(None);
        }

        // Filter by severity
        let min_severity = &ctx.config.attachments.lsp_diagnostics_min_severity;
        let filtered: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter(|d| severity_passes_filter(&d.severity, *min_severity))
            .collect();

        if filtered.is_empty() {
            return Ok(None);
        }

        let content = format_diagnostics(&filtered);

        // LSP diagnostics use a special XML tag
        let reminder = SystemReminder::new(AttachmentType::LspDiagnostics, content);

        // Note: The XML tag is already set correctly via attachment_type
        debug_assert_eq!(reminder.xml_tag(), XmlTag::NewDiagnostics);

        Ok(Some(reminder))
    }
}

/// Check if a severity passes the minimum severity filter.
fn severity_passes_filter(severity: &str, min_severity: DiagnosticSeverity) -> bool {
    let severity_level = match severity.to_lowercase().as_str() {
        "error" => DiagnosticSeverity::Error,
        "warning" | "warn" => DiagnosticSeverity::Warning,
        "information" | "info" => DiagnosticSeverity::Info,
        "hint" => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Hint, // Unknown = show
    };

    severity_level <= min_severity
}

/// Format diagnostics into a readable string.
fn format_diagnostics(diagnostics: &[&DiagnosticInfo]) -> String {
    let mut content = String::new();
    content.push_str("New diagnostics detected:\n\n");

    // Group by file
    let mut by_file: std::collections::HashMap<&std::path::PathBuf, Vec<&&DiagnosticInfo>> =
        std::collections::HashMap::new();

    for diag in diagnostics {
        by_file.entry(&diag.file_path).or_default().push(diag);
    }

    for (file, diags) in by_file {
        content.push_str(&format!("**{}**:\n", file.display()));

        for diag in diags {
            let code_str = diag
                .code
                .as_ref()
                .map(|c| format!(" [{c}]"))
                .unwrap_or_default();

            content.push_str(&format!(
                "  - Line {}, Col {}: [{}]{} {}\n",
                diag.line, diag.column, diag.severity, code_str, diag.message
            ));
        }

        content.push('\n');
    }

    content.push_str("Please review and address these issues.");

    content
}

#[cfg(test)]
#[path = "lsp_diagnostics.test.rs"]
mod tests;
