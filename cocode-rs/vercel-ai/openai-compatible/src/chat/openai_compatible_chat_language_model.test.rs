use super::*;
use std::sync::Arc;
use vercel_ai_provider::LanguageModelV4CallOptions;

fn make_config() -> Arc<OpenAICompatibleConfig> {
    Arc::new(OpenAICompatibleConfig {
        provider: "xai.chat".into(),
        base_url: "https://api.x.ai/v1".into(),
        headers: Arc::new(|| {
            let mut h = std::collections::HashMap::new();
            h.insert("Authorization".into(), "Bearer test".into());
            h
        }),
        query_params: None,
        client: None,
        include_usage: true,
        supports_structured_outputs: false,
        transform_request_body: None,
        metadata_extractor: None,
        supported_urls: None,
        error_handler: Arc::new(
            crate::openai_compatible_error::OpenAICompatibleFailedResponseHandler::new("xai"),
        ),
        full_url: None,
    })
}

#[test]
fn get_args_basic() {
    let model = OpenAICompatibleChatLanguageModel::new("grok-2", make_config());
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

    let (body, warnings) = model.get_args(&options).expect("get_args");
    assert!(warnings.is_empty());
    assert_eq!(body["model"], "grok-2");
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["max_tokens"], 100);
}

#[test]
fn get_args_with_reasoning_effort() {
    let model = OpenAICompatibleChatLanguageModel::new("deepseek-r1", make_config());

    // Build proper ProviderOptions with nested HashMap
    let mut xai_opts = HashMap::new();
    xai_opts.insert(
        "reasoningEffort".into(),
        serde_json::Value::String("high".into()),
    );
    let mut provider_opts_map = HashMap::new();
    provider_opts_map.insert("xai".into(), xai_opts);

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
        provider_options: Some(vercel_ai_provider::ProviderOptions(provider_opts_map)),
        ..Default::default()
    };

    let (body, _) = model.get_args(&options).expect("get_args");
    assert_eq!(body["reasoning_effort"], "high");
}

#[test]
fn get_args_applies_transform_body() {
    let config = Arc::new(OpenAICompatibleConfig {
        provider: "custom.chat".into(),
        base_url: "https://api.example.com/v1".into(),
        headers: Arc::new(HashMap::new),
        query_params: None,
        client: None,
        include_usage: true,
        supports_structured_outputs: false,
        transform_request_body: Some(Arc::new(|mut body| {
            body["custom_field"] = Value::String("added".into());
            body
        })),
        metadata_extractor: None,
        supported_urls: None,
        error_handler: Arc::new(
            crate::openai_compatible_error::OpenAICompatibleFailedResponseHandler::new("xai"),
        ),
        full_url: None,
    });

    let model = OpenAICompatibleChatLanguageModel::new("test-model", config);
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
        ..Default::default()
    };

    let (body, _) = model.get_args(&options).expect("get_args");
    assert_eq!(body["custom_field"], "added");
}

#[test]
fn provider_options_name_extracts_prefix() {
    let config = make_config();
    assert_eq!(config.provider_options_name(), "xai");
}

#[test]
fn config_url_with_query_params() {
    let config = OpenAICompatibleConfig {
        provider: "test.chat".into(),
        base_url: "https://api.example.com/v1".into(),
        headers: Arc::new(HashMap::new),
        query_params: Some(HashMap::from([("api-version".into(), "2024-01".into())])),
        client: None,
        include_usage: true,
        supports_structured_outputs: false,
        transform_request_body: None,
        metadata_extractor: None,
        supported_urls: None,
        error_handler: Arc::new(
            crate::openai_compatible_error::OpenAICompatibleFailedResponseHandler::new("test"),
        ),
        full_url: None,
    };
    let url = config.url("/chat/completions");
    assert!(url.starts_with("https://api.example.com/v1/chat/completions?"));
    assert!(url.contains("api-version=2024-01"));
}

#[test]
fn get_args_warns_on_top_k() {
    let model = OpenAICompatibleChatLanguageModel::new("test", make_config());
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
        top_k: Some(10),
        ..Default::default()
    };

    let (_, warnings) = model.get_args(&options).expect("get_args");
    assert!(!warnings.is_empty());
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, Warning::Unsupported { feature, .. } if feature == "topK"))
    );
}

#[test]
fn get_args_passthrough_provider_specific_keys() {
    let model = OpenAICompatibleChatLanguageModel::new("grok-2", make_config());

    // Build provider options with both schema keys and passthrough keys
    let mut xai_opts = HashMap::new();
    xai_opts.insert("user".into(), Value::String("test-user".into()));
    xai_opts.insert("logitBias".into(), json!({"50256": -100.0}));
    xai_opts.insert("parallelToolCalls".into(), Value::Bool(true));
    xai_opts.insert("store".into(), Value::Bool(true));
    let mut provider_opts_map = HashMap::new();
    provider_opts_map.insert("xai".into(), xai_opts);

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
        provider_options: Some(vercel_ai_provider::ProviderOptions(provider_opts_map)),
        ..Default::default()
    };

    let (body, _) = model.get_args(&options).expect("get_args");
    // Schema key: extracted and set via typed field
    assert_eq!(body["user"], "test-user");
    // Passthrough keys: spread into body as-is
    assert_eq!(body["logitBias"], json!({"50256": -100.0}));
    assert_eq!(body["parallelToolCalls"], true);
    assert_eq!(body["store"], true);
}

#[test]
fn get_args_openai_compatible_fallback_key() {
    let model = OpenAICompatibleChatLanguageModel::new("test", make_config());

    // Use "openaiCompatible" key instead of provider name "xai"
    let mut compat_opts = HashMap::new();
    compat_opts.insert("reasoningEffort".into(), Value::String("low".into()));
    let mut provider_opts_map = HashMap::new();
    provider_opts_map.insert("openaiCompatible".into(), compat_opts);

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
        provider_options: Some(vercel_ai_provider::ProviderOptions(provider_opts_map)),
        ..Default::default()
    };

    let (body, _) = model.get_args(&options).expect("get_args");
    assert_eq!(body["reasoning_effort"], "low");
}

#[test]
fn get_args_warns_on_json_schema_fallback() {
    let model = OpenAICompatibleChatLanguageModel::new("test", make_config());
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
        response_format: Some(vercel_ai_provider::ResponseFormat::Json {
            schema: Some(json!({"type": "object", "properties": {"answer": {"type": "string"}}})),
            name: Some("test".into()),
            description: None,
        }),
        ..Default::default()
    };

    let (body, warnings) = model.get_args(&options).expect("get_args");
    // Should fall back to json_object since supports_structured_outputs is false
    assert_eq!(body["response_format"]["type"], "json_object");
    // Should emit a warning about the schema fallback
    assert!(warnings.iter().any(|w| {
        matches!(w, Warning::Unsupported { feature, .. } if feature == "responseFormat.schema")
    }));
}
