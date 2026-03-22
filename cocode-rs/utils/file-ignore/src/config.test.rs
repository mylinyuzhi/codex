use super::*;

#[test]
fn test_default_config() {
    let config = IgnoreConfig::default();
    assert!(config.respect_gitignore);
    assert!(config.respect_ignore);
    assert!(!config.include_hidden);
    assert!(!config.follow_links);
    assert!(config.custom_excludes.is_empty());
}

#[test]
fn test_respecting_all() {
    let config = IgnoreConfig::respecting_all();
    assert!(config.respect_gitignore);
    assert!(config.respect_ignore);
}

#[test]
fn test_ignoring_none() {
    let config = IgnoreConfig::ignoring_none();
    assert!(!config.respect_gitignore);
    assert!(!config.respect_ignore);
    assert!(config.include_hidden);
}

#[test]
fn test_builder_pattern() {
    let config = IgnoreConfig::default()
        .with_gitignore(true)
        .with_ignore(true)
        .with_hidden(true)
        .with_follow_links(true)
        .with_excludes(vec!["*.log".to_string()]);

    assert!(config.respect_gitignore);
    assert!(config.respect_ignore);
    assert!(config.include_hidden);
    assert!(config.follow_links);
    assert_eq!(config.custom_excludes, vec!["*.log"]);
}
