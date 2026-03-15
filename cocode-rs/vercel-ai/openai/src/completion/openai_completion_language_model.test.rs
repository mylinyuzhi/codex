use super::*;

fn make_config() -> Arc<OpenAIConfig> {
    Arc::new(OpenAIConfig {
        provider: "openai.completion".into(),
        base_url: "https://api.openai.com/v1".into(),
        headers: Arc::new(|| {
            let mut h = std::collections::HashMap::new();
            h.insert("Authorization".into(), "Bearer test".into());
            h
        }),
        client: None,
    })
}

#[test]
fn creates_model() {
    let model = OpenAICompletionLanguageModel::new("gpt-3.5-turbo-instruct", make_config());
    assert_eq!(model.model_id(), "gpt-3.5-turbo-instruct");
    assert_eq!(model.provider(), "openai.completion");
}
