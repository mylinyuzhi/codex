//! Tests for keybinding bridge.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyEventState;
use crossterm::event::KeyModifiers;

use crate::events::TuiCommand;
use crate::keybinding_bridge::KeybindingContext;
use crate::keybinding_bridge::active_context;
use crate::keybinding_bridge::map_key;
use crate::keybinding_bridge::should_log_key_command;
use crate::state::AppState;
use crate::state::PanePromptState;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl_shift(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn model_picker_state() -> AppState {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::ModelPicker(
        crate::state::ModelPickerState {
            role: coco_types::ModelRole::Main,
            entries: vec![crate::state::ModelEntry {
                provider: "openai".into(),
                provider_display: "OpenAI".into(),
                model_id: "gpt-5-5".into(),
                display_name: "GPT-5.5".into(),
                context_window: Some(272_000),
                supported_efforts: vec![coco_types::ReasoningEffort::Auto],
                default_effort: Some(coco_types::ReasoningEffort::Auto),
                is_current_for_role: true,
                unavailable_reasons: Vec::new(),
            }],
            filter: String::new(),
            selected: 0,
            effort: Some(coco_types::ReasoningEffort::Auto),
        },
    ));
    state
}

fn copy_picker_state() -> AppState {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::CopyPicker(
        crate::state::CopyPickerState {
            full_text: "full".into(),
            code_blocks: vec![crate::state::CopyPickerCodeBlock {
                code: "code".into(),
                lang: Some("rust".into()),
            }],
            message_age: 0,
            selected: crate::state::CopyPickerSelection::Full,
        },
    ));
    state
}

fn install_permission_prompt(state: &mut AppState) {
    state.ui.push_prompt(PanePromptState::Permission(
        crate::state::PermissionPromptState {
            request_id: "r1".into(),
            tool_name: "Bash".into(),
            description: "run".into(),
            detail: crate::state::PermissionDetail::Generic {
                input_preview: "ls".into(),
            },
            risk_level: None,
            show_always_allow: true,
            classifier_checking: false,
            classifier_auto_approved: None,
            choices: None,
            selected_choice: 0,
            display_input: coco_types::PermissionDisplayInput::Command("ls".into()),
            original_input: None,
            permission_suggestions: vec![],
        },
    ));
}

#[test]
fn test_default_context_is_chat() {
    let state = AppState::new();
    assert_eq!(active_context(&state), KeybindingContext::Chat);
}

#[test]
fn test_help_modal_context() {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Help);
    assert_eq!(active_context(&state), KeybindingContext::Scrollable);
}

#[test]
fn test_transcript_modal_context() {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Transcript(
        crate::state::transcript::TranscriptState::new(),
    ));
    assert_eq!(active_context(&state), KeybindingContext::Transcript);
}

#[test]
fn test_transcript_modal_uses_pager_controls() {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Transcript(
        crate::state::transcript::TranscriptState::new(),
    ));

    assert!(matches!(
        map_key(&state, press(KeyCode::Up)),
        Some(TuiCommand::TranscriptScrollLines(-1))
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::Down)),
        Some(TuiCommand::TranscriptScrollLines(1))
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::PageUp)),
        Some(TuiCommand::TranscriptPage(-1))
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::PageDown)),
        Some(TuiCommand::TranscriptPage(1))
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::Home)),
        Some(TuiCommand::TranscriptJumpStart)
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::End)),
        Some(TuiCommand::TranscriptJumpEnd)
    ));
    assert!(matches!(
        map_key(&state, press(KeyCode::Tab)),
        Some(TuiCommand::TranscriptSelectNext)
    ));
    assert!(map_key(&state, press(KeyCode::BackTab)).is_none());
    assert!(matches!(
        map_key(&state, press(KeyCode::Esc)),
        Some(TuiCommand::Cancel)
    ));
    assert!(matches!(
        map_key(&state, ctrl(KeyCode::Char('c'))),
        Some(TuiCommand::Interrupt)
    ));
    assert!(matches!(
        map_key(&state, ctrl(KeyCode::Char('d'))),
        Some(TuiCommand::RequestExit)
    ));
}

#[test]
fn test_permission_prompt_context() {
    let mut state = AppState::new();
    install_permission_prompt(&mut state);
    assert_eq!(active_context(&state), KeybindingContext::Confirmation);
}

#[test]
fn test_model_picker_context() {
    let state = model_picker_state();
    assert_eq!(active_context(&state), KeybindingContext::ModelPicker);
}

#[test]
fn test_settings_theme_tab_uses_theme_picker_context() {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Settings(
        crate::widgets::settings_panel::SettingsPanelState::new(
            &state.ui.theme_state,
            state.ui.display_settings,
        ),
    ));
    assert_eq!(active_context(&state), KeybindingContext::ThemePicker);
}

#[test]
fn test_theme_picker_ctrl_t_toggles_syntax_highlighting() {
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Settings(
        crate::widgets::settings_panel::SettingsPanelState::new(
            &state.ui.theme_state,
            state.ui.display_settings,
        ),
    ));
    let cmd = map_key(&state, ctrl(KeyCode::Char('t')));
    assert!(matches!(cmd, Some(TuiCommand::ToggleSyntaxHighlighting)));
}

#[test]
fn test_model_picker_left_right_cycle_effort() {
    let state = model_picker_state();
    let left = map_key(&state, press(KeyCode::Left));
    let right = map_key(&state, press(KeyCode::Right));
    assert!(matches!(left, Some(TuiCommand::ModelPickerCycleEffort(-1))));
    assert!(matches!(right, Some(TuiCommand::ModelPickerCycleEffort(1))));
}

#[test]
fn test_model_picker_tab_cycles_role() {
    let state = model_picker_state();
    let cmd = map_key(&state, press(KeyCode::Tab));
    assert!(matches!(cmd, Some(TuiCommand::SettingsNextTab)));
}

#[test]
fn test_copy_picker_w_writes_to_file() {
    let state = copy_picker_state();
    let cmd = map_key(&state, press(KeyCode::Char('w')));
    assert!(matches!(cmd, Some(TuiCommand::CopyPickerWriteToFile)));
}

#[test]
fn test_ctrl_c_interrupts() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('c')));
    assert!(matches!(cmd, Some(TuiCommand::Interrupt)));
}

#[test]
fn test_ctrl_d_requests_exit() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('d')));
    assert!(matches!(cmd, Some(TuiCommand::RequestExit)));
}

#[test]
fn test_reserved_ctrl_d_wins_in_confirmation_context() {
    let mut state = AppState::new();
    install_permission_prompt(&mut state);

    let cmd = map_key(&state, ctrl(KeyCode::Char('d')));

    assert!(matches!(cmd, Some(TuiCommand::RequestExit)));
}

#[test]
fn test_ctrl_q_quits() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('q')));
    assert!(matches!(cmd, Some(TuiCommand::Quit)));
}

#[test]
fn test_ctrl_o_toggles_transcript() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('o')));
    assert!(matches!(cmd, Some(TuiCommand::ToggleTranscript)));
}

#[test]
fn test_ctrl_shift_o_toggles_teammate_preview() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl_shift(KeyCode::Char('o')));
    assert!(matches!(
        cmd,
        Some(TuiCommand::ToggleTeammateMessagePreview)
    ));
}

#[test]
fn test_enter_submits() {
    let mut state = AppState::new();
    state.ui.input.textarea.insert_str("h");
    let cmd = map_key(&state, press(KeyCode::Enter));
    assert!(matches!(cmd, Some(TuiCommand::SubmitInput)));
}

#[test]
fn test_enter_queues_during_streaming() {
    let mut state = AppState::new();
    state.ui.input.textarea.insert_str("h");
    state.ui.streaming = Some(crate::state::ui::StreamingState::new());
    let cmd = map_key(&state, press(KeyCode::Enter));
    assert!(matches!(cmd, Some(TuiCommand::QueueInput)));
}

#[test]
fn test_char_inserts() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::Char('x')));
    assert!(matches!(cmd, Some(TuiCommand::InsertChar('x'))));
}

#[test]
fn test_plain_character_input_is_not_logged_as_key_operation() {
    assert!(!should_log_key_command(&TuiCommand::InsertChar('x')));
    assert!(should_log_key_command(&TuiCommand::SubmitInput));
    assert!(should_log_key_command(&TuiCommand::Cancel));
}

#[test]
fn test_tab_toggles_plan() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::Tab));
    assert!(matches!(cmd, Some(TuiCommand::TogglePlanMode)));
}

#[test]
fn test_f1_shows_help() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::F(1)));
    assert!(matches!(cmd, Some(TuiCommand::ShowHelp)));
}

#[test]
fn test_esc_cancels() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::Esc));
    assert!(matches!(cmd, Some(TuiCommand::Cancel)));
}

#[test]
fn test_prompt_y_approves() {
    let mut state = AppState::new();
    install_permission_prompt(&mut state);
    let cmd = map_key(&state, press(KeyCode::Char('y')));
    assert!(matches!(cmd, Some(TuiCommand::Approve)));
}

#[test]
fn test_prompt_n_denies() {
    let mut state = AppState::new();
    install_permission_prompt(&mut state);
    let cmd = map_key(&state, press(KeyCode::Char('n')));
    assert!(matches!(cmd, Some(TuiCommand::Deny)));
}

#[test]
fn test_ctrl_t_cycles_view_in_chat_context() {
    // Ctrl+T now binds globally to `app:toggleTodos` (view cycle:
    // Chat → Tasks → Subagents). The previous Chat-context shadow that
    // routed Ctrl+T to `ChatCycleThinking` has moved to Ctrl+Y so the
    // view cycle wins from every context including the input bar.
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('t')));
    assert!(matches!(cmd, Some(TuiCommand::ToggleExpandedTasksView)));
}

#[test]
fn test_ctrl_y_cycles_thinking_level_in_chat_context() {
    // coco-rs extension: in Chat context Ctrl+Y cycles the Main role's
    // thinking effort through the active model's
    // `supported_thinking_levels` (`ChatCycleThinking → CycleThinkingLevel`).
    // Displaces the readline `yank` default; the legacy `input:yank`
    // cascade only applies in non-Chat contexts.
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('y')));
    assert!(matches!(cmd, Some(TuiCommand::CycleThinkingLevel)));
}

#[test]
fn test_f2_toggles_thinking_display() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::F(2)));

    assert!(matches!(cmd, Some(TuiCommand::ToggleThinking)));
}

#[test]
fn test_pageup_scrolls() {
    let state = AppState::new();
    let cmd = map_key(&state, press(KeyCode::PageUp));
    assert!(matches!(cmd, Some(TuiCommand::PageUp)));
}

#[test]
fn test_ctrl_f_kills_all_agents() {
    // Spec: crate-coco-tui.md §Keyboard Shortcuts — Ctrl+F = kill all agents.
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('f')));
    assert!(matches!(cmd, Some(TuiCommand::KillAllAgents)));
}

#[test]
fn test_ctrl_shift_f_opens_global_search() {
    // TS `app:globalSearch` is bound to `ctrl+shift+f`
    // (`defaultBindings.ts:53-58`, gated on QUICK_SEARCH which coco-rs
    // doesn't gate). Fast mode moved off this key — TS binds it to
    // `meta+o` (`alt+o`) only.
    let state = AppState::new();
    let key = KeyEvent {
        code: KeyCode::Char('f'),
        modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let cmd = map_key(&state, key);
    assert!(matches!(cmd, Some(TuiCommand::ShowGlobalSearch)));
}

#[test]
fn test_autocomplete_context_not_activated_with_empty_items() {
    // Async trigger installed but results haven't arrived yet — items is
    // empty. Arrow keys must keep passing through to input editing so the
    // user can navigate history while search runs.
    let mut state = AppState::new();
    state.ui.active_suggestions = Some(crate::state::ActiveSuggestions {
        kind: crate::state::SuggestionKind::At,
        items: Vec::new(),
        selected: 0,
        query: "src".into(),
        trigger_pos: 0,
    });
    assert_eq!(active_context(&state), KeybindingContext::Chat);
}

#[test]
fn test_autocomplete_context_when_suggestions_active() {
    // Spec: crate-coco-tui.md §Autocomplete Systems — once suggestions are
    // visible, key dispatch must route Up/Down/Tab/Esc through the
    // Autocomplete context.
    let mut state = AppState::new();
    state.ui.active_suggestions = Some(crate::state::ActiveSuggestions {
        kind: crate::state::SuggestionKind::SlashCommand,
        items: vec![crate::widgets::suggestion_popup::SuggestionItem {
            label: "/help".into(),
            description: None,
            metadata: None,
        }],
        selected: 0,
        query: String::new(),
        trigger_pos: 0,
    });
    assert_eq!(active_context(&state), KeybindingContext::Autocomplete);

    let tab = map_key(&state, press(KeyCode::Tab));
    assert!(matches!(tab, Some(TuiCommand::SurfaceConfirm)));

    let up = map_key(&state, press(KeyCode::Up));
    assert!(matches!(up, Some(TuiCommand::SurfacePrev)));

    // Typing a character should fall through to input editing, not be
    // swallowed by the autocomplete context.
    let ch = map_key(&state, press(KeyCode::Char('x')));
    assert!(matches!(ch, Some(TuiCommand::InsertChar('x'))));
}

#[test]
fn test_alt_v_pastes() {
    // Spec parity with Ctrl+V — Alt+V also pastes from clipboard.
    let state = AppState::new();
    let key = KeyEvent {
        code: KeyCode::Char('v'),
        modifiers: KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let cmd = map_key(&state, key);
    assert!(matches!(cmd, Some(TuiCommand::PasteFromClipboard)));
}
