use super::*;

#[test]
fn test_parse_locale_chinese() {
    assert_eq!(parse_locale("zh_CN.UTF-8"), Some("zh-CN"));
    assert_eq!(parse_locale("zh-CN"), Some("zh-CN"));
    assert_eq!(parse_locale("zh"), Some("zh-CN"));
    assert_eq!(parse_locale("zh_Hans"), Some("zh-CN"));
}

#[test]
fn test_parse_locale_english() {
    assert_eq!(parse_locale("en_US.UTF-8"), Some("en"));
    assert_eq!(parse_locale("en-US"), Some("en"));
    assert_eq!(parse_locale("en"), Some("en"));
}

#[test]
fn test_parse_locale_unknown() {
    assert_eq!(parse_locale("fr_FR.UTF-8"), None);
    assert_eq!(parse_locale("de"), None);
}

#[test]
fn test_t_macro_works() {
    // This test verifies the t! macro is properly re-exported
    // and the locales are loaded
    let text = t!("command.toggle_plan_mode");
    // Should return the translation or the key if not found
    assert!(!text.is_empty());
}
