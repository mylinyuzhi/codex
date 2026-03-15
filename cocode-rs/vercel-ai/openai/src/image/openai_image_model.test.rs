use super::*;

fn make_config() -> Arc<OpenAIConfig> {
    Arc::new(OpenAIConfig {
        provider: "openai.image".into(),
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
    let model = OpenAIImageModel::new("dall-e-3", make_config());
    assert_eq!(model.model_id(), "dall-e-3");
    assert_eq!(model.provider(), "openai.image");
    assert_eq!(model.max_images_per_call(), 10);
}
