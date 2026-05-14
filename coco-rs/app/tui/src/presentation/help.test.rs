use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::set_locale;

#[test]
fn help_content_uses_live_bindings_and_static_fallbacks() {
    set_locale("en");
    let state = AppState::default();
    let theme = Theme::default();

    let (title, body, border) = help_content(&state, &theme);

    assert_eq!(title, " Help ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("tab            Toggle plan mode"));
    assert!(body.contains("ctrl+t         Cycle thinking level"));
    assert!(body.contains("!cmd           Run a shell command inline"));
    assert!(body.contains("@path          Autocomplete a file path"));
}
