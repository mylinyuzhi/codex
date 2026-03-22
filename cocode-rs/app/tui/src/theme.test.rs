use super::*;

#[test]
fn test_theme_name_roundtrip() {
    for name in ThemeName::all() {
        let s = name.as_str();
        let parsed = ThemeName::from_str(s);
        assert_eq!(parsed, Some(*name));
    }
}

#[test]
fn test_theme_name_case_insensitive() {
    assert_eq!(ThemeName::from_str("DARK"), Some(ThemeName::Dark));
    assert_eq!(ThemeName::from_str("Dracula"), Some(ThemeName::Dracula));
    assert_eq!(ThemeName::from_str("NORD"), Some(ThemeName::Nord));
}

#[test]
fn test_theme_by_name() {
    for name in ThemeName::all() {
        let theme = Theme::by_name(*name);
        assert_eq!(theme.name, *name);
    }
}

#[test]
fn test_default_theme() {
    let theme = Theme::default();
    assert_eq!(theme.name, ThemeName::Default);
}
