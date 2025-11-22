//! Model family extensions for downstream features
//!
//! This module defines model families for models with downstream-specific
//! features (e.g., Gemini models with smart edit support).

use crate::openai_models::model_family::ModelFamily;

/// Gemini 2.5 Pro - Smart Edit optimized
///
/// Gemini 2.5 Pro excels at instruction-based code editing with semantic understanding.
/// Enables the Smart Edit tool by default for this model family.
pub fn gemini_2_5_pro() -> ModelFamily {
    crate::model_family!(
        "gemini-2.5-pro", "Gemini 2.5 Pro",
        smart_edit_enabled: true,
        supports_parallel_tool_calls: true,
    )
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
}
