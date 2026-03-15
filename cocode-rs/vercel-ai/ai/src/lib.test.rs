use super::*;

#[test]
fn test_prompt_from_str() {
    let prompt: Prompt = "Hello, world!".into();
    assert!(prompt.system.is_none());
}

#[test]
fn test_prompt_with_system() {
    let prompt = Prompt::user("Hello").with_system("Be helpful.");
    assert!(prompt.system.is_some());
}

#[test]
fn test_language_model_from_string() {
    let model: LanguageModel = "gpt-4".into();
    assert!(model.is_string());
}
