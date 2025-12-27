//! System reminder configuration.
//!
//! Configuration for the system reminder attachment system.

use serde::Deserialize;
use serde::Serialize;

/// Minimum severity level for LSP diagnostics to be injected.
///
/// Only diagnostics at or above this severity level will be included in system reminders.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LspDiagnosticsMinSeverity {
    /// Only inject errors (most restrictive, default for production).
    #[default]
    Error,
    /// Inject errors and warnings.
    Warning,
    /// Inject errors, warnings, and info messages.
    Info,
    /// Inject all diagnostics including hints (least restrictive).
    Hint,
}

/// System reminder configuration.
///
/// Controls the behavior of the system reminder attachment system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SystemReminderConfig {
    /// Master enable/disable (default: true).
    pub enabled: bool,

    /// User-defined critical instruction (always injected when set).
    /// Matches criticalSystemReminder_EXPERIMENTAL in Claude Code.
    #[serde(default)]
    pub critical_instruction: Option<String>,

    /// Per-attachment enable/disable (granular control).
    #[serde(default)]
    pub attachments: AttachmentSettings,

    /// Custom timeout in milliseconds (default: 1000).
    #[serde(default)]
    pub timeout_ms: Option<i64>,
}

impl Default for SystemReminderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            critical_instruction: None,
            attachments: AttachmentSettings::default(),
            timeout_ms: Some(1000),
        }
    }
}

/// Per-attachment enable/disable settings (Phase 1: 6 types).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AttachmentSettings {
    /// Critical instruction reminder (default: true).
    pub critical_instruction: bool,
    /// Plan mode instructions (default: true).
    pub plan_mode: bool,
    /// Plan tool reminder - update_plan tool usage (default: true).
    pub plan_tool_reminder: bool,
    /// File change notifications (default: true).
    pub changed_files: bool,
    /// Background task status (default: true).
    pub background_task: bool,
    /// LSP diagnostics notifications (default: true).
    pub lsp_diagnostics: bool,
    /// Minimum severity for LSP diagnostics (default: error only).
    #[serde(default)]
    pub lsp_diagnostics_min_severity: LspDiagnosticsMinSeverity,
}

impl Default for AttachmentSettings {
    fn default() -> Self {
        Self {
            critical_instruction: true,
            plan_mode: true,
            plan_tool_reminder: true,
            changed_files: true,
            background_task: true,
            lsp_diagnostics: true,
            lsp_diagnostics_min_severity: LspDiagnosticsMinSeverity::default(),
        }
    }
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_reminder_config_default() {
        let config = SystemReminderConfig::default();
        assert!(config.enabled);
        assert!(config.critical_instruction.is_none());
        assert_eq!(config.timeout_ms, Some(1000));
    }

    #[test]
    fn test_attachment_settings_default() {
        let settings = AttachmentSettings::default();
        assert!(settings.critical_instruction);
        assert!(settings.plan_mode);
        assert!(settings.plan_tool_reminder);
        assert!(settings.changed_files);
        assert!(settings.background_task);
        assert!(settings.lsp_diagnostics);
    }

    #[test]
    fn test_config_deserialize() {
        let toml = r#"
            enabled = true
            critical_instruction = "Always run tests"
            timeout_ms = 2000

            [attachments]
            critical_instruction = true
            plan_mode = false
            plan_tool_reminder = true
            changed_files = false
            background_task = true
        "#;

        let config: SystemReminderConfig = toml::from_str(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(
            config.critical_instruction,
            Some("Always run tests".to_string())
        );
        assert_eq!(config.timeout_ms, Some(2000));
        assert!(config.attachments.critical_instruction);
        assert!(!config.attachments.plan_mode);
        assert!(config.attachments.plan_tool_reminder);
        assert!(!config.attachments.changed_files);
        assert!(config.attachments.background_task);
    }

    #[test]
    fn test_config_deserialize_partial() {
        let toml = r#"
            enabled = false
        "#;

        let config: SystemReminderConfig = toml::from_str(toml).unwrap();
        assert!(!config.enabled);
        // Defaults should apply
        assert!(config.critical_instruction.is_none());
        assert!(config.attachments.plan_mode);
    }

    #[test]
    fn test_config_serialize() {
        let config = SystemReminderConfig {
            enabled: true,
            critical_instruction: Some("Test instruction".to_string()),
            attachments: AttachmentSettings {
                critical_instruction: true,
                plan_mode: false,
                ..Default::default()
            },
            timeout_ms: Some(1500),
        };

        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("enabled = true"));
        assert!(toml_str.contains("Test instruction"));
    }
}
