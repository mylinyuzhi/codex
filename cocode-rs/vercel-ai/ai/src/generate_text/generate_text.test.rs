use super::*;

#[test]
fn test_generate_text_options() {
    let options = GenerateTextOptions::new("gpt-4", "Hello")
        .with_max_steps(5)
        .with_tool_choice(LanguageModelV4ToolChoice::auto());

    assert!(options.model.is_string());
    assert_eq!(options.max_steps, Some(5));
}
