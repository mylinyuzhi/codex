//! Builtin model instruction bodies.
//!
//! The instruction text is model metadata, so it lives with the
//! provider-agnostic model catalog instead of prompt assembly.

pub(crate) const GPT_5_4: &str = include_str!("../../instructions/gpt5_4_prompt.md");
pub(crate) const GPT_5_5: &str = include_str!("../../instructions/gpt5_5_prompt.md");
pub(crate) const GPT_5_3_CODEX: &str = include_str!("../../instructions/gpt5_3_codex_prompt.md");
pub(crate) const GEMINI: &str = include_str!("../../instructions/gemini_prompt.md");

pub(crate) fn builtin_base_instructions(model_id: &str) -> Option<&'static str> {
    match model_id {
        "gpt-5-4" => Some(GPT_5_4),
        "gpt-5-5" => Some(GPT_5_5),
        "gpt-5-3-codex" => Some(GPT_5_3_CODEX),
        "gemini-2.5-pro" | "gemini-2.5-flash" => Some(GEMINI),
        _ => None,
    }
}
