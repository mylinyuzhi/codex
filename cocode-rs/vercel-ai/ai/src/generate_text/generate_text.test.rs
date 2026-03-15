use std::sync::Arc;

use super::*;

use crate::test_utils::MockLanguageModel;

#[test]
fn test_generate_text_options() {
    let options = GenerateTextOptions::new("gpt-4", "Hello")
        .with_max_steps(5)
        .with_tool_choice(LanguageModelV4ToolChoice::auto());

    assert!(options.model.is_string());
    assert_eq!(options.max_steps, Some(5));
}

#[test]
fn test_generate_text_options_builders() {
    let options = GenerateTextOptions::new("gpt-4", "Hello")
        .with_max_steps(3)
        .with_active_tools(vec!["tool1".to_string()])
        .with_stop_when(super::super::stop_condition::step_count_is(5));

    assert_eq!(options.max_steps, Some(3));
    assert_eq!(options.active_tools, Some(vec!["tool1".to_string()]));
    assert_eq!(options.stop_when.len(), 1);
}

#[test]
fn test_prepare_step_overrides_default() {
    let overrides = PrepareStepOverrides::default();
    assert!(overrides.tool_choice.is_none());
    assert!(overrides.active_tools.is_none());
    assert!(overrides.model.is_none());
    assert!(overrides.system.is_none());
    assert!(overrides.provider_options.is_none());
}

#[tokio::test]
async fn test_generate_text_basic() {
    let model = MockLanguageModel::with_text("Hello, world!");
    let result = generate_text(GenerateTextOptions {
        model: crate::model::LanguageModel::from_v4(Arc::new(model)),
        prompt: crate::prompt::Prompt::user("Hi"),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(result.text, "Hello, world!");
    assert!(!result.call_id.is_empty());
    assert_eq!(result.model_id, Some("mock-model".to_string()));
}

#[tokio::test]
async fn test_generate_text_with_callbacks() {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    let started = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();
    let finished_clone = finished.clone();

    let model = MockLanguageModel::with_text("Response");
    let callbacks = GenerateTextCallbacks::new()
        .with_on_start(move |_| {
            started_clone.store(true, Ordering::SeqCst);
        })
        .with_on_finish(move |_| {
            finished_clone.store(true, Ordering::SeqCst);
        });

    let result = generate_text(GenerateTextOptions {
        model: crate::model::LanguageModel::from_v4(Arc::new(model)),
        prompt: crate::prompt::Prompt::user("Hi"),
        callbacks,
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(result.text, "Response");
    assert!(started.load(Ordering::SeqCst));
    assert!(finished.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_generate_text_error() {
    let model = MockLanguageModel::builder()
        .with_error("Test error")
        .build();

    let result = generate_text(GenerateTextOptions {
        model: crate::model::LanguageModel::from_v4(Arc::new(model)),
        prompt: crate::prompt::Prompt::user("Hi"),
        ..Default::default()
    })
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_generate_text_with_output_parsing() {
    let model = MockLanguageModel::builder()
        .with_text_response(r#"{"name": "test", "value": 42}"#)
        .build();

    let output = crate::generate_text::Output::new(serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "value": { "type": "integer" }
        }
    }));

    let result = generate_text(GenerateTextOptions {
        model: crate::model::LanguageModel::from_v4(Arc::new(model)),
        prompt: crate::prompt::Prompt::user("Generate JSON"),
        output: Some(output),
        ..Default::default()
    })
    .await
    .unwrap();

    assert!(result.output.is_some());
    let output_value = result.output.unwrap();
    assert_eq!(output_value["name"], "test");
    assert_eq!(output_value["value"], 42);
}
