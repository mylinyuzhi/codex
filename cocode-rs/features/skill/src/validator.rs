//! Skill validation.
//!
//! Validates that a [`SkillInterface`] conforms to the required constraints
//! before it can be used as a loaded skill. Validation is fail-open at the
//! collection level but strict per-skill: a skill that fails validation is
//! reported but does not block other skills from loading.

use crate::interface::SkillInterface;

/// Maximum allowed length for a skill name.
pub const MAX_NAME_LEN: i32 = 64;

/// Maximum allowed length for a skill description.
pub const MAX_DESCRIPTION_LEN: i32 = 1024;

/// Maximum allowed length for a skill prompt.
pub const MAX_PROMPT_LEN: i32 = 65536;

/// Maximum allowed length for the `when_to_use` field.
pub const MAX_WHEN_TO_USE_LEN: i32 = 1024;

/// Maximum allowed length for the `argument_hint` field.
pub const MAX_ARGUMENT_HINT_LEN: i32 = 256;

/// Maximum allowed length for skill prompt content (inline or file).
pub const SKILL_PROMPT_MAX_CHARS: i32 = 15000;

/// Valid values for the `model` field.
const VALID_MODELS: &[&str] = &["sonnet", "opus", "haiku", "inherit"];

/// Valid values for the `context` field.
const VALID_CONTEXTS: &[&str] = &["main", "fork"];

/// Validates a skill interface and returns any validation errors.
///
/// Returns `Ok(())` if the skill passes all validation checks, or
/// `Err(errors)` with a list of human-readable error messages.
///
/// # Validation Rules
///
/// - `name` must not be empty and must not exceed [`MAX_NAME_LEN`] characters
/// - `name` must contain only alphanumeric characters, hyphens, and underscores
/// - `description` must not be empty and must not exceed [`MAX_DESCRIPTION_LEN`]
/// - At least one of `prompt_file` or `prompt_inline` must be present
/// - If `prompt_inline` is present, it must not exceed [`MAX_PROMPT_LEN`]
/// - `when_to_use` must not exceed [`MAX_WHEN_TO_USE_LEN`] if present
/// - `argument_hint` must not exceed [`MAX_ARGUMENT_HINT_LEN`] if present
/// - `model` must be one of: sonnet, opus, haiku, inherit (if present)
/// - `context` must be one of: main, fork (if present)
pub fn validate_skill(interface: &SkillInterface) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Validate name
    if interface.name.is_empty() {
        errors.push("name must not be empty".to_string());
    } else if interface.name.len() as i32 > MAX_NAME_LEN {
        errors.push(format!(
            "name exceeds max length of {MAX_NAME_LEN}: got {}",
            interface.name.len()
        ));
    } else if !is_valid_name(&interface.name) {
        errors.push(format!(
            "name contains invalid characters: '{}' (only alphanumeric, hyphens, underscores allowed)",
            interface.name
        ));
    }

    // Validate description
    if interface.description.is_empty() {
        errors.push("description must not be empty".to_string());
    } else if interface.description.len() as i32 > MAX_DESCRIPTION_LEN {
        errors.push(format!(
            "description exceeds max length of {MAX_DESCRIPTION_LEN}: got {}",
            interface.description.len()
        ));
    }

    // Validate prompt source
    let has_file = interface
        .prompt_file
        .as_ref()
        .is_some_and(|f| !f.is_empty());
    let has_inline = interface
        .prompt_inline
        .as_ref()
        .is_some_and(|p| !p.is_empty());

    if !has_file && !has_inline {
        errors.push("either prompt_file or prompt_inline must be specified".to_string());
    }

    // Validate inline prompt length
    if let Some(ref prompt) = interface.prompt_inline
        && prompt.len() as i32 > MAX_PROMPT_LEN
    {
        errors.push(format!(
            "prompt_inline exceeds max length of {MAX_PROMPT_LEN}: got {}",
            prompt.len()
        ));
    }

    // Validate when_to_use length
    if let Some(ref when) = interface.when_to_use
        && when.len() as i32 > MAX_WHEN_TO_USE_LEN
    {
        errors.push(format!(
            "when_to_use exceeds max length of {MAX_WHEN_TO_USE_LEN}: got {}",
            when.len()
        ));
    }

    // Validate argument_hint length
    if let Some(ref hint) = interface.argument_hint
        && hint.len() as i32 > MAX_ARGUMENT_HINT_LEN
    {
        errors.push(format!(
            "argument_hint exceeds max length of {MAX_ARGUMENT_HINT_LEN}: got {}",
            hint.len()
        ));
    }

    // Validate model value
    if let Some(ref model) = interface.model
        && !VALID_MODELS.contains(&model.as_str())
    {
        errors.push(format!(
            "model must be one of {VALID_MODELS:?}, got '{model}'"
        ));
    }

    // Validate context value
    if let Some(ref context) = interface.context
        && !VALID_CONTEXTS.contains(&context.as_str())
    {
        errors.push(format!(
            "context must be one of {VALID_CONTEXTS:?}, got '{context}'"
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Checks if a skill name contains only valid characters.
fn is_valid_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
#[path = "validator.test.rs"]
mod tests;
