use super::*;

#[test]
fn test_builder_required_fields() {
    let result = EnvironmentInfo::builder()
        .cwd("/tmp/test")
        .model("claude-3-opus")
        .build();
    assert!(result.is_ok());

    let env = result.unwrap();
    assert_eq!(env.cwd, PathBuf::from("/tmp/test"));
    assert_eq!(env.model, "claude-3-opus");
    assert!(!env.date.is_empty());
}

#[test]
fn test_builder_all_fields() {
    let env = EnvironmentInfo::builder()
        .platform("darwin")
        .os_version("Darwin 24.0.0")
        .cwd("/home/user/project")
        .is_git_repo(true)
        .git_branch("main")
        .date("2025-01-29")
        .model("claude-3-opus")
        .context_window(200000)
        .max_output_tokens(16384)
        .build()
        .unwrap();

    assert_eq!(env.platform, "darwin");
    assert_eq!(env.os_version, "Darwin 24.0.0");
    assert!(env.is_git_repo);
    assert_eq!(env.git_branch.as_deref(), Some("main"));
    assert_eq!(env.date, "2025-01-29");
    assert_eq!(env.context_window, 200000);
    assert_eq!(env.max_output_tokens, 16384);
}

#[test]
fn test_builder_missing_cwd() {
    let result = EnvironmentInfo::builder().model("test-model").build();
    assert!(result.is_err());
}

#[test]
fn test_builder_missing_model() {
    let result = EnvironmentInfo::builder().cwd("/tmp").build();
    assert!(result.is_err());
}

#[test]
fn test_serde_roundtrip() {
    let env = EnvironmentInfo::builder()
        .platform("linux")
        .cwd("/tmp/test")
        .model("test-model")
        .date("2025-01-29")
        .build()
        .unwrap();

    let json = serde_json::to_string(&env).unwrap();
    let parsed: EnvironmentInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.platform, env.platform);
    assert_eq!(parsed.model, env.model);
}
