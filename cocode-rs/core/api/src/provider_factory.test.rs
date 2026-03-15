use super::*;
use cocode_protocol::ModelInfo;

fn make_provider_info(provider_type: ProviderType, base_url: &str) -> ProviderInfo {
    ProviderInfo::new("Test", provider_type, base_url).with_api_key("test-api-key")
}

#[test]
fn test_create_openai_provider() {
    let info = make_provider_info(ProviderType::Openai, "https://api.openai.com/v1");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "openai");
}

#[test]
fn test_create_anthropic_provider() {
    let info = make_provider_info(ProviderType::Anthropic, "https://api.anthropic.com");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "anthropic");
}

#[test]
fn test_create_gemini_provider() {
    let info = make_provider_info(
        ProviderType::Gemini,
        "https://generativelanguage.googleapis.com",
    );
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "gemini");
}

#[test]
fn test_create_volcengine_provider() {
    let info = make_provider_info(
        ProviderType::Volcengine,
        "https://ark.cn-beijing.volces.com/api/v3",
    );
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "volcengine");
}

#[test]
fn test_create_zai_provider() {
    let info = make_provider_info(ProviderType::Zai, "https://api.z.ai/api/paas/v4");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "zhipuai");
}

#[test]
fn test_create_openai_compat_provider() {
    let info = make_provider_info(ProviderType::OpenaiCompat, "https://custom.api.com/v1");
    let result = create_provider(&info);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "Test");
}

#[test]
fn test_create_model_with_slug() {
    let info = make_provider_info(ProviderType::Openai, "https://api.openai.com/v1");
    let result = create_model(&info, "gpt-4o");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model_name(), "gpt-4o");
}

#[test]
fn test_create_model_with_alias() {
    let model_info = ModelInfo {
        slug: "deepseek-r1".to_string(),
        ..Default::default()
    };

    let info = make_provider_info(
        ProviderType::Volcengine,
        "https://ark.cn-beijing.volces.com/api/v3",
    )
    .with_model_aliased("deepseek-r1", model_info, "ep-20250101-xxxxx");

    let result = create_model(&info, "deepseek-r1");
    assert!(result.is_ok());
    // The model name should be the alias (endpoint ID)
    assert_eq!(result.unwrap().model_name(), "ep-20250101-xxxxx");
}

#[test]
fn test_missing_api_key() {
    let info = ProviderInfo::new("Test", ProviderType::Openai, "https://api.openai.com/v1");
    // API key is empty
    let result = create_provider(&info);
    assert!(result.is_err());
}
