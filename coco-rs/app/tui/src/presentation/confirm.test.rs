use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::locale_test_guard;
use crate::state::CostWarningPromptState;
use crate::state::FeedbackState;
use crate::state::PlanApprovalPromptState;
use crate::state::PluginHintState;
use crate::state::TaskDetailState;
use crate::theme::Theme;
use coco_tui_ui::style::UiStyles;

#[test]
fn cost_warning_content_formats_cents() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = CostWarningPromptState {
        current_cost_cents: 123,
        threshold_cents: 456,
    };

    let (title, body, border) = cost_warning_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Cost Warning ");
    assert_eq!(border, theme.warning);
    assert!(body.contains("Current cost: $1.23"));
    assert!(body.contains("Threshold: $4.56"));
}

#[test]
fn task_detail_content_applies_scroll_window() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = TaskDetailState {
        task_id: "task-1".to_string(),
        task_type: "build".to_string(),
        description: "Build output".to_string(),
        output: (0..25)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        status: "running".to_string(),
        scroll: 3,
    };

    let (title, body, border) = task_detail_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Task: build [4/25] ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("Build output"));
    assert!(body.contains("line-3"));
    assert!(body.contains("line-22"));
    assert!(!body.lines().any(|line| line == "line-2"));
    assert!(!body.lines().any(|line| line == "line-23"));
}

fn running_shell(id: &str, cmd: &str) -> crate::state::session::TaskEntry {
    crate::state::session::TaskEntry {
        task_id: id.to_string(),
        description: cmd.to_string(),
        status: crate::state::session::TaskEntryStatus::Running,
        kind: crate::state::session::TaskEntryKind::Shell,
        started_at_ms: 0,
    }
}

#[test]
fn background_tasks_list_marks_selection_and_pill() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = crate::state::AppState::default();
    state.session.active_tasks = vec![
        running_shell("s0", "sleep 100"),
        running_shell("s1", "tail -f log"),
    ];
    let bt = crate::state::BackgroundTasksState {
        selected: 1,
        detail: None,
    };

    let (title, body, _) = background_tasks_content(&bt, &state, 0, UiStyles::new(&theme));

    assert_eq!(title, " Background tasks ");
    assert!(body.contains("2 shells"));
    assert!(body.contains("❯ tail -f log (running)"));
    assert!(body.contains("  sleep 100 (running)"));
    assert!(body.contains("Enter to view"));
}

#[test]
fn background_tasks_detail_shows_status_runtime_command_and_output() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = crate::state::AppState::default();
    state.session.active_tasks = vec![running_shell("s1", "sleep 100")];
    let bt = crate::state::BackgroundTasksState {
        selected: 0,
        detail: Some("s1".to_string()),
    };

    // 1h 19m 32s elapsed since started_at_ms = 0.
    let now_ms = (3600 + 19 * 60 + 32) * 1000;
    let (title, body, _) = background_tasks_content(&bt, &state, now_ms, UiStyles::new(&theme));

    assert_eq!(title, " Shell details ");
    assert!(body.contains("Status:   running"));
    assert!(body.contains("Runtime:  1h 19m 32s"));
    assert!(body.contains("Command:  sleep 100"));
    assert!(body.contains("No output available"));
    assert!(body.contains("to go back"));
}

#[test]
fn task_detail_content_clamps_negative_scroll_to_start() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = TaskDetailState {
        task_id: "task-1".to_string(),
        task_type: "build".to_string(),
        description: "Build output".to_string(),
        output: (0..25)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        status: "running".to_string(),
        scroll: -5,
    };

    let (_, body, _) = task_detail_content(&state, UiStyles::new(&theme));

    assert!(body.contains("line-0"));
    assert!(body.contains("line-19"));
    assert!(!body.lines().any(|line| line == "line-20"));
}

#[test]
fn task_detail_content_clamps_past_end_and_shows_position() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = TaskDetailState {
        task_id: "task-1".to_string(),
        task_type: "build".to_string(),
        description: "Build output".to_string(),
        output: (0..3)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        status: "running".to_string(),
        scroll: 10,
    };

    let (title, body, _) = task_detail_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Task: build [4/3] ");
    assert!(!body.lines().any(|line| line == "line-0"));
    assert!(body.contains("↑/↓ Scroll"));
}

#[test]
fn plan_approval_content_truncates_long_preview_and_marks_focus() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = PlanApprovalPromptState::new(
        "req-1".to_string(),
        "planner".to_string(),
        Some(".coco/plans/demo.md".to_string()),
        (0..20)
            .map(|i| format!("step {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    state.toggle_focus();

    let (title, body, border) = plan_approval_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Plan approval — from planner ");
    assert_eq!(border, theme.plan_mode);
    assert!(body.contains("Plan file: .coco/plans/demo.md"));
    assert!(body.contains("step 17"));
    assert!(!body.contains("step 18"));
    assert!(body.contains("plan truncated"));
    assert!(body.contains("  Approve    ▸ Deny"));
}

#[test]
fn feedback_content_marks_selected_option() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = FeedbackState {
        prompt: "How was it?".to_string(),
        options: vec!["Good".to_string(), "Needs work".to_string()],
        selected: 1,
    };

    let (title, body, border) = feedback_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Feedback ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("How was it?"));
    assert!(body.contains("  Good"));
    assert!(body.contains("▸ Needs work"));
}

#[test]
fn plugin_hint_content_renders_recommendation_and_selection() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = PluginHintState {
        plugin_id: "foo@anthropic-plugins".to_string(),
        plugin_name: "foo".to_string(),
        marketplace_name: "anthropic-plugins".to_string(),
        plugin_description: Some("A foo plugin".to_string()),
        source_command: "mytool".to_string(),
        selected: 0,
    };

    let (title, body, border) = plugin_hint_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Plugin Recommendation ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("mytool"));
    assert!(body.contains("foo"));
    assert!(body.contains("anthropic-plugins"));
    assert!(body.contains("A foo plugin"));
    // Selection marker on the install option.
    assert!(body.contains("▸ Yes, install foo"));
    assert!(body.contains("  No"));
    assert!(body.contains("don't show plugin installation hints again"));
}

#[test]
fn plugin_hint_content_omits_missing_description() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = PluginHintState {
        plugin_id: "bar@anthropic-plugins".to_string(),
        plugin_name: "bar".to_string(),
        marketplace_name: "anthropic-plugins".to_string(),
        plugin_description: None,
        source_command: "cli".to_string(),
        selected: 2,
    };

    let (_, body, _) = plugin_hint_content(&state, UiStyles::new(&theme));

    // Disable option is selected.
    assert!(body.contains("▸ No, and don't show plugin installation hints again"));
    assert_eq!(
        state.selected_response(),
        crate::state::PluginHintResponse::Disable
    );
}
