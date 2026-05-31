use super::*;
use pretty_assertions::assert_eq;

use std::sync::Arc;

use coco_messages::AssistantContent;
use coco_messages::TextContent;
use coco_messages::create_assistant_message;
use coco_types::ModelRole;

use crate::i18n::locale_test_guard;
use crate::state::DiffViewState;
use crate::theme::Theme;
use coco_tui_ui::style::UiStyles;

#[test]
fn diff_view_content_formats_diff_lines_and_clamps_negative_scroll() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = DiffViewState {
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

    let (title, body, border) = diff_view_content(&state, UiStyles::new(&theme));

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
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = DiffViewState {
        path: "src/lib.rs".to_string(),
        diff: (0..35)
            .map(|i| format!(" line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        scroll: 3,
    };

    let (title, body, _) = diff_view_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Diff: src/lib.rs [4/35] ");
    assert!(body.lines().any(|line| line == "    line-3"));
    assert!(body.lines().any(|line| line == "    line-32"));
    assert!(!body.lines().any(|line| line == "    line-2"));
    assert!(!body.lines().any(|line| line == "    line-33"));
}

#[test]
fn context_viz_content_caps_bar_when_usage_exceeds_total() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = AppState::default();
    state.session.model_by_role.insert(
        ModelRole::Main,
        crate::state::ModelBinding {
            provider: "openai".into(),
            model_id: "gpt-5.2".into(),
            context_window: Some(100),
            effort: None,
        },
    );
    state
        .session
        .transcript
        .on_message_appended(Arc::new(create_assistant_message(
            vec![AssistantContent::Text(TextContent {
                text: "done".into(),
                provider_metadata: None,
            })],
            "gpt-5.2",
            coco_types::TokenUsage {
                input_tokens: coco_types::InputTokens {
                    total: 140,
                    ..Default::default()
                },
                output_tokens: coco_types::OutputTokens {
                    total: 10,
                    ..Default::default()
                },
            },
        )));
    state.session.token_usage.input_tokens = 42;
    state.session.token_usage.output_tokens = 8;
    state.session.token_usage.cache_read_tokens = 5;

    let (title, body, border) = context_viz_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Context Window ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("[████████████████████████████████████████] 100%"));
    assert!(body.contains("Input:  42"));
    assert!(body.contains("Output: 8"));
    assert!(body.contains("Cache:  5"));
    assert!(body.contains("Used: 150 / 100"));
}
