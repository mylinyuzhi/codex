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
use crate::state::AppState;

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

#[test]
fn test_default_context_is_chat() {
    let state = AppState::new();
    assert_eq!(active_context(&state), KeybindingContext::Chat);
}

#[test]
fn test_help_overlay_context() {
    let mut state = AppState::new();
    state.ui.overlay = Some(crate::state::Overlay::Help);
    assert_eq!(active_context(&state), KeybindingContext::Scrollable);
}

#[test]
fn test_permission_overlay_context() {
    let mut state = AppState::new();
    state.ui.overlay = Some(crate::state::Overlay::Permission(
        crate::state::PermissionOverlay {
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
        },
    ));
    assert_eq!(active_context(&state), KeybindingContext::Confirmation);
}

#[test]
fn test_ctrl_c_interrupts() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('c')));
    assert!(matches!(cmd, Some(TuiCommand::Interrupt)));
}

#[test]
fn test_ctrl_q_quits() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('q')));
    assert!(matches!(cmd, Some(TuiCommand::Quit)));
}

#[test]
fn test_enter_submits() {
    let mut state = AppState::new();
    state.ui.input.insert_char('h');
    let cmd = map_key(&state, press(KeyCode::Enter));
    assert!(matches!(cmd, Some(TuiCommand::SubmitInput)));
}

#[test]
fn test_enter_queues_during_streaming() {
    let mut state = AppState::new();
    state.ui.input.insert_char('h');
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
fn test_overlay_y_approves() {
    let mut state = AppState::new();
    state.ui.overlay = Some(crate::state::Overlay::Permission(
        crate::state::PermissionOverlay {
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
        },
    ));
    let cmd = map_key(&state, press(KeyCode::Char('y')));
    assert!(matches!(cmd, Some(TuiCommand::Approve)));
}

#[test]
fn test_overlay_n_denies() {
    let mut state = AppState::new();
    state.ui.overlay = Some(crate::state::Overlay::Permission(
        crate::state::PermissionOverlay {
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
        },
    ));
    let cmd = map_key(&state, press(KeyCode::Char('n')));
    assert!(matches!(cmd, Some(TuiCommand::Deny)));
}

#[test]
fn test_ctrl_t_cycles_thinking() {
    let state = AppState::new();
    let cmd = map_key(&state, ctrl(KeyCode::Char('t')));
    assert!(matches!(cmd, Some(TuiCommand::CycleThinkingLevel)));
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
fn test_ctrl_shift_f_toggles_fast_mode() {
    // Spec: crate-coco-tui.md §Keyboard Shortcuts — Ctrl+Shift+F = fast mode.
    let state = AppState::new();
    let key = KeyEvent {
        code: KeyCode::Char('f'),
        modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let cmd = map_key(&state, key);
    assert!(matches!(cmd, Some(TuiCommand::ToggleFastMode)));
}

#[test]
fn test_autocomplete_context_not_activated_with_empty_items() {
    // Async trigger installed but results haven't arrived yet — items is
    // empty. Arrow keys must keep passing through to input editing so the
    // user can navigate history while search runs.
    let mut state = AppState::new();
    state.ui.active_suggestions = Some(crate::state::ActiveSuggestions {
        kind: crate::state::SuggestionKind::File,
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
        }],
        selected: 0,
        query: String::new(),
        trigger_pos: 0,
    });
    assert_eq!(active_context(&state), KeybindingContext::Autocomplete);

    let tab = map_key(&state, press(KeyCode::Tab));
    assert!(matches!(tab, Some(TuiCommand::OverlayConfirm)));

    let up = map_key(&state, press(KeyCode::Up));
    assert!(matches!(up, Some(TuiCommand::OverlayPrev)));

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
