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
    if let Some(ref prompt) = interface.prompt_inline {
        if prompt.len() as i32 > MAX_PROMPT_LEN {
            errors.push(format!(
                "prompt_inline exceeds max length of {MAX_PROMPT_LEN}: got {}",
                prompt.len()
            ));
        }
    }

    // Validate when_to_use length
    if let Some(ref when) = interface.when_to_use {
        if when.len() as i32 > MAX_WHEN_TO_USE_LEN {
            errors.push(format!(
                "when_to_use exceeds max length of {MAX_WHEN_TO_USE_LEN}: got {}",
                when.len()
            ));
        }
    }

    // Validate argument_hint length
    if let Some(ref hint) = interface.argument_hint {
        if hint.len() as i32 > MAX_ARGUMENT_HINT_LEN {
            errors.push(format!(
                "argument_hint exceeds max length of {MAX_ARGUMENT_HINT_LEN}: got {}",
                hint.len()
            ));
        }
    }

    // Validate model value
    if let Some(ref model) = interface.model {
        if !VALID_MODELS.contains(&model.as_str()) {
            errors.push(format!(
                "model must be one of {:?}, got '{model}'",
                VALID_MODELS
            ));
        }
    }

    // Validate context value
    if let Some(ref context) = interface.context {
        if !VALID_CONTEXTS.contains(&context.as_str()) {
            errors.push(format!(
                "context must be one of {:?}, got '{context}'",
                VALID_CONTEXTS
            ));
        }
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
mod tests {
    use super::*;
    use crate::interface::SkillInterface;

    fn valid_interface() -> SkillInterface {
        SkillInterface {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
            prompt_file: None,
            prompt_inline: Some("Analyze the diff".to_string()),
            allowed_tools: None,
            when_to_use: None,
            user_invocable: None,
            disable_model_invocation: None,
            model: None,
            context: None,
            agent: None,
            argument_hint: None,
            aliases: None,
            hooks: None,
        }
    }

    #[test]
    fn test_valid_skill() {
        let result = validate_skill(&valid_interface());
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_name() {
        let mut iface = valid_interface();
        iface.name = String::new();
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("name must not be empty"))
        );
    }

    #[test]
    fn test_name_too_long() {
        let mut iface = valid_interface();
        iface.name = "a".repeat(65);
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("name exceeds max length"))
        );
    }

    #[test]
    fn test_name_invalid_chars() {
        let mut iface = valid_interface();
        iface.name = "my skill!".to_string();
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("invalid characters"))
        );
    }

    #[test]
    fn test_valid_name_with_hyphens_and_underscores() {
        let mut iface = valid_interface();
        iface.name = "my-cool_skill-v2".to_string();
        assert!(validate_skill(&iface).is_ok());
    }

    #[test]
    fn test_empty_description() {
        let mut iface = valid_interface();
        iface.description = String::new();
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("description must not be empty"))
        );
    }

    #[test]
    fn test_description_too_long() {
        let mut iface = valid_interface();
        iface.description = "x".repeat(1025);
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("description exceeds max length"))
        );
    }

    #[test]
    fn test_no_prompt_source() {
        let mut iface = valid_interface();
        iface.prompt_file = None;
        iface.prompt_inline = None;
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("prompt_file or prompt_inline"))
        );
    }

    #[test]
    fn test_empty_prompt_sources_treated_as_missing() {
        let mut iface = valid_interface();
        iface.prompt_file = Some(String::new());
        iface.prompt_inline = Some(String::new());
        let result = validate_skill(&iface);
        assert!(result.is_err());
    }

    #[test]
    fn test_prompt_inline_too_long() {
        let mut iface = valid_interface();
        iface.prompt_inline = Some("x".repeat(65537));
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("prompt_inline exceeds max length"))
        );
    }

    #[test]
    fn test_prompt_file_only() {
        let mut iface = valid_interface();
        iface.prompt_inline = None;
        iface.prompt_file = Some("prompt.md".to_string());
        assert!(validate_skill(&iface).is_ok());
    }

    #[test]
    fn test_multiple_errors_collected() {
        let mut iface = valid_interface();
        iface.name = String::new();
        iface.description = String::new();
        iface.prompt_file = None;
        iface.prompt_inline = None;
        let result = validate_skill(&iface);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.len() >= 3,
            "expected at least 3 errors, got {errors:?}"
        );
    }

    #[test]
    fn test_when_to_use_too_long() {
        let mut iface = valid_interface();
        iface.when_to_use = Some("x".repeat(1025));
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("when_to_use exceeds max length"))
        );
    }

    #[test]
    fn test_argument_hint_too_long() {
        let mut iface = valid_interface();
        iface.argument_hint = Some("x".repeat(257));
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("argument_hint exceeds max length"))
        );
    }

    #[test]
    fn test_invalid_model() {
        let mut iface = valid_interface();
        iface.model = Some("gpt-4".to_string());
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("model must be one of"))
        );
    }

    #[test]
    fn test_valid_model_values() {
        for model in &["sonnet", "opus", "haiku", "inherit"] {
            let mut iface = valid_interface();
            iface.model = Some(model.to_string());
            assert!(
                validate_skill(&iface).is_ok(),
                "model '{model}' should be valid"
            );
        }
    }

    #[test]
    fn test_invalid_context() {
        let mut iface = valid_interface();
        iface.context = Some("background".to_string());
        let result = validate_skill(&iface);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|e| e.contains("context must be one of"))
        );
    }

    #[test]
    fn test_valid_context_values() {
        for ctx in &["main", "fork"] {
            let mut iface = valid_interface();
            iface.context = Some(ctx.to_string());
            assert!(
                validate_skill(&iface).is_ok(),
                "context '{ctx}' should be valid"
            );
        }
    }

    #[test]
    fn test_valid_with_all_new_fields() {
        let mut iface = valid_interface();
        iface.when_to_use = Some("When doing X".to_string());
        iface.user_invocable = Some(true);
        iface.disable_model_invocation = Some(false);
        iface.model = Some("sonnet".to_string());
        iface.context = Some("fork".to_string());
        iface.agent = Some("my-agent".to_string());
        iface.argument_hint = Some("<file>".to_string());
        iface.aliases = Some(vec!["alias1".to_string()]);
        assert!(validate_skill(&iface).is_ok());
    }
}
