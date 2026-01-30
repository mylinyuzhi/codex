//! Compaction and session memory configuration.
//!
//! Defines settings for automatic context compaction and session memory management.

use serde::{Deserialize, Serialize};

// ============================================================================
// Session Memory Constants
// ============================================================================

/// Default minimum tokens for session memory extraction.
pub const DEFAULT_SESSION_MEMORY_MIN_TOKENS: i32 = 10000;

/// Default maximum tokens for session memory extraction.
pub const DEFAULT_SESSION_MEMORY_MAX_TOKENS: i32 = 40000;

/// Default cooldown in seconds between memory extractions.
pub const DEFAULT_EXTRACTION_COOLDOWN_SECS: i32 = 60;

// ============================================================================
// Context Restoration Constants
// ============================================================================

/// Default maximum files for context restoration.
pub const DEFAULT_CONTEXT_RESTORE_MAX_FILES: i32 = 5;

/// Default token budget for context restoration.
pub const DEFAULT_CONTEXT_RESTORE_BUDGET: i32 = 50000;

/// Default maximum tokens per file during context restoration.
pub const DEFAULT_MAX_TOKENS_PER_FILE: i32 = 5000;

// ============================================================================
// Threshold Control Constants
// ============================================================================

/// Auto-compact target buffer (minimum tokens to preserve).
pub const DEFAULT_MIN_TOKENS_TO_PRESERVE: i32 = 13000;

/// Warning threshold offset.
pub const DEFAULT_WARNING_THRESHOLD_OFFSET: i32 = 20000;

/// Error threshold offset.
pub const DEFAULT_ERROR_THRESHOLD_OFFSET: i32 = 20000;

/// Hard blocking limit offset.
pub const DEFAULT_MIN_BLOCKING_OFFSET: i32 = 3000;

// ============================================================================
// Micro-Compact Constants
// ============================================================================

/// Micro-compact minimum savings tokens.
pub const DEFAULT_MICRO_COMPACT_MIN_SAVINGS: i32 = 20000;

/// Micro-compact trigger threshold.
pub const DEFAULT_MICRO_COMPACT_THRESHOLD: i32 = 40000;

/// Number of recent tool results to keep during micro-compaction.
pub const DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP: i32 = 3;

// ============================================================================
// Full Compact Constants
// ============================================================================

/// Maximum retries for summary generation.
pub const DEFAULT_MAX_SUMMARY_RETRIES: i32 = 2;

/// Maximum output tokens for compact operation.
pub const DEFAULT_MAX_COMPACT_OUTPUT_TOKENS: i32 = 16000;

/// Token estimation safety margin coefficient (1.33x).
pub const DEFAULT_TOKEN_SAFETY_MARGIN: f64 = 1.3333333333333333;

/// Token estimate for images.
pub const DEFAULT_TOKENS_PER_IMAGE: i32 = 1334;

// ============================================================================
// CompactConfig
// ============================================================================

/// Compaction and session memory configuration.
///
/// Controls automatic context compaction behavior and session memory management.
///
/// # Environment Variables
///
/// - `DISABLE_COMPACT`: Completely disable compaction feature
/// - `DISABLE_AUTO_COMPACT`: Disable automatic compaction (manual still works)
/// - `DISABLE_MICRO_COMPACT`: Disable micro-compaction (frequent small compactions)
/// - `COCODE_AUTOCOMPACT_PCT_OVERRIDE`: Override auto-compact percentage threshold (0-100)
/// - `COCODE_BLOCKING_LIMIT_OVERRIDE`: Override blocking limit for compaction
/// - `COCODE_SESSION_MEMORY_MIN_TOKENS`: Minimum tokens for session memory
/// - `COCODE_SESSION_MEMORY_MAX_TOKENS`: Maximum tokens for session memory
/// - `COCODE_EXTRACTION_COOLDOWN_SECS`: Cooldown between memory extractions
/// - `COCODE_CONTEXT_RESTORE_MAX_FILES`: Maximum files for context restoration
/// - `COCODE_CONTEXT_RESTORE_BUDGET`: Token budget for context restoration
/// - `COCODE_MIN_TOKENS_TO_PRESERVE`: Auto-compact target buffer
/// - `COCODE_WARNING_THRESHOLD_OFFSET`: Warning threshold offset
/// - `COCODE_ERROR_THRESHOLD_OFFSET`: Error threshold offset
/// - `COCODE_MIN_BLOCKING_OFFSET`: Hard blocking limit offset
/// - `COCODE_MICRO_COMPACT_MIN_SAVINGS`: Micro-compact minimum savings tokens
/// - `COCODE_MICRO_COMPACT_THRESHOLD`: Micro-compact trigger threshold
/// - `COCODE_RECENT_TOOL_RESULTS_TO_KEEP`: Recent tool results to keep
/// - `COCODE_MAX_SUMMARY_RETRIES`: Maximum retries for summary generation
/// - `COCODE_MAX_COMPACT_OUTPUT_TOKENS`: Maximum output tokens for compact
/// - `COCODE_TOKEN_SAFETY_MARGIN`: Token estimation safety margin
/// - `COCODE_TOKENS_PER_IMAGE`: Token estimate for images
/// - `COCODE_MAX_TOKENS_PER_FILE`: Maximum tokens per file
///
/// # Example
///
/// ```json
/// {
///   "compact": {
///     "disable_compact": false,
///     "disable_auto_compact": false,
///     "session_memory_min_tokens": 15000,
///     "session_memory_max_tokens": 50000,
///     "min_tokens_to_preserve": 13000,
///     "micro_compact_min_savings": 20000
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CompactConfig {
    // ========================================================================
    // Feature Toggles
    // ========================================================================
    /// Completely disable compaction feature.
    #[serde(default)]
    pub disable_compact: bool,

    /// Disable automatic compaction (manual still works).
    #[serde(default)]
    pub disable_auto_compact: bool,

    /// Disable micro-compaction (frequent small compactions).
    #[serde(default)]
    pub disable_micro_compact: bool,

    // ========================================================================
    // Override Values
    // ========================================================================
    /// Override auto-compact percentage threshold (0-100).
    #[serde(default)]
    pub autocompact_pct_override: Option<i32>,

    /// Override blocking limit for compaction.
    #[serde(default)]
    pub blocking_limit_override: Option<i32>,

    // ========================================================================
    // Session Memory
    // ========================================================================
    /// Minimum tokens for session memory extraction.
    #[serde(default = "default_session_memory_min_tokens")]
    pub session_memory_min_tokens: i32,

    /// Maximum tokens for session memory extraction.
    #[serde(default = "default_session_memory_max_tokens")]
    pub session_memory_max_tokens: i32,

    /// Cooldown in seconds between memory extractions.
    #[serde(default = "default_extraction_cooldown_secs")]
    pub extraction_cooldown_secs: i32,

    // ========================================================================
    // Context Restoration
    // ========================================================================
    /// Maximum files for context restoration.
    #[serde(default = "default_context_restore_max_files")]
    pub context_restore_max_files: i32,

    /// Token budget for context restoration.
    #[serde(default = "default_context_restore_budget")]
    pub context_restore_budget: i32,

    /// Maximum tokens per file during context restoration.
    #[serde(default = "default_max_tokens_per_file")]
    pub max_tokens_per_file: i32,

    // ========================================================================
    // Threshold Control
    // ========================================================================
    /// Auto-compact target buffer (minimum tokens to preserve).
    #[serde(default = "default_min_tokens_to_preserve")]
    pub min_tokens_to_preserve: i32,

    /// Warning threshold offset.
    #[serde(default = "default_warning_threshold_offset")]
    pub warning_threshold_offset: i32,

    /// Error threshold offset.
    #[serde(default = "default_error_threshold_offset")]
    pub error_threshold_offset: i32,

    /// Hard blocking limit offset.
    #[serde(default = "default_min_blocking_offset")]
    pub min_blocking_offset: i32,

    // ========================================================================
    // Micro-Compact
    // ========================================================================
    /// Micro-compact minimum savings tokens.
    #[serde(default = "default_micro_compact_min_savings")]
    pub micro_compact_min_savings: i32,

    /// Micro-compact trigger threshold.
    #[serde(default = "default_micro_compact_threshold")]
    pub micro_compact_threshold: i32,

    /// Number of recent tool results to keep during micro-compaction.
    #[serde(default = "default_recent_tool_results_to_keep")]
    pub recent_tool_results_to_keep: i32,

    // ========================================================================
    // Full Compact
    // ========================================================================
    /// Maximum retries for summary generation.
    #[serde(default = "default_max_summary_retries")]
    pub max_summary_retries: i32,

    /// Maximum output tokens for compact operation.
    #[serde(default = "default_max_compact_output_tokens")]
    pub max_compact_output_tokens: i32,

    /// Token estimation safety margin coefficient (1.33x).
    #[serde(default = "default_token_safety_margin")]
    pub token_safety_margin: f64,

    /// Token estimate for images.
    #[serde(default = "default_tokens_per_image")]
    pub tokens_per_image: i32,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            // Feature toggles
            disable_compact: false,
            disable_auto_compact: false,
            disable_micro_compact: false,
            // Overrides
            autocompact_pct_override: None,
            blocking_limit_override: None,
            // Session memory
            session_memory_min_tokens: DEFAULT_SESSION_MEMORY_MIN_TOKENS,
            session_memory_max_tokens: DEFAULT_SESSION_MEMORY_MAX_TOKENS,
            extraction_cooldown_secs: DEFAULT_EXTRACTION_COOLDOWN_SECS,
            // Context restoration
            context_restore_max_files: DEFAULT_CONTEXT_RESTORE_MAX_FILES,
            context_restore_budget: DEFAULT_CONTEXT_RESTORE_BUDGET,
            max_tokens_per_file: DEFAULT_MAX_TOKENS_PER_FILE,
            // Threshold control
            min_tokens_to_preserve: DEFAULT_MIN_TOKENS_TO_PRESERVE,
            warning_threshold_offset: DEFAULT_WARNING_THRESHOLD_OFFSET,
            error_threshold_offset: DEFAULT_ERROR_THRESHOLD_OFFSET,
            min_blocking_offset: DEFAULT_MIN_BLOCKING_OFFSET,
            // Micro-compact
            micro_compact_min_savings: DEFAULT_MICRO_COMPACT_MIN_SAVINGS,
            micro_compact_threshold: DEFAULT_MICRO_COMPACT_THRESHOLD,
            recent_tool_results_to_keep: DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP,
            // Full compact
            max_summary_retries: DEFAULT_MAX_SUMMARY_RETRIES,
            max_compact_output_tokens: DEFAULT_MAX_COMPACT_OUTPUT_TOKENS,
            token_safety_margin: DEFAULT_TOKEN_SAFETY_MARGIN,
            tokens_per_image: DEFAULT_TOKENS_PER_IMAGE,
        }
    }
}

impl CompactConfig {
    /// Check if compaction is enabled (not disabled).
    pub fn is_compaction_enabled(&self) -> bool {
        !self.disable_compact
    }

    /// Check if auto-compaction is enabled.
    pub fn is_auto_compact_enabled(&self) -> bool {
        !self.disable_compact && !self.disable_auto_compact
    }

    /// Check if micro-compaction is enabled.
    pub fn is_micro_compact_enabled(&self) -> bool {
        !self.disable_compact && !self.disable_micro_compact
    }

    /// Calculate auto-compact target based on available tokens.
    ///
    /// If `autocompact_pct_override` is set, uses that percentage of available tokens,
    /// capped at `available_tokens - min_tokens_to_preserve`.
    pub fn auto_compact_target(&self, available_tokens: i32) -> i32 {
        if let Some(pct) = self.autocompact_pct_override {
            let calculated = (available_tokens as f64 * (pct as f64 / 100.0)).floor() as i32;
            calculated.min(available_tokens - self.min_tokens_to_preserve)
        } else {
            available_tokens - self.min_tokens_to_preserve
        }
    }

    /// Calculate blocking limit based on available tokens.
    ///
    /// Uses `blocking_limit_override` if set, otherwise `available_tokens - min_blocking_offset`.
    pub fn blocking_limit(&self, available_tokens: i32) -> i32 {
        self.blocking_limit_override
            .unwrap_or(available_tokens - self.min_blocking_offset)
    }

    /// Calculate warning threshold based on target.
    pub fn warning_threshold(&self, target: i32) -> i32 {
        target - self.warning_threshold_offset
    }

    /// Calculate error threshold based on target.
    pub fn error_threshold(&self, target: i32) -> i32 {
        target - self.error_threshold_offset
    }

    /// Apply safety margin to token estimate.
    pub fn estimate_tokens_with_margin(&self, base_estimate: i32) -> i32 {
        (base_estimate as f64 * self.token_safety_margin).ceil() as i32
    }

    /// Validate configuration values.
    ///
    /// Returns an error message if any values are invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Validate autocompact_pct_override
        if let Some(pct) = self.autocompact_pct_override {
            if !(0..=100).contains(&pct) {
                return Err(format!("autocompact_pct_override must be 0-100, got {pct}"));
            }
        }

        // Validate session memory tokens
        if self.session_memory_min_tokens > self.session_memory_max_tokens {
            return Err(format!(
                "session_memory_min_tokens ({}) > session_memory_max_tokens ({})",
                self.session_memory_min_tokens, self.session_memory_max_tokens
            ));
        }

        // Validate non-negative values
        if self.extraction_cooldown_secs < 0 {
            return Err(format!(
                "extraction_cooldown_secs must be >= 0, got {}",
                self.extraction_cooldown_secs
            ));
        }

        if self.context_restore_max_files < 0 {
            return Err(format!(
                "context_restore_max_files must be >= 0, got {}",
                self.context_restore_max_files
            ));
        }

        if self.context_restore_budget < 0 {
            return Err(format!(
                "context_restore_budget must be >= 0, got {}",
                self.context_restore_budget
            ));
        }

        if self.max_tokens_per_file < 0 {
            return Err(format!(
                "max_tokens_per_file must be >= 0, got {}",
                self.max_tokens_per_file
            ));
        }

        if self.min_tokens_to_preserve < 0 {
            return Err(format!(
                "min_tokens_to_preserve must be >= 0, got {}",
                self.min_tokens_to_preserve
            ));
        }

        if self.micro_compact_min_savings < 0 {
            return Err(format!(
                "micro_compact_min_savings must be >= 0, got {}",
                self.micro_compact_min_savings
            ));
        }

        if self.recent_tool_results_to_keep < 0 {
            return Err(format!(
                "recent_tool_results_to_keep must be >= 0, got {}",
                self.recent_tool_results_to_keep
            ));
        }

        if self.max_summary_retries < 1 {
            return Err(format!(
                "max_summary_retries must be >= 1, got {}",
                self.max_summary_retries
            ));
        }

        if self.token_safety_margin < 1.0 {
            return Err(format!(
                "token_safety_margin must be >= 1.0, got {}",
                self.token_safety_margin
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Default value functions for serde
// ============================================================================

fn default_session_memory_min_tokens() -> i32 {
    DEFAULT_SESSION_MEMORY_MIN_TOKENS
}

fn default_session_memory_max_tokens() -> i32 {
    DEFAULT_SESSION_MEMORY_MAX_TOKENS
}

fn default_extraction_cooldown_secs() -> i32 {
    DEFAULT_EXTRACTION_COOLDOWN_SECS
}

fn default_context_restore_max_files() -> i32 {
    DEFAULT_CONTEXT_RESTORE_MAX_FILES
}

fn default_context_restore_budget() -> i32 {
    DEFAULT_CONTEXT_RESTORE_BUDGET
}

fn default_max_tokens_per_file() -> i32 {
    DEFAULT_MAX_TOKENS_PER_FILE
}

fn default_min_tokens_to_preserve() -> i32 {
    DEFAULT_MIN_TOKENS_TO_PRESERVE
}

fn default_warning_threshold_offset() -> i32 {
    DEFAULT_WARNING_THRESHOLD_OFFSET
}

fn default_error_threshold_offset() -> i32 {
    DEFAULT_ERROR_THRESHOLD_OFFSET
}

fn default_min_blocking_offset() -> i32 {
    DEFAULT_MIN_BLOCKING_OFFSET
}

fn default_micro_compact_min_savings() -> i32 {
    DEFAULT_MICRO_COMPACT_MIN_SAVINGS
}

fn default_micro_compact_threshold() -> i32 {
    DEFAULT_MICRO_COMPACT_THRESHOLD
}

fn default_recent_tool_results_to_keep() -> i32 {
    DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP
}

fn default_max_summary_retries() -> i32 {
    DEFAULT_MAX_SUMMARY_RETRIES
}

fn default_max_compact_output_tokens() -> i32 {
    DEFAULT_MAX_COMPACT_OUTPUT_TOKENS
}

fn default_token_safety_margin() -> f64 {
    DEFAULT_TOKEN_SAFETY_MARGIN
}

fn default_tokens_per_image() -> i32 {
    DEFAULT_TOKENS_PER_IMAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_config_default() {
        let config = CompactConfig::default();
        // Feature toggles
        assert!(!config.disable_compact);
        assert!(!config.disable_auto_compact);
        assert!(!config.disable_micro_compact);
        // Overrides
        assert!(config.autocompact_pct_override.is_none());
        assert!(config.blocking_limit_override.is_none());
        // Session memory
        assert_eq!(
            config.session_memory_min_tokens,
            DEFAULT_SESSION_MEMORY_MIN_TOKENS
        );
        assert_eq!(
            config.session_memory_max_tokens,
            DEFAULT_SESSION_MEMORY_MAX_TOKENS
        );
        assert_eq!(
            config.extraction_cooldown_secs,
            DEFAULT_EXTRACTION_COOLDOWN_SECS
        );
        // Context restoration
        assert_eq!(
            config.context_restore_max_files,
            DEFAULT_CONTEXT_RESTORE_MAX_FILES
        );
        assert_eq!(
            config.context_restore_budget,
            DEFAULT_CONTEXT_RESTORE_BUDGET
        );
        assert_eq!(config.max_tokens_per_file, DEFAULT_MAX_TOKENS_PER_FILE);
        // Threshold control
        assert_eq!(
            config.min_tokens_to_preserve,
            DEFAULT_MIN_TOKENS_TO_PRESERVE
        );
        assert_eq!(
            config.warning_threshold_offset,
            DEFAULT_WARNING_THRESHOLD_OFFSET
        );
        assert_eq!(
            config.error_threshold_offset,
            DEFAULT_ERROR_THRESHOLD_OFFSET
        );
        assert_eq!(config.min_blocking_offset, DEFAULT_MIN_BLOCKING_OFFSET);
        // Micro-compact
        assert_eq!(
            config.micro_compact_min_savings,
            DEFAULT_MICRO_COMPACT_MIN_SAVINGS
        );
        assert_eq!(
            config.micro_compact_threshold,
            DEFAULT_MICRO_COMPACT_THRESHOLD
        );
        assert_eq!(
            config.recent_tool_results_to_keep,
            DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP
        );
        // Full compact
        assert_eq!(config.max_summary_retries, DEFAULT_MAX_SUMMARY_RETRIES);
        assert_eq!(
            config.max_compact_output_tokens,
            DEFAULT_MAX_COMPACT_OUTPUT_TOKENS
        );
        assert!((config.token_safety_margin - DEFAULT_TOKEN_SAFETY_MARGIN).abs() < f64::EPSILON);
        assert_eq!(config.tokens_per_image, DEFAULT_TOKENS_PER_IMAGE);
    }

    #[test]
    fn test_compact_config_serde() {
        let json = r#"{
            "disable_compact": true,
            "disable_auto_compact": true,
            "autocompact_pct_override": 80,
            "session_memory_min_tokens": 15000,
            "session_memory_max_tokens": 50000,
            "min_tokens_to_preserve": 15000,
            "micro_compact_min_savings": 25000
        }"#;
        let config: CompactConfig = serde_json::from_str(json).unwrap();
        assert!(config.disable_compact);
        assert!(config.disable_auto_compact);
        assert_eq!(config.autocompact_pct_override, Some(80));
        assert_eq!(config.session_memory_min_tokens, 15000);
        assert_eq!(config.session_memory_max_tokens, 50000);
        assert_eq!(config.min_tokens_to_preserve, 15000);
        assert_eq!(config.micro_compact_min_savings, 25000);
    }

    #[test]
    fn test_is_compaction_enabled() {
        let mut config = CompactConfig::default();
        assert!(config.is_compaction_enabled());

        config.disable_compact = true;
        assert!(!config.is_compaction_enabled());
    }

    #[test]
    fn test_is_auto_compact_enabled() {
        let mut config = CompactConfig::default();
        assert!(config.is_auto_compact_enabled());

        config.disable_auto_compact = true;
        assert!(!config.is_auto_compact_enabled());

        config.disable_auto_compact = false;
        config.disable_compact = true;
        assert!(!config.is_auto_compact_enabled());
    }

    #[test]
    fn test_is_micro_compact_enabled() {
        let mut config = CompactConfig::default();
        assert!(config.is_micro_compact_enabled());

        config.disable_micro_compact = true;
        assert!(!config.is_micro_compact_enabled());

        config.disable_micro_compact = false;
        config.disable_compact = true;
        assert!(!config.is_micro_compact_enabled());
    }

    #[test]
    fn test_auto_compact_target() {
        let config = CompactConfig::default();
        let available = 200000;

        // Without override, target = available - min_tokens_to_preserve
        let target = config.auto_compact_target(available);
        assert_eq!(target, available - DEFAULT_MIN_TOKENS_TO_PRESERVE);

        // With override
        let mut config_with_override = CompactConfig::default();
        config_with_override.autocompact_pct_override = Some(80);
        let target = config_with_override.auto_compact_target(available);
        // 80% of 200000 = 160000, capped at 200000 - 13000 = 187000
        assert_eq!(target, 160000);

        // High percentage should be capped
        let mut config_high_pct = CompactConfig::default();
        config_high_pct.autocompact_pct_override = Some(99);
        let target = config_high_pct.auto_compact_target(available);
        // 99% = 198000, but capped at 187000
        assert_eq!(target, 187000);
    }

    #[test]
    fn test_blocking_limit() {
        let config = CompactConfig::default();
        let available = 200000;

        // Without override
        let limit = config.blocking_limit(available);
        assert_eq!(limit, available - DEFAULT_MIN_BLOCKING_OFFSET);

        // With override
        let mut config_with_override = CompactConfig::default();
        config_with_override.blocking_limit_override = Some(180000);
        let limit = config_with_override.blocking_limit(available);
        assert_eq!(limit, 180000);
    }

    #[test]
    fn test_warning_and_error_thresholds() {
        let config = CompactConfig::default();
        let target = 180000;

        let warning = config.warning_threshold(target);
        assert_eq!(warning, target - DEFAULT_WARNING_THRESHOLD_OFFSET);

        let error = config.error_threshold(target);
        assert_eq!(error, target - DEFAULT_ERROR_THRESHOLD_OFFSET);
    }

    #[test]
    fn test_estimate_tokens_with_margin() {
        let config = CompactConfig::default();

        let base = 10000;
        let with_margin = config.estimate_tokens_with_margin(base);
        // 10000 * 1.333... = 13333.33..., ceil = 13334
        assert_eq!(with_margin, 13334);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = CompactConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_pct() {
        let config = CompactConfig {
            autocompact_pct_override: Some(150),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_min_greater_than_max() {
        let config = CompactConfig {
            session_memory_min_tokens: 50000,
            session_memory_max_tokens: 10000,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_negative_values() {
        // Test various negative value validations
        let test_cases = [
            CompactConfig {
                min_tokens_to_preserve: -1,
                ..Default::default()
            },
            CompactConfig {
                micro_compact_min_savings: -1,
                ..Default::default()
            },
            CompactConfig {
                recent_tool_results_to_keep: -1,
                ..Default::default()
            },
            CompactConfig {
                max_tokens_per_file: -1,
                ..Default::default()
            },
        ];

        for config in test_cases {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn test_validate_max_summary_retries() {
        let config = CompactConfig {
            max_summary_retries: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = CompactConfig {
            max_summary_retries: 1,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_token_safety_margin() {
        let config = CompactConfig {
            token_safety_margin: 0.9,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = CompactConfig {
            token_safety_margin: 1.0,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }
}
