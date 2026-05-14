use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::set_locale;
use crate::state::DiffViewOverlay;
use crate::theme::Theme;

#[test]
fn diff_view_content_formats_diff_lines_and_clamps_negative_scroll() {
    set_locale("en");
    let theme = Theme::default();
    let overlay = DiffViewOverlay {
        path: "src/lib.rs".to_string(),
        diff: "\
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,2 @@
-old
+new
 context"
            .to_string(),
        scroll: -4,
    };

    let (title, body, border) = diff_view_content(&overlay, &theme);

    assert_eq!(title, " Diff: src/lib.rs [1/6] ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("    --- a/src/lib.rs"));
    assert!(body.contains("    +++ b/src/lib.rs"));
    assert!(body.contains("  @@ -1,2 +1,2 @@"));
    assert!(body.contains("  - old"));
    assert!(body.contains("  + new"));
}

#[test]
fn diff_view_content_scrolls_and_caps_to_thirty_lines() {
    set_locale("en");
    let theme = Theme::default();
    let overlay = DiffViewOverlay {
        path: "src/lib.rs".to_string(),
        diff: (0..35)
            .map(|i| format!(" line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        scroll: 3,
    };

    let (title, body, _) = diff_view_content(&overlay, &theme);

    assert_eq!(title, " Diff: src/lib.rs [4/35] ");
    assert!(body.lines().any(|line| line == "    line-3"));
    assert!(body.lines().any(|line| line == "    line-32"));
    assert!(!body.lines().any(|line| line == "    line-2"));
    assert!(!body.lines().any(|line| line == "    line-33"));
}

#[test]
fn context_viz_content_caps_bar_when_usage_exceeds_total() {
    set_locale("en");
    let theme = Theme::default();
    let mut state = AppState::default();
    state.session.context_window_used = 150;
    state.session.context_window_total = 100;
    state.session.token_usage.input_tokens = 42;
    state.session.token_usage.output_tokens = 8;
    state.session.token_usage.cache_read_tokens = 5;

    let (title, body, border) = context_viz_content(&state, &theme);

    assert_eq!(title, " Context Window ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("[████████████████████████████████████████] 150%"));
    assert!(body.contains("Input:  42"));
    assert!(body.contains("Output: 8"));
    assert!(body.contains("Cache:  5"));
    assert!(body.contains("Used: 150 / 100"));
}
