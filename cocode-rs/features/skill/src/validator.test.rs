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
