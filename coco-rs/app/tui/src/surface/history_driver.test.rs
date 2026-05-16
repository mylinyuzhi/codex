use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::*;
use crate::display_settings::SyntaxHighlighting;
use crate::presentation::styles::UiStyles;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::theme::Theme;

#[test]
fn driver_emit_append_only_uses_finalized_transcript_renderer() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines([
        "old0    ", "old1    ", "old2    ", "old3    ", "old4    ", "old5    ", "view    ",
    ]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 8, 1));
    let messages = vec![ChatMessage::assistant_text("a1", "hello")];
    let mut driver = SurfaceHistoryDriver::new();

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &messages, options(&theme, 8))
        .expect("emit");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 3,
        }
    );
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec![
            "old3    ",
            "old4    ",
            "old5    ",
            "header  ",
            "⏺ hello ",
            "        ",
            "view    "
        ]
    );
}

#[test]
fn driver_note_width_schedules_stream_replay_after_resize() {
    let mut driver = SurfaceHistoryDriver::new();

    assert_eq!(
        driver.note_width(80, false),
        HistoryWidthChange {
            initialized: true,
            changed: false,
        }
    );
    assert_eq!(
        driver.note_width(100, true),
        HistoryWidthChange {
            initialized: false,
            changed: true,
        }
    );
    assert!(!driver.replay_due(Instant::now()));

    driver.reflow.force_due_for_test();

    assert!(driver.replay_due(Instant::now()));
    assert!(driver.stream_finish_replay_needed());
    assert!(!driver.stream_finish_replay_needed());
}

#[test]
fn driver_replay_all_replaces_owned_history_and_marks_stream_replay() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines([
        "old0    ", "old1    ", "old2    ", "old3    ", "old4    ", "old5    ", "view    ",
    ]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 8, 1));
    terminal.note_history_rows_inserted(6);
    let messages = vec![ChatMessage::assistant_text("a1", "world")];
    let mut driver = SurfaceHistoryDriver::new();

    let outcome = driver
        .replay_all(&mut terminal, header(), &messages, options(&theme, 8), true)
        .expect("replay");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 3,
        }
    );
    assert_eq!(terminal.visible_history_rows(), 3);
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec![
            "        ",
            "        ",
            "        ",
            "header  ",
            "⏺ world ",
            "        ",
            "        "
        ]
    );
    assert!(driver.stream_finish_replay_needed());
}

fn header() -> Vec<Line<'static>> {
    vec![Line::from("header")]
}

fn options(theme: &Theme, width: u16) -> HistoryLineRenderOptions<'_> {
    HistoryLineRenderOptions {
        styles: UiStyles::new(theme),
        width,
        syntax_highlighting: SyntaxHighlighting::Disabled,
        show_system_reminders: false,
    }
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}
