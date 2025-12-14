//! Model family extensions for downstream features
//!
//! This module defines model families for models with downstream-specific
//! features (e.g., Gemini models with smart edit support).

use crate::openai_models::model_family::ModelFamily;
use crate::truncate::TruncationPolicy;

const GEMINI_PRO_INSTRUCTIONS: &str = include_str!("../../gemini_pro_prompt.md");

const GEMINI_PRO_CONTEXT_WINDOW_300K: i64 = 300_000;

/// Gemini 3.0 Pro - Smart Edit optimized
///
/// Gemini 3.0 Pro excels at instruction-based code editing with semantic understanding.
/// Enables the Smart Edit tool by default for this model family.
pub fn gemini_3_0_pro() -> ModelFamily {
    crate::model_family!(
        "gemini-3.0-pro", "Gemini 3.0 Pro",
        base_instructions: GEMINI_PRO_INSTRUCTIONS.to_string(),
        smart_edit_enabled: true,
        supports_parallel_tool_calls: true,
        context_window: Some(GEMINI_PRO_CONTEXT_WINDOW_300K),
        truncation_policy: TruncationPolicy::Tokens(10_000),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_3_0_pro_has_smart_edit() {
        let family = gemini_3_0_pro();
        assert!(family.smart_edit_enabled);
        assert_eq!(family.family, "Gemini 3.0 Pro");
        assert_eq!(family.slug, "gemini-3.0-pro");
    }
}
