use super::*;

#[test]
fn model_id_and_provider() {
    let model = GoogleGenerativeAIVideoModel::new(
        "veo-2.0-generate-001",
        GoogleGenerativeAIVideoModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(|| HashMap::new()),
            client: None,
            poll_interval: None,
            poll_timeout: None,
        },
    );
    assert_eq!(model.model_id(), "veo-2.0-generate-001");
    assert_eq!(model.provider(), "google.generative-ai");
}

#[test]
fn default_poll_settings() {
    let model = GoogleGenerativeAIVideoModel::new(
        "veo-2.0-generate-001",
        GoogleGenerativeAIVideoModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(|| HashMap::new()),
            client: None,
            poll_interval: None,
            poll_timeout: None,
        },
    );
    assert_eq!(model.poll_interval(), Duration::from_secs(5));
    assert_eq!(model.poll_timeout(), Duration::from_secs(300));
}

#[test]
fn custom_poll_settings() {
    let model = GoogleGenerativeAIVideoModel::new(
        "veo-2.0-generate-001",
        GoogleGenerativeAIVideoModelConfig {
            provider: "google.generative-ai".to_string(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            headers: Arc::new(|| HashMap::new()),
            client: None,
            poll_interval: Some(Duration::from_secs(10)),
            poll_timeout: Some(Duration::from_secs(600)),
        },
    );
    assert_eq!(model.poll_interval(), Duration::from_secs(10));
    assert_eq!(model.poll_timeout(), Duration::from_secs(600));
}
