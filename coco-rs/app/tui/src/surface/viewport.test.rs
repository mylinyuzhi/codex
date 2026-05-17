use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::state::session::ChatMessage;
use crate::state::ui::StreamingState;
use crate::surface::overlay::HistorySurfaceMode;
use crate::surface::terminal::SurfaceTerminal;

#[test]
fn interactive_viewport_does_not_render_session_header() {
    let backend = TestBackend::new(48, 6);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 6));
    let state = AppState::new();

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan());
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("COCO"));
    assert!(!text.contains("Type a message"));
    assert!(text.contains("❯"));
}

#[test]
fn interactive_viewport_desired_height_tracks_idle_composer() {
    let state = AppState::new();

    assert_eq!(
        interactive_viewport_desired_height(&state, 48, 12, native_plan()),
        4
    );
}

#[test]
fn interactive_viewport_desired_height_never_exceeds_cap() {
    let state = AppState::new();

    assert_eq!(
        interactive_viewport_desired_height(&state, 48, 2, native_plan()),
        2
    );
}

#[test]
fn interactive_viewport_does_not_render_finalized_messages() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "finalized history"));

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan());
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("finalized history"));
}

#[test]
fn interactive_viewport_renders_finalized_messages_in_viewport_history_mode() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "fallback history"));

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, viewport_history_plan());
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("fallback history"));
}

#[test]
fn interactive_viewport_renders_active_streaming_tail() {
    let backend = TestBackend::new(48, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 10));
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    streaming.append_text("live response");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan());
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("live response"));
}

#[test]
fn interactive_viewport_reports_input_rect_for_cursor_policy() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let state = AppState::new();
    let mut layout = FrameLayout::default();

    terminal
        .draw_viewport(|frame| {
            layout = render_interactive_viewport(frame, &state, native_plan());
        })
        .expect("draw");

    assert_eq!(layout.input.height, 3);
    assert_eq!(layout.input.width, 48);
}

fn native_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        overlay_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    }
}

fn viewport_history_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        history_surface: HistorySurfaceMode::Viewport,
        ..native_plan()
    }
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}
