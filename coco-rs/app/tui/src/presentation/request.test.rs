use super::*;
use coco_types::PermissionAskChoice;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::i18n::locale_test_guard;
use crate::state::OtherInputState;
use crate::state::PermissionDetail;
use crate::state::QuestionFocusTarget;
use crate::state::QuestionItem;
use crate::state::QuestionOption;
use crate::state::QuestionPage;
use crate::state::SubmitAction;
use crate::theme::Theme;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::widgets::QuestionRow;

fn permission_prompt(detail: PermissionDetail) -> PermissionPromptState {
    let display_input = match &detail {
        PermissionDetail::Generic { input_preview } => {
            coco_types::PermissionDisplayInput::Text(input_preview.clone())
        }
        _ => coco_types::PermissionDisplayInput::Empty,
    };
    PermissionPromptState {
        request_id: "req-1".to_string(),
        tool_name: "Edit".to_string(),
        description: "Allow this operation?".to_string(),
        detail,
        risk_level: None,
        show_always_allow: false,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: None,
        selected_choice: 0,
        display_input,
        original_input: None,
        permission_suggestions: vec![],
        worker_badge: None,
    }
}

#[test]
fn permission_content_title_includes_worker_badge() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = permission_prompt(PermissionDetail::Generic {
        input_preview: "ls".to_string(),
    });
    state.worker_badge = Some(coco_types::WorkerBadge {
        name: "researcher".to_string(),
        color: coco_types::AgentColorName::Cyan,
    });
    let (title, _body, _border) = permission_content(&state, UiStyles::new(&theme));
    // The worker name is surfaced in the title so the leader sees who is
    // asking (gap 12). TS `PermissionRequestTitle.tsx:32`.
    assert!(title.contains("· @researcher"), "got title: {title}");
}

#[test]
fn permission_content_omits_badge_without_worker() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = permission_prompt(PermissionDetail::Generic {
        input_preview: "ls".to_string(),
    });
    let (title, _body, _border) = permission_content(&state, UiStyles::new(&theme));
    assert!(!title.contains('@'), "no badge expected: {title}");
}

#[test]
fn permission_content_uses_high_risk_border() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = permission_prompt(PermissionDetail::Bash {
        command: "rm -rf target/tmp".to_string(),
        risk_description: Some("Deletes files".to_string()),
        working_dir: Some("/repo".to_string()),
    });
    state.risk_level = Some(RiskLevel::High);
    state.show_always_allow = true;

    let (title, body, border) = permission_content(&state, UiStyles::new(&theme));

    assert_eq!(border, theme.error);
    assert!(title.contains("Edit"));
    assert!(body.contains("rm -rf target/tmp"));
    assert!(body.contains("/repo"));
    assert!(body.contains("Deletes files"));
    assert!(body.contains("always allow Edit for this session"));
}

#[test]
fn permission_content_renders_choices_instead_of_default_actions() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = permission_prompt(PermissionDetail::Generic {
        input_preview: "Pick an option".to_string(),
    });
    state.show_always_allow = true;
    state.selected_choice = 1;
    state.choices = Some(vec![
        PermissionAskChoice {
            value: "keep".to_string(),
            label: "Keep context".to_string(),
            description: Some("Continue with current context".to_string()),
        },
        PermissionAskChoice {
            value: "clear".to_string(),
            label: "Clear context".to_string(),
            description: Some("Start a smaller plan".to_string()),
        },
    ]);

    let (_, body, _) = permission_content(&state, UiStyles::new(&theme));

    assert!(body.contains("  Keep context"));
    assert!(body.contains("▸ Clear context"));
    assert!(body.contains("Start a smaller plan"));
    assert!(!body.contains("Always"));
}

#[test]
fn generic_permission_content_uses_display_input_not_raw_original_input() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = permission_prompt(PermissionDetail::Generic {
        input_preview: "safe display".to_string(),
    });
    state.original_input = Some(json!({"secret": "raw value"}));
    state.display_input = coco_types::PermissionDisplayInput::Json("{\"safe\":true}".to_string());

    let (_, body, _) = permission_content(&state, UiStyles::new(&theme));

    assert!(body.contains("{\"safe\":true}"));
    assert!(!body.contains("raw value"));
}

#[test]
fn permission_content_truncates_unicode_file_edit_preview() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let diff = "切".repeat(501);
    let state = permission_prompt(PermissionDetail::FileEdit {
        path: "src/lib.rs".to_string(),
        diff,
    });

    let (_, body, _) = permission_content(&state, UiStyles::new(&theme));

    assert!(body.contains("File: src/lib.rs"));
    assert!(body.contains(&format!("{}...", "切".repeat(500))));
}

fn question_prompt(question: QuestionItem) -> QuestionPromptState {
    QuestionPromptState {
        request_id: "req-1".to_string(),
        original_input: json!({"source": "test"}),
        questions: vec![question],
        current_question: QuestionPage::Question(0),
        focus_target: QuestionFocusTarget::QuestionOption(0),
        is_in_plan_mode: false,
    }
}

fn choice_mark(row: &QuestionRow) -> &RowMark {
    let QuestionRow::Choice(choice) = row else {
        panic!("expected choice row");
    };
    &choice.mark
}

fn action_label(row: &QuestionRow) -> &str {
    let QuestionRow::Action(action) = row else {
        panic!("expected action row");
    };
    &action.label
}

#[test]
fn project_question_exposes_other_composer_buffer() {
    let _locale = locale_test_guard("en");
    let state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![QuestionOption {
            label: "OAuth".to_string(),
            description: "Use browser login".to_string(),
            preview: None,
        }],
        multi_select: false,
        selected: None,
        checked: Vec::new(),
        other_input: OtherInputState {
            focused: true,
            value: "device code".to_string(),
            committed: false,
        },
    });
    let mut state = state;
    state.focus_target = QuestionFocusTarget::OtherInput;

    let view = project_question(&state);

    assert_eq!(view.header.title, " Question ");
    assert_eq!(view.header.chip.as_deref(), Some("Auth"));
    assert_eq!(view.rows.len(), 2);
    let QuestionRow::Input(input) = &view.rows[1] else {
        panic!("expected trailing input row");
    };
    assert_eq!(input.number, 2);
    assert_eq!(input.value, "device code");
    assert!(!input.selected);
    assert!(input.focused);
}

#[test]
fn project_question_initial_single_select_has_focus_but_no_committed_check() {
    let _locale = locale_test_guard("en");
    let state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![
            QuestionOption {
                label: "OAuth".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "Device".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: false,
        selected: None,
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    });

    let view = project_question(&state);

    assert!(matches!(
        choice_mark(&view.rows[0]),
        RowMark::Radio {
            selected: false,
            focused: true
        }
    ));
}

#[test]
fn project_question_committed_single_select_shows_check_separate_from_focus() {
    let _locale = locale_test_guard("en");
    let mut state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![
            QuestionOption {
                label: "OAuth".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "Device".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: false,
        selected: Some(0),
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    });
    state.focus_target = QuestionFocusTarget::QuestionOption(1);

    let view = project_question(&state);

    assert!(matches!(
        choice_mark(&view.rows[0]),
        RowMark::Radio {
            selected: true,
            focused: false
        }
    ));
    assert!(matches!(
        choice_mark(&view.rows[1]),
        RowMark::Radio {
            selected: false,
            focused: true
        }
    ));
}

#[test]
fn project_question_uncommitted_free_text_value_has_no_check() {
    let _locale = locale_test_guard("en");
    let mut state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![QuestionOption {
            label: "OAuth".to_string(),
            description: String::new(),
            preview: None,
        }],
        multi_select: false,
        selected: Some(0),
        checked: Vec::new(),
        other_input: OtherInputState {
            focused: true,
            value: "device code".to_string(),
            committed: false,
        },
    });
    state.focus_target = QuestionFocusTarget::OtherInput;

    let view = project_question(&state);

    let QuestionRow::Input(input) = &view.rows[1] else {
        panic!("expected free-text row");
    };
    assert!(!input.selected, "typing alone must not show a check");
}

#[test]
fn project_question_committed_free_text_value_shows_check() {
    let _locale = locale_test_guard("en");
    let mut state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![QuestionOption {
            label: "OAuth".to_string(),
            description: String::new(),
            preview: None,
        }],
        multi_select: false,
        selected: None,
        checked: Vec::new(),
        other_input: OtherInputState {
            focused: true,
            value: "device code".to_string(),
            committed: true,
        },
    });
    state.focus_target = QuestionFocusTarget::OtherInput;

    let view = project_question(&state);

    let QuestionRow::Input(input) = &view.rows[1] else {
        panic!("expected free-text row");
    };
    assert!(
        input.selected,
        "Enter-confirmed free text should show a check"
    );
}

#[test]
fn project_question_truncates_long_header_chip_to_12() {
    let _locale = locale_test_guard("en");
    let state = question_prompt(QuestionItem {
        header: "Authentication method".to_string(),
        question: "Which?".to_string(),
        options: vec![
            QuestionOption {
                label: "A".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "B".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: false,
        selected: None,
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    });

    let view = project_question(&state);
    // TS `ASK_USER_QUESTION_TOOL_CHIP_WIDTH` = 12: 11 chars + ellipsis.
    assert_eq!(view.header.chip.as_deref(), Some("Authenticat…"));
    assert_eq!(view.header.chip.as_deref().unwrap().chars().count(), 12);
}

#[test]
fn project_question_multiselect_footer_and_hints() {
    let _locale = locale_test_guard("en");
    let mut state = question_prompt(QuestionItem {
        header: "Tools".to_string(),
        question: "Pick tools".to_string(),
        options: vec![
            QuestionOption {
                label: "Read".to_string(),
                description: String::new(),
                preview: Some("read preview".to_string()),
            },
            QuestionOption {
                label: "Write".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: true,
        selected: None,
        checked: vec![0],
        other_input: OtherInputState::default(),
    });
    state.is_in_plan_mode = true;

    let view = project_question(&state);

    assert!(matches!(
        choice_mark(&view.rows[0]),
        RowMark::Check {
            checked: true,
            focused: true
        }
    ));
    assert_eq!(view.preview.as_deref(), Some("read preview"));
    // Plan mode adds the Skip-interview footer action.
    assert_eq!(view.footer_actions.len(), 2);
    assert!(
        view.footer_actions
            .iter()
            .any(|f| f.label.contains("Skip interview"))
    );
    assert!(view.hints.contains("Space to toggle"));
}

#[test]
fn project_question_nav_answered_reflects_each_question() {
    let _locale = locale_test_guard("en");
    let q = |header: &str, multi: bool| QuestionItem {
        header: header.to_string(),
        question: format!("{header}?"),
        options: vec![
            QuestionOption {
                label: "A".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "B".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: multi,
        selected: None,
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    };
    let state = QuestionPromptState {
        request_id: "r".to_string(),
        original_input: json!({}),
        // Q1 single-select starts uncommitted → unanswered; Q2
        // multi-select with nothing checked → unanswered.
        questions: vec![q("One", false), q("Two", true)],
        current_question: QuestionPage::Question(0),
        focus_target: QuestionFocusTarget::QuestionOption(0),
        is_in_plan_mode: false,
    };
    let nav = project_question(&state)
        .header
        .nav
        .expect("multi-question nav strip");
    assert!(!nav.tabs[0].answered, "single-select starts unanswered");
    assert!(!nav.tabs[1].answered, "empty multi-select is unanswered");
    // The Submit tab is present; not yet ready (Q2 unanswered).
    let submit = nav.submit.expect("submit tab present with >1 question");
    assert!(!submit.ready, "ready only when every question is answered");
}

#[test]
fn project_question_submit_focus_shows_review_body_and_ready_tab() {
    let _locale = locale_test_guard("en");
    let q = |header: &str| QuestionItem {
        header: header.to_string(),
        question: format!("{header}?"),
        options: vec![
            QuestionOption {
                label: "A".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "B".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: false,
        selected: Some(0),
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    };
    let state = QuestionPromptState {
        request_id: "r".to_string(),
        original_input: json!({}),
        // Both single-select → pre-answered with their first option ("A").
        questions: vec![q("One"), q("Two")],
        current_question: QuestionPage::Submit,
        focus_target: QuestionFocusTarget::SubmitAction(SubmitAction::SubmitAnswers),
        is_in_plan_mode: false,
    };
    let view = project_question(&state);
    let submit = view
        .header
        .nav
        .expect("nav strip")
        .submit
        .expect("submit tab");
    assert!(submit.focused, "Submit tab is focused");
    assert!(submit.ready, "all questions answered → ✔");
    // Body = review of every answer + the "Ready to submit?" prompt.
    assert!(
        view.submit_review
            .as_ref()
            .is_some_and(|review| review.contains("Review your answers")),
        "review header: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    assert!(
        view.submit_review
            .as_ref()
            .is_some_and(|review| review.contains("One?") && review.contains("Two?")),
        "lists every question: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    assert!(
        view.submit_review
            .as_ref()
            .is_some_and(|review| review.contains("→ A")),
        "shows answers: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    assert!(
        view.submit_review
            .as_ref()
            .is_some_and(|review| review.contains("Ready to submit")),
        "submit prompt: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    assert!(
        !view
            .submit_review
            .as_ref()
            .is_some_and(|review| review.contains("not answered all")),
        "no warning when all answered: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    // Rows are the Submit / Cancel confirmation list.
    assert_eq!(view.rows.len(), 2, "Submit/Cancel rows");
    assert_eq!(action_label(&view.rows[0]), "Submit answers");
    assert_eq!(action_label(&view.rows[1]), "Cancel");
}

#[test]
fn project_question_submit_focus_warns_when_unanswered() {
    let _locale = locale_test_guard("en");
    let q = |header: &str, multi: bool| QuestionItem {
        header: header.to_string(),
        question: format!("{header}?"),
        options: vec![
            QuestionOption {
                label: "A".to_string(),
                description: String::new(),
                preview: None,
            },
            QuestionOption {
                label: "B".to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: multi,
        selected: Some(0),
        checked: Vec::new(),
        other_input: OtherInputState::default(),
    };
    let state = QuestionPromptState {
        request_id: "r".to_string(),
        original_input: json!({}),
        // Q2 multi-select with nothing checked → unanswered.
        questions: vec![q("One", false), q("Two", true)],
        current_question: QuestionPage::Submit,
        focus_target: QuestionFocusTarget::SubmitAction(SubmitAction::SubmitAnswers),
        is_in_plan_mode: false,
    };
    let view = project_question(&state);
    assert!(
        view.submit_review
            .as_ref()
            .is_some_and(|review| review.contains("⚠ You have not answered all questions")),
        "warning shown: {}",
        view.submit_review.as_deref().unwrap_or("")
    );
    assert!(
        !view.header.nav.unwrap().submit.unwrap().ready,
        "Submit tab not ready when a question is unanswered"
    );
}

#[test]
fn project_question_clamps_negative_focus_and_selection() {
    let _locale = locale_test_guard("en");
    let mut state = QuestionPromptState {
        request_id: "req-1".to_string(),
        original_input: json!({"source": "test"}),
        questions: vec![
            QuestionItem {
                header: "First".to_string(),
                question: "Pick first".to_string(),
                options: vec![
                    QuestionOption {
                        label: "Alpha".to_string(),
                        description: String::new(),
                        preview: Some("alpha preview".to_string()),
                    },
                    QuestionOption {
                        label: "Beta".to_string(),
                        description: String::new(),
                        preview: None,
                    },
                ],
                multi_select: false,
                selected: Some(0),
                checked: Vec::new(),
                other_input: OtherInputState::default(),
            },
            QuestionItem {
                header: "Second".to_string(),
                question: "Pick second".to_string(),
                options: Vec::new(),
                multi_select: false,
                selected: None,
                checked: Vec::new(),
                other_input: OtherInputState::default(),
            },
        ],
        current_question: QuestionPage::Question(99),
        focus_target: QuestionFocusTarget::QuestionOption(0),
        is_in_plan_mode: false,
    };

    let view = project_question(&state);

    // >1 question → the nav strip (not a bare chip) carries every header,
    // current clamped to question 0.
    assert_eq!(view.header.chip, None);
    let nav = view.header.nav.as_ref().expect("multi-question nav strip");
    assert_eq!(nav.current, 1);
    assert_eq!(nav.tabs.len(), 2);
    assert_eq!(nav.tabs[0].header, "First");
    assert_eq!(nav.tabs[1].header, "Second");
    assert!(matches!(&view.rows[0], QuestionRow::Input(_)));
    assert_eq!(view.preview, None);

    state.current_question = QuestionPage::Question(0);
    state.focus_target = QuestionFocusTarget::QuestionOption(1);
    state.questions[0].selected = Some(1);
    let view = project_question(&state);
    // selected option 1 is focused.
    assert!(matches!(
        choice_mark(&view.rows[1]),
        RowMark::Radio {
            selected: true,
            focused: true
        }
    ));
}
