use super::*;

use crate::i18n::locale_test_guard;
use crate::state::AppState;

#[test]
fn footer_view_renders_model_tokens_context_and_messages() {
    let _locale = locale_test_guard("en");
    let mut state = AppState::default();
    state.session.provider = "openai".into();
    state.session.model = "gpt-5.2".into();
    state.session.token_usage.input_tokens = 1_500;
    state.session.token_usage.output_tokens = 250;
    state.session.token_usage.cache_read_tokens = 750;
    state.session.context_window_used = 80;
    state.session.context_window_total = 100;

    let FooterView::Status { spans } = footer_view(&state) else {
        panic!("expected status footer");
    };
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();

    assert!(text.contains(" openai/gpt-5.2"));
    assert!(text.contains("↑1.5K ↓250"));
    assert!(text.contains("cache 50%"));
    assert!(text.contains("ctx 80%"));
    assert!(text.contains("0 msgs"));
    assert!(
        spans
            .iter()
            .any(|span| span.text == "ctx 80%" && span.tone == FooterTone::Warning)
    );
}
