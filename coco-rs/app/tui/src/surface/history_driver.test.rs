use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use uuid::Uuid;

use super::*;
use crate::state::derive::test_helpers;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::theme::Theme;
use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;

#[test]
fn driver_emit_append_only_uses_finalized_transcript_renderer() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines([
        "old0    ", "old1    ", "old2    ", "old3    ", "old4    ", "old5    ", "view    ",
    ]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 8, 1));
    let cells = vec![test_helpers::assistant_text_cell("hello")];
    let mut driver = SurfaceHistoryDriver::new();

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, options(&theme, 8))
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
    let cells = vec![test_helpers::assistant_text_cell("world")];
    let mut driver = SurfaceHistoryDriver::new();

    let outcome = driver
        .replay_all(&mut terminal, header(), &cells, options(&theme, 8), true)
        .expect("replay");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 3,
        }
    );
    assert_eq!(terminal.visible_history_rows(), 3);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 3, 8, 1));
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec![
            "header  ",
            "⏺ world ",
            "        ",
            "        ",
            "        ",
            "        ",
            "        "
        ]
    );
    assert!(driver.stream_finish_replay_needed());
}

#[test]
fn driver_replay_all_reanchors_viewport_to_replayed_history_bottom() {
    let theme = Theme::default();
    let backend = TestBackend::new(48, 30);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 26, 48, 4));
    terminal.note_history_rows_inserted(26);
    let cells = vec![
        test_helpers::user_text_cell(Uuid::new_v4(), "hello"),
        test_helpers::assistant_text_cell("short reply"),
    ];
    let mut driver = SurfaceHistoryDriver::new();

    let outcome = driver
        .replay_all(&mut terminal, header(), &cells, options(&theme, 48), false)
        .expect("replay");

    let HistoryEmissionOutcome::Replayed { rows, .. } = outcome else {
        panic!("expected replay outcome, got {outcome:?}");
    };
    assert_eq!(terminal.viewport_area().top(), rows);

    let lines = plain_buffer_lines(terminal.backend().buffer());
    let assistant = line_index(&lines, "⏺ short reply");
    let input_top = terminal.viewport_area().top() as usize;
    let gap = input_top.saturating_sub(assistant + 1);
    assert!(
        gap <= 3,
        "replay left {gap} rows between assistant and viewport:\n{}",
        lines.join("\n")
    );
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
        show_thinking: false,
        kb_handle: None,
        reasoning_metadata: None,
    }
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}

fn line_index(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in {lines:#?}"))
}
