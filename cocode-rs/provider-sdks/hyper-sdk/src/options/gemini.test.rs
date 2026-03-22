use super::*;
use crate::options::downcast_options;

#[test]
fn test_gemini_options() {
    let opts = GeminiOptions::new()
        .with_thinking_level(ThinkingLevel::High)
        .with_grounding(true);

    assert_eq!(opts.thinking_level, Some(ThinkingLevel::High));
    assert_eq!(opts.grounding, Some(true));
}

#[test]
fn test_downcast() {
    let opts: Box<dyn ProviderOptionsData> = GeminiOptions::new()
        .with_thinking_level(ThinkingLevel::Medium)
        .boxed();

    let gemini_opts = downcast_options::<GeminiOptions>(&opts);
    assert!(gemini_opts.is_some());
    assert_eq!(
        gemini_opts.unwrap().thinking_level,
        Some(ThinkingLevel::Medium)
    );
}

#[test]
fn test_safety_settings() {
    let opts = GeminiOptions::new().with_safety_settings(vec![SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockOnlyHigh,
    }]);

    assert!(opts.safety_settings.is_some());
    assert_eq!(opts.safety_settings.as_ref().unwrap().len(), 1);
}

#[test]
fn test_include_thoughts() {
    let opts = GeminiOptions::new()
        .with_thinking_level(ThinkingLevel::High)
        .with_include_thoughts(true);

    assert_eq!(opts.thinking_level, Some(ThinkingLevel::High));
    assert_eq!(opts.include_thoughts, Some(true));
}
