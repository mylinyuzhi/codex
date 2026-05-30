use std::sync::Arc;

use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;
use coco_tui_ui::theme::ThemeName;

use super::highlight_code;

#[test]
fn highlights_known_language() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let out = highlight_code(
        "fn main() {}\n",
        "rust",
        styles,
        SyntaxHighlighting::Enabled,
    );
    let lines = out.expect("rust is a known grammar");
    assert!(!lines.is_empty());
    // The first line carries the `fn` keyword as a styled span.
    let text: String = lines[0].iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("fn"), "expected keyword in {text:?}");
}

#[test]
fn unknown_language_falls_back_to_none() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    assert!(
        highlight_code(
            "some text\n",
            "definitely-not-a-language",
            styles,
            SyntaxHighlighting::Enabled,
        )
        .is_none()
    );
}

#[test]
fn disabled_highlighting_returns_none() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    assert!(
        highlight_code(
            "fn main() {}\n",
            "rust",
            styles,
            SyntaxHighlighting::Disabled
        )
        .is_none()
    );
}

#[test]
fn cache_hit_returns_ptr_equal_arc() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    // Unique content so no sibling test populated this key first.
    let code = "fn cache_hit_returns_ptr_equal_arc() {}\n";
    let a = highlight_code(code, "rust", styles, SyntaxHighlighting::Enabled).expect("a");
    let b = highlight_code(code, "rust", styles, SyntaxHighlighting::Enabled).expect("b");
    assert!(
        Arc::ptr_eq(&a, &b),
        "second call must reuse the cached Arc (refcount bump, no re-tokenize)"
    );
}

#[test]
fn cache_key_includes_theme() {
    // Same code + language, different theme. If the key ignored the theme, the
    // second call would HIT the first theme's entry and return a ptr-equal Arc;
    // asserting non-equality proves the theme is part of the key.
    let code = "fn cache_key_includes_theme() {}\n";
    let t1 = Theme::from_name(ThemeName::Default);
    let t2 = Theme::from_name(ThemeName::Dracula);
    let a = highlight_code(
        code,
        "rust",
        UiStyles::new(&t1),
        SyntaxHighlighting::Enabled,
    )
    .expect("a");
    let b = highlight_code(
        code,
        "rust",
        UiStyles::new(&t2),
        SyntaxHighlighting::Enabled,
    )
    .expect("b");
    assert!(
        !Arc::ptr_eq(&a, &b),
        "a different theme must not reuse another theme's cached highlight"
    );
}

#[test]
fn cache_key_includes_code() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let a = highlight_code(
        "fn aaa_distinct() {}\n",
        "rust",
        styles,
        SyntaxHighlighting::Enabled,
    )
    .expect("a");
    let b = highlight_code(
        "fn bbb_distinct() {}\n",
        "rust",
        styles,
        SyntaxHighlighting::Enabled,
    )
    .expect("b");
    assert!(
        !Arc::ptr_eq(&a, &b),
        "distinct code must be a distinct cache entry"
    );
}
