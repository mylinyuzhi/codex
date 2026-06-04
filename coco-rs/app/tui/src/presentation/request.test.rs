use super::*;
use coco_types::PermissionAskChoice;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::i18n::locale_test_guard;
use crate::state::PermissionDetail;
use crate::state::QuestionOption;
use crate::theme::Theme;
use coco_tui_ui::style::UiStyles;

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
        focus: QuestionFocus::Question(0),
        is_in_plan_mode: false,
    }
}

#[test]
fn question_content_renders_other_answer_buffer() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = question_prompt(QuestionItem {
        header: "Auth".to_string(),
        question: "Which auth flow?".to_string(),
        options: vec![
            QuestionOption {
                label: "OAuth".to_string(),
                description: "Use browser login".to_string(),
                preview: None,
            },
            QuestionOption {
                label: OTHER_OPTION_LABEL.to_string(),
                description: String::new(),
                preview: None,
            },
        ],
        multi_select: false,
        selected: 1,
        checked: Vec::new(),
        notes: "device code".to_string(),
        editing_notes: true,
    });

    let (title, body, border) = question_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Question ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("[Auth]"));
    assert!(body.contains("▸  Other"));
    assert!(body.contains("your answer: device code▌"));
}

#[test]
fn question_content_renders_multiselect_footer_hints() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
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
        selected: 0,
        checked: vec![0],
        notes: String::new(),
        editing_notes: false,
    });
    state.is_in_plan_mode = true;

    let (_, body, _) = question_content(&state, UiStyles::new(&theme));

    assert!(body.contains("> [x] Read"));
    assert!(body.contains("— preview —"));
    assert!(body.contains("read preview"));
    assert!(body.contains("Skip interview and plan immediately"));
    assert!(body.contains("Space: toggle"));
}

#[test]
fn question_content_clamps_negative_focus_and_selection() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
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
                selected: -3,
                checked: Vec::new(),
                notes: String::new(),
                editing_notes: false,
            },
            QuestionItem {
                header: "Second".to_string(),
                question: "Pick second".to_string(),
                options: Vec::new(),
                multi_select: false,
                selected: 0,
                checked: Vec::new(),
                notes: String::new(),
                editing_notes: false,
            },
        ],
        focus: QuestionFocus::Question(-2),
        is_in_plan_mode: false,
    };

    let (_, body, _) = question_content(&state, UiStyles::new(&theme));

    assert!(body.contains("[First] 1/2"));
    assert!(body.contains("▸  Alpha"));
    assert!(body.contains("alpha preview"));
    assert!(!body.contains("[Second]"));

    state.questions[0].selected = 99;
    let (_, body, _) = question_content(&state, UiStyles::new(&theme));
    assert!(body.contains("▸  Beta"));
}
