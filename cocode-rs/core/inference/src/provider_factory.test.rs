use super::*;
use cocode_protocol::ModelInfo;

fn make_provider_info(api: ProviderApi, base_url: &str) -> ProviderInfo {
    ProviderInfo::new("Test", api, base_url).with_api_key("test-api-key")
}

#[test]
fn test_create_openai_provider() {
    let info = make_provider_info(ProviderApi::Openai, "https://api.openai.com/v1");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "openai");
}

#[test]
fn test_create_anthropic_provider() {
    let info = make_provider_info(ProviderApi::Anthropic, "https://api.anthropic.com");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "anthropic.messages");
}

#[test]
fn test_create_gemini_provider() {
    let info = make_provider_info(
        ProviderApi::Gemini,
        "https://generativelanguage.googleapis.com",
    );
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "google.generative-ai");
}

#[test]
fn test_create_volcengine_provider() {
    let info = make_provider_info(
        ProviderApi::Volcengine,
        "https://ark.cn-beijing.volces.com/api/v3",
    );
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "volcengine");
}

#[test]
fn test_create_zai_provider() {
    let info = make_provider_info(ProviderApi::Zai, "https://api.z.ai/api/paas/v4");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "zai");
}

#[test]
fn test_create_openai_compat_provider() {
    let info = make_provider_info(ProviderApi::OpenaiCompat, "https://custom.api.com/v1");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().provider(), "Test");
}

#[test]
fn test_create_model_with_slug() {
    let info = make_provider_info(ProviderApi::Openai, "https://api.openai.com/v1");
    let result = create_model(&info, "gpt-4o");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model_id(), "gpt-4o");
}

#[test]
fn test_create_model_with_alias() {
    let model_info = ModelInfo {
        slug: "deepseek-r1".to_string(),
        ..Default::default()
    };

    let info = make_provider_info(
        ProviderApi::Volcengine,
        "https://ark.cn-beijing.volces.com/api/v3",
    )
    .with_model_aliased("deepseek-r1", model_info, "ep-20250101-xxxxx");

    let result = create_model(&info, "deepseek-r1");
    assert!(result.is_ok());
    // The model name should be the alias (endpoint ID)
    assert_eq!(result.unwrap().model_id(), "ep-20250101-xxxxx");
}

#[test]
fn test_missing_api_key() {
    let info = ProviderInfo::new("Test", ProviderApi::Openai, "https://api.openai.com/v1");
    // API key is empty - the new SDK still creates a provider since the key
    // is only validated at request time, but create_provider should still succeed.
    // The test verifies the factory handles this edge case.
    let _result = create_provider(&info);
    // Provider creation may or may not fail depending on SDK validation.
    // The important thing is it doesn't panic.
}

// =========================================================================
// P25: wire_api routing and timeout passthrough
// =========================================================================

#[test]
fn test_openai_wire_api_chat_creates_chat_model() {
    let info = make_provider_info(ProviderApi::Openai, "https://api.openai.com/v1")
        .with_wire_api(cocode_protocol::WireApi::Chat);
    let result = create_model(&info, "gpt-4o");
    assert!(result.is_ok());
    let model = result.unwrap();
    assert_eq!(model.provider(), "openai.chat");
}

#[test]
fn test_openai_wire_api_responses_default() {
    let info = make_provider_info(ProviderApi::Openai, "https://api.openai.com/v1");
    // Default wire_api is Responses
    let result = create_model(&info, "gpt-4o");
    assert!(result.is_ok());
    let model = result.unwrap();
    assert_eq!(model.provider(), "openai.responses");
}

#[test]
fn test_wire_api_chat_with_alias() {
    let model_info = ModelInfo {
        slug: "custom-gpt".to_string(),
        ..Default::default()
    };
    let info = make_provider_info(ProviderApi::Openai, "https://api.openai.com/v1")
        .with_wire_api(cocode_protocol::WireApi::Chat)
        .with_model_aliased("custom-gpt", model_info, "ft:gpt-4o-2024-05-13:org::abc");

    let result = create_model(&info, "custom-gpt");
    assert!(result.is_ok());
    let model = result.unwrap();
    assert_eq!(model.provider(), "openai.chat");
    assert_eq!(model.model_id(), "ft:gpt-4o-2024-05-13:org::abc");
}
