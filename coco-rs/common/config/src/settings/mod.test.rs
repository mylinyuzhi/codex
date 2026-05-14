use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

#[test]
fn test_parse_settings_accepts_jsonc_comments_and_trailing_commas() {
    let settings = parse_settings(
        r#"{
            // JSONC is accepted in settings.json-shaped content.
            language: "zh-CN",
            "features": {
                "web_search": true,
            },
        }"#,
    )
    .expect("parse JSONC settings");

    assert_eq!(settings.language.as_deref(), Some("zh-CN"));
    assert_eq!(settings.features.get("web_search"), Some(&true));
}

#[test]
fn test_load_settings_with_accepts_jsonc_layers() {
    let tmp = TempDir::new().expect("tempdir");
    let cwd = tmp.path().join("project");
    std::fs::create_dir_all(cwd.join(".claude")).expect("project settings dir");

    let user_path = tmp.path().join("settings.json");
    let managed_path = tmp.path().join("managed-settings.json");
    let flag_path = tmp.path().join("flag-settings.json");

    std::fs::write(
        &user_path,
        r#"{
            "language": "en",
            "features": {
                "web_search": true,
            },
        }"#,
    )
    .expect("write user settings");
    std::fs::write(
        cwd.join(".claude/settings.json"),
        r#"{
            // Project settings can also use comments.
            "output_style": "project",
        }"#,
    )
    .expect("write project settings");
    std::fs::write(
        &flag_path,
        r#"{
            "language": "fr",
        }"#,
    )
    .expect("write flag settings");
    std::fs::write(
        &managed_path,
        r#"{
            "features": {
                "web_fetch": true,
            },
        }"#,
    )
    .expect("write managed settings");

    let settings = load_settings_with(&cwd, Some(&flag_path), &user_path, &managed_path)
        .expect("load JSONC settings");

    assert_eq!(settings.merged.language.as_deref(), Some("fr"));
    assert_eq!(settings.merged.output_style.as_deref(), Some("project"));
    assert_eq!(settings.merged.features.get("web_search"), Some(&true));
    assert_eq!(settings.merged.features.get("web_fetch"), Some(&true));
    assert!(settings.per_source.contains_key(&SettingSource::User));
    assert!(settings.per_source.contains_key(&SettingSource::Project));
    assert!(settings.per_source.contains_key(&SettingSource::Flag));
    assert!(settings.per_source.contains_key(&SettingSource::Policy));
}
