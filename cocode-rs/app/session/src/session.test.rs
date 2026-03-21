use super::*;

#[test]
fn test_session_new() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);

    assert!(!session.id.is_empty());
    assert_eq!(session.model(), Some("gpt-5"));
    assert_eq!(session.provider(), Some("openai"));
    assert_eq!(session.provider_type(), Some(ProviderType::Openai));
    assert_eq!(session.working_dir, PathBuf::from("/test"));
    assert_eq!(session.max_turns, Some(200));
    assert!(session.title.is_none());
    assert!(!session.ephemeral);
}

#[test]
fn test_session_with_id() {
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-sonnet-4"));
    let session = Session::with_id("test-id", PathBuf::from("/test"), selection);

    assert_eq!(session.id, "test-id");
    assert_eq!(session.model(), Some("claude-sonnet-4"));
    assert_eq!(session.provider_type(), Some(ProviderType::Anthropic));
}

#[test]
fn test_session_with_selections() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let session = Session::with_selections(PathBuf::from("/test"), selections);

    assert_eq!(session.model(), Some("claude-opus-4"));
    assert_eq!(
        session
            .model_for_role(ModelRole::Fast)
            .unwrap()
            .model_name(),
        "claude-haiku"
    );
}

#[test]
fn test_session_builder() {
    let session = Session::builder()
        .working_dir("/project")
        .model("openai", "gpt-5")
        .max_turns(100)
        .title("Test Session")
        .ephemeral(true)
        .build();

    assert_eq!(session.model(), Some("gpt-5"));
    assert_eq!(session.provider(), Some("openai"));
    assert_eq!(session.max_turns, Some(100));
    assert_eq!(session.title, Some("Test Session".to_string()));
    assert!(session.ephemeral);
}

#[test]
fn test_session_builder_with_type() {
    let session = Session::builder()
        .working_dir("/project")
        .model_with_type("my-custom-openai", ProviderType::Openai, "gpt-5")
        .build();

    assert_eq!(session.model(), Some("gpt-5"));
    assert_eq!(session.provider(), Some("my-custom-openai"));
    assert_eq!(session.provider_type(), Some(ProviderType::Openai));
}

#[test]
fn test_session_builder_with_selections() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let session = Session::builder()
        .working_dir("/project")
        .selections(selections)
        .build();

    assert_eq!(session.model(), Some("claude-opus-4"));
    assert_eq!(
        session
            .model_for_role(ModelRole::Fast)
            .unwrap()
            .model_name(),
        "claude-haiku"
    );
}

#[test]
fn test_session_touch() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let mut session = Session::new(PathBuf::from("/test"), selection);

    let before = session.last_activity_at;
    std::thread::sleep(std::time::Duration::from_millis(10));
    session.touch();
    assert!(session.last_activity_at > before);
}

#[test]
fn test_session_model_for_role_or_main() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);

    // Fast role not set, should fall back to main
    let fast = session.model_for_role_or_main(ModelRole::Fast);
    assert!(fast.is_some());
    assert_eq!(fast.unwrap().model_name(), "gpt-5");
}

#[test]
fn test_session_set_model_for_role() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let mut session = Session::new(PathBuf::from("/test"), selection);

    // Set fast role
    session.set_model_for_role(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("openai", "gpt-4o-mini")),
    );

    assert_eq!(
        session
            .model_for_role(ModelRole::Fast)
            .unwrap()
            .model_name(),
        "gpt-4o-mini"
    );
}

#[test]
fn test_session_serde() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);

    let json = serde_json::to_string(&session).expect("serialize");
    let parsed: Session = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.id, session.id);
    assert_eq!(parsed.model(), session.model());
    assert_eq!(parsed.provider_type(), session.provider_type());
}

#[test]
fn test_session_serde_multi_role() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let session = Session::with_selections(PathBuf::from("/test"), selections);

    let json = serde_json::to_string(&session).expect("serialize");
    let parsed: Session = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.model(), Some("claude-opus-4"));
    assert_eq!(
        parsed.model_for_role(ModelRole::Fast).unwrap().model_name(),
        "claude-haiku"
    );
}
