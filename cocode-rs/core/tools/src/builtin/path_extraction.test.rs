use super::*;

#[test]
fn test_parse_paths_simple() {
    let response = "/path/to/file.txt\n./relative/file.rs\n../parent/file.go";
    let paths = LlmPathExtractor::parse_paths(response);

    assert_eq!(paths.len(), 3);
    assert_eq!(paths[0], PathBuf::from("/path/to/file.txt"));
    assert_eq!(paths[1], PathBuf::from("./relative/file.rs"));
    assert_eq!(paths[2], PathBuf::from("../parent/file.go"));
}

#[test]
fn test_parse_paths_with_noise() {
    let response = "The command modified:\n/file1.txt\n\nNote: some text\n./file2.rs";
    let paths = LlmPathExtractor::parse_paths(response);

    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], PathBuf::from("/file1.txt"));
    assert_eq!(paths[1], PathBuf::from("./file2.rs"));
}

#[test]
fn test_parse_paths_empty() {
    let response = "No file paths found";
    let paths = LlmPathExtractor::parse_paths(response);

    assert!(paths.is_empty());
}

#[test]
fn test_parse_paths_with_extensions() {
    let response = "main.rs\nCargo.toml\nREADME.md";
    let paths = LlmPathExtractor::parse_paths(response);

    assert_eq!(paths.len(), 3);
}

#[test]
fn test_from_model_roles_fast() {
    // Can't test without a real client, but we can test the logic
    let mut roles = ModelRoles::default();
    roles.set(
        ModelRole::Main,
        ModelSpec::new("anthropic", "claude-sonnet"),
    );
    roles.set(ModelRole::Fast, ModelSpec::new("anthropic", "claude-haiku"));

    // Fast role should be returned (not main)
    let fast_spec = roles.get(ModelRole::Fast).unwrap();
    assert_eq!(fast_spec.model, "claude-haiku");
}

#[test]
fn test_from_model_roles_fallback() {
    let mut roles = ModelRoles::default();
    roles.set(
        ModelRole::Main,
        ModelSpec::new("anthropic", "claude-sonnet"),
    );
    // No fast role set

    // Should fall back to main
    let fast_spec = roles.get(ModelRole::Fast).unwrap();
    assert_eq!(fast_spec.model, "claude-sonnet");
}

#[test]
fn test_from_model_roles_none() {
    let roles = ModelRoles::default();
    // No roles set

    // Should return None
    assert!(roles.get(ModelRole::Fast).is_none());
}
