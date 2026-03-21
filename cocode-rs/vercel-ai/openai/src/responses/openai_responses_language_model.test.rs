use super::*;
use std::sync::Arc;
use vercel_ai_provider::LanguageModelV4CallOptions;

fn make_config() -> Arc<OpenAIConfig> {
    Arc::new(OpenAIConfig {
        provider: "openai.responses".into(),
        base_url: "https://api.openai.com/v1".into(),
        headers: Arc::new(|| {
            let mut h = std::collections::HashMap::new();
            h.insert("Authorization".into(), "Bearer test".into());
            h
        }),
        client: None,
        full_url: None,
    })
}

#[test]
fn get_args_basic() {
    let model = OpenAIResponsesLanguageModel::new("gpt-4o", make_config());
    let options = LanguageModelV4CallOptions {
        prompt: vec![vercel_ai_provider::LanguageModelV4Message::User {
            content: vec![vercel_ai_provider::UserContentPart::Text(
                vercel_ai_provider::TextPart {
                    text: "Hello".into(),
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        }],
        temperature: Some(0.7),
        ..Default::default()
    };

    let (body, warnings) = model.get_args(&options).expect("get_args");
    assert!(warnings.is_empty());
    assert_eq!(body["model"], "gpt-4o");
    assert!(
        body["temperature"]
            .as_f64()
            .is_some_and(|v| (v - 0.7).abs() < 0.01)
    );
    assert!(body["input"].is_array());
}

#[test]
fn get_args_reasoning_model() {
    let model = OpenAIResponsesLanguageModel::new("o3", make_config());
    let options = LanguageModelV4CallOptions {
        prompt: vec![vercel_ai_provider::LanguageModelV4Message::User {
            content: vec![vercel_ai_provider::UserContentPart::Text(
                vercel_ai_provider::TextPart {
                    text: "Hello".into(),
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        }],
        temperature: Some(0.5),
        max_output_tokens: Some(100),
        ..Default::default()
    };

    let (body, _) = model.get_args(&options).expect("get_args");
    assert_eq!(body["model"], "o3");
    assert_eq!(body["max_output_tokens"], 100);
    // Temperature should be omitted for reasoning models
    assert!(body.get("temperature").is_none());
}
