use super::*;
use coco_types::PermissionAskChoice;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::i18n::set_locale;
use crate::state::PermissionDetail;
use crate::state::QuestionOption;
use crate::theme::Theme;

fn permission_overlay(detail: PermissionDetail) -> PermissionOverlay {
    PermissionOverlay {
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
        original_input: None,
    }
}

#[test]
fn permission_content_uses_high_risk_border() {
    set_locale("en");
    let theme = Theme::default();
    let mut overlay = permission_overlay(PermissionDetail::Bash {
        command: "rm -rf target/tmp".to_string(),
        risk_description: Some("Deletes files".to_string()),
        working_dir: Some("/repo".to_string()),
    });
    overlay.risk_level = Some(RiskLevel::High);
    overlay.show_always_allow = true;

    let (title, body, border) = permission_content(&overlay, &theme);

    assert_eq!(border, theme.error);
    assert!(title.contains("Edit"));
    assert!(body.contains("rm -rf target/tmp"));
    assert!(body.contains("/repo"));
    assert!(body.contains("Deletes files"));
    assert!(body.contains("Always"));
}

#[test]
fn permission_content_renders_choices_instead_of_default_actions() {
    set_locale("en");
    let theme = Theme::default();
    let mut overlay = permission_overlay(PermissionDetail::Generic {
        input_preview: "Pick an option".to_string(),
    });
    overlay.show_always_allow = true;
    overlay.selected_choice = 1;
    overlay.choices = Some(vec![
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

    let (_, body, _) = permission_content(&overlay, &theme);

    assert!(body.contains("  Keep context"));
    assert!(body.contains("▸ Clear context"));
    assert!(body.contains("Start a smaller plan"));
    assert!(!body.contains("Always"));
}

#[test]
fn permission_content_truncates_unicode_file_edit_preview() {
    set_locale("en");
    let theme = Theme::default();
    let diff = "切".repeat(501);
    let overlay = permission_overlay(PermissionDetail::FileEdit {
        path: "src/lib.rs".to_string(),
        diff,
    });

    let (_, body, _) = permission_content(&overlay, &theme);

    assert!(body.contains("File: src/lib.rs"));
    assert!(body.contains(&format!("{}...", "切".repeat(500))));
}

fn question_overlay(question: QuestionItem) -> QuestionOverlay {
    QuestionOverlay {
        request_id: "req-1".to_string(),
        original_input: json!({"source": "test"}),
        questions: vec![question],
        focus: QuestionFocus::Question(0),
        is_in_plan_mode: false,
    }
}

#[test]
fn question_content_renders_other_answer_buffer() {
    set_locale("en");
    let theme = Theme::default();
    let overlay = question_overlay(QuestionItem {
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

    let (title, body, border) = question_content(&overlay, &theme);

    assert_eq!(title, " Question ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("[Auth]"));
    assert!(body.contains("▸  Other"));
    assert!(body.contains("your answer: device code▌"));
}

#[test]
fn question_content_renders_multiselect_footer_hints() {
    set_locale("en");
    let theme = Theme::default();
    let mut overlay = question_overlay(QuestionItem {
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
    overlay.is_in_plan_mode = true;

    let (_, body, _) = question_content(&overlay, &theme);

    assert!(body.contains("> [x] Read"));
    assert!(body.contains("— preview —"));
    assert!(body.contains("read preview"));
    assert!(body.contains("Skip interview and plan immediately"));
    assert!(body.contains("Space: toggle"));
}

#[test]
fn question_content_clamps_negative_focus_and_selection() {
    set_locale("en");
    let theme = Theme::default();
    let mut overlay = QuestionOverlay {
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

    let (_, body, _) = question_content(&overlay, &theme);

    assert!(body.contains("[First] 1/2"));
    assert!(body.contains("▸  Alpha"));
    assert!(body.contains("alpha preview"));
    assert!(!body.contains("[Second]"));

    overlay.questions[0].selected = 99;
    let (_, body, _) = question_content(&overlay, &theme);
    assert!(body.contains("▸  Beta"));
}
