//! Model family extensions for downstream features
//!
//! This module defines model families for models with downstream-specific
//! features (e.g., Gemini models with smart edit support).

use crate::model_family::ModelFamily;
use crate::tools::spec::ConfigShellToolType;
use crate::truncate::TruncationPolicy;

/// Gemini 2.5 Pro - Smart Edit optimized
///
/// Gemini 2.5 Pro excels at instruction-based code editing with semantic understanding.
/// Enables the Smart Edit tool by default for this model family.
pub fn gemini_2_5_pro() -> ModelFamily {
    ModelFamily {
        slug: "gemini-2.5-pro".to_string(),
        family: "Gemini 2.5 Pro".to_string(),

        // Smart Edit: Enabled (key feature for Gemini)
        smart_edit_enabled: true,

        // Gemini capabilities
        needs_special_apply_patch_instructions: false,
        supports_reasoning_summaries: false,
        reasoning_summary_format: crate::config::types::ReasoningSummaryFormat::None,
        supports_parallel_tool_calls: true,
        apply_patch_tool_type: None,

        // Use base instructions
        base_instructions: include_str!("../prompt.md").to_string(),

        // No experimental tools needed
        experimental_supported_tools: Vec::new(),

        // Context window settings
        effective_context_window_percent: 95,

        // Verbosity support
        support_verbosity: false,
        default_verbosity: None,

        // Reasoning settings
        default_reasoning_effort: None,

        // Shell tool settings
        shell_type: ConfigShellToolType::Default,

        // Truncation policy
        truncation_policy: TruncationPolicy::Bytes(10_000),
    }
}

/// Gemini 2.0 Flash Thinking Experimental - Smart Edit optimized
///
/// Fast Gemini model with thinking capabilities, also optimized for smart edit.
pub fn gemini_2_0_flash_thinking() -> ModelFamily {
    ModelFamily {
        slug: "gemini-2.0-flash-thinking-exp".to_string(),
        family: "Gemini 2.0 Flash Thinking".to_string(),

        // Smart Edit: Enabled
        smart_edit_enabled: true,

        // Similar capabilities to 2.5 Pro
        needs_special_apply_patch_instructions: false,
        supports_reasoning_summaries: false,
        reasoning_summary_format: crate::config::types::ReasoningSummaryFormat::None,
        supports_parallel_tool_calls: true,
        apply_patch_tool_type: None,
        base_instructions: include_str!("../prompt.md").to_string(),
        experimental_supported_tools: Vec::new(),
        effective_context_window_percent: 95,
        support_verbosity: false,
        default_verbosity: None,
        default_reasoning_effort: None,
        shell_type: ConfigShellToolType::Default,
        truncation_policy: TruncationPolicy::Bytes(10_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_2_5_pro_has_smart_edit() {
        let family = gemini_2_5_pro();
        assert!(family.smart_edit_enabled);
        assert_eq!(family.family, "Gemini 2.5 Pro");
        assert_eq!(family.slug, "gemini-2.5-pro");
    }

    #[test]
    fn test_gemini_2_0_flash_thinking_has_smart_edit() {
        let family = gemini_2_0_flash_thinking();
        assert!(family.smart_edit_enabled);
        assert_eq!(family.family, "Gemini 2.0 Flash Thinking");
    }
}
