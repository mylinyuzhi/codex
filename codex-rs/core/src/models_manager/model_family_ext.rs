//! Model family extensions for downstream features
//!
//! This module defines model families for models with downstream-specific
//! features (e.g., Gemini models with smart edit support).

use crate::models_manager::model_family::ModelFamily;
use crate::truncate::TruncationPolicy;

const GEMINI_INSTRUCTIONS: &str = include_str!("../../gemini_prompt.md");

const GEMINI_CONTEXT_WINDOW_300K: i64 = 300_000;

/// Dispatcher for all Gemini models
pub fn gemini_model(slug: &str) -> ModelFamily {
    if slug.starts_with("gemini-3-pro") {
        gemini_3_0_pro()
    } else if slug.starts_with("gemini-3-flash") {
        gemini_3_0_flash()
    } else {
        gemini_default(slug)
    }
}

/// Gemini 3.0 Pro - Smart Edit optimized
fn gemini_3_0_pro() -> ModelFamily {
    crate::model_family!(
        "gemini-3-pro", "Gemini 3 Pro",
        base_instructions: GEMINI_INSTRUCTIONS.to_string(),
        smart_edit_enabled: true,
        supports_parallel_tool_calls: true,
        context_window: Some(GEMINI_CONTEXT_WINDOW_300K),
        truncation_policy: TruncationPolicy::Tokens(10_000),
    )
}

/// Gemini 3.0 Flash - Same capabilities as Pro
fn gemini_3_0_flash() -> ModelFamily {
    crate::model_family!(
        "gemini-3-flash", "Gemini 3 Flash",
        base_instructions: GEMINI_INSTRUCTIONS.to_string(),
        smart_edit_enabled: true,
        supports_parallel_tool_calls: true,
        context_window: Some(GEMINI_CONTEXT_WINDOW_300K),
        truncation_policy: TruncationPolicy::Tokens(10_000),
    )
}

/// Default Gemini model - For unknown Gemini variants
fn gemini_default(slug: &str) -> ModelFamily {
    crate::model_family!(
        slug, "Gemini",
        base_instructions: GEMINI_INSTRUCTIONS.to_string(),
        smart_edit_enabled: true,
        supports_parallel_tool_calls: true,
        context_window: Some(GEMINI_CONTEXT_WINDOW_300K),
        truncation_policy: TruncationPolicy::Tokens(10_000),
    )
}
